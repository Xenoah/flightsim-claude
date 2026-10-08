"""Adversarial failure evidence fixtures; no simulator or Windows qualification."""
import copy
import hashlib
import importlib.util
import json
import io
import math
from pathlib import Path
import struct
import tempfile
import unittest
import zlib
from unittest import mock

SPEC = importlib.util.spec_from_file_location('failure', Path(__file__).parents[1] / 'project-analytical-runtime-failure.py')
f = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(f)


def png():
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 640, 360, 8, 2, 0, 0, 0)) +
            chunk(b'IDAT', zlib.compress((b'\0' + b'\x80' * (640 * 3)) * 360)) + chunk(b'IEND', b''))


class FailureProjectionTests(unittest.TestCase):
    def fixture(self, root, *, code=1, outcome='failed'):
        private = root / 'private'; base = private / 'commands/default-swift'; base.mkdir(parents=True)
        (base / 'stdout').write_bytes(b'')
        raw = (b'INFO aircraft model fitted: private path C:\\private\\model.glb\n'
               b'ERROR wgpu error: Validation Error in Device::create_render_pipeline C:\\private\\shader.wgsl\n'
               b'With label tonemapping pipeline\n'
               b'thread "Render thread" panicked at C:\\private\\secret.rs:12:9\n'
               b'ERROR token=SECRET_VALUE arbitrary unmatched diagnostic\n')
        (base / 'stderr').write_bytes(raw)
        journal = {'command': ['C:\\private\\flightsim-app.exe'], 'cwd': 'C:\\private',
                   'outcome': outcome, 'exit_code': code, 'elapsed_seconds': 43.217, 'timeout_seconds': 180,
                   'stdout': f.capture.file_record(base / 'stdout'), 'stderr': f.capture.file_record(base / 'stderr')}
        f.capture.write_json(base / 'journal.json', journal)
        return private, base, journal

    def test_actual_exit_timing_markers_and_unknown_lines_remain_nonapproving(self):
        with tempfile.TemporaryDirectory() as temporary:
            private, base, _ = self.fixture(Path(temporary))
            value = f.project(private, 'a' * 40, 'default-swift')
            self.assertEqual((value['exit_code'], value['elapsed_ms'], value['supervisor_timeout_seconds']), (1, 43217, 180))
            self.assertEqual(value['png']['state'], 'absent')
            self.assertIn('gpu_validation', value['observed_message_classes'])
            self.assertIn('panic_marker', value['observed_message_classes'])
            self.assertEqual(value['root_cause'], 'not_established')
            encoded = f.canonical(value)
            for private_text in (b'SECRET_VALUE', b'C:', b'private', b'shader.wgsl', b'secret.rs', b'token='):
                self.assertNotIn(private_text, encoded)
            self.assertFalse(value['runtime_accepted']); self.assertFalse(value['release_authorized'])
            raw = (base / 'stderr').read_bytes()
            for row in value['streams']['stderr']['selected_lines']:
                piece = raw[row['start_byte']:row['start_byte'] + row['byte_length']]
                self.assertEqual(hashlib.sha256(piece).hexdigest(), row['sha256'])

    def test_valid_private_png_never_turns_exit_one_into_success_or_exports_pixels(self):
        with tempfile.TemporaryDirectory() as temporary:
            private, _, _ = self.fixture(Path(temporary)); data = png()
            (private / 'default-swift.png').write_bytes(data)
            value = f.project(private, 'a' * 40, 'default-swift')
            self.assertEqual(value['png']['state'], 'present_valid_png')
            self.assertEqual((value['png']['width'], value['png']['height']), (640, 360))
            self.assertEqual(value['png']['record']['sha256'], hashlib.sha256(data).hexdigest())
            self.assertFalse(value['runtime_accepted']); self.assertEqual(value['exit_code'], 1)
            self.assertNotIn('data', value['png'])

    def test_invalid_and_oversized_png_have_explicit_distinct_states(self):
        with tempfile.TemporaryDirectory() as temporary:
            private, _, _ = self.fixture(Path(temporary)); path = private / 'default-swift.png'
            path.write_bytes(b'not a PNG')
            self.assertEqual(f.project(private, 'a' * 40, 'default-swift')['png']['state'], 'present_invalid_png')
            with mock.patch.object(f, 'MAX_PNG', 2):
                self.assertEqual(f.project(private, 'a' * 40, 'default-swift')['png']['state'], 'present_exceeds_bound')
            self.assertEqual(f.png_observation(private, 'absent-light-single')['state'], 'not_requested')

    def test_supervisor_timeout_is_distinct_from_app_exit_one(self):
        with tempfile.TemporaryDirectory() as temporary:
            private, _, _ = self.fixture(Path(temporary), code=1, outcome='timed_out')
            self.assertEqual(f.project(private, 'a' * 40, 'default-swift')['outcome'], 'timed_out')

    def test_failure_export_rejects_resealed_message_payload_and_raw_tail_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private, base, _ = self.fixture(root); export = root / 'export'; export.mkdir()
            commands = [{'id': 'default-swift'}]; binding = f.save(private, 'a' * 40, commands)
            (export / f.EXPORT).write_bytes((private / f.EXPORT).read_bytes())
            f.verify_export(export, private, 'a' * 40, commands, binding)
            value = json.loads((private / f.EXPORT).read_bytes())
            value['streams']['stderr']['selected_lines'][0]['messages'] = ['PRIVATE_PAYLOAD']
            for path in (private / f.EXPORT, export / f.EXPORT): path.write_bytes(f.canonical(value))
            with self.assertRaises(ValueError):
                f.verify_export(export, private, 'a' * 40, commands, f.capture.file_record(export / f.EXPORT))
            (base / 'stderr').write_bytes((base / 'stderr').read_bytes() + b'ERROR trailing failure\n')
            with self.assertRaises(ValueError): f.project(private, 'a' * 40, 'default-swift')

    def test_unbounded_timing_and_nonliteral_status_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            private, base, journal = self.fixture(Path(temporary))
            for key, bad in [('elapsed_seconds', math.nan), ('elapsed_seconds', math.inf), ('elapsed_seconds', True),
                             ('timeout_seconds', 181), ('exit_code', 'SECRET'), ('outcome', {'private': 'SECRET'})]:
                value = copy.deepcopy(journal); value[key] = bad; f.capture.write_json(base / 'journal.json', value)
                with self.subTest(key=key, bad=bad), self.assertRaises(ValueError): f.project(private, 'a' * 40, 'default-swift')

    def test_selected_fragments_and_counts_are_bounded_without_raw_message_text(self):
        line = ('ERROR ' + ' '.join(fragment for _, fragment in f.SIGNATURES.values()) + '\n').encode()
        result = f.stream_observations(line * 1000)
        self.assertEqual(len(result['selected_lines']), 8)
        self.assertEqual(result['matching_lines'], 1000)
        self.assertEqual(result['omitted_matching_lines'], 992)
        self.assertTrue(all(count == 1000 for count in result['signature_counts'].values()))
        self.assertFalse(result['unknown_message_text_exported'])

    def test_large_lines_use_bounded_reads_and_explicit_incomplete_coverage(self):
        class BoundedReader(io.BytesIO):
            def readline(self, limit=-1):
                self.assert_limit(limit)
                return super().readline(limit)
            def assert_limit(self, limit):
                if limit != f.MAX_LINE + 1: raise AssertionError('unbounded read')
            def read(self, *args): raise AssertionError('whole-stream read forbidden')
        raw = b'ERROR ' + b'x' * (3 * f.MAX_LINE) + b'PRIVATE_TAIL\n'
        result = f.scan_stream(BoundedReader(raw))
        self.assertEqual(result['record']['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(result['oversized_lines'], 1)
        self.assertEqual(result['coverage'], 'oversized_line_prefixes_only')
        self.assertNotIn('PRIVATE_TAIL', json.dumps(result))
        self.assertEqual(result['selected_lines'][0]['byte_length'], len(raw))

    def test_structured_binding_failure_survives_noisy_middle_without_private_text(self):
        raw = (b'INFO aircraft model fitted: private\n' * 10 +
               b'Texture binding 2 expects dimension = D3, but given a view with dimension = D2\n' +
               b'ERROR private repeated error\n' * 10)
        result = f.stream_observations(raw)
        details = [row['detail'] for row in result['structured_details']]
        self.assertIn({'kind': 'texture_dimension_mismatch', 'binding': 2,
                       'expected_dimension': 'D3', 'actual_dimension': 'D2'}, details)
        self.assertNotIn('private', json.dumps(result))



if __name__ == '__main__': unittest.main()
