"""Synthetic plumbing tests only: no native run, real receipt or review grant."""
import copy
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from unittest.mock import patch


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SCRIPT = Path(__file__).resolve().parents[1] / 'project-ordinary-release-payload.py'
adapter = load('ordinary_payload', SCRIPT)
fixtures = load('ordinary_payload_gate_fixtures', Path(__file__).with_name('test_release_authorization.py'))


class OrdinaryPayloadTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.ReleaseAuthorizationTests(methodName='runTest')
        # Windows runner TEMP can use a short-name alias; production validators
        # intentionally reject aliases. Create fixture bytes canonically instead.
        with patch.object(fixtures.tempfile, 'tempdir', str(Path(fixtures.tempfile.gettempdir()).resolve())):
            self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.repo, self.root = self.fixture.repo, self.fixture.root
        self.private, self.text, self.bundle = (self.root / name for name in ('capture', 'text', 'bundle'))
        self.notices = self.private / 'capture/ordinary/notices'
        self.original = self.repo / adapter.release.DEPENDENCY_INVENTORY
        self.notices.mkdir(parents=True)
        self.text.mkdir()
        self.metadata = self.private / 'capture/ordinary/metadata.json'
        self.metadata.write_bytes(b'{"fixture":"unit-test-only, not native metadata"}')
        self.fixture.inventory.update({
            'metadata_sha256': adapter.capture.file_record(self.metadata)['sha256'],
            'supplement_manifest_sha256': None, 'scope': 'Synthetic fixture, not whole-target review',
            'review_status': 'not_reviewed',
        })
        for notice in self.fixture.inventory['packages'][0]['notices']:
            notice['bytes'] = (self.original.parent / notice['path']).stat().st_size
        self.fixture.refresh_evidence()
        self.fixture.authorize_fixture()
        self.expected = self.fixture.git('rev-parse', 'HEAD').stdout.decode().strip()
        self.source = {'source_sha': self.expected,
                       'source_tree': self.fixture.git('rev-parse', 'HEAD^{tree}').stdout.decode().strip()}
        shutil.copytree(self.original.parent, self.notices, dirs_exist_ok=True)
        self.captured = self.notices / adapter.release.staging.INVENTORY
        self.executable = self.private / 'target-ordinary' / adapter.release.TARGET / 'release/flightsim-app.exe'
        self.executable.parent.mkdir(parents=True)
        self.executable.write_bytes(b'UNIT TEST PLACEHOLDER, NOT A NATIVE EXECUTABLE')
        self.summary = self.text / adapter.capture.EXPORT_NAME
        self.summary.write_bytes(b'UNIT TEST PLACEHOLDER, NOT NATIVE EVIDENCE')
        self.refresh_verified()
        self.rebuild_bundle()

    def refresh_verified(self):
        self.verified = {'status': adapter.capture.PASS, 'source_sha': self.expected,
                         'source_tree': self.source['source_tree'], 'source_recipe': adapter.capture.check.SOURCE_RECIPE,
                         'builds': {'ordinary': {
                             'executable': adapter.capture.file_record(self.executable),
                             'inventory': adapter.capture.file_record(self.captured),
                             'metadata': adapter.capture.file_record(self.metadata), 'lut_payloads_found': 2,
                         }}}
        self.summary.write_bytes((json.dumps(self.verified, indent=2, sort_keys=True) + '\n').encode('ascii'))

    def rebuild_bundle(self):
        if self.bundle.exists():
            shutil.rmtree(self.bundle)
        self.bundle.mkdir()
        self.gate, self.plan = adapter.release.inspect(self.repo)
        for item in self.plan['files']:
            target = self.bundle / item['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.repo / item['source'], target)
        shutil.copyfile(self.executable, self.bundle / 'flightsim-app.exe')

    def project(self, source_effect=None):
        with patch.object(adapter.capture, 'validate_export', return_value=self.verified) as validated:
            with patch.object(adapter.capture, 'source_evidence', side_effect=source_effect,
                              return_value=self.source) as observed:
                result = adapter.project_payload(self.repo, self.expected, self.private, self.text, self.bundle)
        validated.assert_called_once_with(self.text, repo=self.repo, expected=self.expected, private=self.private)
        self.assertEqual(observed.call_count, 2)
        return result

    def captured_value(self, value):
        self.captured.write_text(json.dumps(value), encoding='utf-8')

    def test_exact_payload_binds_real_gate_plan_and_remains_unapproved(self):
        result = self.project()
        self.assertEqual(result['source_sha'], self.expected)
        self.assertEqual(result['source_tree'], self.source['source_tree'])
        self.assertEqual(result['release_inventory_sha256'], adapter.release.hash_value(self.plan))
        self.assertEqual(result['authorization_receipt_sha256'], self.gate['authorization_sha256'])
        self.assertEqual(result['inventory_comparison']['status'], 'raw_bytes_equal')
        self.assertEqual(result['inventory_comparison']['original'], adapter.capture.file_record(self.original))
        self.assertEqual(result['bindings']['native_executable'], adapter.capture.file_record(self.executable))
        self.assertEqual({row['path'] for row in result['shipped_files']},
                         {row['path'] for row in self.plan['files']} | {'flightsim-app.exe'})
        for key in ('release_authorized', 'dependency_review_approved', 'review_applicability_approved', 'runtime_accepted'):
            self.assertIs(result[key], False)
        self.assertEqual(result['archive_and_extracted_smoke_binding'], 'not_evaluated')
        self.assertEqual(result['whole_target_native_review_applicability'], 'not_evaluated')

    def test_metadata_drift_preserves_both_raw_inventory_and_metadata_hashes(self):
        original_bytes = self.original.read_bytes()
        self.metadata.write_bytes(b'{"fixture":"different native metadata bytes"}')
        value = json.loads(original_bytes)
        value['metadata_sha256'] = adapter.capture.file_record(self.metadata)['sha256']
        self.captured_value(value)
        captured_bytes = self.captured.read_bytes()
        self.refresh_verified()
        result = self.project()['inventory_comparison']
        self.assertEqual(result['different_fields'], ['metadata_sha256'])
        self.assertEqual(result['status'], 'metadata_digest_changed_requires_applicability_review')
        self.assertEqual(result['metadata_difference_cause'], 'not_established')
        self.assertFalse(result['review_applicability_approved'])
        self.assertNotEqual(result['original'], result['captured'])
        self.assertNotEqual(result['original_metadata_sha256'], result['captured_metadata_sha256'])
        self.assertEqual(self.original.read_bytes(), original_bytes)
        self.assertEqual(self.captured.read_bytes(), captured_bytes)
        self.assertEqual((self.bundle / 'third-party/dependency-inventory.json').read_bytes(), original_bytes)

    def test_serialization_drift_is_not_claimed_byte_identical(self):
        self.captured_value(json.loads(self.original.read_bytes()))
        self.captured.write_bytes(b'\n' + self.captured.read_bytes() + b'\n')
        result = adapter.compare_inventories(self.original, self.captured)
        self.assertFalse(result['raw_bytes_equal'])
        self.assertEqual(result['different_fields'], [])
        self.assertEqual(result['status'], 'encoding_changed_requires_applicability_review')

    def test_every_non_metadata_field_change_fails_even_with_metadata_drift(self):
        original = json.loads(self.original.read_bytes())
        for field in sorted(adapter.INVENTORY_FIELDS - {'metadata_sha256'}):
            with self.subTest(field=field):
                value = copy.deepcopy(original)
                value['metadata_sha256'] = '9' * 64
                if isinstance(value[field], list):
                    value[field].append({'id': 'different', 'reason': 'extra obligation'})
                elif value[field] is None:
                    value[field] = '8' * 64
                elif type(value[field]) is int:
                    value[field] += 1
                else:
                    value[field] += 'changed'
                self.captured_value(value)
                with self.assertRaises(ValueError):
                    adapter.compare_inventories(self.original, self.captured)

    def test_same_counts_do_not_hide_package_or_notice_changes(self):
        original = json.loads(self.original.read_bytes())
        for mutate in (
            lambda value: value['packages'][0].update(features=['other']),
            lambda value: value['packages'][0].update(source_checksum='4' * 64),
            lambda value: value['packages'][0]['notices'][0].update(sha256='6' * 64),
            lambda value: value['packages'][0]['notices'][0].update(bytes=True),
        ):
            value = copy.deepcopy(original)
            mutate(value)
            self.captured_value(value)
            with self.assertRaisesRegex(ValueError, 'content differs'):
                adapter.compare_inventories(self.original, self.captured)

    def test_json_types_are_not_coerced(self):
        value = json.loads(self.original.read_bytes())
        for invalid in (True, 1.0):
            value['schema_version'] = invalid
            self.captured_value(value)
            with self.assertRaises(ValueError):
                adapter.compare_inventories(self.original, self.captured)

    def test_unknown_removed_or_duplicate_inventory_keys_fail(self):
        value = json.loads(self.original.read_bytes())
        value['ignored_field'] = 'must not be stripped'
        self.captured_value(value)
        with self.assertRaises(ValueError):
            adapter.compare_inventories(self.original, self.captured)
        del value['ignored_field']
        del value['scope']
        self.captured_value(value)
        with self.assertRaises(ValueError):
            adapter.compare_inventories(self.original, self.captured)
        self.captured.write_bytes(self.original.read_bytes().rstrip()[:-1] + b', "schema_version": 1}')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            adapter.compare_inventories(self.original, self.captured)

    def test_malformed_and_unbounded_json_fail(self):
        for data in (b'[]', b'{"x":NaN}', b'{"x":Infinity}', b'{"x":1e999}', b'\xff', b'{"x":"\0"}', b'{'):
            with self.subTest(data=data):
                self.captured.write_bytes(data)
                with self.assertRaises(ValueError):
                    adapter.compare_inventories(self.original, self.captured)
        self.captured.write_bytes(b' ' * 20)
        with patch.object(adapter, 'MAX_JSON_BYTES', 10):
            with self.assertRaisesRegex(ValueError, 'budget'):
                adapter.compare_inventories(self.original, self.captured)

    def test_duplicate_package_identity_fails(self):
        value = json.loads(self.original.read_bytes())
        value['packages'].append(copy.deepcopy(value['packages'][0]))
        self.captured_value(value)
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            adapter.compare_inventories(self.original, self.captured)

    def test_native_validation_is_not_optional_or_a_caller_supplied_summary(self):
        with patch.object(adapter.capture, 'validate_export', side_effect=ValueError('no native evidence')):
            with self.assertRaisesRegex(ValueError, 'no native evidence'):
                adapter.project_payload(self.repo, self.expected, self.private, self.text, self.bundle)

    def test_payload_mutation_between_verification_and_first_snapshot_fails(self):
        verify = adapter.release.verify_bundle

        def mutate(*args):
            verify(*args)
            (self.bundle / 'README.md').write_bytes(b'changed after successful real verification')

        with patch.object(adapter.release, 'verify_bundle', side_effect=mutate):
            with self.assertRaisesRegex(ValueError, 'snapshot differs from authorized plan'):
                self.project()

    def test_summary_mutation_immediately_after_validation_fails(self):
        def validate(*_args, **_kwargs):
            self.summary.write_bytes(b'unvalidated replacement after native validator returned')
            return self.verified

        with patch.object(adapter.capture, 'validate_export', side_effect=validate):
            with self.assertRaisesRegex(ValueError, 'summary changed across validation'):
                adapter.project_payload(self.repo, self.expected, self.private, self.text, self.bundle)

    def test_returned_summary_cannot_differ_from_unchanged_raw_input(self):
        value = copy.deepcopy(self.verified)
        value['source_tree'] = 'f' * 40
        with patch.object(adapter.capture, 'validate_export', return_value=value):
            with self.assertRaisesRegex(ValueError, 'summary changed across validation'):
                adapter.project_payload(self.repo, self.expected, self.private, self.text, self.bundle)

    def test_notice_comparison_cannot_record_bytes_outside_authorized_plan(self):
        compare = adapter.compare_notices

        def changed(*args):
            result = compare(*args)
            result[0]['sha256'] = 'f' * 64
            return result

        with patch.object(adapter, 'compare_notices', side_effect=changed):
            with self.assertRaisesRegex(ValueError, 'notice comparison differs from authorized plan'):
                self.project()

    def test_missing_native_records_or_wrong_source_and_recipe_fail(self):
        good = copy.deepcopy(self.verified)
        for mutate in (
            lambda value: value.update(status='failed'),
            lambda value: value.update(source_sha='1' * 40),
            lambda value: value.update(source_tree='2' * 40),
            lambda value: value.update(source_recipe='other'),
            lambda value: value['builds'].clear(),
            lambda value: value['builds']['ordinary'].update(lut_payloads_found=3),
        ):
            self.verified = copy.deepcopy(good)
            mutate(self.verified)
            with self.assertRaises((ValueError, KeyError)):
                self.project()

    def test_changed_executable_inventory_or_metadata_fails_native_binding(self):
        for path in (self.executable, self.captured, self.metadata):
            original = path.read_bytes()
            path.write_bytes(original + b'changed')
            with self.assertRaisesRegex(ValueError, 'same-build native input differs'):
                self.project()
            path.write_bytes(original)

    def test_inventory_cannot_claim_foreign_metadata_with_refreshed_raw_binding(self):
        value = json.loads(self.captured.read_bytes())
        value['metadata_sha256'] = '7' * 64
        self.captured_value(value)
        self.refresh_verified()
        with self.assertRaisesRegex(ValueError, 'metadata digest differs'):
            self.project()

    def test_missing_authorization_stops_before_native_validation(self):
        (self.repo / adapter.release.AUTHORIZATION).unlink()
        self.fixture.commit()
        with patch.object(adapter.capture, 'validate_export') as validated:
            with self.assertRaisesRegex(ValueError, 'gate is blocked'):
                adapter.project_payload(self.repo, self.expected, self.private, self.text, self.bundle)
        validated.assert_not_called()

    def test_missing_review_or_uncommitted_input_still_uses_existing_gate(self):
        (self.repo / adapter.release.DEPENDENCY_REVIEW).unlink()
        with self.assertRaises(subprocess.CalledProcessError):
            self.project()

    def test_diagnostic_members_and_arbitrary_extra_payload_are_rejected(self):
        for name in ('bundle-manifest.json', 'distribution-info.json', 'full-readiness.json', 'extra.txt'):
            path = self.bundle / name
            path.write_text('diagnostic must remain a sidecar')
            with self.assertRaisesRegex(ValueError, 'extra or changed file'):
                self.project()
            path.unlink()

    def test_replaced_executable_source_notice_inventory_and_review_fail(self):
        for name in ('flightsim-app.exe', 'README.md', 'third-party/licenses/test/LICENSE',
                     'third-party/dependency-inventory.json', adapter.release.DEPENDENCY_REVIEW):
            path = self.bundle / name
            original = path.read_bytes()
            path.write_bytes(original + b'changed')
            with self.assertRaisesRegex(ValueError, 'extra or changed file'):
                self.project()
            path.write_bytes(original)

    def test_missing_payload_file_fails(self):
        (self.bundle / 'README.md').unlink()
        with self.assertRaisesRegex(ValueError, 'exactly the authorized payload'):
            self.project()

    def test_changed_or_unreferenced_capture_notice_fails(self):
        path = self.notices / 'licenses/test/LICENSE'
        path.write_bytes(b'different license')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            self.project()
        shutil.copyfile(self.original.parent / 'licenses/test/LICENSE', path)
        (self.notices / 'secret.txt').write_text('not referenced')
        with self.assertRaisesRegex(ValueError, 'unexpected dependency-notice'):
            self.project()

    def test_source_or_payload_mutation_during_projection_fails(self):
        calls = 0

        def source(*_):
            nonlocal calls
            calls += 1
            if calls == 2:
                (self.bundle / 'README.md').write_bytes(b'changed while projecting')
            return self.source

        with self.assertRaisesRegex(ValueError, 'changed during projection'):
            self.project(source)

    def test_native_inputs_mutation_during_projection_fails(self):
        for path in (self.executable, self.captured, self.metadata, self.summary):
            original = path.read_bytes()
            calls = 0

            def source(*_):
                nonlocal calls
                calls += 1
                if calls == 2:
                    path.write_bytes(original + b'changed')
                return self.source

            with self.assertRaisesRegex(ValueError, 'native inputs changed'):
                self.project(source)
            path.write_bytes(original)

    def test_captured_notice_mutation_during_projection_fails(self):
        calls = 0

        def source(*_):
            nonlocal calls
            calls += 1
            if calls == 2:
                (self.notices / 'licenses/test/LICENSE').write_bytes(b'changed captured notice')
            return self.source

        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            self.project(source)

    def test_symlink_and_hardlink_inputs_fail(self):
        for method in ('symlink', 'hardlink'):
            link = self.root / ('linked-' + method + '.json')
            if method == 'symlink':
                link.symlink_to(self.original)
            else:
                os.link(self.original, link)
            try:
                with self.assertRaises(ValueError):
                    adapter.compare_inventories(link, self.captured)
            finally:
                link.unlink()

    def test_relative_and_overlapping_roots_fail(self):
        with self.assertRaisesRegex(ValueError, 'absolute'):
            adapter.project_payload(Path('relative'), self.expected, self.private, self.text, self.bundle)
        with self.assertRaisesRegex(ValueError, 'disjoint'):
            adapter.project_payload(self.repo, self.expected, self.private, self.text, self.repo / 'bundle')

    @unittest.skipIf(os.name == 'nt', 'POSIX double-slash root alias')
    def test_double_slash_root_alias_cannot_bypass_disjointness(self):
        alias = Path('/' + str(self.repo))
        self.assertEqual(alias.resolve(), self.repo)
        with self.assertRaisesRegex(ValueError, 'canonical absolute'):
            adapter.absolute(alias)
        with self.assertRaisesRegex(ValueError, 'canonical absolute'):
            adapter.project_payload(self.repo, self.expected, alias, self.text, self.bundle)

    def test_aliased_payload_paths_and_empty_directories_fail(self):
        for name in ('CON', 'nul.txt', 'LPT1.txt', 'COM\u00b9.log', 'trailing.', 'trailing ', 'a:b', 'a\\b', 'a?b'):
            with self.subTest(name=name):
                with self.assertRaises(ValueError):
                    adapter.member_name(name)
        (self.bundle / 'empty').mkdir()
        with self.assertRaisesRegex(ValueError, 'empty or unexplained'):
            adapter.snapshot_payload(self.bundle)

    def test_case_colliding_or_linked_payloads_fail(self):
        path = self.bundle / 'readme.md'
        path.write_bytes(b'case collision')
        with self.assertRaisesRegex(ValueError, 'case-colliding'):
            adapter.snapshot_payload(self.bundle)
        path.unlink()
        path.symlink_to(self.bundle / 'README.md')
        with self.assertRaises(ValueError):
            adapter.snapshot_payload(self.bundle)
        path.unlink()
        os.link(self.bundle / 'README.md', path)
        with self.assertRaises(ValueError):
            adapter.snapshot_payload(self.bundle)

    def test_projection_budget_is_bounded(self):
        with patch.object(adapter, 'MAX_MEMBERS', 1):
            with self.assertRaisesRegex(ValueError, 'budget'):
                adapter.snapshot_payload(self.bundle)

    def test_cli_failure_emits_no_success_document(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        args = ['--repo', str(self.repo), '--expected-sha', self.expected,
                '--build-private', str(self.private), '--build-text', str(self.text), '--bundle', str(self.bundle)]
        sentinel = '/private/SENTINEL-do-not-export/inventory.json'
        for error in (ValueError(sentinel), OSError(sentinel), AttributeError(sentinel),
                      subprocess.CalledProcessError(1, ['git', sentinel])):
            with patch.object(adapter, 'project_payload', side_effect=error):
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    self.assertEqual(adapter.main(args), 2)
            self.assertEqual(stdout.getvalue(), '')
            self.assertIn('projection failed', stderr.getvalue())
            self.assertNotIn(sentinel, stderr.getvalue())

    def test_real_cli_malformed_committed_package_has_bounded_failure(self):
        self.fixture.inventory['packages'] = [42]
        self.fixture.refresh_evidence()
        args = [adapter.sys.executable, str(SCRIPT), '--repo', str(self.repo), '--expected-sha', self.expected,
                '--build-private', str(self.private), '--build-text', str(self.text), '--bundle', str(self.bundle)]
        completed = subprocess.run(args, capture_output=True, text=True, check=False, timeout=10)
        self.assertEqual(completed.returncode, 2)
        self.assertEqual(completed.stdout, '')
        self.assertEqual(completed.stderr, 'Ordinary payload projection failed: input validation did not complete.\n')
        with patch.object(adapter, 'MAX_PAYLOAD_BYTES', 1):
            with self.assertRaisesRegex(ValueError, 'budget'):
                adapter.snapshot_payload(self.bundle)


if __name__ == '__main__':
    unittest.main()
