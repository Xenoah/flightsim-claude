"""Synthetic consumer boundaries only; no native execution or authorization."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    value = importlib.util.module_from_spec(spec); spec.loader.exec_module(value)
    return value
stage = load('full_stage_tests', 'stage-full-windows-candidate.py')
native = stage.native


class FullStageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name); self.repo = self.root / 'repo'; self.repo.mkdir()
        self.notices = self.root / 'notices'; self.notices.mkdir()
        self.exe = self.root / 'build/flightsim-app.exe'; self.exe.parent.mkdir(); self.exe.write_bytes(b'SYNTHETIC-NOT-AN-EXECUTABLE')
        self.out = self.root / 'candidate'
        (self.repo / 'Cargo.toml').write_text('[workspace.package]\nversion="0.0.0-test"\n')
        names = stage.release.SOURCE_FILES
        for name in names:
            p = self.repo / name; p.parent.mkdir(parents=True, exist_ok=True); p.write_text('synthetic ' + name)
        assets = [{'path': n, 'sha256': stage.capture.file_record(self.repo / n)['sha256'],
                   'review_state': 'original_source_recorded'} for n in names if n.startswith('assets/')]
        self.manifest = {'schema_version': 1, 'assets': assets, 'release_external_assets': [a['path'] for a in assets],
                         'required_bundle_files': [], 'historical_excluded_assets': [], 'historical_excluded_dependency_assets': []}
        self.save_manifest()
        self.inventory = {'schema_version': 1, 'kind': 'cargo-dependency-notices', 'target': stage.release.TARGET,
                          'root_package': 'flightsim-app', 'packages': [{'name': 'flightsim-app', 'version': '0.0.0-test',
                            'features': ['default'], 'notices': []}], 'embedded_assets': []}
        license_path = self.notices / 'licenses/synthetic/LICENSE'
        license_path.parent.mkdir(parents=True); license_path.write_text('SYNTHETIC TEST DATA ONLY')
        self.inventory['packages'][0]['notices'] = [{'path': 'licenses/synthetic/LICENSE', **stage.capture.file_record(license_path)}]
        self.save_inventory()
        (self.notices / 'README.txt').write_text('synthetic test notice')
        self.report = {'schema_version': 1, 'status': 'blocked', 'blockers': [
            {'code': 'BUNDLE_NOT_CHECKED', 'category': 'integrity'},
            {'code': 'DEPENDENCY_REVIEW_REQUIRED', 'category': 'review'}]}
        self.info = {'schema_version': 1, 'package': 'flightsim-app', 'package_version': '0.0.0-test',
                     'profile': 'development', 'region_downloads': False, 'default_aircraft': 'light-single',
                     'default_model': 'aircraft/light_single.glb', 'bundled_aircraft': ['light-single', 'swift-sport'],
                     'target_os': 'windows', 'target_arch': 'x86_64', 'target_env': 'msvc', 'release_authorized': False}
        self.addCleanup(patch.stopall)
        patch.object(stage.readiness, 'check', return_value=self.report).start()
        patch.object(native.release.readiness, 'check', return_value=self.report).start()
        patch.object(stage.release, 'verify_distribution_info', return_value=self.info).start()

    def save_manifest(self):
        p = self.repo / stage.release.ASSET_MANIFEST; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(json.dumps(self.manifest))

    def save_inventory(self):
        (self.notices / 'dependency-inventory.json').write_text(json.dumps(self.inventory))

    def test_full_payload_keeps_both_models_and_no_authority(self):
        self.assertTrue(stage.stage(self.repo, self.exe, self.notices, self.out))
        manifest = json.loads((self.out / 'bundle-manifest.json').read_text())
        self.assertFalse(manifest['release_authorized'])
        self.assertEqual(manifest['rights_status'], 'blocked')
        self.assertTrue((self.out / 'assets/aircraft/light_single.glb').is_file())
        self.assertTrue((self.out / 'assets/aircraft/swift_sport.glb').is_file())
        self.assertEqual((self.out / 'flightsim-app.exe').read_bytes(), self.exe.read_bytes())

    def bundle_fixture(self):
        stage.stage(self.repo, self.exe, self.notices, self.out)
        return {'builds': {'ordinary': {'executable': stage.capture.file_record(self.exe),
                'inventory': stage.capture.file_record(self.notices / 'dependency-inventory.json')}}}

    def test_exact_full_bundle_revalidation(self):
        verified = self.bundle_fixture()
        self.assertEqual(native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)['kind'], stage.IDENTITY)

    def test_undeclared_extra_file_fails_projection(self):
        verified = self.bundle_fixture(); (self.out / 'extra.txt').write_text('unapproved')
        with self.assertRaisesRegex(ValueError, 'undeclared'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_resealed_extra_file_is_still_outside_allowlist(self):
        verified = self.bundle_fixture(); p = self.out / 'extra.txt'; p.write_text('unapproved')
        mpath = self.out / 'bundle-manifest.json'; m = json.loads(mpath.read_text())
        m['files'].append({'path': 'extra.txt', **stage.capture.file_record(p)})
        mpath.write_text(json.dumps(m))
        with self.assertRaisesRegex(ValueError, 'outside the full'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_wrong_manifest_kind_or_authority_rejected(self):
        verified = self.bundle_fixture(); p = self.out / 'bundle-manifest.json'; original = json.loads(p.read_text())
        for field, changed in [('kind', 'local-commercial-review-candidate'), ('release_authorized', True),
                               ('inventory_excludes_itself', False), ('schema_version', True)]:
            value = {**original, field: changed}; p.write_text(json.dumps(value))
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'wrong full bundle'):
                native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_duplicate_and_casefold_member_paths_rejected(self):
        verified = self.bundle_fixture(); p = self.out / 'bundle-manifest.json'; original = json.loads(p.read_text())
        for alias in (original['files'][0]['path'], original['files'][0]['path'].upper()):
            value = {**original, 'files': original['files'] + [{**original['files'][0], 'path': alias}]}
            p.write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'duplicate, colliding'):
                native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_resealed_wrong_aircraft_handshake_rejected(self):
        verified = self.bundle_fixture(); path = self.out / 'distribution-info.json'
        info = json.loads(path.read_text()); info['bundled_aircraft'] = ['swift-sport']; path.write_text(json.dumps(info))
        mpath = self.out / 'bundle-manifest.json'; m = json.loads(mpath.read_text()); m['distribution'] = info
        for row in m['files']:
            if row['path'] == path.name: row.update(stage.capture.file_record(path))
        mpath.write_text(json.dumps(m))
        with self.assertRaisesRegex(ValueError, 'distribution identity'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_resealed_original_readme_copy_is_rejected(self):
        verified = self.bundle_fixture(); path = self.out / 'third-party/README.txt'
        path.write_text('changed notice despite resealed local manifest')
        mpath = self.out / 'bundle-manifest.json'; value = json.loads(mpath.read_text())
        for row in value['files']:
            if row['path'] == 'third-party/README.txt': row.update(stage.capture.file_record(path))
        mpath.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'original notice differs'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_resealed_readiness_cannot_claim_runtime_acceptance(self):
        verified = self.bundle_fixture(); path = self.out / 'full-readiness.json'
        report = json.loads(path.read_text()); report['runtime_accepted'] = True
        path.write_text(json.dumps(report))
        mpath = self.out / 'bundle-manifest.json'; value = json.loads(mpath.read_text())
        for row in value['files']:
            if row['path'] == path.name: row.update(stage.capture.file_record(path))
        mpath.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'readiness differs'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_rights_status_cannot_claim_a_missing_review_was_consumed(self):
        verified = self.bundle_fixture(); path = self.out / 'bundle-manifest.json'
        value = json.loads(path.read_text()); value['rights_status'] = 'review_consumed_final_acceptance_required'
        path.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'readiness differs'):
            native.validate_bundle(self.repo, self.out, verified, original_notices=self.notices)

    def test_copy_mutation_is_rejected(self):
        original = stage.notices_api.copy_file
        def corrupt(source, target):
            original(source, target)
            if target.name == 'README.txt': target.write_text('corrupted')
        with patch.object(stage.notices_api, 'copy_file', side_effect=corrupt):
            with self.assertRaisesRegex(ValueError, 'notice bytes changed'):
                stage.stage(self.repo, self.exe, self.notices, self.out)
        self.assertFalse(self.out.exists())

    def test_historical_denied_hash_cannot_be_reapproved(self):
        row = self.manifest['assets'][0]
        self.manifest['historical_excluded_assets'] = [{'sha256': row['sha256'], 'review_state': 'unresolved'}]
        self.save_manifest()
        with self.assertRaisesRegex(ValueError, 'historically unresolved'):
            stage.source_files(self.repo)

    def test_changed_reviewed_model_rejected(self):
        (self.repo / 'assets/aircraft/light_single.glb').write_bytes(b'changed model')
        with self.assertRaisesRegex(ValueError, 'differs from reviewed'):
            stage.source_files(self.repo)

    def test_unlisted_notice_rejected(self):
        (self.notices / 'unexpected.txt').write_text('unexpected')
        with self.assertRaisesRegex(ValueError, 'unexpected dependency-notice'):
            stage.stage(self.repo, self.exe, self.notices, self.out)

    def test_other_application_recipes_rejected(self):
        for features in (['commercial-staging'], ['analytic-tonemapping', 'commercial-staging'], ['default', 'region-downloads']):
            with self.subTest(features=features):
                self.inventory['packages'][0]['features'] = features; self.save_inventory()
                with self.assertRaisesRegex(ValueError, 'ordinary default recipe'):
                    stage.check_inventory(self.repo, self.notices / 'dependency-inventory.json')

    def test_target_and_version_rejected(self):
        for key, value in [('target', 'x86_64-pc-windows-gnu'), ('root_package', 'other')]:
            old = self.inventory[key]; self.inventory[key] = value; self.save_inventory()
            with self.assertRaisesRegex(ValueError, 'wrong full target'):
                stage.check_inventory(self.repo, self.notices / 'dependency-inventory.json')
            self.inventory[key] = old
        self.inventory['packages'][0]['version'] = '99.0.0'; self.save_inventory()
        with self.assertRaisesRegex(ValueError, 'ordinary default recipe'):
            stage.check_inventory(self.repo, self.notices / 'dependency-inventory.json')

    def test_integrity_blocker_is_not_a_review_candidate(self):
        self.report['blockers'].append({'code': 'DEPENDENCY_LOCK_CHANGED', 'category': 'integrity'})
        with self.assertRaisesRegex(ValueError, 'integrity blocks'):
            stage.stage(self.repo, self.exe, self.notices, self.out)
        self.assertFalse(self.out.exists())

    def test_parent_alias_and_source_overlap_rejected(self):
        with self.assertRaisesRegex(ValueError, 'parent aliases'):
            stage.local_path(self.root / 'alias/../repo/output')
        with self.assertRaisesRegex(ValueError, 'disjoint'):
            stage.stage(self.repo, self.exe, self.notices, self.repo / 'output')

    def test_existing_destination_and_notice_links_rejected(self):
        self.out.mkdir()
        with self.assertRaisesRegex(ValueError, 'fresh output'):
            stage.stage(self.repo, self.exe, self.notices, self.out)
        self.out.rmdir()
        try: (self.notices / 'link').symlink_to(self.notices / 'README.txt')
        except OSError as e:
            import os
            if os.name == 'nt' and getattr(e, 'winerror', None) == 1314: self.skipTest('Windows symlink privilege unavailable')
            raise
        with self.assertRaises(ValueError): stage.stage(self.repo, self.exe, self.notices, self.out)


class FullNativeBindingTests(unittest.TestCase):
    def build(self):
        return {'status': native.capture.PASS, 'source_recipe': native.SOURCE_RECIPE,
                'builds': {'ordinary': {'lut_payloads_found': 2}}}

    def test_only_explicit_two_lut_build_is_admitted(self):
        value = self.build()
        with patch.object(native.capture, 'validate_export', return_value=value):
            self.assertEqual(native.validate_build(Path('/repo'), 'a' * 40, Path('/private'), Path('/evidence')), value)
            del value['source_recipe']
            with self.assertRaisesRegex(ValueError, 'explicit admitted two-LUT'):
                native.validate_build(Path('/repo'), 'a' * 40, Path('/private'), Path('/evidence'))

    def test_old_three_lut_and_incomplete_builds_rejected(self):
        for field, changed in [('status', 'failed'), ('source_recipe', 'bevy-0.18.1-upstream-three-lut-source-v1')]:
            value = self.build(); value[field] = changed
            with patch.object(native.capture, 'validate_export', return_value=value), self.assertRaises(ValueError):
                native.validate_build(Path('/repo'), 'a' * 40, Path('/private'), Path('/evidence'))
        value = self.build(); value['builds']['ordinary']['lut_payloads_found'] = 3
        with patch.object(native.capture, 'validate_export', return_value=value), self.assertRaisesRegex(ValueError, 'exactly the two'):
            native.validate_build(Path('/repo'), 'a' * 40, Path('/private'), Path('/evidence'))

    def test_core_pipeline_cannot_be_registry_or_workspace(self):
        for source, members in [('registry+https://github.com/rust-lang/crates.io-index', []), (None, ['core'])]:
            package = {'name': 'bevy_core_pipeline', 'version': '0.18.1', 'source': source, 'id': 'core'}
            with self.assertRaisesRegex(ValueError, 'exact reviewed path patch'):
                native.modified_source(Path('/repo'), package, {'workspace_members': members}, {'files': []}, Path('/private/core.crate'))

    def test_unrecognized_path_patch_retains_strict_original_rule(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); package = {'name': 'new-package', 'version': '1.0.0', 'source': None, 'id': 'unknown'}
            with self.assertRaisesRegex(ValueError, 'tracked modified-source'):
                native.modified_source(root, package, {'workspace_members': []}, {'files': []}, root / 'core.crate')


if __name__ == '__main__': unittest.main()
