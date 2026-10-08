"""Synthetic hostile boundaries and real tiny subprocesses, never Windows proof."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('qualification', Path(__file__).parents[1] / 'qualify-analytical-swift-windows.py')
q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(q)
REPO = Path(__file__).resolve().parents[2]


def record(data=b''):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def summary():
    return {'schema_version': 1, 'identity': q.IDENTITY, 'phase': 'regressions', 'source_sha': 'a' * 40,
            'source_tree': None, 'status': 'failed', 'release_authorized': False, 'native_runtime_qualified': False,
            'distribution_qualified': False, 'appearance_accepted': False, 'checks': {}, 'commands': [], 'images': {},
            'open_reviews': q.OPEN_REVIEWS, 'limits': q.LIMITS, 'bindings': {}, 'native_projection': None, 'runtime_facts': None, 'ui_capabilities': None}


def synthetic_pe(normal='KERNEL32.dll', delay='USER32.dll'):
    # Hand-authored PE32+ fixture from Microsoft's field layout. Not a runnable PE.
    data = bytearray(0x800)
    data[:2] = b'MZ'; struct.pack_into('<I', data, 0x3c, 0x80)
    data[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<HHIIIHH', data, 0x84, 0x8664, 1, 0, 0, 0, 240, 0x22)
    optional = 0x98
    struct.pack_into('<H', data, optional, 0x20b)
    struct.pack_into('<I', data, optional + 60, 0x200)
    struct.pack_into('<H', data, optional + 68, 3)
    struct.pack_into('<I', data, optional + 108, 16)
    section = optional + 240
    data[section:section + 8] = b'.rdata\0\0'
    struct.pack_into('<IIII', data, section + 8, 0x600, 0x1000, 0x600, 0x200)
    if normal:
        struct.pack_into('<II', data, optional + 120, 0x1000, 40)
        struct.pack_into('<IIIII', data, 0x200, 0x1100, 0, 0, 0x1200, 0x1100)
        data[0x400:0x401 + len(normal)] = normal.encode('ascii') + b'\0'
    if delay:
        struct.pack_into('<II', data, optional + 112 + 13 * 8, 0x1080, 64)
        struct.pack_into('<IIIIIIII', data, 0x280, 1, 0x1280, 0, 0, 0, 0, 0, 0)
        data[0x480:0x481 + len(delay)] = delay.encode('ascii') + b'\0'
    return data


class PlanTests(unittest.TestCase):
    def test_combined_release_and_identity_commands_preserve_exact_selection(self):
        plans = q.regression_plan()
        self.assertEqual(len(plans), 15)
        for step in plans:
            command = step['command']
            self.assertEqual(command[:2], ['cargo', '+1.93.0'])
            for arg in ('--locked', '--offline', '--release', '--target', 'x86_64-pc-windows-msvc'):
                self.assertIn(arg, command)
            self.assertNotIn('--workspace', command)
        exact = [p for p in plans if p['exact']]
        self.assertEqual(len(exact), 4)
        for step in exact:
            self.assertIn('--no-default-features', step['command'])
            self.assertIn('analytic-tonemapping,commercial-staging', step['command'])
            self.assertEqual(step['command'][-2:], ['--', '--exact'])

    def test_no_diagnostic_appearance_or_release_claim(self):
        text = (REPO / 'scripts/qualify-analytical-swift-windows.py').read_text()
        self.assertNotIn('--windows-readback-diagnostic', text)
        self.assertEqual(q.candidate.CAPTURE_TIMEOUT_SECONDS, 180)
        self.assertIn('interactive_map_camera_lifecycle_unexecuted', q.LIMITS)
        workflow = (REPO / '.github/workflows/analytical-swift-windows-qualification.yml').read_text()
        self.assertIn('workflow_dispatch:', workflow)
        self.assertIn("branches: ['qualification/analytical-swift-reviewed']", workflow)
        self.assertIn('  regressions:\n    needs: source-ci\n', workflow)
        self.assertIn('  runtime:\n    needs: source-ci\n', workflow)
        for forbidden in ('workflow_run:', 'contents: write', 'actions/cache', 'gh release', '**', '*.png'):
            self.assertNotIn(forbidden, workflow)

    def test_exact_test_requires_named_execution_not_substring(self):
        name = next(iter(q.EXACT_TESTS.values()))
        good = f'running 1 test\ntest {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 20 filtered out; finished in 0.00s\n'
        q.check_test_output(good, name)
        for bad in (good.replace(name, 'wrong'), good.replace('running 1 test', 'running 0 tests'),
                    good.replace('1 passed', '0 passed'), good + 'test result: ok. 1 passed; 0 failed;\n',
                    '1 passed; 0 failed'):
            with self.subTest(bad=bad), self.assertRaises(ValueError): q.check_test_output(bad, name)

    def test_every_private_command_has_an_exact_revalidation_specification(self):
        repo, private = Path('/synthetic/source'), Path('/synthetic/private')
        for phase in ('regressions', 'runtime'):
            specs = q.command_specifications(repo, private, phase, 'a' * 40)
            self.assertEqual(set(specs), set(q.expected_ids(phase)))
            self.assertTrue(all(isinstance(item, str) for command in specs.values() for item in command))
        plans = q.regression_plan()
        self.assertEqual({step['target'] for step in plans}, {'analytic', 'ordinary'})

    def test_compile_failure_requires_one_exact_diagnostic(self):
        messages = [{'reason': 'compiler-message', 'message': {'level': 'error', 'message': q.MIXED}},
                    {'reason': 'build-finished', 'success': False}]
        raw = '\n'.join(json.dumps(item) for item in messages).encode()
        q.check_rejection(raw, 101, q.MIXED)
        for code in (0, 1, -1):
            with self.assertRaises(ValueError): q.check_rejection(raw, code, q.MIXED)
        messages[0]['message']['message'] = 'disk exhausted'
        with self.assertRaises(ValueError): q.check_rejection('\n'.join(map(json.dumps, messages)).encode(), 101, q.MIXED)


class EvidenceBoundaryTests(unittest.TestCase):
    def test_false_means_literal_false_and_extra_fields_fail(self):
        q.validate_shape(summary())
        for key in ('release_authorized', 'appearance_accepted', 'native_runtime_qualified', 'distribution_qualified'):
            for value in (True, 0, 'false', None):
                bad = summary(); bad[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError): q.validate_shape(bad)
        bad = summary(); bad['private_path'] = '/secret'
        with self.assertRaises(ValueError): q.validate_shape(bad)

    def test_failed_phase_cannot_claim_pass_or_export_partial_screenshot(self):
        for key, value in (('checks', {'extracted_runtime_acceptance': 'observed_pass'}),
                           ('images', {'default-swift.png': {'width': 640, 'height': 360, 'sha256': 'a' * 64}})):
            bad = summary(); bad[key] = value
            with self.assertRaises(ValueError): q.validate_shape(bad)
        bad = summary(); bad['status'] = 'engineering_evidence_complete_reviews_required'
        with self.assertRaises(ValueError): q.validate_shape(bad)

    def test_command_status_cannot_be_relabelled_or_continue_after_timeout(self):
        one = {'id': 'rustc', 'outcome': 'succeeded', 'exit_code': 7, 'stdout': record(), 'stderr': record()}
        bad = summary(); bad['commands'] = [one]
        with self.assertRaises(ValueError): q.validate_shape(bad)
        one.update(outcome='timed_out', exit_code=-9)
        bad['commands'].append({'id': 'fetch', 'outcome': 'succeeded', 'exit_code': 0, 'stdout': record(), 'stderr': record()})
        with self.assertRaises(ValueError): q.validate_shape(bad)

    def test_real_small_subprocess_nonzero_exit_is_private_and_stops(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runner = q.Runner(root, root, os.environ.copy(), 'regressions')
            with self.assertRaises(ValueError):
                runner.run('tiny', [sys.executable, '-c', "import sys; print('private'); sys.exit(7)"], timeout=10)
            self.assertEqual(runner.commands[0]['exit_code'], 7)
            self.assertEqual((root / 'commands/tiny/stdout').read_text().strip(), 'private')

    @unittest.skipIf(sys.platform == 'win32', 'native production path is intentionally not launched by a unit test')
    def test_real_linux_entry_cannot_create_native_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = q.qualify(REPO, 'a' * 40, root / 'private', root / 'export', 'runtime')
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['commands'], [])
            self.assertEqual({p.name for p in (root / 'export').iterdir()}, {'qualification.json'})

    def test_export_rejects_extra_file_duplicate_keys_and_hardlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private = root / 'private'; private.mkdir(); export = root / 'export'; export.mkdir()
            value = summary(); q.write_json(private / 'result.json', value); q.write_json(export / q.EXPORT, value)
            q.validate_export(export, REPO, 'a' * 40, private)
            (export / 'extra.exe').write_bytes(b'MZ')
            with self.assertRaises(ValueError): q.validate_export(export, REPO, 'a' * 40, private)
            (export / 'extra.exe').unlink()
            raw = (export / q.EXPORT).read_bytes()
            (export / q.EXPORT).write_bytes(raw.replace(b'{', b'{"schema_version": 1,', 1))
            with self.assertRaises(ValueError): q.validate_export(export, REPO, 'a' * 40, private)
            (export / q.EXPORT).unlink(); os.link(private / 'result.json', export / q.EXPORT)
            with self.assertRaises(ValueError): q.validate_export(export, REPO, 'a' * 40, private)


class NativeProjectionTests(unittest.TestCase):
    def parse(self, data):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'synthetic.exe'; path.write_bytes(data)
            return q.native.pe_imports(path)

    def test_pe_normal_and_delay_imports_are_distinct_and_no_static_claim(self):
        value = self.parse(synthetic_pe())
        self.assertEqual(value['ordinary_imports'], ['kernel32.dll'])
        self.assertEqual(value['delay_imports'], ['user32.dll'])
        self.assertEqual(value['static_contributions'], 'not_established')
        self.assertEqual(self.parse(synthetic_pe(None, None))['delay_imports'], [])

    def test_pe_rejects_paths_unbounded_strings_bad_rvas_and_missing_terminator(self):
        for name in ('../kernel32.dll', 'C:/private.dll', 'secret\n.dll', 'a' * 128 + '.dll'):
            with self.subTest(name=name), self.assertRaises(ValueError): self.parse(synthetic_pe(name))
        for offset, value in ((0x98 + 120, 0xfffffff0), (0x200 + 12, 0x1700), (0x280, 0)):
            bad = synthetic_pe(); struct.pack_into('<I', bad, offset, value)
            with self.subTest(offset=offset), self.assertRaises(ValueError): self.parse(bad)
        bad = synthetic_pe(); struct.pack_into('<I', bad, 0x98 + 124, 20)
        with self.assertRaises(ValueError): self.parse(bad)

    def test_pe_rejects_ambiguous_virtual_sections_and_wrong_machine(self):
        bad = synthetic_pe(); struct.pack_into('<H', bad, 0x84, 0x14c)
        with self.assertRaises(ValueError): self.parse(bad)
        bad = synthetic_pe(); struct.pack_into('<H', bad, 0x86, 2)
        section = 0x98 + 240
        bad[section + 40:section + 80] = bad[section:section + 40]
        with self.assertRaises(ValueError): self.parse(bad)

    def test_relative_paths_tokens_and_features_have_no_private_payload_lane(self):
        for path in ('../secret', '/absolute', 'C:/secret', 'a\\b', 'a//b', 'a/./b', 'https://example.com'):
            with self.subTest(path=path), self.assertRaises(ValueError): q.native.relative(path)
        for feature in ('\nsecret', 'a' * 161):
            with self.assertRaises(ValueError): q.native.features([feature])
        with self.assertRaises(ValueError): q.native.features(['default', 'default'])

    def test_reviewed_header_spans_reject_drift_and_untracked_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); package = root / 'crate'; package.mkdir()
            data = b'header licence\nfn body() {}\n'; (package / 'lib.rs').write_bytes(data)
            packages = {'raw-private-id': {'name': 'example', 'version': '1.0.0', 'manifest_path': str(package / 'Cargo.toml')}}
            row = {'package_id': 'example@1.0.0', 'source_relative_path': 'lib.rs', 'source_sha256': hashlib.sha256(data).hexdigest(),
                   'start_byte': 0, 'byte_length': 14, 'excerpt_sha256': hashlib.sha256(data[:14]).hexdigest()}
            manifest = root / q.native.HEADER_MANIFEST; manifest.parent.mkdir(parents=True)
            q.write_json(manifest, {'schema_version': 1, 'records': [row]})
            source = {'files': [{'path': q.native.HEADER_MANIFEST}]}
            self.assertEqual(q.native.headers(root, packages, source)['records'], [row])
            with self.assertRaises(ValueError): q.native.headers(root, packages, {'files': []})
            (package / 'lib.rs').write_bytes(data + b'changed')
            with self.assertRaises(ValueError): q.native.headers(root, packages, source)

    def test_native_projection_retains_conservative_scope_without_exporting_private_paths(self):
        # Synthetic native packet inputs exercise projection, not capture authority.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); repo = root / 'secret-source'; repo.mkdir()
            build = root / 'secret-build'; directory = build / 'capture/analytic'; directory.mkdir(parents=True)
            text = root / 'build-text'; text.mkdir(); bundle = root / 'bundle'; bundle.mkdir()
            notice = directory / 'notices/licenses/flightsim-app-0.1.0/LICENSE-MIT'; notice.parent.mkdir(parents=True)
            notice.write_bytes(b'synthetic licence\n')
            (repo / 'Cargo.lock').write_bytes(b'synthetic lock')
            (text / q.capture.EXPORT_NAME).write_bytes(b'synthetic already-audited summary')
            executable = bundle / 'flightsim-app.exe'; executable.write_bytes(synthetic_pe())
            package_id = 'path+file://' + str(repo) + '#flightsim-app@0.1.0'
            package = {'id': package_id, 'name': 'flightsim-app', 'version': '0.1.0', 'manifest_path': str(repo / 'Cargo.toml')}
            selected = ['analytic-tonemapping', 'commercial-staging']
            metadata = {'packages': [package], 'workspace_members': [package_id],
                        'resolve': {'nodes': [{'id': package_id, 'features': selected, 'deps': []}]}}
            row = {'id': 'flightsim-app@0.1.0', 'source': 'workspace', 'source_checksum': None,
                   'source_revision': None, 'license_expression': 'MIT OR Apache-2.0',
                   'notices': [{'path': 'licenses/flightsim-app-0.1.0/LICENSE-MIT', 'upstream_path': 'LICENSE-MIT',
                                **q.record(notice)}]}
            q.write_json(directory / 'metadata.json', metadata)
            q.write_json(directory / 'notices/dependency-inventory.json', {'packages': [row], 'embedded_assets': []})
            (directory / 'graph.txt').write_text('flightsim-app v0.1.0 (' + str(repo) + ') features=[analytic-tonemapping,commercial-staging]\n')
            (directory / 'messages.jsonl').write_text(json.dumps({'reason': 'build-script-executed', 'package_id': package_id,
                                                                'linked_libs': ['static=native-example']}) + '\n')
            (directory / 'rustc.txt').write_text('rustc 1.93.0 (synthetic)\ncommit-hash: ' + 'b' * 40 + '\n')
            q.write_json(bundle / 'bundle-manifest.json', {'files': [{'path': 'flightsim-app.exe', **q.record(executable)}]})
            source = {'source_tree': 'c' * 40, 'files': []}
            verified = {'status': q.capture.PASS, 'builds': {'analytic': {'executable': q.record(executable)}}}
            with mock.patch.object(q.native.capture, 'validate_export', return_value=verified), \
                    mock.patch.object(q.native.capture, 'source_evidence', return_value=source):
                result = q.native.project(repo, 'a' * 40, build, text, bundle)
                encoded = json.dumps(result)
                self.assertNotIn(str(root), encoded)
                self.assertNotIn(package_id, encoded)
                self.assertEqual(result['actual_static_contributions'], 'not_established')
                self.assertIs(result['dependency_review_approved'], False)
                self.assertEqual(result['packages'][0]['exact_features'], selected)
                self.assertEqual(result['build_script_link_requests'][0]['linked_libs'], ['static=native-example'])
                self.assertEqual(result['runtime_coverage']['status'], 'not_established')
                (bundle / 'private.dll').write_bytes(synthetic_pe())
                with self.assertRaises(ValueError): q.native.project(repo, 'a' * 40, build, text, bundle)


class RuntimeFactsIntegrationTests(unittest.TestCase):
    def test_unavailable_ui_capability_does_not_discard_independent_facts(self):
        runner = mock.Mock(repo=Path('/source'), private=Path('/private'))
        events = []
        runner.run.side_effect = lambda *a, **kw: events.append(a[0])
        with mock.patch.object(q, 'validate_runtime_facts', side_effect=lambda *a: events.append('facts-validated')), \
             mock.patch.object(q.ui_capabilities, 'collect', side_effect=lambda *a: events.append('ui-unavailable') or {'probes': {'ocr': {'state': 'unavailable'}}}):
            q.collect_runtime_observations(runner, {'source_sha': 'a' * 40}, Path('/original-build'), Path('/original-text'))
        self.assertEqual(events, ['runtime-facts', 'facts-validated', 'ui-unavailable'])
        command = runner.run.call_args.args[1]
        self.assertIn(str(Path('/original-build') / 'capture/analytic/build.stderr'), command)
        self.assertIn(str(Path('/original-build') / 'target-analytic' / q.check.TARGET / 'release/flightsim-app.exe'), command)

    def test_partial_factual_exports_do_not_create_qualification(self):
        value = summary(); value['phase'] = 'runtime'
        value['runtime_facts'] = record(b'bounded factual json')
        value['ui_capabilities'] = record(b'bounded unavailable json')
        q.validate_shape(value)
        for key in ('release_authorized', 'appearance_accepted', 'native_runtime_qualified', 'distribution_qualified'):
            self.assertIs(value[key], False)
        value['status'] = 'engineering_evidence_complete_reviews_required'
        with self.assertRaises(ValueError): q.validate_shape(value)

    def test_runtime_facts_bind_exact_original_trace_executable_and_compiler(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); build = root / 'frozen-build'; text = root / 'text'; private = root / 'runtime-facts'
            (build / 'capture/analytic').mkdir(parents=True); private.mkdir()
            compiler = 'release: 1.93.0\ncommit-hash: ' + 'a' * 40 + '\nhost: x86_64-pc-windows-msvc\nLLVM version: 21.1.8\n'
            (build / 'capture/analytic/rustc.txt').write_text(compiler)
            facts = {'source_sha': 'b' * 40, 'target': q.check.TARGET,
                     'rust': {'identity': q.runtime_facts.compiler_identity(compiler)}}
            manifest = {'recipe_cfg_args': ['-D', 'warnings'], 'linker_inputs': {
                'trace': str(build / 'capture/analytic/build.stderr'),
                'executable': str(build / 'target-analytic' / q.check.TARGET / 'release/flightsim-app.exe')}}
            def write():
                q.write_json(private / q.runtime_facts.PRIVATE_NAME, manifest)
                (private / q.runtime_facts.PROJECTION_NAME).write_bytes(q.runtime_facts.canonical(facts))
            write()
            with mock.patch.object(q.native.capture, 'validate_export', return_value={'status': q.capture.PASS}), \
                 mock.patch.object(q.native.runtime_facts, 'project', return_value=facts):
                q.native.validate_runtime_facts(private, root, 'b' * 40, build, text)
                manifest['linker_inputs']['executable'] = str(root / 'other.exe'); write()
                with self.assertRaises(ValueError): q.native.validate_runtime_facts(private, root, 'b' * 40, build, text)
                manifest['linker_inputs']['executable'] = str(build / 'target-analytic' / q.check.TARGET / 'release/flightsim-app.exe')
                facts['rust']['identity']['commit'] = 'c' * 40; write()
                with self.assertRaises(ValueError): q.native.validate_runtime_facts(private, root, 'b' * 40, build, text)


if __name__ == '__main__':
    unittest.main()
