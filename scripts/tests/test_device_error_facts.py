"""Source-backed synthetic records and hostile framing/privacy probes for P3."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('device_facts_tests', ROOT / 'scripts/project-device-error-facts.py')
p = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(p)
OLD = importlib.util.spec_from_file_location('old_failure_tests', ROOT / 'scripts/project-analytical-runtime-failure.py')
old = importlib.util.module_from_spec(OLD); OLD.loader.exec_module(old)
HEADER = b'Error in Surface::present: Validation Error\n\nCaused by:\n  Parent device is lost\n'
RESULT = 'ERROR wgpu_hal::auxil::dxgi::result: '
GOOD = (RESULT + 'Signal fence failed: The GPU device instance has been suspended. (0x887A0005)\n').encode()


def record(raw): return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)}


class DeviceFactsTests(unittest.TestCase):
    def scan(self, raw):
        value = p._scan(io.BytesIO(raw))
        self.assertEqual(value['record'], record(raw))
        prior = old.scan_stream(io.BytesIO(raw))
        self.assertEqual(value['marker_lines_total'], prior['error_lines'])
        self.assertEqual(value['marker_lines_total'], value['pre_fatal_marker_lines'] + value['fatal_boundary_marker_lines'] + value['post_fatal_marker_lines'])
        self.assertEqual(len(value['rows']) + value['omitted_rows'], value['pre_fatal_marker_lines'])
        self.assertEqual(sum(value['disposition_counts'].values()), value['pre_fatal_marker_lines'])
        self.assertLessEqual(len(p.canonical(value)), 16384)
        self.assertFalse(value['producer_authenticated'])
        self.assertFalse(value['raw_message_text_exported'])
        self.assertFalse(value['runtime_accepted']); self.assertFalse(value['release_authorized'])
        self.assertEqual(value['root_cause'], 'not_established')
        for row in value['rows'] + ([value['fatal_boundary']] if value['fatal_boundary'] else []):
            piece = raw[row['start_byte']:row['start_byte'] + row['byte_length']]
            self.assertEqual(row['sha256'], hashlib.sha256(piece).hexdigest())
        return value

    def test_real_result_adapter_operations_from_locked_sources(self):
        fixture = json.loads((ROOT / 'scripts/tests/fixtures/device-error-source-provenance.json').read_text())
        locked = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
        for crate in fixture['crates']:
            self.assertIn((crate['name'], crate['version'], crate['registry_archive_sha256']), [(x['name'], x['version'], x.get('checksum')) for x in locked])
        self.assertFalse(fixture['actual_native_log_claim'])
        self.assertEqual(len(fixture['result_operations']), 40)
        for operation in fixture['result_operations']:
            with self.subTest(operation=operation['description']):
                self.assertTrue(operation['call_sites'])
                raw = (RESULT + operation['description'] + ' failed: Private localized driver prose. (0x887A0005)\n').encode() + HEADER
                value = self.scan(raw)
                self.assertEqual(value['rows'][0]['facts'], {'operation': operation['operation'], 'hresult':'0x887A0005'})
                self.assertNotIn('Private localized driver prose', json.dumps(value))

    def test_hresult_empty_message_and_alternate_failure_codes(self):
        for code in ('0x887A0001', '0x887A0005', '0x887A0006', '0x887A0007', '0x8007000E', '0xFFFFFFFF'):
            for display in (code, 'Localized text. (' + code + ')'):
                value = self.scan((RESULT + 'Present failed: ' + display + '\n').encode() + HEADER)
                self.assertEqual(value['rows'][0]['facts']['hresult'], code)

    def test_real_descendant_targets_and_exact_wait_debug_grammar(self):
        cases = (
            ('wgpu_hal::dx12::command', 'ID3D12CommandAllocator::Reset() failed with 0x887A0005', 'command_allocator_reset'),
            ('wgpu_hal::dx12::device', 'Wait failed!', 'device_wait_failed'),
            ('wgpu_hal::dx12::device', 'Unexpected wait status: 0xWAIT_EVENT(4294967295)', 'unexpected_device_wait_status'),
            ('wgpu_hal::dx12', 'Unexpected wait status: 0xWAIT_EVENT(ffffffff)', 'unexpected_surface_wait_status'),
            ('wgpu_hal::dx12::descriptor', 'Failed to allocate a handle form a fixed size heap', 'fixed_descriptor_heap_exhausted'),
            ('wgpu_hal::dx12::suballocation', 'DX12 gpu-allocator: No Compatible Memory Type Found', 'no_compatible_memory_type'),
            ('wgpu_core::device::resource', "wgpu-core feature 'trace' is not enabled", 'core_trace_feature_disabled'),
        )
        for target,message,operation in cases:
            with self.subTest(target=target,message=message):
                value = self.scan(('ERROR '+target+': '+message+'\n').encode()+HEADER)
                self.assertEqual(value['rows'][0]['facts']['operation'],operation)
        value = self.scan(b'ERROR wgpu_core::device::resource: Parent device is lost\n'+HEADER)
        self.assertEqual(value['rows'][0]['disposition'],'unknown_template')
        self.assertEqual(value['rows'][0]['target_category'],'core_device_descendant')
        self.assertFalse(value['classification_complete'])

    def test_exception_relay_strips_prefix_and_exports_only_reviewed_trailer(self):
        for name,number in p.MESSAGE_IDS.items():
            for severity in ('ERROR','CORRUPTION'):
                raw=('ERROR wgpu_hal::auxil::dxgi::exception: [ EXECUTION '+severity+' #'+str(number)+': '+name+']\n').encode()+HEADER
                value=self.scan(raw)
                facts=value['rows'][0]['facts']
                self.assertEqual(facts['message_name'], name); self.assertEqual(facts['message_id'], number)
                self.assertEqual(facts['message_category'],'EXECUTION'); self.assertEqual(facts['message_severity'],severity)
                self.assertNotIn('Arbitrary private driver prose',json.dumps(value))

    def test_exception_unknown_mismatched_and_injected_trailers_stay_unknown(self):
        for message in (
            'Resource name: SECRET [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'arbitrary driver prose [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private [ EXECUTION ERROR #9999: UNKNOWN_ID]',
            'private [ EXECUTION ERROR #233: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private [ PRIVATE_CATEGORY ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private [ EXECUTION WARNING #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private [ EXECUTION ERROR #0232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT] extra',
            'private [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT] [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
            'private\' [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]',
        ):
            with self.subTest(message=message):
                value=self.scan(('ERROR wgpu_hal::auxil::dxgi::exception: '+message+'\n').encode()+HEADER)
                self.assertEqual(value['rows'][0]['facts'],{})
                self.assertFalse(value['classification_complete'])
                self.assertNotIn('private',json.dumps(value))

    def test_logger_grammar_rejects_fuzzy_prefix_and_cross_target_templates(self):
        for prefix in ('error ', ' ERROR ', '[ERROR] ', 'INFO ', 'FATAL ', '2026-10-08garbage ERROR ',
                       '2026-10-08 12:00:00Z ERROR ', 'prefix ERROR ', 'ERROR span{label=foo}: '):
            value=self.scan((prefix+'wgpu_hal::dx12::device: Wait failed!\n').encode()+HEADER)
            if value['rows']: self.assertEqual(value['rows'][0]['facts'],{})
        for target in ('wgpu_hal::dx12::device_extra','wgpu_hal::dx12::command','wgpu_core::device::resource','private::module'):
            value=self.scan(('ERROR '+target+': Wait failed!\n').encode()+HEADER)
            self.assertEqual(value['rows'][0]['facts'],{})
        value=self.scan(b'ERROR Present failed: 0x887A0005\n'+HEADER)
        self.assertEqual(value['rows'][0]['disposition'],'malformed_prefix')

    def test_exact_timestamp_sgr_and_crlf_source_formatter_shapes(self):
        for prefix in ('', '2026-10-08T18:00:00.123456Z ', '\x1b[2m2026-10-08T18:00:00.123456Z\x1b[0m '):
            value=self.scan((prefix+'\x1b[31mERROR\x1b[0m \x1b[2mwgpu_hal::dx12::device:\x1b[0m Wait failed!\r\n').encode()+HEADER)
            self.assertEqual(value['rows'][0]['facts'],{'operation':'device_wait_failed'})

    def test_unknown_hresult_does_not_harvest_arbitrary_numbers_or_tokens(self):
        for display in ('error 0x887A0005 trailing','0x887a0005','0x887A00050','0x00000005', '-2005270523',
                        'HRESULT(0x887A0005)','DXGI_ERROR_DEVICE_REMOVED','code 7',
                        'private (0x887A0006) (0x887A0005)', 'private "label" (0x887A0005)'):
            value=self.scan((RESULT+'Present failed: '+display+'\n').encode()+HEADER)
            self.assertEqual(value['rows'][0]['facts'],{})
            self.assertIn(value['rows'][0]['disposition'],('unknown_hresult','unsafe_label_or_path'))

    def test_all_pre_fatal_markers_survive_info_debug_and_nonadjacent_noise(self):
        raw=GOOD+b'INFO unrelated::target: ordinary\nDEBUG target: details\n'+b'ordinary noise\n'*100+GOOD+HEADER
        value=self.scan(raw)
        self.assertEqual(value['pre_fatal_marker_lines'],2)
        self.assertEqual(len(value['rows']),2)
        self.assertEqual(value['nearby_marker_lines'],1)

    def test_known_267_byte_gap_shape_is_not_an_actual_log_reconstruction(self):
        # Only span dimensions mirror the receipt. Text here is intentionally unknown.
        line=b'ERROR unknown::target: '+b'x'*(267-len(b'ERROR unknown::target: ')-1)+b'\n'
        panic=b"thread 'synthetic' panicked at "+b'x'*(188-len(b"thread 'synthetic' panicked at ")-1)+b'\n'
        prefix=(b'x'*1023+b'\n')*(1089504//1024)
        remaining=1089504-len(prefix)
        if remaining: prefix+=b'x'*(remaining-1)+b'\n'
        raw=prefix+line+b'\n'+panic+HEADER
        value=self.scan(raw)
        row=value['rows'][0]
        self.assertEqual((row['start_byte'],row['byte_length']),(1089504,267))
        self.assertEqual(value['fatal_boundary']['start_byte'],1089960)
        self.assertEqual(value['rows'][0]['disposition'],'unknown_target')
        self.assertEqual(value['nearby_marker_lines'],1)
        self.assertEqual(row['facts'],{})

    def test_first_eight_last_eight_and_all_omitted_dispositions_accounted(self):
        raw=b''.join((GOOD if n%2 else b'ERROR unknown::target: private\n') for n in range(100))+HEADER
        value=self.scan(raw)
        self.assertEqual(len(value['rows']),16); self.assertEqual(value['omitted_rows'],84)
        self.assertEqual(value['omitted_disposition_counts']['recognized'],42)
        self.assertEqual(value['omitted_disposition_counts']['unknown_target'],42)
        self.assertEqual(value['disposition_counts']['recognized'],50)
        self.assertEqual(value['nearby_marker_lines'],8)
        self.assertFalse(value['classification_complete'])
        offsets=[]; offset=0
        for line in raw.splitlines(keepends=True)[:100]: offsets.append(offset); offset+=len(line)
        self.assertEqual([x['start_byte'] for x in value['rows']],offsets[:8]+offsets[-8:])

    def test_unknown_markers_rejected_explicitly_even_if_not_a_severity(self):
        raw=b'INFO unrelated::module: last error was private\nfatal thing\nERROR unknown::target: private\n'+HEADER+b'ERROR after::fatal: private\n'+HEADER
        value=self.scan(raw)
        self.assertEqual(value['pre_fatal_marker_lines'],3)
        self.assertEqual(value['disposition_counts']['malformed_prefix'],2)
        self.assertEqual(value['disposition_counts']['unknown_target'],1)
        self.assertEqual(value['post_fatal_marker_lines'],2)

    def test_p2_path_and_token_regressions_never_export_arbitrary_prose(self):
        private_values=(r'C:\Users\Private Person (work)\sensitive\source.rs',r'C:\Users\Private,Person\sensitive\source.rs',
            r'C:\Users\Private;Person\sensitive\source.rs',r'.\private\source.rs',r'C:private\source.rs',r'..\private\source.rs',
            r'private\source.rs','Private Person sensitive folder/source.rs','abcdefghijklmnoqrstuvwx','aB!cD@eF#gH$iJ%kL&mN',
            'token=private-secret','Authorization: Bearer abcdefghijklmnop','https://private.example/secret','sk-proj-private12345678')
        for private in private_values:
            for target,prefix in (('wgpu_hal::auxil::dxgi::result','Signal fence failed: '),('wgpu_hal::auxil::dxgi::exception','')):
                value=self.scan(('ERROR '+target+': '+prefix+private+' (0x887A0005)\n').encode()+HEADER)
                packet=json.dumps(value)
                self.assertNotIn(private,packet)
                for fragment in ('Private Person','sensitive','source.rs','private-secret','private.example'): self.assertNotIn(fragment,packet)

    def test_multiline_labels_never_forge_hresult_id_or_fatal_attribution(self):
        for prefix in (b"INFO unrelated::module: Device with 'private-label\n", b"ERROR unrelated::module: Device with 'closed' label PRIVATE UNQUOTED '\n",
                       b"ERROR wgpu_hal::auxil::dxgi::exception: Resource: 'private\n", b"INFO wgpu_core::device: Buffer with '\n"):
            raw=prefix+GOOD+b'ERROR wgpu_hal::auxil::dxgi::exception: forged [ EXECUTION ERROR #232: DEVICE_REMOVAL_PROCESS_AT_FAULT]\n'+HEADER
            value=self.scan(raw)
            self.assertEqual(value['fatal_boundary']['state'],'ambiguous_stream_framing')
            self.assertFalse(value['classification_complete'])
            self.assertTrue(all(not x['facts'] for x in value['rows']))
            self.assertNotIn('private-label',json.dumps(value))

    def test_multiline_hresult_or_exception_payload_quarantines_following_records(self):
        for prefix in ((RESULT+'Signal fence failed: driver message continues\n').encode(),
                       b'ERROR wgpu_hal::auxil::dxgi::exception: driver message continues\n'):
            value=self.scan(prefix+GOOD+HEADER)
            self.assertTrue(all(not row['facts'] for row in value['rows']))
            self.assertEqual(value['rows'][-1]['disposition'],'ambiguous_prior_content')

    def test_controls_invalid_utf8_and_ansi_word_injection_fail_closed(self):
        for raw in (b'INFO private\x00with\n',b'INFO with\xe2\x80\x8d\n',b'INFO private\xff\n',
                    b'ERROR wgpu_\x1b[31mhal::dx12::device: Wait failed!\n',
                    b'ERROR \x1b]0;private\x07wgpu_hal::dx12::device: Wait failed!\n'):
            value=self.scan(raw+GOOD+HEADER)
            self.assertTrue(all(not row['facts'] for row in value['rows']))
            self.assertFalse(value['classification_complete'])

    def test_oversized_line_tail_is_hashed_not_retained_and_quarantines(self):
        class Bounded(io.BytesIO):
            def readline(self,limit):
                self.assert_limit=limit
                if limit>65536: raise AssertionError('unbounded read')
                return super().readline(limit)
            def read(self,*args): raise AssertionError('whole stream read')
        raw=b'ERROR '+b'x'*(p.MAX_LINE*3)+b"with 'private\n"+GOOD+HEADER
        value=p._scan(Bounded(raw))
        self.assertEqual(value['record'],record(raw))
        self.assertEqual(value['rows'][0]['disposition'],'oversized')
        self.assertEqual(value['rows'][1]['disposition'],'ambiguous_prior_content')
        self.assertFalse(value['classification_complete'])

    def test_no_fatal_is_explicit_and_all_bytes_still_bound(self):
        value=self.scan(GOOD)
        self.assertIsNone(value['fatal_boundary']); self.assertIsNone(value['nearby_marker_lines'])
        self.assertFalse(value['classification_complete'])
        self.assertEqual(self.scan(b'')['pre_fatal_marker_lines'],0)

    def test_fatal_heading_is_exact_not_ansi_cleaned_or_prefix_matched(self):
        for header in (b'\x1b[31m'+HEADER,b' '+HEADER,HEADER.replace(b'Validation Error',b'validation Error'),b'prefix '+HEADER):
            value=self.scan(GOOD+header)
            self.assertIsNone(value['fatal_boundary'])

    def test_visible_continuations_revoke_earlier_and_capped_facts(self):
        for tail in (b'Actual continuation (0x8007000E)\n'+HEADER,
                     HEADER+b'Actual continuation (0x8007000E)\n'):
            raw=GOOD*25+b'ERROR wgpu_hal::dx12::device: Wait failed!\n'+tail
            value=self.scan(raw)
            self.assertTrue(all(not row['facts'] for row in value['rows']))
            self.assertEqual(value['disposition_counts']['recognized'],0)
            self.assertEqual(value['omitted_disposition_counts']['recognized'],0)
            self.assertEqual(value['nearby_disposition_counts']['recognized'],0)
            self.assertEqual(value['disposition_counts']['ambiguous_stream_framing'],26)
            self.assertEqual(value['fatal_boundary']['state'],'ambiguous_stream_framing')

    def test_all_level_exception_and_unknown_dynamic_templates_are_framing_hazards(self):
        for first in (b' WARN wgpu_hal::auxil::dxgi::exception: object name begins\n',
                      b'INFO wgpu_hal::auxil::dxgi::exception: object name begins\n',
                      b'ERROR wgpu_hal::dx12: ResizeBuffers failed: object name begins\n',
                      b'ERROR wgpu_core::device::resource: Shader error: dynamic shader text\n'):
            value=self.scan(first+GOOD+HEADER)
            self.assertTrue(all(not row['facts'] for row in value['rows']))
            self.assertFalse(value['classification_complete'])

    def test_real_panic_cause_and_bevy_postlude_preserve_clean_error_facts(self):
        raw=GOOD+b"\nthread 'Render thread' panicked at C:\\private\\wgpu_core.rs:3858:18:\n"+HEADER
        raw+=b'\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n'
        raw+=b'Encountered a panic in system `bevy_render::renderer::render_system`!\n'
        value=self.scan(raw)
        self.assertEqual(value['rows'][0]['facts'],{'operation':'signal_fence','hresult':'0x887A0005'})
        self.assertTrue(value['classification_complete'])
        self.assertNotIn('private',json.dumps(value))

    def test_bound_file_and_same_length_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'stderr'; path.write_bytes(GOOD+HEADER)
            self.assertEqual(p.project_stream(path,expected_record=record(GOOD+HEADER))['record'],record(GOOD+HEADER))
            path.write_bytes((GOOD+HEADER).replace(b'887A0005',b'887A0007'))
            with self.assertRaisesRegex(ValueError,'differs from expected record'): p.project_stream(path,expected_record=record(GOOD+HEADER))
            for bad in (None,{},dict(record(GOOD),bytes=True),dict(record(GOOD),extra='private')):
                with self.assertRaises(ValueError): p.project_stream(path,expected_record=bad)

    def test_file_links_and_windows_reparse_ancestors_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary); path=root/'stderr'; path.write_bytes(GOOD)
            link=root/'link'; link.symlink_to(path)
            with self.assertRaises(ValueError): p.project_stream(link,expected_record=record(GOOD))
            hard=root/'hard'; os.link(path,hard)
            with self.assertRaises(ValueError): p.project_stream(hard,expected_record=record(GOOD))
            real=Path.lstat
            def inspect(part):
                detail=real(part)
                return SimpleNamespace(st_mode=detail.st_mode,st_file_attributes=0x400) if part==root else detail
            with mock.patch.object(Path,'lstat',autospec=True,side_effect=inspect),mock.patch.object(Path,'open',side_effect=AssertionError('must not open')):
                with self.assertRaises(ValueError): p.project_stream(path,expected_record=record(GOOD))

    def test_stream_and_public_output_limits_are_enforced(self):
        with mock.patch.object(p.present,'MAX_STREAM',128):
            with self.assertRaises(ValueError): p._scan(io.BytesIO(b'x'*129))
        raw=(b'ERROR wgpu_hal::auxil::dxgi::exception: [ EXECUTION ERROR #904: COMMAND_LIST_MULTIPLE_SWAPCHAIN_BUFFER_REFERENCES]\n')*16+HEADER
        self.scan(raw)


if __name__=='__main__': unittest.main()
