"""Source-shaped present failures and hostile privacy/boundary fixtures.

These tests exercise the projector only; they are not native runtime evidence.
"""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    'present_excerpt', Path(__file__).parents[1] / 'project-present-error-excerpt.py')
p = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(p)

HEADER = b'Error in Surface::present: Validation Error\n'
CAUSE_HEADER = HEADER + b'\nCaused by:\n'


def record(raw):
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)}


class PresentExcerptTests(unittest.TestCase):
    def project(self, raw):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'stderr'
            path.write_bytes(raw)
            return p.project_stream(path, expected_record=record(raw))

    def cause(self, text):
        return self.project(CAUSE_HEADER + b'  ' + text.encode() + b'\n')

    def test_exact_prior_fatal_literal_is_identified_by_independent_hash(self):
        self.assertEqual(len(HEADER), 44)
        self.assertEqual(hashlib.sha256(HEADER).hexdigest(),
                         '4cbeb207d73b1fb2f2a49d74979523ee282e8a364e8e2c99bd388fd6a3a8e18a')
        value = self.project(HEADER)
        self.assertEqual(value['fatal_header']['sha256'], p.FATAL_LINE_SHA256)
        self.assertEqual(value['cause_block']['state'], 'caused_by_header_missing')
        self.assertEqual(value['root_cause'], 'not_established')
        self.assertFalse(value['runtime_accepted'])
        self.assertFalse(value['release_authorized'])

    def test_every_locked_surface_and_device_static_display_form(self):
        fixtures = {
            'Surface is invalid': 'SurfaceError::Invalid',
            'Surface is not configured for presentation': 'SurfaceError::NotConfigured',
            'Surface image is already acquired': 'SurfaceError::AlreadyAcquired',
            'Texture has been destroyed': 'SurfaceError::TextureDestroyed',
            'Parent device is lost': 'DeviceError::Lost',
            'Not enough memory left.': 'DeviceError::OutOfMemory',
            'Out of memory': 'hal::DeviceError::OutOfMemory',
            'Device is lost': 'hal::DeviceError::Lost',
            'Unexpected error variant (driver implementation is at fault)': 'hal::DeviceError::Unexpected',
            'Surface is lost': 'hal::SurfaceError::Lost',
            'Surface is outdated, needs to be re-created': 'hal::SurfaceError::Outdated',
        }
        for text, variant in fixtures.items():
            with self.subTest(text=text):
                value = self.cause(text)
                line = value['cause_block']['lines'][0]
                self.assertEqual(line['text'], text)
                self.assertEqual(line['source_variant'], variant)
                self.assertFalse(line['redacted'])
                self.assertFalse(line['truncated'])

    def test_nested_error_tree_retains_individual_source_forms(self):
        raw = CAUSE_HEADER + b'  Parent device is lost\n    Device is lost\n'
        value = self.project(raw)
        self.assertEqual([line['source_variant'] for line in value['cause_block']['lines']],
                         ['DeviceError::Lost', 'hal::DeviceError::Lost'])

    def test_resource_label_bearing_blocks_are_conservatively_omitted(self):
        fixtures = (
            "Device with 'Ada's private device' label of Texture with 'private-texture' label doesn't match Device with 'target-secret' label",
            "Device with 'private label without terminator",
            "Device with 'private' label tailsecret' label of Texture with 'thing' label doesn't match Device with 'other' label",
            "Device with 'safe' label PRIVATE UNQUOTED ' label of Texture with 'texture' label doesn't match Device with 'other' label",
            "Device with 'private-first-line\n  Private Person purchase history\n  Parent device is lost\n  closing-label' label of Texture with 'thing' label doesn't match Device with 'other' label",
        )
        for text in fixtures:
            with self.subTest(text=text):
                value = self.cause(text)
                self.assertEqual(value['cause_block']['lines'], [])
                self.assertEqual(value['cause_block']['stop_reason'], 'ambiguous_resource_label')
                self.assertTrue(value['cause_block']['truncated'])
                encoded = json.dumps(value)
                for secret in ('private-first', 'purchase history', 'tailsecret', 'PRIVATE UNQUOTED', 'DeviceError::Lost', 'DeviceError::DeviceMismatch'):
                    self.assertNotIn(secret, encoded)

    def test_label_injection_before_header_cannot_forge_a_cause_block(self):
        for prefix in (b'ERROR wgpu_hal::auxil::dxgi::result: Present failed: ',
                       b'INFO wgpu_core::device: ', b'ERROR unrecognized::target: ', b''):
            raw = (prefix + b"Device with 'private-label\n" + CAUSE_HEADER +
                   b'  Private Person purchase history\n  Parent device is lost\n')
            with self.subTest(prefix=prefix):
                value = self.project(raw)
                self.assertIsNone(value['fatal_header'])
                self.assertEqual(value['fatal_header_count'], 1)
                self.assertEqual(value['omitted_fatal_blocks'], 1)
                self.assertEqual(value['error_lines'], [])
                self.assertEqual(value['cause_block']['lines'], [])
                self.assertEqual(value['cause_block']['stop_reason'], 'ambiguous_resource_label_before_header')
                self.assertNotIn('purchase history', json.dumps(value))
                self.assertNotIn('DeviceError::Lost', json.dumps(value))

    def test_path_punctuation_and_relative_paths_cannot_leak_tail_content(self):
        paths = (
            r'C:\Users\Private Person (work)\sensitive\source.rs',
            r'C:\Users\Private,Person\sensitive\source.rs',
            r'C:\Users\Private;Person\sensitive\source.rs',
            r'.\private\source.rs', r'..\private\source.rs',
            r'C:private\source.rs', r'private\source.rs',
            '/home/Private Person (work),folder/sensitive/source.rs',
            './private/source.rs', '../private/source.rs', 'private/source.rs',
            r'Private Person sensitive folder\source.rs',
            'Private Person sensitive folder/source.rs',
        )
        for path in paths:
            with self.subTest(path=path):
                value = self.cause('Surface failure at ' + path)
                text = value['cause_block']['lines'][0]['text']
                self.assertEqual(text, '[redacted-path-bearing-line]')
                for secret in ('Private', 'sensitive', 'source.rs', 'private'):
                    self.assertNotIn(secret, text)

    def test_dx12_present_error_preceding_panic_survives_with_hresult(self):
        raw = (b'2026-10-08T14:12:01.120Z ERROR wgpu_hal::auxil::dxgi::result: '
               b'Present failed: DXGI_ERROR_DEVICE_REMOVED (0x887A0005)\n'
               b'\nthread \'Render thread\' panicked at C:\\private\\source.rs:77:5:\n' +
               CAUSE_HEADER + b'  Parent device is lost\n')
        value = self.project(raw)
        self.assertEqual(len(value['error_lines']), 1)
        self.assertEqual(value['error_lines'][0]['text'],
                         'ERROR wgpu_hal::auxil::dxgi::result: Present failed: DXGI_ERROR_DEVICE_REMOVED (0x887A0005)')
        self.assertNotIn('private', json.dumps(value))
        self.assertNotIn('2026-10', json.dumps(value))

    def test_windows_hresult_display_and_bounded_numeric_forms(self):
        for text in (
            'Present failed: The GPU device instance has been suspended. (0x887A0005)',
            'Present failed: 0x8007000E',
            'Present failed: E_OUTOFMEMORY HRESULT(-2147024882)',
            'Present failed: DXGI_ERROR_DEVICE_RESET (0x887A0007)',
            'Present failed: DXGI_ERROR_DRIVER_INTERNAL_ERROR.',
            'Present failed: code=4294967295',
        ):
            with self.subTest(text=text):
                value = self.project(('ERROR wgpu_hal::auxil::dxgi::result: ' + text + '\n').encode() +
                                     CAUSE_HEADER + b'  Parent device is lost\n')
                self.assertEqual(value['error_lines'][0]['text'], 'ERROR wgpu_hal::auxil::dxgi::result: ' + text)
        for value in ('4294967296', '-2147483649', '0x100000000', '12345678901234567890'):
            with self.subTest(value=value):
                projected = self.cause('Present failed: HRESULT(' + value + ')')
                self.assertNotIn(value, projected['cause_block']['lines'][0]['text'])

    def test_unknown_cause_exports_sanitized_text_and_honest_variant(self):
        text = 'Surface acquisition cannot be presented before submission; code 7'
        value = self.cause(text)
        self.assertEqual(value['cause_block']['unrecognized_lines'], 1)
        self.assertEqual(value['cause_block']['lines'][0]['text'], text)
        self.assertEqual(value['cause_block']['lines'][0]['source_variant'], 'unrecognized')
        self.assertEqual(value['root_cause'], 'not_established')

    def test_redacts_absolute_paths_urls_and_quoted_strings(self):
        samples = (
            (r'C:\Users\Secret Person\source.rs', ('Secret Person', 'source.rs', 'C:')),
            (r'\\private-server\private-share\source.rs', ('private-server', 'private-share')),
            ('/home/private-user/source.rs', ('private-user', 'source.rs')),
            ('https://private.example/path?token=private-value', ('private.example', 'private-value')),
            ('file:///home/private-user/source.rs', ('private-user', 'source.rs')),
            ('www.private.example/sensitive', ('private.example', 'sensitive')),
            ('"a private resource label"', ('private resource',)),
            ("'a private resource label'", ('private resource',)),
        )
        for text, secrets in samples:
            with self.subTest(text=text):
                line = self.cause('Surface presentation failed at ' + text)['cause_block']['lines'][0]
                if text.startswith(('C:', '\\\\', '/')):
                    self.assertEqual(line['text'], '[redacted-path-bearing-line]')
                else:
                    self.assertIn('Surface presentation failed', line['text'])
                self.assertTrue(line['redacted'])
                for secret in secrets:
                    self.assertNotIn(secret, line['text'])

    def test_redacts_credentials_jwt_api_keys_and_token_like_values(self):
        samples = (
            ('password=myprivatepassword', 'myprivatepassword'),
            ('api_key: private-api-value', 'private-api-value'),
            ('Authorization: Bearer PRIVATE_AUTH_VALUE', 'PRIVATE_AUTH_VALUE'),
            ('client_secret="private value with spaces"', 'private value'),
            ('refresh-token=private-refresh-value', 'private-refresh-value'),
            ('GITHUB_TOKEN: tiny-secret', 'tiny-secret'),
            ('OPENAI_API_KEY: tiny-secret', 'tiny-secret'),
            ('AWS_SECRET_ACCESS_KEY: tiny-secret', 'tiny-secret'),
            ('cookie=session-private-value', 'session-private-value'),
            ('Bearer aBcDeFg0123456789qwerty', 'aBcDeFg0123456789qwerty'),
            ('Basic cHJpdmF0ZTpzZWNyZXQ=', 'cHJpdmF0ZTpzZWNyZXQ='),
            ('eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJwcml2YXRlIn0.signature123456789', 'eyJhbGci'),
            ('sk-proj-abcdefghijklmnopqrstuv1234', 'sk-proj-'),
            ('ghp_abcdefghijklmnopqrstuv1234', 'ghp_'),
            ('github_pat_abcdefghijklmnopqrstuv1234', 'github_pat_'),
            ('AKIA1234567890ABCDEF', 'AKIA123'),
            ('xoxb-1234567890-abcdefghijk', 'xoxb-'),
            ('aBcD0eF1gH2iJ3kL4mN5', 'aBcD0eF1'),
            ('qwertyAsDfGHjKLzXcvBnm', 'qwertyAsDfGH'),
            ('q' * 40, 'q' * 24),
            ('abcdefghijklmnoqrstuvwx', 'abcdefghijklmnoqrstuvwx'),
            ('aB!cD@eF#gH$iJ%kL&mN', 'aB!cD@eF#gH$iJ%kL&mN'),
        )
        for text, secret in samples:
            with self.subTest(text=text):
                line = self.cause('Surface presentation failed; ' + text)['cause_block']['lines'][0]
                self.assertNotIn(secret, line['text'])
                self.assertIn('[redacted-', line['text'])

    def test_ansi_and_format_controls_cannot_split_secret_markers(self):
        text = 'Surface presentation failed; to\x1b[31mken\x1b[0m=private-ansi-value'
        value = self.cause(text)
        self.assertNotIn('private-ansi-value', json.dumps(value))
        value = self.cause('Surface presentation failed; api_\u200bkey=private-zero-width-value')
        self.assertNotIn('private-zero-width-value', json.dumps(value))

    def test_exact_headers_only_and_no_free_standing_causes(self):
        for raw in (
            b'Caused by:\n  Parent device is lost\n',
            b'Error in Device::create_texture: Validation Error\n\nCaused by:\n  Parent device is lost\n',
            b' ' + CAUSE_HEADER + b'  Parent device is lost\n',
            b'ERROR ' + CAUSE_HEADER + b'  Parent device is lost\n',
            b'\x1b[31m' + CAUSE_HEADER + b'  Parent device is lost\n',
            HEADER[:-1] + b' extra\n\nCaused by:\n  Parent device is lost\n',
            HEADER[:-1] + b'\r\r\n\nCaused by:\n  Parent device is lost\n',
        ):
            with self.subTest(raw=raw):
                value = self.project(raw)
                self.assertIsNone(value['fatal_header'])
                self.assertEqual(value['cause_block']['lines'], [])

    def test_crlf_and_final_line_without_newline(self):
        raw = CAUSE_HEADER.replace(b'\n', b'\r\n') + b'  Texture has been destroyed'
        value = self.project(raw)
        self.assertEqual(value['cause_block']['lines'][0]['text'], 'Texture has been destroyed')
        self.assertEqual(value['record'], record(raw))

    def test_bounded_blank_handling_around_caused_by(self):
        for blanks in (0, 1, 2):
            value = self.project(HEADER + b'\n' * blanks + b'Caused by:\n  Parent device is lost\n')
            self.assertEqual(len(value['cause_block']['lines']), 1)
        value = self.project(HEADER + b'\n' * 3 + b'Caused by:\n  Parent device is lost\n')
        self.assertEqual(value['cause_block']['stop_reason'], 'header_blank_limit')
        self.assertTrue(value['cause_block']['truncated'])
        value = self.project(CAUSE_HEADER + b'\n  Parent device is lost\n')
        self.assertEqual(value['cause_block']['lines'], [])

    def test_rejects_interleaved_and_nonmatching_caused_by_headers(self):
        for gap in (b'INFO unrelated private activity\n', b' Caused by:\n', b'Caused by: extra\n',
                    b'Caused by:\x1b[0m\n'):
            value = self.project(HEADER + b'\n' + gap + b'Caused by:\n  Parent device is lost\n')
            self.assertEqual(value['cause_block']['lines'], [])
            self.assertEqual(value['cause_block']['stop_reason'], 'nonmatching_caused_by_header')

    def test_stops_at_interleaved_logs_environment_and_stack_locations(self):
        for boundary in (
            b'  INFO private environment is shown here\n',
            b'  [INFO] private environment is shown here\n',
            b'  2026-10-08T14:12:01Z ERROR arbitrary: private value\n',
            b'  PATH=/home/private-env\n',
            b'  export CUSTOM_PRIVATE=secret-env\n',
            b'  Environment: private-value\n',
            b'  {"private-key": "private-value"}\n',
            b'  stack backtrace:\n',
            b'  0: private::stack::frame\n',
            b'    at C:\\private\\source.rs:15:5\n',
            b'  C:\\private\\source.rs:15:5\n',
            b'  /private/source.rs:15:5\n',
            b'  crates/private/source.rs:15:5\n',
            b'  thread \'private-thread\' panicked at /private/path\n',
            b'plain private unrelated prose\n',
            b'\n',
        ):
            with self.subTest(boundary=boundary):
                value = self.project(CAUSE_HEADER + b'  Surface is invalid\n' + boundary + b'  Parent device is lost\n')
                self.assertEqual(len(value['cause_block']['lines']), 1)
                self.assertNotIn('private-', json.dumps(value))
                self.assertNotIn('Parent device is lost', json.dumps(value))

    def test_requires_formatter_shaped_indentation(self):
        for indent in (b'', b' ', b'   ', b' ' * 18, b'\t', b'  \t'):
            with self.subTest(indent=indent):
                value = self.project(CAUSE_HEADER + indent + b'Parent device is lost\n')
                self.assertEqual(value['cause_block']['lines'], [])
        self.assertEqual(len(self.project(CAUSE_HEADER + b' ' * 16 + b'Parent device is lost\n')['cause_block']['lines']), 1)

    def test_error_selection_requires_nearby_fatal_and_relevant_subject(self):
        error = b'ERROR wgpu_hal::auxil::dxgi::result: Present failed: 0x887A0005\n'
        for raw in (
            error,
            error + b'INFO unrelated private event\n' + CAUSE_HEADER,
            error + b'\n' * 9 + CAUSE_HEADER,
            error + b'thread \'private\' panicked at ' + b'x' * 4096 + b'\n' + CAUSE_HEADER,
            b'ERROR unrelated::module: Present failed: private\n' + CAUSE_HEADER,
            b'ERROR wgpu_core::device: private environment diagnostic\n' + CAUSE_HEADER,
        ):
            with self.subTest(raw=raw[:100]):
                self.assertEqual(self.project(raw)['error_lines'], [])

    def test_four_error_line_cap_has_explicit_omission_count(self):
        raw = b''.join(('ERROR Present failed: code ' + str(number) + '\n').encode() for number in range(6)) + CAUSE_HEADER
        value = self.project(raw)
        self.assertEqual(len(value['error_lines']), 4)
        self.assertEqual(value['omitted_nearby_error_lines'], 2)
        self.assertTrue(value['error_lines'][0]['text'].endswith('code 2'))

    def test_error_bytes_are_capped_after_redaction_not_before(self):
        raw = ('ERROR Present failed: ' + 'é' * 100 + ' password=private-long-secret ' + 'z' * 1000 + '\n').encode() + CAUSE_HEADER
        value = self.project(raw)
        line = value['error_lines'][0]
        self.assertTrue(line['truncated'])
        self.assertLessEqual(len(json.dumps(line['text']).encode()), 512)
        self.assertNotIn('private-long-secret', json.dumps(value))

    def test_cause_line_limit_and_extra_headers_are_explicit(self):
        value = self.project(CAUSE_HEADER + b'  Surface is invalid\n' * 9)
        self.assertEqual(len(value['cause_block']['lines']), 8)
        self.assertTrue(value['cause_block']['truncated'])
        self.assertEqual(value['cause_block']['stop_reason'], 'cause_line_limit')
        value = self.project(CAUSE_HEADER + b'  Surface is invalid\n' + CAUSE_HEADER + b'  Parent device is lost\n')
        self.assertEqual(value['fatal_header_count'], 2)
        self.assertEqual(value['omitted_fatal_blocks'], 1)
        self.assertEqual(len(value['cause_block']['lines']), 1)
        self.assertNotIn('Parent device is lost', json.dumps(value))

    def test_cause_source_byte_limit_never_spills_into_following_text(self):
        value = self.project(CAUSE_HEADER + b'  Surface is invalid\n' + b'  ' + b'x' * 2048 + b'\n  PRIVATE_AFTER_LIMIT\n')
        self.assertEqual(len(value['cause_block']['lines']), 1)
        self.assertEqual(value['cause_block']['stop_reason'], 'cause_byte_limit')
        self.assertTrue(value['cause_block']['truncated'])
        self.assertNotIn('PRIVATE_AFTER_LIMIT', json.dumps(value))

    def test_json_escaping_and_total_export_are_bounded(self):
        raw = (('ERROR Present failed: ' + 'é' * 240 + '\n').encode() * 4 + CAUSE_HEADER +
               ('  ' + 'é' * 125 + '\n').encode() * 8)
        value = self.project(raw)
        self.assertLessEqual(len(p.canonical(value)), 16 * 1024)
        self.assertLessEqual(sum(len(json.dumps(line['text']).encode()) for line in value['cause_block']['lines']), 2048)
        self.assertTrue(value['cause_block']['truncated'])

    def test_streaming_hash_includes_omitted_oversized_lines_and_tail(self):
        class BoundedReader(io.BytesIO):
            def readline(self, limit=-1):
                if limit != 65536:
                    raise AssertionError('readline must be bounded to 64 KiB')
                return super().readline(limit)

            def read(self, *args):
                raise AssertionError('whole stream read forbidden')

        raw = (b'private noise ' + b'x' * (p.MAX_LINE * 3) + b'\n' + CAUSE_HEADER +
               b'  Texture has been destroyed\n\nPRIVATE_TAIL\n')
        value = p._scan(BoundedReader(raw))
        self.assertEqual(value['record'], record(raw))
        self.assertEqual(value['oversized_lines'], 1)
        self.assertNotIn('PRIVATE_TAIL', json.dumps(value))
        self.assertNotIn('private noise', json.dumps(value))

    def test_oversized_cause_line_is_omitted_and_stops_block(self):
        raw = CAUSE_HEADER + b'  Surface is invalid ' + b'x' * p.MAX_LINE + b'\n  Parent device is lost\n'
        value = self.project(raw)
        self.assertEqual(value['cause_block']['lines'], [])
        self.assertEqual(value['cause_block']['stop_reason'], 'oversized_line')
        self.assertTrue(value['cause_block']['truncated'])
        self.assertEqual(value['record'], record(raw))

    def test_selected_byte_spans_and_hashes_bind_original_not_redacted_text(self):
        raw = (b'ERROR Present failed: code 7 token=private-secret\n' + CAUSE_HEADER +
               b'  Surface is invalid\n    unknown technical cause /private/path\n')
        value = self.project(raw)
        for row in [value['fatal_header'], *value['error_lines'], *value['cause_block']['lines']]:
            piece = raw[row['start_byte']:row['start_byte'] + row['byte_length']]
            self.assertEqual(row['sha256'], hashlib.sha256(piece).hexdigest())
        self.assertEqual(value['record'], record(raw))

    def test_full_record_required_and_same_length_mutations_rejected(self):
        raw = CAUSE_HEADER + b'  Surface is invalid\n\nprivate-A\n'
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'stderr'
            path.write_bytes(raw)
            for bad in (None, {}, {'sha256': 'a' * 64, 'bytes': len(raw)},
                        {'sha256': record(raw)['sha256'], 'bytes': True},
                        dict(record(raw), private='extra'),
                        {'sha256': record(raw)['sha256'].upper(), 'bytes': len(raw)}):
                with self.subTest(bad=bad), self.assertRaises(ValueError):
                    p.project_stream(path, expected_record=bad)
            path.write_bytes(raw.replace(b'private-A', b'private-B'))
            with self.assertRaisesRegex(ValueError, 'differs from expected record'):
                p.project_stream(path, expected_record=record(raw))

    def test_empty_stream_is_bound_and_not_misreported_as_fatal(self):
        value = self.project(b'')
        self.assertEqual(value['record'], record(b''))
        self.assertEqual(value['fatal_header_count'], 0)
        self.assertEqual(value['cause_block']['state'], 'fatal_header_absent')

    def test_stream_size_bound_applies_before_read_and_during_growth(self):
        with mock.patch.object(p, 'MAX_STREAM', 128):
            with self.assertRaises(ValueError):
                self.project(b'x' * 129)
            with self.assertRaisesRegex(ValueError, 'exceeds byte limit'):
                p._scan(io.BytesIO(b'x' * 129))

    def test_links_are_not_accepted_as_private_stream_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = root / 'stderr'
            original.write_bytes(HEADER)
            symlink = root / 'symlink'
            symlink.symlink_to(original)
            with self.assertRaisesRegex(ValueError, 'linked runtime stream'):
                p.project_stream(symlink, expected_record=record(HEADER))
            hardlink = root / 'hardlink'
            os.link(original, hardlink)
            with self.assertRaisesRegex(ValueError, 'invalid bounded runtime stream'):
                p.project_stream(hardlink, expected_record=record(HEADER))

    def test_windows_reparse_file_and_ancestor_rejected_without_following(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / 'stderr'
            path.write_bytes(HEADER)
            real_lstat = Path.lstat
            for reparse_path in (root, path):
                inspected = []

                def inspect(part):
                    inspected.append(part)
                    details = real_lstat(part)
                    if part == reparse_path:
                        return SimpleNamespace(st_mode=details.st_mode, st_file_attributes=0x400)
                    return details

                with self.subTest(reparse_path=reparse_path), \
                        mock.patch.object(Path, 'lstat', autospec=True, side_effect=inspect), \
                        mock.patch.object(Path, 'open', side_effect=AssertionError('reparse path opened')), \
                        self.assertRaisesRegex(ValueError, 'reparse point rejected'):
                    p.project_stream(path, expected_record=record(HEADER))
                self.assertEqual(inspected[-1], reparse_path)
                if reparse_path == root:
                    self.assertNotIn(path, inspected)


if __name__ == '__main__':
    unittest.main()
