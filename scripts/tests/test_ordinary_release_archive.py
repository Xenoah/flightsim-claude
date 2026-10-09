"""Synthetic ordinary byte/smoke boundaries; never run FlightSim or grant approval."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import unittest
import warnings
from unittest.mock import patch
import zipfile
import zlib


SCRIPT = Path(__file__).resolve().parents[1] / 'verify-ordinary-release-archive.py'
SPEC = importlib.util.spec_from_file_location('ordinary_release_archive_tests', SCRIPT)
q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(q)
DATA = b'SYNTHETIC UNIT-TEST BYTES, NOT A NATIVE EXECUTABLE'
TOP = q.archive_top_level('0.6.0-alpha.99')


def blob(data=DATA):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def png(*, pixels=None, width=640, height=360, extra=b''):
    def chunk(tag, data):
        return struct.pack('!I', len(data)) + tag + data + struct.pack('!I', zlib.crc32(tag + data))
    pixels = pixels if pixels is not None else (b'\0' + b'\x22\x66\xaa' * width) * height
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(pixels) + extra) + chunk(b'IEND', b''))


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.bundle = self.root / 'staged'; self.bundle.mkdir()
        (self.bundle / 'flightsim-app.exe').write_bytes(DATA)
        (self.bundle / 'assets').mkdir()
        (self.bundle / 'assets/model.glb').write_bytes(b'SYNTHETIC MODEL DATA')
        self.archive = self.root / (TOP + '.zip')
        self.files = {'flightsim-app.exe': blob()}

    def zipped(self, data=DATA, *, compression=zipfile.ZIP_DEFLATED, info=None):
        with zipfile.ZipFile(self.archive, 'w', compression=compression) as zipped:
            zipped.writestr(info or TOP + '/flightsim-app.exe', data)
        return {'flightsim-app.exe': blob(data)}

    def test_create_audit_fresh_extract_preserve_exact_staged_bytes(self):
        files = q.create_archive(self.bundle, self.archive, TOP)
        self.assertEqual(files, q.adapter.snapshot_payload(self.bundle))
        binding = q.audit_archive(self.archive, files, TOP)
        extracted = q.extract_verified_archive(self.archive, self.root / 'extracted', files, TOP)
        self.assertEqual(extracted, self.root / 'extracted' / TOP)
        self.assertEqual(q.adapter.snapshot_payload(extracted), files)
        self.assertEqual(q.audit_archive(self.archive, files, TOP), binding)
        self.assertEqual({p.name for p in self.root.iterdir()}, {'staged', TOP + '.zip', 'extracted'})

    def test_existing_outputs_and_overlap_rejected_without_overwrite(self):
        files = q.create_archive(self.bundle, self.archive, TOP)
        before = self.archive.read_bytes()
        with self.assertRaises(ValueError): q.create_archive(self.bundle, self.archive, TOP)
        self.assertEqual(self.archive.read_bytes(), before)
        with self.assertRaises(ValueError): q.create_archive(self.bundle, self.bundle / (TOP + '.zip'), TOP)
        target = self.root / 'already'; target.mkdir()
        with self.assertRaises(ValueError): q.extract_verified_archive(self.archive, target, files, TOP)
        self.assertFalse(list(target.iterdir()))

    def test_archive_filename_and_top_level_are_fixed_version_shape(self):
        self.assertEqual(q.archive_top_level('1.2.3'), 'flightsim-claude-v1.2.3-windows-x86_64')
        for bad in ('../secret', 'v1.2.3', '1.2', '', True, '1.2.3/other'):
            with self.subTest(bad=bad), self.assertRaises(ValueError): q.archive_top_level(bad)
        for bad in ('candidate', TOP + '/extra', TOP + '.zip'):
            with self.subTest(bad=bad), self.assertRaises(ValueError): q.validate_top_level(bad)
        with self.assertRaises(ValueError): q.create_archive(self.bundle, self.root / 'candidate.zip', TOP)

    def test_large_deflate_and_empty_member_boundaries(self):
        for length in (0, 1048576, 1048577, 2097153):
            with self.subTest(length=length):
                files = self.zipped(b'A' * length)
                q.audit_archive(self.archive, files, TOP)
        q.audit_archive(self.archive, self.zipped(compression=zipfile.ZIP_STORED), TOP)

    def test_hidden_deflate_tail_rejected_even_if_zipfile_ignores_it(self):
        self.zipped()
        with zipfile.ZipFile(self.archive) as zipped:
            central, compressed = zipped.start_dir, zipped.infolist()[0].compress_size
        hidden = b'HIDDEN-UNAPPROVED-PAYLOAD'
        raw = bytearray(self.archive.read_bytes()); raw[central:central] = hidden
        struct.pack_into('<I', raw, 18, compressed + len(hidden))
        struct.pack_into('<I', raw, central + len(hidden) + 20, compressed + len(hidden))
        struct.pack_into('<I', raw, len(raw) - 22 + 16, central + len(hidden))
        self.archive.write_bytes(raw)
        with zipfile.ZipFile(self.archive) as zipped:
            self.assertEqual(zipped.read(TOP + '/flightsim-app.exe'), DATA)
        with self.assertRaisesRegex(ValueError, 'hidden bytes'):
            q.audit_archive(self.archive, self.files, TOP)

    def test_underreported_inflated_size_and_incomplete_deflate_rejected(self):
        self.zipped(b'A' * 10000)
        with zipfile.ZipFile(self.archive) as zipped:
            central = zipped.start_dir
        raw = bytearray(self.archive.read_bytes())
        struct.pack_into('<I', raw, 22, 1); struct.pack_into('<I', raw, central + 24, 1)
        self.archive.write_bytes(raw)
        with self.assertRaisesRegex(ValueError, 'exceeded frozen length'):
            q.audit_archive(self.archive, {'flightsim-app.exe': blob(b'A')}, TOP)
        self.zipped()
        with zipfile.ZipFile(self.archive) as zipped:
            central, compressed = zipped.start_dir, zipped.infolist()[0].compress_size
        raw = bytearray(self.archive.read_bytes()); del raw[central - 1]
        struct.pack_into('<I', raw, 18, compressed - 1)
        struct.pack_into('<I', raw, central - 1 + 20, compressed - 1)
        struct.pack_into('<I', raw, len(raw) - 22 + 16, central - 1)
        self.archive.write_bytes(raw)
        with self.assertRaises((ValueError, zlib.error)):
            q.audit_archive(self.archive, self.files, TOP)

    def test_extraction_budget_survives_archive_change_after_audit(self):
        self.zipped()
        audit = q.audit_archive
        def changed_after_audit(*args):
            binding = audit(*args)
            self.zipped(b'A' * 10000)
            return binding
        with patch.object(q, 'audit_archive', side_effect=changed_after_audit):
            with self.assertRaisesRegex(ValueError, 'extraction exceeded'):
                q.extract_verified_archive(self.archive, self.root / 'extracted', self.files, TOP)
        self.assertLessEqual((self.root / 'extracted' / TOP / 'flightsim-app.exe').stat().st_size, len(DATA))

    def test_prefix_trailer_archive_comment_and_member_extra_rejected(self):
        for prefix, suffix in ((b'prefix', b''), (b'', b'trailer')):
            self.zipped(); self.archive.write_bytes(prefix + self.archive.read_bytes() + suffix)
            with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)
        self.zipped()
        with zipfile.ZipFile(self.archive, 'a') as zipped: zipped.comment = b'private hidden comment'
        with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)
        for field, value in (('extra', b'\xfe\xca\x04\x00hide'), ('comment', b'hidden')):
            info = zipfile.ZipInfo(TOP + '/flightsim-app.exe'); setattr(info, field, value)
            self.zipped(info=info)
            with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)

    def test_changed_bytes_crc_and_local_central_mismatch_rejected(self):
        self.zipped(b'X' * len(DATA))
        with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)
        for offset in (6, 8, 14, 18, 22, 26, 28, 30):
            self.zipped(); raw = bytearray(self.archive.read_bytes()); raw[offset] ^= 1
            self.archive.write_bytes(raw)
            with self.subTest(offset=offset), self.assertRaises((ValueError, zipfile.BadZipFile, zlib.error)):
                q.audit_archive(self.archive, self.files, TOP)

    def test_links_directories_unsupported_compression_and_extra_member_rejected(self):
        info = zipfile.ZipInfo(TOP + '/flightsim-app.exe')
        info.create_system = 3; info.external_attr = (stat.S_IFLNK | 0o777) << 16
        self.zipped(info=info)
        with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)
        self.zipped(compression=zipfile.ZIP_BZIP2)
        with self.assertRaises(ValueError): q.audit_archive(self.archive, self.files, TOP)
        for extra in (TOP + '/extra.log', TOP + '/unused/', TOP + '/flightsim-app.exe'):
            self.zipped()
            with warnings.catch_warnings():
                warnings.simplefilter('ignore', UserWarning)
                with zipfile.ZipFile(self.archive, 'a') as zipped: zipped.writestr(extra, b'')
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                q.audit_archive(self.archive, self.files, TOP)

    def test_bounds_and_windows_aliases_rejected(self):
        self.zipped()
        for limit in ('MAX_ARCHIVE', 'MAX_UNPACKED', 'MAX_MEMBERS'):
            with patch.object(q, limit, 0), self.assertRaises(ValueError):
                q.audit_archive(self.archive, self.files, TOP)
        for name in ('../secret', '/abs', 'C:relative', 'a\\b', 'a//b', 'a/./b', 'NUL',
                     'COM1.log', 'COM\xb9.log', 'trailing.', 'trailing ', 'a?b', 'a<b', 'a\x00b'):
            with self.subTest(name=name), self.assertRaises(ValueError): q.relative(name)
        for files in ({'A': blob(), 'a': blob()}, {'a': blob(), 'a/b': blob()},
                      {'A/x': blob(), 'a/y': blob()}, {'a': {'sha256': '0' * 64, 'bytes': True}}):
            with self.subTest(files=files), self.assertRaises(ValueError): q.validate_files(files)

    def test_staged_symlink_hardlink_empty_dir_and_case_collision_rejected(self):
        for kind in ('symlink', 'hardlink', 'empty', 'case'):
            extra = self.bundle / 'unexpected'
            if kind == 'symlink': extra.symlink_to(self.bundle / 'flightsim-app.exe')
            elif kind == 'hardlink': os.link(self.bundle / 'flightsim-app.exe', extra)
            elif kind == 'empty': extra.mkdir()
            else:
                extra = self.bundle / 'ASSETS'; extra.mkdir(); (extra / 'other').write_bytes(b'other')
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                q.create_archive(self.bundle, self.archive, TOP)
            if extra.is_dir() and not extra.is_symlink(): shutil.rmtree(extra)
            else: extra.unlink()

    def test_changed_staged_and_changed_extract_bytes_are_rejected(self):
        original = q.adapter.snapshot_payload
        calls = 0
        def changed(root):
            nonlocal calls
            calls += 1
            if calls == 2: (self.bundle / 'flightsim-app.exe').write_bytes(b'changed')
            return original(root)
        with patch.object(q.adapter, 'snapshot_payload', side_effect=changed), self.assertRaises(ValueError):
            q.create_archive(self.bundle, self.archive, TOP)
        self.archive.unlink(); (self.bundle / 'flightsim-app.exe').write_bytes(DATA)
        files = q.create_archive(self.bundle, self.archive, TOP)
        def changed_extract_snapshot(root):
            if root == self.root / 'extracted':
                (root / TOP / 'flightsim-app.exe').write_bytes(b'changed')
            return original(root)
        with patch.object(q.adapter, 'snapshot_payload', side_effect=changed_extract_snapshot), self.assertRaises(ValueError):
            q.extract_verified_archive(self.archive, self.root / 'extracted', files, TOP)


class SmokeTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.bundle, self.private = self.root / 'bundle', self.root / 'private'
        self.bundle.mkdir(); (self.bundle / 'flightsim-app.exe').write_bytes(DATA)
        self.calls = []
        self.error = None
        self.change_bundle = False

    def execute(self, command, *, cwd, env, stdout, stderr, journal, timeout):
        self.calls.append(command)
        self.assertEqual(timeout, 180)
        self.assertEqual({name: env[name] for name in q.WARP}, q.WARP)
        self.assertFalse(set(name.upper() for name in env).intersection(q.ASSET_ENVIRONMENT_POLICY['unset']))
        self.assertEqual({name for name in env if name.upper() in q.WARP}, set(q.WARP))
        self.assertEqual(command[1], '--internal-release-smoke')
        self.assertEqual(len(command), 4)
        aircraft = q.SCENES[command[2]][0]
        image = Path(command[3])
        log = ('SYNTHETIC TEST MARKERS ONLY\n' + q.DIAGNOSTIC_MARKER + '\naircraft model fitted: 10.9 m (' + aircraft + ')\n'
               'Screenshot saved to fixture.png\nBatch capture complete: status 0\n')
        stdout.write_text(log, encoding='utf-8'); stderr.write_bytes(b''); image.write_bytes(png())
        result = {'command': command, 'cwd': str(cwd), 'timeout_seconds': timeout,
                  'outcome': 'succeeded', 'exit_code': 0, 'elapsed_seconds': 0.1,
                  'stdout': q.record(stdout), 'stderr': q.record(stderr)}
        if self.error: result.update(self.error)
        if self.change_bundle: (self.bundle / 'flightsim-app.exe').write_bytes(b'changed')
        q.capture.write_json(journal, result)
        return result

    def run_fixture(self):
        with patch.object(q.sys, 'platform', 'win32'), patch.object(q.platform, 'machine', return_value='AMD64'):
            with patch.object(q.capture, 'execute', side_effect=self.execute):
                return q.run_smoke(self.bundle, self.private)

    def test_exact_two_commands_and_read_only_revalidation(self):
        result = self.run_fixture()
        self.assertEqual(list(result['scenes']), sorted(q.SCENES))
        self.assertEqual(len(self.calls), 2)
        self.assertIs(result['recipient_assent_collected'], False)
        self.assertIs(result['component_terms_dialog_tested'], False)
        self.assertEqual(result['asset_environment_policy'], q.ASSET_ENVIRONMENT_POLICY)
        for (name, (aircraft, view, image)), actual in zip(q.SCENES.items(), self.calls):
            self.assertEqual(actual, [str(self.bundle / 'flightsim-app.exe'), '--internal-release-smoke', name,
                str(self.private / image)])
            scene = result['scenes'][name]
            self.assertEqual(scene['execution']['runtime_environment'], q.WARP)
            self.assertEqual(scene['execution']['asset_environment_policy'], q.ASSET_ENVIRONMENT_POLICY)
            self.assertEqual(scene['screenshot']['width'], 640)
            self.assertEqual(scene['screenshot']['height'], 360)
        for flag in ('runtime_accepted', 'release_authorized', 'appearance_accepted', 'distribution_qualified'):
            self.assertNotIn(flag, result)
        before = {path: (path.read_bytes(), path.stat().st_mtime_ns) for path in self.private.rglob('*') if path.is_file()}
        with patch.object(q.capture, 'execute', side_effect=AssertionError('must not execute')):
            self.assertEqual(q.validate_smoke(self.bundle, self.private), result)
        self.assertEqual(before, {path: (path.read_bytes(), path.stat().st_mtime_ns)
                                 for path in self.private.rglob('*') if path.is_file()})

    def test_poisoned_asset_roots_and_mixed_case_warp_are_cleared(self):
        poison = {'BEVY_ASSET_ROOT': '/foreign/assets', 'bevy_asset_root': '/other/assets',
                  'CaRgO_MaNiFeSt_DiR': '/checkout', 'cargo_manifest_dir': '/another/checkout',
                  'wgpu_backend': 'vulkan', 'Wgpu_Force_Fallback_Adapter': '0'}
        with patch.dict(q.os.environ, poison):
            result = self.run_fixture()
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(result['asset_environment_policy'], q.ASSET_ENVIRONMENT_POLICY)
        for scene in result['scenes'].values():
            self.assertEqual(scene['execution']['asset_environment_policy'], q.ASSET_ENVIRONMENT_POLICY)
        # Validation checks the recorded launch policy, not the validator's own
        # ambient environment, and still never starts another process.
        with patch.dict(q.os.environ, poison), patch.object(q.capture, 'execute') as execute:
            self.assertEqual(q.validate_smoke(self.bundle, self.private), result)
            execute.assert_not_called()

    def test_missing_or_changed_asset_environment_policy_fails_revalidation(self):
        self.run_fixture()
        journal = self.private / 'commands/light-single-cockpit/journal.json'
        original = journal.read_bytes()
        for policy in (None, {'unset': [], 'name_matching': 'case_insensitive'},
                       {'unset': ['BEVY_ASSET_ROOT', 'CARGO_MANIFEST_DIR'], 'name_matching': 'case_sensitive'}):
            actual = json.loads(original)
            if policy is None: actual.pop('asset_environment_policy')
            else: actual['asset_environment_policy'] = policy
            q.capture.write_json(journal, actual)
            with self.subTest(policy=policy), self.assertRaises(ValueError):
                q.validate_smoke(self.bundle, self.private)
        journal.write_bytes(original)
        manifest = self.private / q.EXPORT
        value = json.loads(manifest.read_bytes()); value.pop('asset_environment_policy')
        manifest.write_bytes(q.canonical(value))
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)

    def test_native_platform_and_fresh_private_required_before_launch(self):
        with patch.object(q.sys, 'platform', 'linux'), patch.object(q.capture, 'execute') as execute:
            with self.assertRaisesRegex(ValueError, 'native Windows'): q.run_smoke(self.bundle, self.private)
            execute.assert_not_called()
        self.private.mkdir()
        with self.assertRaises(ValueError): self.run_fixture()
        self.assertEqual(self.calls, [])

    def test_failures_keep_originals_and_never_create_success_or_second_scene(self):
        for error in ({'outcome': 'failed', 'exit_code': 1}, {'outcome': 'timed_out', 'exit_code': -9},
                      {'outcome': 'termination_failed', 'exit_code': None}, {'outcome': 'launch_error', 'exit_code': None},
                      {'elapsed_seconds': 180.001}, {'exit_code': True}):
            self.error = error; self.calls = []
            with self.subTest(error=error), self.assertRaises(ValueError): self.run_fixture()
            self.assertEqual(len(self.calls), 1)
            self.assertFalse((self.private / q.EXPORT).exists())
            self.assertTrue((self.private / 'commands/light-single-cockpit/journal.json').is_file())
            shutil.rmtree(self.private)

    def test_bundle_mutation_during_runtime_is_rejected(self):
        self.change_bundle = True
        with self.assertRaisesRegex(ValueError, 'runtime changed'): self.run_fixture()
        self.assertFalse((self.private / q.EXPORT).exists())

    def test_every_smoke_proof_fatal_log_and_elapsed_boundary(self):
        result = {'outcome': 'succeeded', 'exit_code': 0, 'elapsed_seconds': 180}
        tokens = [q.DIAGNOSTIC_MARKER, 'aircraft model fitted:', '(light-single)', 'Screenshot saved to', 'Batch capture complete: status 0']
        log = '\n'.join(tokens)
        q.smoke(log, result, 'light-single-cockpit')
        q.smoke(log + '\nlowercase error is descriptive text', result, 'light-single-cockpit')
        for token in tokens:
            with self.subTest(token=token), self.assertRaises(ValueError):
                q.smoke(log.replace(token, ''), result, 'light-single-cockpit')
        for fatal in ('ERROR boom', '\x1b[31mERROR\x1b[0m boom', 'thread main panicked at crash',
                      'panic at crash', 'Failed to load asset', 'unregistered type'):
            with self.subTest(fatal=fatal), self.assertRaises(ValueError):
                q.smoke(log + '\n' + fatal, result, 'light-single-cockpit')
        for elapsed in (-1, 180.001, float('inf'), float('nan'), True):
            with self.subTest(elapsed=elapsed), self.assertRaises(ValueError):
                q.smoke(log, {**result, 'elapsed_seconds': elapsed}, 'light-single-cockpit')

    def test_original_bundle_journal_stream_image_manifest_mutations_fail(self):
        self.run_fixture()
        paths = [self.bundle / 'flightsim-app.exe', self.private / q.EXPORT,
                 self.private / 'flightsim-windows-smoke.png',
                 *(self.private / 'commands/light-single-cockpit' / name for name in ('stdout', 'stderr', 'journal.json'))]
        for path in paths:
            raw = path.read_bytes(); path.write_bytes(raw + b'changed')
            with self.subTest(path=path.name), self.assertRaises((ValueError, OSError)):
                q.validate_smoke(self.bundle, self.private)
            path.write_bytes(raw)
        extra = self.private / 'diagnostic.log'; extra.write_bytes(b'extra')
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)

    def test_changed_scene_command_environment_and_timeout_fail_even_rebound(self):
        self.run_fixture()
        path = self.private / 'commands/light-single-cockpit/journal.json'
        original = path.read_bytes()
        for field, value in (('command', ['other']), ('cwd', str(self.root)),
                             ('timeout_seconds', 181), ('timeout_seconds', 180.0),
                             ('runtime_environment', {'WGPU_BACKEND': 'vulkan'})):
            actual = json.loads(original); actual[field] = value; q.capture.write_json(path, actual)
            with self.subTest(field=field), self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)
        path.write_bytes(original)

    def test_unknown_flags_duplicate_keys_and_nonfinite_journal_rejected(self):
        self.run_fixture()
        path = self.private / 'commands/light-single-cockpit/journal.json'
        original = path.read_bytes()
        actual = json.loads(original); actual['runtime_accepted'] = True
        for raw in (q.canonical(actual), b'{"cwd":"x",' + original[1:],
                    original.replace(b'0.1', b'NaN')):
            path.write_bytes(raw)
            with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)
        path.write_bytes(original)
        manifest = self.private / q.EXPORT
        value = json.loads(manifest.read_bytes()); value['runtime_accepted'] = True
        manifest.write_bytes(q.canonical(value))
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)

    def test_complete_png_crc_rows_deflate_end_and_trailing_bytes(self):
        self.run_fixture()
        path = self.private / 'flightsim-windows-smoke.png'
        correct = path.read_bytes()
        bad_crc = bytearray(correct); bad_crc[30] ^= 1
        malformed = (correct[:-12], correct + b'trailing', bytes(bad_crc), png(width=639),
                     png(pixels=b'\0' * 5), png(extra=b'hidden zlib tail'),
                     png(pixels=(b'\5' + b'\0' * 1920) * 360))
        for index, raw in enumerate(malformed):
            path.write_bytes(raw)
            with self.subTest(index=index), self.assertRaises(ValueError):
                q.candidate.validate_png(path)
            with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)
        path.write_bytes(correct)

    def test_private_links_and_relocated_originals_fail(self):
        self.run_fixture()
        destination = self.root / 'copy'; shutil.copytree(self.private, destination)
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, destination)
        original = self.private / 'commands/light-single-cockpit/stdout'
        alias = self.root / 'hardlink'; os.link(original, alias)
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)
        alias.unlink()
        raw = original.read_bytes(); original.unlink(); original.symlink_to(self.root / 'replacement')
        (self.root / 'replacement').write_bytes(raw)
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)

    def test_stream_bound_and_missing_png_fail(self):
        self.run_fixture()
        with patch.object(q, 'MAX_STREAM', 0), self.assertRaises(ValueError):
            q.validate_smoke(self.bundle, self.private)
        (self.private / 'flightsim-windows-smoke.png').unlink()
        with self.assertRaises(ValueError): q.validate_smoke(self.bundle, self.private)

    def test_validate_only_cli_never_captures(self):
        self.run_fixture()
        result = subprocess.run([sys.executable, str(SCRIPT), '--validate-only', '--bundle', str(self.bundle),
                                 '--private', str(self.private)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        (self.bundle / 'flightsim-app.exe').write_bytes(b'changed')
        result = subprocess.run([sys.executable, str(SCRIPT), '--validate-only', '--bundle', str(self.bundle),
                                 '--private', str(self.private)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertNotIn(str(self.bundle), result.stderr)


if __name__ == '__main__':
    unittest.main()
