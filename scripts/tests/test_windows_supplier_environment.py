"""Synthetic environment/export tests; never claims an actual Windows capture."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SCRIPTS = Path(__file__).parents[1]
probe = load('supplier_probe', SCRIPTS / 'probe-windows-supplier-environment.py')
fixtures = load('runtime_fixtures', Path(__file__).with_name('test_analytical_runtime_facts.py'))


class SupplierEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.RuntimeFactsTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.private = self.fixture.private
        self.evidence = self.fixture.root / 'public'
        self.sha = 'a' * 40
        # Synthetic public image metadata, unrelated to the real test host.
        patcher = patch.dict(probe.os.environ, {'ImageOS': 'win25', 'ImageVersion': '20261001.1.0'})
        patcher.start()
        self.addCleanup(patcher.stop)

    def export(self, **kwargs):
        self.fixture.collect(**kwargs)
        probe.export(self.private, self.evidence, self.sha)
        return json.loads((self.evidence / probe.NAME).read_bytes()), json.loads(
            (self.evidence / probe.facts.PROJECTION_NAME).read_bytes())

    def test_exact_environment_export_replays_installed_candidates_without_leaks(self):
        receipt, runtime = self.export()
        probe.validate_export(self.private, self.evidence, self.sha)
        self.assertEqual(receipt['scope'], 'environment_only')
        self.assertEqual(receipt['application_build_inputs'], 'not_supplied')
        self.assertEqual(receipt['probe_source_sha'], self.sha)
        self.assertEqual(receipt['probe_base_sha'], '55094fa928fbb8f907a2103740ee6824171c26e7')
        self.assertEqual(receipt['runner_image'], {'image_os': 'win25', 'image_version': '20261001.1.0'})
        self.assertTrue(runtime['rust']['recipe_settings_supplied'])
        self.assertFalse(runtime['rust']['actual_compiler_invocation_observed'])
        self.assertEqual(runtime['final_link'], probe.facts.final_link_facts(None))
        self.assertEqual(runtime['microsoft']['visual_studio_candidates'][0]['product'],
                         'Microsoft.VisualStudio.Product.Enterprise')
        self.assertEqual(runtime['microsoft']['sdk_candidates'][0]['version'], '10.0.26100.0')
        self.assertEqual([row['id'] for row in runtime['query_bindings']],
                         ['rustc-version', 'rustc-default-cfg', 'rustc-recipe-cfg', 'rustc-sysroot', 'vswhere'])
        self.assertEqual(receipt['runtime_facts'], probe.facts.file_record(self.evidence / probe.facts.PROJECTION_NAME))
        self.assertEqual((self.evidence / probe.facts.PROJECTION_NAME).read_bytes(),
                         (self.private / probe.facts.PROJECTION_NAME).read_bytes())
        self.assertEqual({path.name for path in self.evidence.iterdir()}, {probe.NAME, probe.facts.PROJECTION_NAME})
        for path in self.evidence.iterdir():
            for secret in (b'PRIVATE', b'Program Files', b'Users', b'"command"', b'"source"',
                           str(self.fixture.root).encode()):
                self.assertNotIn(secret, path.read_bytes())
        self.assertFalse(runtime['release_authorized'])
        self.assertFalse(runtime['dependency_review_approved'])
        self.assertFalse(runtime['native_runtime_coverage_complete'])

    def test_missing_queries_and_candidates_remain_unknown(self):
        self.fixture.outputs = {}
        _, value = self.export(env={})
        self.assertEqual(value['rust']['identity']['status'], 'not_established')
        self.assertFalse(value['rust']['identity']['matches_required_identity'])
        self.assertEqual(value['microsoft']['visual_studio_candidates'], [])
        self.assertEqual(value['microsoft']['sdk_candidates'], [])
        self.assertEqual(value['final_link']['reason'], 'not_requested')

    def test_image_identity_is_bounded_and_optional(self):
        self.assertEqual(probe.image_identity({}), {'image_os': None, 'image_version': None})
        for env in ({'ImageOS': '/private/path'}, {'ImageVersion': 'url?token=secret'},
                    {'ImageOS': 'win25', 'IMAGEOS': 'win22'}, {'ImageVersion': '1.' * 100}):
            with self.subTest(env=env), self.assertRaises(ValueError):
                probe.image_identity(env)
        self.assertEqual(probe.image_identity({'IMAGEOS': 'win25', 'IMAGEVERSION': '20261001.1.0'}),
                         {'image_os': 'win25', 'image_version': '20261001.1.0'})

    def test_exact_official_vs2026_image_token_is_admitted(self):
        # Official source for the failed run's exact image release:
        # actions/runner-images, win25-vs2026/20260925.250,
        # helpers/GenerateResourcesAndImage.ps1, Get-PackerTemplate.
        with patch.dict(probe.os.environ, {'ImageOS': 'win25-vs2026', 'ImageVersion': '20260925.250.1'}):
            receipt, _ = self.export()
            self.assertEqual(receipt['runner_image'],
                             {'image_os': 'win25-vs2026', 'image_version': '20260925.250.1'})
            probe.validate_export(self.private, self.evidence, self.sha)
        for value in ('win25-vs2027', 'win25-secret', 'win25-vs2026/secret',
                      'win25-vs2026\\secret', 'win25-vs2026?token=secret', 'win25-vs2026 PRIVATE'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                probe.image_identity({'ImageOS': value})

    def test_image_metadata_failure_is_preflight_and_source_label_only(self):
        with patch.dict(probe.os.environ, {'ImageOS': 'PRIVATE PATH'}):
            with patch.object(probe.facts, 'collect_runtime_facts') as collect:
                with patch('sys.stderr', new_callable=io.StringIO) as stderr:
                    code = probe.main(['--private', str(self.private), '--evidence', str(self.evidence),
                                       '--source-sha', self.sha])
        collect.assert_not_called()
        self.assertEqual(code, 1)
        self.assertIn('[image_identity:invalid_identity]', stderr.getvalue())
        self.assertNotIn('PRIVATE PATH', stderr.getvalue())

    def test_error_codes_never_echo_untrusted_text(self):
        cases = [(ValueError('unsafe identity'), 'invalid_identity'),
                 (ValueError('aggregate file budget exceeded'), 'aggregate_file_budget'),
                 (ValueError('unsafe identity PRIVATE'), 'invalid_value'),
                 (OSError('PRIVATE PATH'), 'io_error'), (KeyError('PRIVATE'), 'missing_field'),
                 (TypeError('PRIVATE'), 'invalid_type'),
                 (UnicodeError('PRIVATE'), 'encoding_error'),
                 (probe.subprocess.SubprocessError('PRIVATE'), 'subprocess_error')]
        for error, expected in cases:
            with self.subTest(kind=type(error).__name__):
                self.assertEqual(probe.error_code(error), expected)
                self.assertNotIn('PRIVATE', probe.error_code(error))

    def test_public_schema_and_no_build_claims_are_closed(self):
        _, original = self.export()
        mutations = [
            lambda v: v.update(host_path='PRIVATE'),
            lambda v: v.update(source_sha='b' * 40),
            lambda v: v.update(release_authorized=True),
            lambda v: v['microsoft'].update(selected_sdk='10.0.26100.0'),
            lambda v: v['final_link'].update(reason='missing_inputs'),
            lambda v: v['rust'].update(recipe_settings_supplied=False),
        ]
        for mutate in mutations:
            value = copy.deepcopy(original)
            mutate(value)
            with self.subTest(value=value), self.assertRaises(ValueError):
                probe.validate_runtime(value, self.sha)

    def test_different_recipe_cfg_cannot_be_relabelled(self):
        self.fixture.collect(recipe=False)
        with self.assertRaisesRegex(ValueError, 'warnings-only'):
            probe.export(self.private, self.evidence, self.sha)
        self.assertFalse(self.evidence.exists())

    def test_extra_public_file_cannot_be_uploaded(self):
        self.export()
        (self.evidence / 'private.stdout').write_bytes(b'PRIVATE')
        with self.assertRaisesRegex(ValueError, 'exactly'):
            probe.validate_export(self.private, self.evidence, self.sha)

    def test_existing_public_directory_is_not_reused(self):
        self.export()
        with self.assertRaisesRegex(ValueError, 'fresh public'):
            probe.export(self.private, self.evidence, self.sha)

    def test_noncanonical_and_extended_receipt_are_rejected(self):
        receipt, _ = self.export()
        path = self.evidence / probe.NAME
        path.write_bytes(probe.facts.canonical(receipt) + b'\n')
        with self.assertRaisesRegex(ValueError, 'differs'):
            probe.validate_export(self.private, self.evidence, self.sha)
        receipt['host_path'] = 'PRIVATE'
        path.write_bytes(probe.facts.canonical(receipt))
        with self.assertRaisesRegex(ValueError, 'differs'):
            probe.validate_export(self.private, self.evidence, self.sha)

    def test_mutated_runtime_export_is_rejected(self):
        _, runtime = self.export()
        runtime['microsoft']['visual_studio_candidates'][0]['version'] = '17.99'
        (self.evidence / probe.facts.PROJECTION_NAME).write_bytes(probe.facts.canonical(runtime))
        with self.assertRaisesRegex(ValueError, 'differs'):
            probe.validate_export(self.private, self.evidence, self.sha)

    def test_changed_runner_identity_prevents_validation(self):
        self.export()
        with patch.dict(probe.os.environ, {'ImageVersion': '20261002.1.0'}):
            with self.assertRaisesRegex(ValueError, 'differs'):
                probe.validate_export(self.private, self.evidence, self.sha)

    def test_changed_installed_library_prevents_upload_validation(self):
        self.export()
        self.fixture.rlib.write_bytes(b'changed installed library')
        with self.assertRaisesRegex(ValueError, 'installed file changed'):
            probe.validate_export(self.private, self.evidence, self.sha)

    def test_changed_private_notice_prevents_upload_validation(self):
        _, value = self.export()
        row = next(row for row in value['rust']['notices'] if row['id'] == 'copyright_library')
        (self.private / (row['evidence_id'] + '.bin')).write_bytes(b'changed private notice')
        with self.assertRaisesRegex(ValueError, 'snapshot changed'):
            probe.validate_export(self.private, self.evidence, self.sha)

    def test_private_and_public_directories_must_be_disjoint(self):
        for private, public in ((self.private, self.private), (self.private, self.private / 'public'),
                                (self.private / 'private', self.private),
                                (self.private, self.private / 'other' / '..')):
            with self.subTest(private=private, public=public), self.assertRaisesRegex(ValueError, 'separate'):
                probe.separate_directories(private, public)

    def test_public_symlink_is_rejected(self):
        self.export()
        link = self.fixture.root / 'public-link'
        try:
            link.symlink_to(self.evidence, target_is_directory=True)
        except OSError:
            self.skipTest('host does not grant symlink creation')
        with self.assertRaises(ValueError):
            probe.validate_export(self.private, link, self.sha)

    def test_cli_collect_supplies_only_source_defined_hypothetical_cfg(self):
        observed = []
        def collect(private, **kwargs):
            observed.append((private, kwargs))
            return self.fixture.collect()
        with patch.object(probe.facts, 'collect_runtime_facts', side_effect=collect):
            with patch('sys.stdout', new_callable=io.StringIO):
                code = probe.main(['--private', str(self.private), '--evidence', str(self.evidence),
                                   '--source-sha', self.sha])
        self.assertEqual(code, 0)
        self.assertEqual(observed, [(self.private, {'source_sha': self.sha, 'recipe_cfg_args': ['-D', 'warnings']})])

    def test_cli_failure_never_echoes_private_error(self):
        with patch.object(probe.facts, 'collect_runtime_facts', side_effect=ValueError('PRIVATE PATH')):
            with patch('sys.stderr', new_callable=io.StringIO) as stderr:
                code = probe.main(['--private', str(self.private), '--evidence', str(self.evidence),
                                   '--source-sha', self.sha])
        self.assertEqual(code, 1)
        self.assertNotIn('PRIVATE PATH', stderr.getvalue())
        self.assertIn('[runtime_collection:invalid_value]', stderr.getvalue())
        self.assertFalse(self.evidence.exists())

    def test_real_collection_refuses_non_windows(self):
        with patch.object(probe.facts.sys, 'platform', 'linux'):
            with self.assertRaisesRegex(ValueError, 'native Windows'):
                probe.facts.collect_runtime_facts(self.private, source_sha=self.sha)
        self.assertFalse(self.private.exists())


class SupplierWorkflowTests(unittest.TestCase):
    def test_workflow_has_only_dedicated_push_and_uploads_only_validated_json(self):
        raw = (SCRIPTS.parent / '.github/workflows/windows-supplier-environment.yml').read_text()
        self.assertIn('on:\n  push:\n    branches: [qualification/windows-supplier-probe]\n\n', raw)
        self.assertIn('fetch-depth: 3', raw)
        self.assertIn('git rev-parse HEAD^^', raw)
        self.assertIn('e60d5944c7e201b4b3b760f55616e890cb241d17', raw)
        self.assertIn('git rev-parse HEAD^', raw)
        self.assertIn(probe.BASE_SHA, raw)
        self.assertIn('runs-on: windows-latest', raw)
        self.assertIn('timeout-minutes: 20', raw)
        self.assertIn('contents: read', raw)
        self.assertIn('persist-credentials: false', raw)
        self.assertIn('ref: ${{ github.sha }}', raw)
        self.assertIn('1.93.0-x86_64-pc-windows-msvc --profile minimal --no-self-update', raw)
        self.assertIn("if: success() && steps.evidence.outputs.validated == 'true'", raw)
        self.assertEqual(raw.count('uses: actions/upload-artifact@'), 1)
        self.assertIn('${{ runner.temp }}/supplier-environment-public/runtime-facts.json', raw)
        self.assertIn('${{ runner.temp }}/supplier-environment-public/environment-only.json', raw)
        self.assertIn('--validate `', raw)
        self.assertIn('retention-days: 7', raw)
        for forbidden in ('workflow_dispatch:', 'pull_request:', 'schedule:', 'workflow_run:', 'cargo ',
                          'curl', 'download-artifact', 'actions/cache', 'contents: write',
                          '--linker-trace', '--audited-executable'):
            self.assertNotIn(forbidden, raw)
        for action in ('checkout', 'setup-python', 'upload-artifact'):
            self.assertRegex(raw, 'uses: actions/' + action + '@[0-9a-f]{40} ')
