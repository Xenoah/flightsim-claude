"""Synthetic policy/runtime evidence boundaries; no simulator or publication."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import tempfile
import unittest
import zlib
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('full_qualification_tests', ROOT / 'scripts/qualify-full-windows-candidate.py')
q = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(q)


def blob(data=b'SYNTHETIC TEST DATA ONLY'):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def failed():
    return {'schema_version': 1, 'identity': q.IDENTITY, 'source_sha': '1'*40, 'source_tree': '2'*40,
            'policy_sha': '3'*40, 'policy_tree': '4'*40, 'source_recipe': q.native.SOURCE_RECIPE,
            'status': 'failed', 'limits': q.LIMITS, 'commands': [], 'images': {}, 'bindings': {},
            'failure_sidecar': None, 'focused_exterior': {'status': 'not_attempted', 'command': None, 'image': None, 'appearance_accepted': False}, **{k: False for k in q.FLAGS}}


class FullArchiveTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup); self.root = Path(temp.name)
        self.path = self.root / 'candidate.zip'; self.files = {'flightsim-app.exe': blob()}

    def zipped(self, data=b'SYNTHETIC TEST DATA ONLY'):
        with zipfile.ZipFile(self.path, 'w', zipfile.ZIP_DEFLATED) as z:
            z.writestr(q.BUNDLE + '/flightsim-app.exe', data)
        return {'flightsim-app.exe': blob(data)}

    def test_exact_compressed_members_extract_and_recheck(self):
        files = self.zipped(); q.archive_members(self.path, files, self.root / 'extracted')
        self.assertEqual((self.root / 'extracted' / q.BUNDLE / 'flightsim-app.exe').read_bytes(), b'SYNTHETIC TEST DATA ONLY')
        q.archive_members(self.path, files)

    def test_large_deflate_drain_boundaries(self):
        for n in (1048576, 1048577, 2097153):
            with self.subTest(n=n): q.audit_archive(self.path, self.zipped(b'A' * n))

    def test_hidden_deflate_bytes_rejected_even_when_zipfile_ignores_them(self):
        self.zipped()
        with zipfile.ZipFile(self.path) as z: central, compressed = z.start_dir, z.infolist()[0].compress_size
        hidden = b'HIDDEN-UNAPPROVED-PAYLOAD'; raw = bytearray(self.path.read_bytes()); raw[central:central] = hidden
        struct.pack_into('<I', raw, 18, compressed + len(hidden))
        struct.pack_into('<I', raw, central + len(hidden) + 20, compressed + len(hidden))
        struct.pack_into('<I', raw, len(raw) - 22 + 16, central + len(hidden)); self.path.write_bytes(raw)
        with zipfile.ZipFile(self.path) as z: self.assertEqual(z.read(q.BUNDLE + '/flightsim-app.exe'), b'SYNTHETIC TEST DATA ONLY')
        with self.assertRaisesRegex(ValueError, 'hidden bytes'): q.audit_archive(self.path, self.files)

    def test_prefix_trailer_comment_and_extra_rejected(self):
        for before, after in ((b'prefix', b''), (b'', b'trailer')):
            self.zipped(); self.path.write_bytes(before + self.path.read_bytes() + after)
            with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        self.zipped()
        with zipfile.ZipFile(self.path, 'a') as z: z.comment = b'private comment'
        with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        info = zipfile.ZipInfo(q.BUNDLE + '/flightsim-app.exe'); info.extra = b'\xfe\xca\x04\x00hide'
        with zipfile.ZipFile(self.path, 'w') as z: z.writestr(info, b'SYNTHETIC TEST DATA ONLY')
        with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)

    def test_changed_bytes_links_and_unexpected_members_rejected(self):
        self.zipped(b'changed')
        with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        info = zipfile.ZipInfo(q.BUNDLE + '/flightsim-app.exe'); info.create_system = 3; info.external_attr = (stat.S_IFLNK | 0o777) << 16
        with zipfile.ZipFile(self.path, 'w') as z: z.writestr(info, b'SYNTHETIC TEST DATA ONLY')
        with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        self.zipped()
        with zipfile.ZipFile(self.path, 'a') as z: z.writestr(q.BUNDLE + '/extra.txt', b'extra')
        with self.assertRaises(ValueError): q.audit_archive(self.path, self.files)

    def test_bounds_and_unsafe_windows_paths(self):
        self.zipped()
        with patch.object(q, 'MAX_ARCHIVE', 1), self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        with patch.object(q, 'MAX_UNPACKED', 1), self.assertRaises(ValueError): q.audit_archive(self.path, self.files)
        for name in ('../secret', '/abs', 'C:relative', 'a\\b', 'a//b', 'a/./b', 'NUL', 'COM1.log', 'trailing.', 'trailing '):
            with self.subTest(name=name), self.assertRaises(ValueError): q.relative(name)


class PolicyInheritanceTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup); self.root = Path(temp.name)
        self.source = self.root / 'source'; self.policy = self.root / 'policy'; self.source.mkdir()
        def git(where, *args):
            return subprocess.check_output(['git', '-c', 'user.name=Synthetic fixture', '-c', 'user.email=synthetic@invalid',
                                           '-C', str(where), *args], stderr=subprocess.DEVNULL)
        self.git = git
        git(self.source, 'init', '-q'); (self.source / 'Cargo.toml').write_text('SYNTHETIC SOURCE ONLY\n')
        git(self.source, 'add', '.'); git(self.source, 'commit', '-qm', 'synthetic source')
        self.s = git(self.source, 'rev-parse', 'HEAD').decode().strip(); self.st = git(self.source, 'rev-parse', 'HEAD^{tree}').decode().strip()
        subprocess.run(['git', 'clone', '-q', '--no-hardlinks', str(self.source), str(self.policy)], check=True)
        (self.policy / 'scripts').mkdir(); (self.policy / 'scripts/qualify-full-windows-candidate.py').write_text('# SYNTHETIC POLICY ONLY\n')
        git(self.policy, 'add', '.'); git(self.policy, 'commit', '-qm', 'synthetic policy')
        self.p = git(self.policy, 'rev-parse', 'HEAD').decode().strip(); self.pt = git(self.policy, 'rev-parse', 'HEAD^{tree}').decode().strip()
        self.source_value = {'source_sha': self.s, 'source_tree': self.st,
                             'files': [{'path': 'Cargo.toml', 'checkout_sha256': blob((self.source / 'Cargo.toml').read_bytes())['sha256']}]}
        self.patch = patch.object(q, 'POLICY_ROOT', self.policy); self.patch.start(); self.addCleanup(self.patch.stop)

    def test_real_git_inheritance_is_factual_not_authority(self):
        result = q.policy_evidence(self.source_value, self.p, self.pt)
        self.assertTrue(result['inherited_source_files_byte_identical']); self.assertFalse(result['release_authorized'])

    def test_wrong_external_pin_and_untracked_files_rejected(self):
        with self.assertRaises(ValueError): q.policy_evidence(self.source_value, 'f'*40, self.pt)
        (self.policy / 'extra').write_text('untracked')
        with self.assertRaisesRegex(ValueError, 'not clean'): q.policy_evidence(self.source_value, self.p, self.pt)

    def test_committed_source_change_rejected(self):
        (self.policy / 'Cargo.toml').write_text('changed inherited source')
        self.git(self.policy, 'add', '.'); self.git(self.policy, 'commit', '-qm', 'synthetic bad policy')
        p = self.git(self.policy, 'rev-parse', 'HEAD').decode().strip(); t = self.git(self.policy, 'rev-parse', 'HEAD^{tree}').decode().strip()
        with self.assertRaisesRegex(ValueError, 'changed inherited'): q.policy_evidence(self.source_value, p, t)

    def test_assume_unchanged_does_not_hide_changed_code(self):
        self.git(self.policy, 'update-index', '--assume-unchanged', 'Cargo.toml')
        (self.policy / 'Cargo.toml').write_text('hidden mutation')
        with self.assertRaisesRegex(ValueError, 'canonical commit'): q.policy_evidence(self.source_value, self.p, self.pt)


class FullRuntimeContractTests(unittest.TestCase):
    def test_exact_two_existing_scene_commands(self):
        self.assertEqual(list(q.SCENES), ['light-single-cockpit', 'swift-sport-chase'])
        for name, (aircraft, view, _) in q.SCENES.items():
            self.assertEqual(q.command(Path('/bundle/flightsim-app.exe'), Path('/private/image.png'), name),
                ['/bundle/flightsim-app.exe', '--screenshot', '/private/image.png', '--screenshot-delay', '5',
                 '--exit-after-screenshot', '--aircraft', aircraft, '--view', view, '--traffic', 'synthetic'])
        self.assertEqual(q.TIMEOUT, 180)

    def test_success_requires_model_png_marker_zero_and_deadline(self):
        actual = {'outcome': 'succeeded', 'exit_code': 0, 'elapsed_seconds': 179.9}
        log = 'aircraft model fitted: 10.9 m (light-single)\nScreenshot saved to test.png\nBatch capture complete: status 0'
        q.smoke(log, actual, 'light-single-cockpit')
        for bad in (log.replace('(light-single)', '(swift-sport)'), log.replace('Batch capture complete: status 0', ''), log + '\nERROR failed'):
            with self.assertRaises(ValueError): q.smoke(bad, actual, 'light-single-cockpit')
        for row in ({**actual, 'elapsed_seconds': 180.001}, {**actual, 'exit_code': 1}, {**actual, 'outcome': 'timed_out'}):
            with self.assertRaises(ValueError): q.smoke(log, row, 'light-single-cockpit')

    def test_failure_shape_cannot_become_authority_or_invented_success(self):
        value = failed(); q.validate_shape(value)
        for flag in q.FLAGS:
            bad = {**value, flag: True}
            with self.assertRaises(ValueError): q.validate_shape(bad)
        with self.assertRaises(ValueError): q.validate_shape({**value, 'status': 'ordinary_smoke_observed_reviews_required'})
        with self.assertRaises(ValueError): q.validate_shape({**value, 'bindings': {'dependency_review': blob()}})

    def test_sidecar_is_bound_redacted_and_non_authorizing(self):
        with tempfile.TemporaryDirectory() as t:
            private = Path(t); base = private / 'commands/light-single-cockpit'; base.mkdir(parents=True)
            (base / 'stdout').write_bytes(b'')
            (base / 'stderr').write_bytes(b'ERROR wgpu_hal::auxil::dxgi::result: Present failed: Error { code: HRESULT(0x887A0005), message: "PRIVATE-DRIVER-PROSE" }\n')
            row = {'id': 'light-single-cockpit', 'outcome': 'failed', 'exit_code': 1, 'elapsed_seconds': 1,
                   'stdout': q.record(base / 'stdout'), 'stderr': q.record(base / 'stderr')}
            result = q.sidecar(private, row, {'source_sha': '1'*40, 'source_tree': '2'*40},
                               {'policy_sha': '3'*40, 'policy_tree': '4'*40}, blob())
            self.assertNotIn('PRIVATE-DRIVER-PROSE', json.dumps(result))
            self.assertFalse(result['release_authorized']); self.assertFalse(result['producer_authenticated'])
            (base / 'stderr').write_bytes(b'changed')
            with self.assertRaises(ValueError): q.sidecar(private, row, {'source_sha':'1'*40,'source_tree':'2'*40},
                {'policy_sha':'3'*40,'policy_tree':'4'*40}, blob())

    def test_validate_only_accepts_well_formed_failure_for_evidence_upload(self):
        args = ['--repo','/source','--source-sha','1'*40,'--source-tree','2'*40,'--policy-sha','3'*40,'--policy-tree','4'*40,
                '--build-private','/build','--build-evidence','/build-text','--private','/private','--evidence','/export']
        with patch.object(q, 'validate_export', return_value=failed()): self.assertEqual(q.main(args+['--validate-only']), 0)
        with patch.object(q, 'qualify', return_value=failed()): self.assertEqual(q.main(args), 1)



class SyntheticExportBindingTests(unittest.TestCase):
    """Real staging/ZIP/export checks, mocked source/build/runtime authorities."""
    def setUp(self):
        spec = importlib.util.spec_from_file_location('full_stage_fixture_only', ROOT / 'scripts/tests/test_full_native_consumers.py')
        fixtures = importlib.util.module_from_spec(spec); spec.loader.exec_module(fixtures)
        self.fixture = fixtures.FullStageTests(methodName='test_full_payload_keeps_both_models_and_no_authority')
        self.fixture.setUp(); self.addCleanup(self.fixture.doCleanups)
        self.verified = self.fixture.bundle_fixture()
        self.root, self.repo = self.fixture.root, self.fixture.repo
        self.build = self.root / 'captured-build'; self.build.mkdir()
        import shutil
        shutil.copytree(self.fixture.notices, self.build / 'capture/ordinary/notices')
        self.text = self.root / 'captured-text'; self.text.mkdir()
        (self.text / q.capture.EXPORT_NAME).write_text('SYNTHETIC BUILD ATTESTATION, NOT NATIVE EVIDENCE')
        self.private = self.root / 'runtime'; self.export = self.root / 'public'
        self.source = {'source_sha': '1'*40, 'source_tree': '2'*40, 'files': []}
        self.policy = {'policy_sha': '3'*40, 'policy_tree': '4'*40, 'release_authorized': False}
        self.options = {'source_tree': '2'*40, 'policy_sha': '3'*40, 'policy_tree': '4'*40}
        self.fail_first = False
        self.fail_focused = False
        def mocked(obj, name, **kwargs):
            m = patch.object(obj, name, **kwargs); m.start(); self.addCleanup(m.stop)
        mocked(q, 'POLICY_ROOT', new=self.root / 'policy')
        mocked(q.sys, 'platform', new='win32'); mocked(q.platform, 'machine', return_value='AMD64')
        mocked(q.capture, 'source_evidence', return_value=self.source)
        mocked(q, 'policy_evidence', return_value=self.policy)
        mocked(q.native, 'validate_build', return_value=self.verified)
        mocked(q.native.release.readiness, 'check', return_value=self.fixture.report)
        self.facts = {'kind': 'synthetic_native_facts_not_real_evidence', 'release_authorized': False}
        self.caps = {'kind': 'synthetic_caps_not_real_evidence', 'release_authorized': False}
        self.projection = {'kind': 'synthetic_projection_not_real_evidence', 'release_authorized': False}
        def collect_facts(path, **kwargs): path.mkdir(); q.write_json(path / q.runtime_facts.PROJECTION_NAME, self.facts)
        def collect_caps(path, *args): path.mkdir(); q.write_json(path / q.capabilities.EXPORT, self.caps)
        mocked(q.runtime_facts, 'collect_runtime_facts', side_effect=collect_facts)
        mocked(q.native, 'validate_runtime_facts', return_value=self.facts)
        mocked(q.capabilities, 'collect', side_effect=collect_caps); mocked(q.capabilities, 'project', return_value=self.caps)
        mocked(q.native, 'project', return_value=self.projection)
        self.original_notices = {'kind': 'synthetic_original_notice_packet_not_native_evidence', 'release_authorized': False}
        mocked(q.originals, 'prepare', return_value=self.original_notices)
        mocked(q.originals, 'verify', return_value=self.original_notices)

        def stage_captured(repo, expected, build, text, output, **kwargs):
            import shutil
            shutil.copytree(self.fixture.out, output)
        mocked(q.stage, 'stage_captured', side_effect=stage_captured)
        mocked(q.capture, 'execute', side_effect=self.execute)

    @staticmethod
    def png():
        def chunk(tag, data): return struct.pack('!I',len(data))+tag+data+struct.pack('!I',zlib.crc32(tag+data)&0xffffffff)
        pixels=(b'\0'+b'\x22\x66\xaa'*640)*360
        return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',640,360,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(pixels))+chunk(b'IEND',b'')

    def execute(self, command, *, cwd, env, stdout, stderr, journal, timeout):
        aircraft=command[command.index('--aircraft')+1]
        image=Path(command[command.index('--screenshot')+1])
        failed_scene = self.fail_first or (self.fail_focused and image.name == q.FOCUSED_SCENE[2])
        if failed_scene:
            stdout.write_bytes(b''); stderr.write_text('ERROR wgpu_core::device::global: Device lost\n')
        else:
            stdout.write_text('SYNTHETIC TEST MARKERS ONLY\naircraft model fitted: 10.9 m ('+aircraft+')\nScreenshot saved to fixture.png\nBatch capture complete: status 0\n')
            stderr.write_bytes(b''); image.write_bytes(self.png())
        row={'command':command,'cwd':str(cwd),'timeout_seconds':timeout,'outcome':'failed' if failed_scene else 'succeeded',
             'exit_code':1 if failed_scene else 0,'elapsed_seconds':0.1,'stdout':q.record(stdout),'stderr':q.record(stderr)}
        q.write_json(journal,row); return row

    def run_fixture(self):
        return q.qualify(self.repo,'1'*40,self.build,self.text,self.private,self.export,**self.options)

    def verify(self):
        return q.validate_export(self.export,self.repo,'1'*40,self.build,self.text,self.private,**self.options)

    def test_real_export_revalidation_after_synthetic_run(self):
        value=self.run_fixture()
        self.assertEqual(value['status'],'ordinary_smoke_observed_reviews_required')
        self.assertEqual(value,self.verify())
        self.assertEqual(len(value['images']),2)
        self.assertEqual(value['focused_exterior']['status'], 'observed_review_required')
        self.assertTrue((self.export/q.FOCUSED_SCENE[2]).is_file())
        self.assertFalse(value['focused_exterior']['appearance_accepted'])
        self.assertFalse(any(value[k] for k in q.FLAGS))
        self.assertFalse((self.export/q.ARCHIVE).exists())
        self.assertFalse((self.export/'policy.json').exists())

    def test_failed_runtime_keeps_bound_native_facts_without_second_scene(self):
        self.fail_first=True; value=self.run_fixture()
        self.assertEqual(value['status'],'failed'); self.assertEqual(len(value['commands']),1)
        self.assertIn('native_projection',value['bindings']); self.assertEqual(value,self.verify())
        self.assertIsNotNone(value['failure_sidecar']); self.assertFalse(value['runtime_accepted'])

    def test_focused_failure_does_not_change_two_required_smoke_results(self):
        self.fail_focused = True; value = self.run_fixture()
        self.assertEqual(value['status'], 'ordinary_smoke_observed_reviews_required')
        self.assertEqual(len(value['commands']), 2)
        self.assertEqual(value['focused_exterior']['status'], 'failed')
        self.assertIsNone(value['focused_exterior']['image'])
        self.assertFalse((self.export/q.FOCUSED_SCENE[2]).exists())
        self.assertEqual(value, self.verify())

    def test_focused_result_cannot_grant_acceptance_or_precede_required_cases(self):
        value = self.run_fixture()
        bad = copy.deepcopy(value); bad['focused_exterior']['appearance_accepted'] = True
        with self.assertRaises(ValueError): q.validate_shape(bad)
        bad = copy.deepcopy(value); bad['commands'].pop(); bad['status'] = 'failed'
        with self.assertRaises(ValueError): q.validate_shape(bad)
        bad = copy.deepcopy(value); bad['focused_exterior']['command']['id'] = 'unrequested-weather-case'
        with self.assertRaises(ValueError): q.validate_shape(bad)

    def test_focused_view_image_and_acceptance_are_strictly_bound(self):
        self.run_fixture(); path=self.private/'commands'/q.FOCUSED_ID/'journal.json'
        old=json.loads(path.read_text()); bad=copy.deepcopy(old)
        bad['command'][bad['command'].index('--view')+1]='cockpit'; q.write_json(path,bad)
        with self.assertRaisesRegex(ValueError,'runtime invocation'): self.verify()
        q.write_json(path,old)
        (self.private/q.FOCUSED_SCENE[2]).write_bytes(b'changed focused image')
        with self.assertRaises(ValueError): self.verify()

    def test_original_notice_packet_is_available_before_runtime_failure_and_bound(self):
        self.fail_first = True; value = self.run_fixture()
        self.assertIn('original_notices', value['bindings'])
        self.assertTrue((self.export/q.originals.EXPORT_NAME).is_file())
        (self.export/q.originals.EXPORT_NAME).write_bytes(b'changed packet')
        with self.assertRaisesRegex(ValueError, 'public file differs'): self.verify()

    def test_extra_public_file_and_changed_log_rejected(self):
        self.run_fixture(); extra=self.export/'private.log'; extra.write_text('must not export')
        with self.assertRaisesRegex(ValueError,'unexpected public'): self.verify()
        extra.unlink(); path=self.private/'commands/light-single-cockpit/stdout'; path.write_text('changed')
        with self.assertRaisesRegex(ValueError,'runtime stream changed'): self.verify()

    def test_journal_cannot_relabel_another_view_or_environment(self):
        self.run_fixture(); path=self.private/'commands/light-single-cockpit/journal.json'; old=json.loads(path.read_text())
        bad=copy.deepcopy(old); bad['command'][bad['command'].index('--view')+1]='chase'; q.write_json(path,bad)
        with self.assertRaisesRegex(ValueError,'runtime invocation'): self.verify()
        bad=copy.deepcopy(old); bad['runtime_environment']={'WGPU_BACKEND':'vulkan','WGPU_FORCE_FALLBACK_ADAPTER':'1'}; q.write_json(path,bad)
        with self.assertRaisesRegex(ValueError,'runtime invocation'): self.verify()

    def test_changed_archive_and_png_rejected(self):
        self.run_fixture(); path=self.private/q.ARCHIVE; raw=path.read_bytes(); path.write_bytes(raw+b'private')
        with self.assertRaises(ValueError): self.verify()
        path.write_bytes(raw); name=q.SCENES['light-single-cockpit'][2]
        (self.private/name).write_bytes(b'changed')
        with self.assertRaises(ValueError): self.verify()


if __name__ == '__main__': unittest.main()
