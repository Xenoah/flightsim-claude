"""Actual small subprocess tests and synthetic export adversaries, never MSVC proof."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('capture', Path(__file__).parents[1] / 'capture-analytical-swift-msvc.py')
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)
REPO = Path(__file__).resolve().parents[2]


def stream_record(data=b''):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def summary_fixture(passed=False):
    """Schema-only fixture; no test presents it as real native build evidence."""
    value = {'schema_version': 1, 'identity': capture.IDENTITY, 'recipe': capture.check.IDENTITY,
             'status': 'failed', 'stage': 'preflight', 'source_sha': 'a' * 40, 'source_tree': None,
             'release_authorized': False, 'native_runtime_qualified': False, 'distribution_qualified': False,
             'required_unexecuted_gates': list(capture.check.GATES), 'commands': [], 'bindings': {}, 'builds': {}}
    if passed:
        value.update(status=capture.PASS, stage='complete', source_tree='b' * 40)
        value['commands'] = [{'id': name, 'outcome': 'succeeded', 'exit_code': 0,
                              'stdout': stream_record(), 'stderr': stream_record()} for name in capture.COMMAND_IDS]
        value['bindings'] = {name: stream_record() for name in ('source', 'capture_contract', 'analytical_contract',
                              'replay_contract', 'lock', 'capture', 'audit', 'frozen_trees')}
        value['builds'] = {mode: {'executable': stream_record(b'synthetic'), 'inventory': stream_record(),
                          'metadata': stream_record(), 'frozen_artifact_count': 17,
                          'lut_payloads_found': 3 if mode == 'ordinary' else 0} for mode in capture.MODES}
    return value


class SubprocessCaptureTests(unittest.TestCase):
    def invoke(self, root, program=None, *, timeout=10, command=None):
        return capture.execute(command or [sys.executable, '-c', program], cwd=root, env=os.environ.copy(),
                               stdout=root / 'stdout', stderr=root / 'stderr', journal=root / 'journal.json', timeout=timeout)

    def test_actual_nonzero_exit_and_binary_streams_are_retained_without_echo(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.invoke(root, "import os; os.write(1, b'out\\r\\n\\xff\\x00'); os.write(2, b'https://private.invalid/?token=secret\\r\\n'); raise SystemExit(7)")
            self.assertEqual(result['exit_code'], 7)
            self.assertEqual(result['outcome'], 'failed')
            self.assertEqual((root / 'stdout').read_bytes(), b'out\r\n\xff\x00')
            self.assertEqual((root / 'stderr').read_bytes(), b'https://private.invalid/?token=secret\r\n')
            self.assertEqual(result['stdout'], stream_record(b'out\r\n\xff\x00'))
            self.assertEqual(json.loads((root / 'journal.json').read_text()), result)

    def test_actual_success_is_only_command_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.invoke(root, "print('small subprocess only')")
            self.assertEqual(result['exit_code'], 0)
            self.assertEqual(result['outcome'], 'succeeded')
            self.assertNotIn('release_authorized', result)
            self.assertNotIn('builds', result)

    def test_actual_launch_failure_has_no_invented_exit_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.invoke(root, command=[str(root / 'missing-program')])
            self.assertEqual(result['outcome'], 'launch_error')
            self.assertIsNone(result['exit_code'])
            self.assertEqual(json.loads((root / 'journal.json').read_text()), result)

    def test_actual_timeout_retains_output_and_observed_termination_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.invoke(root, "import os,time; os.write(2,b'before-timeout'); time.sleep(60)", timeout=0.4)
            self.assertEqual(result['outcome'], 'timed_out')
            self.assertIs(type(result['exit_code']), int)
            self.assertNotEqual(result['exit_code'], 0)
            self.assertEqual((root / 'stderr').read_bytes(), b'before-timeout')
            self.assertEqual(json.loads((root / 'journal.json').read_text()), result)

    def test_timeout_stops_a_real_descendant(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            child = "import time; from pathlib import Path; Path('child-started').write_text('ready'); time.sleep(2); Path('survived').write_text('bad')"
            parent = 'import subprocess,sys,time; subprocess.Popen([sys.executable,"-c",' + repr(child) + ']); time.sleep(60)'
            result = self.invoke(root, parent, timeout=1)
            self.assertEqual(result['outcome'], 'timed_out')
            self.assertTrue((root / 'child-started').is_file(), 'the descendant must have really started')
            # Use a real second process as a bounded observation wait, independent
            # of the timed-out process group (tests also run on native Windows).
            subprocess.run([sys.executable, '-c', 'import time; time.sleep(2.2)'], check=True)
            self.assertFalse((root / 'survived').exists())

    def test_failed_termination_is_journaled_and_cannot_be_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stopped = capture.stop_process_tree
            def kill_then_report_error(process):
                stopped(process)
                raise OSError('synthetic cleanup reporting failure')
            with mock.patch.object(capture, 'stop_process_tree', side_effect=kill_then_report_error):
                result = self.invoke(root, 'import time; time.sleep(60)', timeout=0.2)
            self.assertEqual(result['outcome'], 'termination_failed')
            self.assertNotEqual(result['exit_code'], 0)
            self.assertEqual(json.loads((root / 'journal.json').read_text()), result)


class BoundaryTests(unittest.TestCase):
    def test_override_and_ancestor_configuration_rejection(self):
        names = ('RUSTC_WRAPPER', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_BUILD_RUSTFLAGS',
                 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER', 'CARGO_ENCODED_RUSTFLAGS',
                 'CARGO_REGISTRY_TOKEN', 'CARGO_REGISTRIES_PRIVATE_INDEX', 'CARGO_INCREMENTAL', 'CL', '_LINK_', 'CC_x86_64_pc_windows_msvc')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); repo = root / 'repo'; repo.mkdir()
            capture.reject_configuration(repo, {'PATH': '/official/bin', 'CARGO_HOME': '/replaced/home'})
            for name in names:
                with self.subTest(name=name), self.assertRaises(ValueError):
                    capture.reject_configuration(repo, {name: 'override'})
            (root / '.cargo').mkdir()
            for name in ('config', 'config.toml'):
                path = root / '.cargo' / name; path.write_text('[build]\nrustflags=[]\n')
                with self.assertRaises(ValueError): capture.reject_configuration(repo, {})
                path.unlink()

    def test_complete_source_canonical_check_detects_assume_unchanged_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            path = root / 'compile-input.rs'; path.write_bytes(b'const VALUE: u32 = 1;\n')
            subprocess.run(['git', '-c', 'core.autocrlf=false', 'add', 'compile-input.rs'], cwd=root, check=True)
            blob = subprocess.check_output(['git', 'rev-parse', ':compile-input.rs'], cwd=root, text=True).strip()
            source = {'canonical_git_object_format': 'sha1', 'files': [{'path': path.name, 'canonical_git_blob': blob}]}
            capture.verify_canonical_sources(root, source)
            subprocess.run(['git', 'update-index', '--assume-unchanged', path.name], cwd=root, check=True)
            path.write_bytes(b'const VALUE: u32 = 2;\n')
            with self.assertRaisesRegex(ValueError, 'canonical'):
                capture.verify_canonical_sources(root, source)

    def test_snapshot_rejects_mutation_addition_and_link(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); path = root / 'artifact'; path.write_bytes(b'original')
            original = capture.snapshot_tree(root)
            path.write_bytes(b'mutated')
            self.assertNotEqual(capture.snapshot_tree(root), original)
            path.write_bytes(b'original'); (root / 'additional').write_bytes(b'other')
            self.assertNotEqual(capture.snapshot_tree(root), original)
            try: (root / 'linked').symlink_to(path)
            except OSError: self.skipTest('host does not grant symlink creation')
            with self.assertRaises(ValueError): capture.snapshot_tree(root)

    @unittest.skipUnless(sys.platform == 'win32', 'real Windows junction requires Windows')
    def test_real_windows_junction_and_its_children_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); target = root / 'target'; target.mkdir()
            (target / 'input').write_text('private')
            junction = root / 'junction'
            command = Path(os.environ['SystemRoot']) / 'System32/cmd.exe'
            subprocess.run([str(command), '/d', '/c', 'mklink', '/J', str(junction), str(target)],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                for path in (junction, junction / 'input'):
                    with self.subTest(path=path), self.assertRaises(ValueError): capture.no_links(path)
            finally:
                junction.rmdir()

    def test_disjoint_and_existing_roots_cannot_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); repo = root / 'repo'; repo.mkdir()
            private = root / 'private'; private.mkdir(); sentinel = private / 'keep'; sentinel.write_text('keep')
            with self.assertRaises(ValueError): capture.capture(repo, 'a' * 40, private, root / 'export')
            self.assertEqual(sentinel.read_text(), 'keep')
            with self.assertRaises(ValueError): capture.disjoint(repo, repo / 'private', root / 'export')

    @unittest.skipIf(sys.platform == 'win32', 'production entry would perform a real build on Windows')
    def test_real_non_windows_entry_exports_only_failed_native_preflight(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = subprocess.run([sys.executable, str(REPO / 'scripts/capture-analytical-swift-msvc.py'),
                '--repo', str(REPO), '--source-sha', 'a' * 40, '--private', str(root / 'private'),
                '--evidence', str(root / 'evidence')], capture_output=True, check=False)
            self.assertEqual(result.returncode, 1)
            value = capture.validate_export(root / 'evidence', expected='a' * 40, private=root / 'private', repo=REPO)
            self.assertEqual(value['status'], 'failed')
            self.assertEqual(value['stage'], 'preflight')
            self.assertEqual(value['commands'], [])
            self.assertNotIn(str(root).encode(), result.stdout + result.stderr)

    def test_contract_has_only_additive_files_and_unchanged_inherited_contracts(self):
        contract = json.loads((REPO / capture.CONTRACT).read_text())
        self.assertEqual(set(contract['source_sha256']), capture.BOUND_FILES)
        for path, expected in {**contract['source_sha256'], **capture.INHERITED_CONTRACTS}.items():
            with self.subTest(path=path): self.assertEqual(capture.check.digest(REPO / path), expected)
        old = json.loads((REPO / capture.check.CONTRACT).read_text())
        for path, expected in old['source_sha256'].items():
            with self.subTest(old_path=path): self.assertEqual(capture.check.digest(REPO / path), expected)


class ExportTests(unittest.TestCase):
    def test_json_writer_emits_ascii_lf_even_with_windows_text_translation(self):
        # Reproduce run 37299384923 on every host using real text I/O newline
        # conversion, without pretending that a native Windows build succeeded.
        class CRLFTextPath(type(Path())):
            def write_text(self, data, encoding=None, errors=None, newline=None):
                return super().write_text(data, encoding=encoding, errors=errors,
                                          newline='\r\n' if newline is None else newline)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            control = CRLFTextPath(root / 'text-control')
            control.write_text('control\n', encoding='ascii')
            self.assertEqual(control.read_bytes(), b'control\r\n')
            path = CRLFTextPath(root / 'record.json')
            capture.write_json(path, {'z': 1, 'a': '\u00e9\n'})
            self.assertEqual(path.read_bytes(), b'{\n  "a": "\\u00e9\\n",\n  "z": 1\n}\n')

    def test_crlf_export_is_rejected_without_normalizing_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); path = root / capture.EXPORT_NAME
            capture.write_json(path, summary_fixture())
            canonical = path.read_bytes()
            self.assertNotIn(b'\r', canonical)
            capture.validate_export(root)
            crlf = canonical.replace(b'\n', b'\r\n')
            path.write_bytes(crlf)
            with self.assertRaisesRegex(ValueError, 'noncanonical export bytes'):
                capture.validate_export(root)
            self.assertEqual(path.read_bytes(), crlf)

    def test_schema_only_fixtures_keep_every_gate_and_false_qualification(self):
        for passed in (False, True):
            capture.validate_summary(summary_fixture(passed))

    def test_export_rejects_payload_strings_unknown_keys_and_forged_status(self):
        mutations = [
            lambda v: v.update(url='https://private.invalid/?sig=secret'),
            lambda v: v.update(release_authorized=True),
            lambda v: v.update(native_runtime_qualified=0),
            lambda v: v.update(required_unexecuted_gates=[]),
            lambda v: v.update(source_sha='../../private/file'),
            lambda v: v['bindings']['source'].update(sha256='https://private.invalid'),
            lambda v: v['commands'][0].update(stdout={'sha256': 'a' * 64, 'bytes': True}),
            lambda v: v['commands'][0].update(stderr={'sha256': 'a' * 64, 'bytes': 1024 ** 4 + 1}),
            lambda v: v['commands'][0].update(exit_code=7),
            lambda v: v['commands'][0].update(outcome='timed_out', exit_code=-9),
            lambda v: v['commands'].reverse(),
            lambda v: v['builds']['analytic'].update(lut_payloads_found=3),
            lambda v: v['builds']['ordinary'].update(frozen_artifact_count=16),
            lambda v: v['builds']['ordinary'].update(executable_bytes='MZpayload'),
            lambda v: v.update(commands=[]),
        ]
        for mutation in mutations:
            value = summary_fixture(True); mutation(value)
            with self.subTest(mutation=mutations.index(mutation)), self.assertRaises(ValueError):
                capture.validate_summary(value)

    def test_directory_rejects_extra_binary_noncanonical_duplicate_and_oversize_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); path = root / capture.EXPORT_NAME
            capture.write_json(path, summary_fixture())
            capture.validate_export(root)
            extra = root / 'payload.exe'; extra.write_bytes(b'MZbinary')
            with self.assertRaises(ValueError): capture.validate_export(root)
            extra.unlink()
            correct = path.read_bytes()
            for raw in (correct + b' ', correct.replace(b'"schema_version": 1,', b'"schema_version": 1, "schema_version": 1,'),
                        b' ' * (capture.MAX_EXPORT_BYTES + 1), b'\xff\x00MZ'):
                path.write_bytes(raw)
                with self.assertRaises((ValueError, UnicodeError)): capture.validate_export(root)

    def test_export_rejects_hardlink_and_symlink_aliases(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); evidence = root / 'export'; evidence.mkdir()
            source = root / 'private.json'; capture.write_json(source, summary_fixture())
            exported = evidence / capture.EXPORT_NAME
            os.link(source, exported)
            with self.assertRaises(ValueError): capture.validate_export(evidence)
            exported.unlink()
            try: exported.symlink_to(source)
            except OSError: self.skipTest('host does not grant symlink creation')
            with self.assertRaises(ValueError): capture.validate_export(evidence)

    def test_upload_gate_binds_invocation_and_private_result(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); evidence = root / 'export'; evidence.mkdir()
            private = root / 'private'; private.mkdir()
            value = summary_fixture()
            capture.write_json(evidence / capture.EXPORT_NAME, value)
            capture.write_json(private / 'progress.json', value)
            capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)
            with self.assertRaisesRegex(ValueError, 'invocation'):
                capture.validate_export(evidence, expected='b' * 40, repo=REPO, private=private)
            value['stage'] = 'fetch'; capture.write_json(evidence / capture.EXPORT_NAME, value)
            with self.assertRaisesRegex(ValueError, 'private completed result'):
                capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)

    def test_synthetic_passing_summary_cannot_be_exported_as_real_capture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); evidence = root / 'export'; evidence.mkdir()
            private = root / 'private'; private.mkdir()
            value = summary_fixture(True)
            capture.write_json(evidence / capture.EXPORT_NAME, value)
            capture.write_json(private / 'progress.json', value)
            # Exact local canonical source and captured bindings are absent; a
            # schema-valid fixture is deliberately insufficient for upload.
            with self.assertRaises((ValueError, OSError)):
                capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)

    def test_private_result_cannot_alias_export_by_hardlink_or_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); evidence = root / 'export'; evidence.mkdir()
            private = root / 'private'; private.mkdir()
            exported = evidence / capture.EXPORT_NAME
            capture.write_json(exported, summary_fixture())
            progress = private / 'progress.json'
            os.link(exported, progress)
            with self.assertRaises(ValueError):
                capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)
            progress.unlink()
            try: progress.symlink_to(exported)
            except OSError: self.skipTest('host does not grant symlink creation')
            with self.assertRaises(ValueError):
                capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)

    def test_export_command_status_must_match_real_private_journal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); evidence = root / 'export'; evidence.mkdir()
            private = root / 'private'; private.mkdir()
            value = summary_fixture(); value['stage'] = 'fetch'
            actual = capture.execute([sys.executable, '-c', 'raise SystemExit(9)'], cwd=REPO, env=os.environ.copy(),
                stdout=private / 'fetch.stdout', stderr=private / 'fetch.stderr',
                journal=private / 'fetch.command.json', timeout=10)
            value['commands'] = [{'id': 'fetch', **{k: actual[k] for k in ('outcome', 'exit_code', 'stdout', 'stderr')}}]
            capture.write_json(evidence / capture.EXPORT_NAME, value)
            capture.write_json(private / 'progress.json', value)
            # It really ran, but it was Python, not the exact authorized Cargo
            # fetch. Merely relabeling a subprocess must not be exportable.
            with self.assertRaisesRegex(ValueError, 'invocation differs'):
                capture.validate_export(evidence, expected='a' * 40, repo=REPO, private=private)


if __name__ == '__main__':
    unittest.main()
