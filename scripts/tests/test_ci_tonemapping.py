"""The CI runner must not mistake unrelated compiler failures for mode guards."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'ci_tonemapping', Path(__file__).resolve().parents[1] / 'ci-tonemapping.py')
CI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CI)
DIAGNOSTIC = 'analytic-tonemapping requires --no-default-features; select one tone mode'


class CommandStatusTests(unittest.TestCase):
    def invoke(self, exit_code, diagnostics=(), success=False, expected=None):
        messages = [
            {'reason': 'compiler-message', 'message': {'level': 'error', 'message': text}}
            for text in diagnostics
        ] + [{'reason': 'build-finished', 'success': success}]
        payload = '\n'.join(json.dumps(message) for message in messages)
        command = [sys.executable, '-c',
                   'import sys; print(sys.argv[1]); sys.exit(int(sys.argv[2]))',
                   payload, str(exit_code)]
        with tempfile.TemporaryDirectory() as directory, \
                contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            evidence = Path(directory)
            outcome = 0
            try:
                CI.run(evidence, os.environ.copy(), 'fixture', command, expected)
            except SystemExit as error:
                outcome = error.code
            record = json.loads((evidence / 'fixture.command.json').read_text())
            self.assertEqual(record['exit_code'], exit_code)
            self.assertEqual(record['command'], command)
            self.assertEqual((evidence / 'fixture.stdout').read_text(), payload + '\n')
            return outcome, record

    def test_successful_command_retains_zero_exit(self):
        self.assertEqual(self.invoke(0, success=True)[0], 0)

    def test_failed_command_preserves_nonzero_exit(self):
        self.assertEqual(self.invoke(17)[0], 17)

    def test_exact_compile_guard_rejection_is_accepted(self):
        outcome, record = self.invoke(101, [DIAGNOSTIC], expected=DIAGNOSTIC)
        self.assertEqual(outcome, 0)
        self.assertTrue(record['matched_expected_rejection'])

    def test_guard_disappearance_fails_even_with_expected_text(self):
        self.assertNotEqual(self.invoke(0, [DIAGNOSTIC], success=True,
                                       expected=DIAGNOSTIC)[0], 0)

    def test_unrelated_compiler_error_is_not_suppressed(self):
        self.assertEqual(self.invoke(101, ['could not find dependency'],
                                     expected=DIAGNOSTIC)[0], 101)
        self.assertEqual(self.invoke(101, [DIAGNOSTIC, 'cannot find value'],
                                     expected=DIAGNOSTIC)[0], 101)

    def test_failed_process_needs_failed_cargo_completion(self):
        self.assertEqual(self.invoke(101, [DIAGNOSTIC], success=True,
                                     expected=DIAGNOSTIC)[0], 101)
        self.assertEqual(self.invoke(130, [DIAGNOSTIC], expected=DIAGNOSTIC)[0], 130)


class ExplicitSourceRecipeTests(unittest.TestCase):
    def test_both_mode_audits_select_exact_two_lut_source_and_private_archive(self):
        for mode in ('ordinary', 'analytic'):
            command = CI.audit_command(mode, Path('/evidence/graph'), Path('/evidence/messages'),
                                       Path('/checkout'), Path('/private/reference.crate'))
            self.assertEqual(command[command.index('--mode') + 1], mode)
            self.assertEqual(command[command.index('--source-recipe') + 1], 'bevy-0.18.1-tony-filmic-source-v1')
            self.assertEqual(command[command.index('--upstream-archive') + 1], '/private/reference.crate')
            self.assertEqual(command[command.index('--source-root') + 1], '/checkout')
            self.assertEqual(command[command.index('--target') + 1], CI.TARGET)

    def test_runner_preserves_all_four_guards_and_analytic_feature_routing(self):
        from unittest import mock
        calls = []
        targets = {}
        def record(evidence, env, name, command, expected_diagnostic=None):
            calls.append((name, command, expected_diagnostic))
            targets[name] = env.get('CARGO_TARGET_DIR')
            return evidence / (name + '.stdout')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def output(command, **kwargs):
                if command[:2] == ['git', 'status']: return ''
                return 'synthetic-provenance'
            with mock.patch.object(sys, 'argv', ['ci-tonemapping.py', str(root / 'evidence'), str(root / 'build'),
                                                '--upstream-archive', str(root / 'private.crate')]), \
                    mock.patch.object(CI, 'inspect_source', return_value={'source_recipe': CI.SOURCE_RECIPE}), \
                    mock.patch.object(CI, 'run', side_effect=record), \
                    mock.patch.object(CI.subprocess, 'check_output', side_effect=output), \
                    mock.patch.dict(os.environ, {'RUSTFLAGS': '-D warnings'}, clear=True):
                CI.main()
            guards = {name: diagnostic for name, _, diagnostic in calls if diagnostic}
            self.assertEqual(set(guards), {'app-mixed', 'render-mixed', 'app-neither', 'sun-clock-neither'})
            self.assertNotEqual(targets['analytic-clippy'], targets['analytic-app-build'])
            self.assertNotEqual(targets['app-mixed'], targets['ordinary-app-build'])
            self.assertTrue(json.loads((root / 'evidence/qa-target-cleanup.json').read_text())['completed'])
            self.assertEqual(guards['app-mixed'], DIAGNOSTIC)
            self.assertEqual(guards['render-mixed'], DIAGNOSTIC)
            for name, command, _ in calls:
                if name in ('analytic-flightsim-render-tests', 'analytic-flightsim-app-tests', 'analytic-app-build'):
                    self.assertIn('--no-default-features', command)
                    self.assertEqual(command[command.index('--features') + 1], 'analytic-tonemapping')
                if name.endswith('-app-audit'):
                    self.assertEqual(command[command.index('--source-recipe') + 1], CI.SOURCE_RECIPE)
            provenance = json.loads((root / 'evidence/provenance.json').read_text())
            self.assertEqual(provenance['source_recipe'], CI.SOURCE_RECIPE)
            self.assertFalse(provenance['release_admitted'])
            self.assertFalse(provenance['native_qualified'])

    def test_private_archive_cannot_be_retained_with_source_or_ci_evidence(self):
        from unittest import mock
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for archive in (Path.cwd() / 'private.crate', root / 'evidence/reference.crate', root / 'build/reference.crate'):
                with self.subTest(archive=archive), \
                        mock.patch.object(sys, 'argv', ['ci-tonemapping.py', str(root / 'evidence'), str(root / 'build'),
                                                        '--upstream-archive', str(archive)]), \
                        mock.patch.object(CI, 'inspect_source') as inspect, \
                        self.assertRaises(SystemExit):
                    CI.main()
                inspect.assert_not_called()

    def test_cleanup_is_limited_to_fresh_owned_qa_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence, build = root / 'evidence', root / 'build'
            evidence.mkdir(); build.mkdir()
            environments = {mode: {'CARGO_TARGET_DIR': str(build / ('qa-' + mode))}
                            for mode in ('ordinary', 'analytic')}
            owned = CI.create_qa_targets(evidence, build, environments)
            for value in environments.values():
                path = Path(value['CARGO_TARGET_DIR']); (path / 'scratch').write_text('owned')
            final = build / 'ordinary'; final.mkdir(); (final / 'keep').write_text('final')
            CI.discard_qa_targets(evidence, build, environments, owned)
            self.assertTrue((final / 'keep').exists())
            self.assertTrue(all(not Path(value['CARGO_TARGET_DIR']).exists() for value in environments.values()))
            environments['ordinary']['CARGO_TARGET_DIR'] = str(final)
            with self.assertRaises(SystemExit): CI.discard_qa_targets(evidence, build, environments, owned)
            self.assertTrue((final / 'keep').exists())

    def test_existing_roots_are_refused_before_any_build_or_deletion(self):
        from unittest import mock
        for existing in ('evidence', 'build'):
            with self.subTest(existing=existing), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                preserved = root / existing; preserved.mkdir(); (preserved / 'keep').write_text('existing')
                with mock.patch.object(sys, 'argv', ['ci-tonemapping.py', str(root / 'evidence'), str(root / 'build'),
                                                    '--upstream-archive', str(root / 'private.crate')]), \
                     mock.patch.object(CI, 'inspect_source', return_value={}), \
                     mock.patch.object(CI, 'run') as run, \
                     mock.patch.object(CI.subprocess, 'check_output') as process, self.assertRaises(SystemExit):
                    CI.main()
                run.assert_not_called(); process.assert_not_called()
                self.assertEqual((preserved / 'keep').read_text(), 'existing')

    def test_cleanup_refuses_replaced_and_symlinked_qa_directories(self):
        for mutation in ('renamed', 'symlink', 'wrong-ownership'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); evidence = root / 'evidence'; build = root / 'build'
                evidence.mkdir(); build.mkdir()
                environments = {mode: {'CARGO_TARGET_DIR': str(build / ('qa-' + mode))}
                                for mode in ('ordinary', 'analytic')}
                owned = CI.create_qa_targets(evidence, build, environments)
                path = build / 'qa-ordinary'; (path / 'keep').write_text('owned')
                if mutation == 'wrong-ownership': owned[str(path)][1] = -1
                else:
                    path.rename(build / 'saved-original')
                    if mutation == 'symlink': path.symlink_to(build / 'saved-original', target_is_directory=True)
                    else: path.mkdir(); (path / 'keep').write_text('replacement')
                with self.assertRaises(SystemExit): CI.discard_qa_targets(evidence, build, environments, owned)
                self.assertTrue((path / 'keep').exists())

    def test_supplied_root_aliases_are_rejected_before_resolution(self):
        from unittest import mock
        for selected in ('evidence', 'build'):
            for kind in ('dangling', 'parent'):
                with self.subTest(selected=selected, kind=kind), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    destination = root / 'destination'
                    if kind == 'parent': destination.mkdir()
                    alias = root / 'alias'
                    try:
                        alias.symlink_to(destination, target_is_directory=True)
                    except OSError as error:
                        if os.name == 'nt' and getattr(error, 'winerror', None) == 1314:
                            self.skipTest('Windows fixture cannot create symlink without privilege')
                        raise
                    values = {'evidence': root / 'evidence', 'build': root / 'build'}
                    values[selected] = alias if kind == 'dangling' else alias / 'child'
                    with mock.patch.object(sys, 'argv', ['ci-tonemapping.py', str(values['evidence']), str(values['build']),
                                                         '--upstream-archive', str(root / 'private.crate')]), \
                         mock.patch.object(CI, 'inspect_source') as inspect, \
                         mock.patch.object(CI, 'run') as run, self.assertRaisesRegex(SystemExit, 'aliased CI directory'):
                        CI.main()
                    inspect.assert_not_called(); run.assert_not_called()
                    self.assertTrue(alias.is_symlink())
                    self.assertFalse((destination / 'child').exists() if kind == 'parent' else destination.exists())

    def test_final_targets_injected_during_qa_are_refused_before_final_commands(self):
        from unittest import mock
        for mode in ('ordinary', 'analytic'):
            for kind in ('existing', 'symlink'):
                with self.subTest(mode=mode, kind=kind), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory); build = root / 'build'; calls = []
                    saved = root / 'saved'; saved.mkdir(); (saved / 'keep').write_text('preserved')
                    target = build / mode
                    def record(evidence, env, name, command, expected_diagnostic=None):
                        calls.append(name)
                        if name == 'render-library-neither':
                            if kind == 'existing':
                                target.mkdir(); (target / 'keep').write_text('preserved')
                            else:
                                try: target.symlink_to(saved, target_is_directory=True)
                                except OSError as error:
                                    if os.name == 'nt' and getattr(error, 'winerror', None) == 1314:
                                        self.skipTest('Windows fixture cannot create symlink without privilege')
                                    raise
                        return evidence / (name + '.stdout')
                    def output(command, **kwargs):
                        return '' if command[:2] == ['git', 'status'] else 'synthetic-provenance'
                    with mock.patch.object(sys, 'argv', ['ci-tonemapping.py', str(root / 'evidence'), str(build),
                                                         '--upstream-archive', str(root / 'private.crate')]), \
                         mock.patch.object(CI, 'inspect_source', return_value={}), \
                         mock.patch.object(CI, 'run', side_effect=record), \
                         mock.patch.object(CI.subprocess, 'check_output', side_effect=output), \
                         mock.patch.dict(os.environ, {'RUSTFLAGS': '-D warnings'}, clear=True), \
                         self.assertRaises(SystemExit):
                        CI.main()
                    self.assertIn('render-library-neither', calls)
                    for suffix in ('graph', 'build', 'audit'):
                        self.assertNotIn(mode + '-app-' + suffix, calls)
                    self.assertEqual((target / 'keep').read_text(), 'preserved')
                    self.assertEqual((saved / 'keep').read_text(), 'preserved')

    def test_workflow_does_not_upload_private_reference_archive(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()
        job = workflow.split('\n  analytic-tonemapping:', 1)[1].split('\n  architecture:', 1)[0]
        self.assertIn('tonemapping-private', job)
        self.assertIn('--upstream-archive', job)
        self.assertIn('4d0810e85c2436e50c67448d48a83bf0bb1b5849899619ae2c7ea817221e9172', job)
        upload = job.split('Retain source/build evidence', 1)[1]
        self.assertIn('path: ${{ runner.temp }}/tonemapping-evidence/', upload)
        self.assertNotIn('tonemapping-private', upload)
        self.assertNotIn('path: ${{ runner.temp }}/\n', upload)


if __name__ == '__main__':
    unittest.main()
