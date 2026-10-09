"""Adversarial synthetic final-release integration; no native run or real grants.

Only native build/source capture, installed-runtime collection, distribution-info
execution, and the Windows smoke subprocess are substituted. The unchanged real
ordinary gate, copy plan, payload projector, applicability validator, ZIP writer,
ZIP auditor, extraction, journal/PNG verification, and exported-artifact checks
run end to end. Native parser specifics have their own focused test suite.
"""
import contextlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import zipfile


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HERE = Path(__file__).resolve().parent
q = load('ordinary_full_release_tests', HERE.parent / 'prepare-ordinary-full-release.py')
reviews = load('ordinary_full_review_fixtures', HERE / 'test_ordinary_release_applicability.py')
archives = load('ordinary_full_archive_fixtures', HERE / 'test_ordinary_release_archive.py')
SYNTHETIC = reviews.SYNTHETIC


class OrdinaryFullReleaseTests(unittest.TestCase):
    def setUp(self):
        self.f = reviews.ApplicabilityFixture(self)
        self.f.fixture.write(q.NOTES_TEMPLATE,
            'SYNTHETIC RELEASE DISCLOSURE ONLY: @SOURCE_SHA@ @SOURCE_SHA@ @SOURCE_SHA@\n')
        self.f.fixture.commit()
        self.f.review['source_content_files'] = q.applicability.source_content(self.f.repo)
        self.f.save()
        self.p = self.f.payload
        self.repo, self.root = self.p.repo, self.p.root
        self.private, self.output = self.root / 'final-private', self.root / 'release-artifacts'
        self.top = 'flightsim-claude-v1.2.3-windows-x86_64'
        self.staged = self.private / 'staged' / self.top
        self.extracted = self.private / 'extracted' / self.top
        self.frozen = self.output / (self.top + '.zip')
        self.execution_calls, self.native_calls = [], []
        self.runtime_calls = []
        self.fail_scene = None
        self.change_executable_during_smoke = False

    def arguments(self):
        return (self.repo, self.p.expected, self.p.private, self.p.text, self.private, self.output)

    def source(self, repo, expected):
        self.assertEqual(repo, self.repo)
        self.assertEqual(expected, self.f.fixture.git('rev-parse', 'HEAD').stdout.decode().strip())
        return copy.deepcopy(self.p.source)

    def collect_runtime(self, private, **kwargs):
        self.runtime_calls.append((private, kwargs))
        self.assertEqual(kwargs, {'source_sha': self.p.expected, 'recipe_cfg_args': ['-D', 'warnings'],
            'linker_trace': self.p.private / 'capture/ordinary/build.stderr',
            'audited_executable': self.p.executable})
        private.mkdir()
        (private / q.native.runtime_facts.PROJECTION_NAME).write_bytes(q.canonical({'synthetic': SYNTHETIC}))

    def native(self, repo, expected, build_private, build_text, bundle, *, runtime_facts_private):
        self.native_calls.append(bundle)
        self.assertEqual(runtime_facts_private, self.private / 'runtime-facts')
        # A real payload projection ensures replacing the native parser does not
        # replace the same-build executable, inventory, notice or source gate.
        packet = q.payload.project_payload(repo, expected, build_private, build_text, bundle)
        runtime = q.capture.file_record(runtime_facts_private / q.native.runtime_facts.PROJECTION_NAME)
        return {'synthetic': SYNTHETIC, 'payload_projection': packet,
                'content_view': copy.deepcopy(self.f.native['content_view']),
                'runtime_facts_binding': runtime, 'release_authorized': False,
                'dependency_review_approved': False, 'runtime_accepted': False}

    def distribution_info(self, executable):
        self.assertEqual(executable, self.p.executable)
        return {'schema_version': 1, 'package': 'flightsim-app', 'package_version': '1.2.3',
            'profile': 'development', 'region_downloads': False,
            'default_aircraft': 'light-single', 'default_model': 'aircraft/light_single.glb',
            'bundled_aircraft': ['light-single', 'swift-sport'], 'target_os': 'windows',
            'target_arch': 'x86_64', 'target_env': 'msvc', 'release_authorized': False}

    def execute(self, command, *, cwd, env, stdout, stderr, journal, timeout):
        self.execution_calls.append(command)
        self.assertEqual(Path(command[0]), self.extracted / 'flightsim-app.exe')
        self.assertEqual(cwd, self.extracted)
        self.assertEqual(q.capture.file_record(Path(command[0])), q.capture.file_record(self.p.executable))
        self.assertEqual({key: env[key] for key in q.archive.WARP}, q.archive.WARP)
        self.assertFalse({'BEVY_ASSET_ROOT', 'CARGO_MANIFEST_DIR'} & {key.upper() for key in env})
        self.assertEqual(timeout, 180)
        self.assertEqual(command[1], '--internal-release-smoke')
        self.assertEqual(len(command), 4)
        aircraft = q.archive.SCENES[command[2]][0]
        screenshot = Path(command[3])
        stdout.write_text('SYNTHETIC UNIT TEST MARKERS, NOT A NATIVE RUN\n' + q.archive.DIAGNOSTIC_MARKER + '\n' +
                          'aircraft model fitted: (' + aircraft + ')\n'
                          'Screenshot saved to fixture.png\nBatch capture complete: status 0\n', encoding='utf-8')
        stderr.write_bytes(b'')
        screenshot.write_bytes(archives.png())
        value = {'command': command, 'cwd': str(cwd), 'exit_code': 0, 'outcome': 'succeeded',
                 'timeout_seconds': timeout, 'elapsed_seconds': 0.1,
                 'stdout': q.capture.file_record(stdout), 'stderr': q.capture.file_record(stderr)}
        if self.fail_scene == aircraft:
            value.update(exit_code=None, outcome='timed_out', elapsed_seconds=180.1)
        if self.change_executable_during_smoke:
            Path(command[0]).write_bytes(b'SYNTHETIC REPLACEMENT EXECUTABLE')
        q.capture.write_json(journal, value)
        return value

    @contextlib.contextmanager
    def boundaries(self):
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(q.sys, 'platform', 'win32'))
            stack.enter_context(patch.object(q.archive.platform, 'machine', return_value='AMD64'))
            stack.enter_context(patch.object(q.capture, 'source_evidence', side_effect=self.source))
            stack.enter_context(patch.object(q.capture, 'validate_export', return_value=self.p.verified))
            stack.enter_context(patch.object(q.release.staging, 'read_distribution_info', side_effect=self.distribution_info))
            stack.enter_context(patch.object(q.native.runtime_facts, 'collect_runtime_facts', side_effect=self.collect_runtime))
            stack.enter_context(patch.object(q.native, 'project', side_effect=self.native))
            stack.enter_context(patch.object(q.archive.capture, 'execute', side_effect=self.execute))
            yield

    def prepare(self):
        with self.boundaries():
            return q.prepare(*self.arguments())

    def validate(self):
        with self.boundaries():
            return q.validate(*self.arguments())

    def test_one_audited_executable_reaches_exact_archive_and_both_extracted_smokes(self):
        result = self.prepare()
        expected_files = q.payload.snapshot_payload(self.staged)
        self.assertEqual(result['files'], expected_files)
        self.assertEqual(q.payload.snapshot_payload(self.extracted), expected_files)
        self.assertEqual(expected_files['flightsim-app.exe'], self.p.verified['builds']['ordinary']['executable'])
        self.assertEqual(result['bindings']['archive'], q.archive.audit_archive(self.frozen, expected_files, self.top))
        self.assertEqual(len(self.execution_calls), 2)
        self.assertEqual(len(self.runtime_calls), 1)
        self.assertGreaterEqual(len(self.native_calls), 5)
        self.assertEqual(set(self.native_calls), {self.staged})
        self.assertEqual({q.archive.SCENES[c[2]][0] for c in self.execution_calls}, {'light-single', 'swift-sport'})
        self.assertEqual(set(result['scenes']), set(q.archive.SCENES))
        component = result['component_terms']
        self.assertEqual(component['source_sha'], self.p.expected)
        self.assertEqual(component['source_tree'], self.p.source['source_tree'])
        self.assertEqual(component['executable'], expected_files['flightsim-app.exe'])
        self.assertEqual(component['documents'], {name: expected_files[name] for name in q.COMPONENT_FILES})
        self.assertIs(component['recipient_assent_collected'], False)
        self.assertIs(component['dialog_tested_by_smoke'], False)
        self.assertNotIn(b'@SOURCE_SHA@', (self.output / q.NOTES).read_bytes())
        self.assertEqual(result['bindings']['publication_notes'], q.capture.file_record(self.output / q.NOTES))
        for key in ('release_authorized', 'dependency_review_approved', 'runtime_accepted', 'appearance_accepted'):
            self.assertIs(result[key], False)
        self.assertEqual(self.f.fixture.git('status', '--porcelain').stdout, b'')

    def test_component_disclosure_output_and_schedule_cannot_be_changed(self):
        self.prepare()
        for name in (q.NOTES, q.PUBLIC):
            path = self.output / name; original = path.read_bytes()
            path.write_bytes(original + b'changed')
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.validate()
            path.write_bytes(original)
        value = json.loads((self.output / q.PUBLIC).read_bytes())
        value['component_terms']['recipient_assent_collected'] = True
        (self.output / q.PUBLIC).write_bytes(q.canonical(value))
        with self.assertRaisesRegex(ValueError, 'final evidence changed'):
            self.validate()

    def test_component_disclosure_requires_exact_source_and_closed_template(self):
        for source in ('main', '0' * 39, 'G' * 40, None):
            with self.subTest(source=source), self.assertRaises(ValueError):
                q.publication_notes(self.repo, source)
        self.f.fixture.write(q.NOTES_TEMPLATE, 'missing exact source links')
        self.f.fixture.commit()
        with self.assertRaisesRegex(ValueError, 'disclosure template'):
            q.publication_notes(self.repo, self.p.expected)

    def test_validate_only_is_read_only_and_does_not_run_native_commands(self):
        expected = self.prepare()
        before_private = q.payload.snapshot_payload(self.private)
        before_output = q.payload.snapshot_payload(self.output)
        with self.boundaries(), patch.object(q.archive.capture, 'execute', side_effect=AssertionError('must not execute')), \
                patch.object(q.native.runtime_facts, 'collect_runtime_facts', side_effect=AssertionError('must not collect')):
            self.assertEqual(q.validate(*self.arguments()), expected)
        self.assertEqual(q.payload.snapshot_payload(self.private), before_private)
        self.assertEqual(q.payload.snapshot_payload(self.output), before_output)
        self.assertEqual(len(self.execution_calls), 2)

    def test_non_windows_preparation_does_not_create_outputs(self):
        with patch.object(q.sys, 'platform', 'linux'), self.assertRaisesRegex(ValueError, 'native Windows'):
            q.prepare(*self.arguments())
        self.assertFalse(self.private.exists()); self.assertFalse(self.output.exists())

    def test_unfinished_capture_and_replaced_audited_executable_stop_before_staging(self):
        original = self.p.verified['status']
        self.p.verified['status'] = 'not_a_completed_audit'
        with self.assertRaisesRegex(ValueError, 'completed same-build audit'):
            self.prepare()
        self.p.verified['status'] = original
        self.p.executable.write_bytes(b'SYNTHETIC NONAUDITED BINARY')
        with self.assertRaisesRegex(ValueError, 'audited executable changed'):
            self.prepare()
        self.assertFalse(self.private.exists()); self.assertFalse(self.output.exists())

    def test_other_distribution_recipe_rejected_before_staging(self):
        with self.boundaries(), patch.object(q.release.staging, 'read_distribution_info',
                return_value={**self.distribution_info(self.p.executable), 'default_aircraft': 'swift-sport'}), \
                self.assertRaisesRegex(ValueError, 'expected offline default Windows'):
            q.prepare(*self.arguments())
        self.assertFalse(self.private.exists()); self.assertFalse(self.output.exists())

    def test_stale_authorization_never_collects_or_stages(self):
        self.f.fixture.write('README.md', 'SYNTHETIC NEW SOURCE')
        self.f.fixture.commit()
        self.f.refresh_source()
        with self.assertRaisesRegex(ValueError, 'gate is blocked'):
            self.prepare()
        self.assertEqual(self.runtime_calls, [])
        self.assertFalse(self.private.exists()); self.assertFalse(self.output.exists())

    def test_missing_applicability_stops_before_runtime_collection_or_staging(self):
        self.f.fixture.git('rm', q.applicability.PATH)
        self.f.fixture.commit(); self.f.fixture.authorize_fixture(); self.f.refresh_source()
        with self.assertRaisesRegex(ValueError, 'committed real applicability review'):
            self.prepare()
        self.assertEqual(self.runtime_calls, [])
        self.assertEqual(self.execution_calls, [])
        self.assertFalse(self.private.exists()); self.assertFalse(self.output.exists())

    def test_unresolved_native_conditions_cannot_produce_archive(self):
        self.f.review['coverage']['unresolved_conditions'] = [SYNTHETIC]
        self.f.save()
        with self.assertRaisesRegex(ValueError, 'conditions remain unresolved'):
            self.prepare()
        self.assertFalse(self.frozen.exists())
        self.assertFalse((self.output / q.PUBLIC).exists())
        self.assertEqual(self.execution_calls, [])

    def test_fresh_roots_and_disjoint_inputs_are_required(self):
        self.private.mkdir()
        with self.assertRaisesRegex(ValueError, 'fresh preparation'):
            self.prepare()
        self.private.rmdir()
        args = list(self.arguments()); args[-1] = self.repo / 'nested-output'
        with self.boundaries(), self.assertRaises(ValueError):
            q.prepare(*args)
        self.assertFalse(self.private.exists())

    def test_smoke_timeout_preserves_original_failure_and_never_exports_success(self):
        self.fail_scene = 'swift-sport'
        with self.assertRaisesRegex(ValueError, 'screenshot did not exit zero'):
            self.prepare()
        journal = self.private / 'smoke/commands/swift-sport-chase/journal.json'
        self.assertEqual(json.loads(journal.read_bytes())['outcome'], 'timed_out')
        self.assertFalse((self.output / q.PUBLIC).exists())
        self.assertFalse((self.private / 'smoke/smoke.json').exists())

    def test_smoke_must_use_unchanged_extracted_executable(self):
        self.change_executable_during_smoke = True
        with self.assertRaisesRegex(ValueError, 'runtime changed frozen ordinary bundle'):
            self.prepare()
        self.assertFalse((self.output / q.PUBLIC).exists())

    def test_exact_archive_staged_and_extracted_bytes_are_independently_rechecked(self):
        self.prepare()
        targets = (self.staged / 'flightsim-app.exe', self.extracted / 'flightsim-app.exe',
                   self.staged / 'README.md', self.extracted / 'README.md', self.p.executable)
        for path in targets:
            with self.subTest(path=path):
                original = path.read_bytes(); path.write_bytes(b'SYNTHETIC TAMPER')
                with self.assertRaises(ValueError): self.validate()
                path.write_bytes(original)
        archive = self.frozen.read_bytes()
        # A well-formed replacement archive must also fail exact member hashing.
        with zipfile.ZipFile(self.frozen, 'w', compression=zipfile.ZIP_DEFLATED) as zipped:
            for name in sorted(q.payload.snapshot_payload(self.staged)):
                content = (self.staged / name).read_bytes()
                if name == 'flightsim-app.exe': content = b'SYNTHETIC DIFFERENT EXECUTABLE'
                zipped.writestr(self.top + '/' + name, content)
        with self.assertRaises(ValueError): self.validate()
        self.frozen.write_bytes(archive)
        self.assertEqual(self.validate()['bindings']['archive'], q.capture.file_record(self.frozen))

    def test_missing_retained_inputs_fail_without_recapture_or_reconstruction(self):
        self.prepare()
        paths = (self.p.summary, self.p.metadata, self.p.captured, self.p.executable,
                 self.private / 'runtime-facts' / q.native.runtime_facts.PROJECTION_NAME,
                 self.private / 'smoke/smoke.json', self.frozen)
        for path in paths:
            with self.subTest(path=path):
                original = path.read_bytes(); path.unlink()
                with self.assertRaises((ValueError, OSError)):
                    self.validate()
                self.assertFalse(path.exists())
                path.write_bytes(original)
        self.assertEqual(len(self.execution_calls), 2)
        self.assertEqual(len(self.runtime_calls), 1)

    def test_smoke_journal_png_and_manifest_cannot_be_replaced(self):
        self.prepare()
        for name in ('smoke.json', 'flightsim-windows-smoke.png', 'flightsim-windows-smoke-swift.png',
                     'commands/light-single-cockpit/stdout', 'commands/swift-sport-chase/stderr',
                     'commands/light-single-cockpit/journal.json'):
            with self.subTest(name=name):
                path = self.private / 'smoke' / name
                original = path.read_bytes(); path.write_bytes(b'SYNTHETIC TAMPER')
                with self.assertRaises(ValueError): self.validate()
                path.write_bytes(original)

    def test_every_exported_artifact_and_extra_members_are_verified(self):
        self.prepare()
        for name in sorted(q.payload.snapshot_payload(self.output)):
            if name.endswith('.zip'): continue
            with self.subTest(name=name):
                path = self.output / name; original = path.read_bytes()
                path.write_bytes(b'SYNTHETIC TAMPER')
                with self.assertRaises(ValueError): self.validate()
                path.write_bytes(original)
        (self.output / 'unexpected-private-input.json').write_bytes(b'SYNTHETIC EXTRA')
        with self.assertRaisesRegex(ValueError, 'unexpected.*release-artifact member'):
            self.validate()
        (self.output / 'unexpected-private-input.json').unlink()
        (self.output / 'empty-directory').mkdir()
        with self.assertRaisesRegex(ValueError, 'empty or unexplained directories'):
            self.validate()

    def test_native_facts_cannot_change_between_projection_and_final_recheck(self):
        self.prepare()
        original = self.native
        calls = 0
        def changed(*args, **kwargs):
            nonlocal calls
            calls += 1
            result = original(*args, **kwargs)
            if calls == 2: result['synthetic'] += ' changed'
            return result
        with self.boundaries(), patch.object(q.native, 'project', side_effect=changed), \
                self.assertRaisesRegex(ValueError, 'native facts changed'):
            q.validate(*self.arguments())

    def test_smoke_originals_changed_during_final_native_recheck_fail(self):
        self.prepare()
        original = self.native
        calls = 0
        def changed(*args, **kwargs):
            nonlocal calls
            calls += 1
            result = original(*args, **kwargs)
            if calls == 2:
                (self.private / 'smoke/commands/light-single-cockpit/stdout').write_bytes(
                    b'SYNTHETIC CHANGED ORIGINAL AFTER SMOKE CHECK')
            return result
        with self.boundaries(), patch.object(q.native, 'project', side_effect=changed), \
                self.assertRaises(ValueError):
            q.validate(*self.arguments())

    def test_export_changed_after_its_read_fails_final_artifact_verification(self):
        self.prepare()
        original = q.capture.file_record
        original_project = q.project
        screenshot = self.output / 'flightsim-claude-v1.2.3-windows-smoke.png'
        changed, projected = False, False
        def project_then_arm(*args, **kwargs):
            nonlocal projected
            result = original_project(*args, **kwargs)
            projected = True
            return result
        def mutate_after_read(path):
            nonlocal changed
            if projected and path == screenshot and not changed:
                changed = True
                (self.output / q.PUBLIC).write_bytes(b'SYNTHETIC LATE EVIDENCE CHANGE')
            return original(path)
        with self.boundaries(), patch.object(q, 'project', side_effect=project_then_arm), \
                patch.object(q.capture, 'file_record', side_effect=mutate_after_read), self.assertRaises(ValueError):
            q.validate(*self.arguments())
        self.assertTrue(changed)

    def test_validate_only_cli_handles_corrupt_archive_without_private_traceback(self):
        self.prepare()
        self.frozen.write_bytes(b'SYNTHETIC CORRUPT ZIP')
        args = ['--repo', str(self.repo), '--expected-sha', self.p.expected,
                '--build-private', str(self.p.private), '--build-text', str(self.p.text),
                '--private', str(self.private), '--output', str(self.output), '--validate-only']
        stdout, stderr = io.StringIO(), io.StringIO()
        with self.boundaries(), contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            self.assertEqual(q.main(args), 1)
        self.assertEqual(stdout.getvalue(), '')
        self.assertIn('Ordinary final evidence blocked', stderr.getvalue())
        self.assertNotIn(str(self.root), stderr.getvalue())
        self.assertNotIn('Traceback', stderr.getvalue())


if __name__ == '__main__':
    unittest.main()
