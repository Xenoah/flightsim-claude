"""Exact aircraft-cockpit source migration; no native, rights or release grant."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    'cockpit_variant_fixture', ROOT / 'scripts/tests/test_component_terms_source_admission.py')
fixture_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture_module)
capture = fixture_module.capture
candidate = capture.check.candidate
MIGRATION = 'scripts/cockpit-variant-source-migration.json'
PREFIX = 'scripts/history/alpha23-cockpit-variant-base-'
CHANGED = {
    'crates/flightsim-app/src/aircraft_picker_tests.rs',
    'crates/flightsim-app/src/aircraft_scene.rs',
    'crates/flightsim-app/src/cockpit_runtime.rs',
    'crates/flightsim-app/src/main.rs',
    'crates/flightsim-render/src/cockpit.rs',
    'crates/flightsim-render/src/cockpit/texture.rs',
}
RUNTIME = {
    'crates/flightsim-app/src/cockpit_layout.rs',
    'crates/flightsim-render/src/cockpit/variants.rs',
}
RELOCATIONS = {
    'crates/flightsim-app/src/aircraft_picker_tests.rs': PREFIX + 'aircraft_picker_tests.rs',
    'crates/flightsim-app/src/aircraft_scene.rs': PREFIX + 'aircraft_scene.rs',
    'crates/flightsim-app/src/cockpit_runtime.rs': PREFIX + 'cockpit_runtime.rs',
    'crates/flightsim-app/src/main.rs': PREFIX + 'flightsim-app-main.rs',
    'crates/flightsim-render/src/cockpit.rs': PREFIX + 'cockpit.rs',
    'crates/flightsim-render/src/cockpit/texture.rs': PREFIX + 'cockpit-texture.rs',
}
CONTRACTS = {PREFIX + name for name in (
    'replay-candidate-contract.json', 'analytical-swift-source-contract.json',
    'analytical-swift-capture-contract.json')}
ADDED = {MIGRATION, *RELOCATIONS.values(), *CONTRACTS, *RUNTIME}


class CockpitVariantSourceAdmissionTests(unittest.TestCase):
    fixture = fixture_module.ComponentTermsSourceAdmissionTests.fixture
    commit = staticmethod(fixture_module.ComponentTermsSourceAdmissionTests.commit)

    def test_exact_six_replacements_twelve_additions_preserve_all_956_prior_pins(self):
        current = candidate.load_replay_contract(ROOT)
        base = json.loads((ROOT / candidate.COCKPIT_VARIANT_BASE_CONTRACT_PATH).read_text())
        migration = json.loads((ROOT / MIGRATION).read_text())
        candidate.validate_alpha23_replay_contract(base)
        self.assertEqual(candidate.COCKPIT_VARIANT_MIGRATION_PATH, MIGRATION)
        self.assertEqual(candidate.COCKPIT_VARIANT_BASE_CONTRACT_PATH,
                         PREFIX + 'replay-candidate-contract.json')
        self.assertEqual(candidate.COCKPIT_VARIANT_SOURCE_PATHS, ADDED)
        self.assertEqual(candidate.COCKPIT_VARIANT_RUNTIME_PATHS, RUNTIME)
        self.assertEqual(candidate.COCKPIT_VARIANT_HISTORICAL_RELOCATIONS, RELOCATIONS)
        self.assertEqual(set(candidate.COCKPIT_VARIANT_BASE_CONTRACT_HASHES), CONTRACTS)
        self.assertEqual(set(candidate.COCKPIT_VARIANT_PREVIOUS_SHA256), CHANGED)
        self.assertEqual(len(CONTRACTS | set(RELOCATIONS.values())), 9)
        self.assertEqual(migration['identity'], 'full-two-aircraft-cockpit-variant-source-migration-v1')
        self.assertEqual(migration['base_source'], candidate.COCKPIT_VARIANT_BASE_SOURCE)
        self.assertEqual(migration['base_source'], '130de6f2872cab4932a4b2e61c6828a7dccc883b')
        self.assertEqual(migration['base_tree'], candidate.COCKPIT_VARIANT_BASE_TREE)
        self.assertEqual(migration['base_tree'], '26b935ac8c60cf0b87a8367f93a45ebc6a4fc2a8')
        self.assertEqual(migration['base_source_kind'], 'published-commit')
        self.assertEqual(migration['base_source_status'], 'released-alpha23-parent')
        self.assertEqual(migration['previous_source_migration_sha256'], candidate.ALPHA23_MIGRATION_SHA256)
        self.assertEqual(migration['preserved_runtime_sha256'], candidate.PRESERVED_RUNTIME_SHA256)
        self.assertEqual(migration['scope'], candidate.SOURCE_ADMISSION_SCOPE)
        self.assertIs(migration['release_authorized'], False)
        self.assertEqual(migration['historical_relocations'], RELOCATIONS)
        self.assertEqual(migration['base_contract_sha256'], candidate.COCKPIT_VARIANT_BASE_CONTRACT_HASHES)
        self.assertEqual(set(migration['replaced_source_sha256']), CHANGED)
        self.assertEqual(set(migration['added_source_sha256']), RUNTIME)
        self.assertEqual(len(base['source_sha256']), 956)
        self.assertEqual(len(current['source_sha256']), 968)
        self.assertEqual(candidate.ALPHA23_REPLAY_CONTRACT_PATHS, set(base['source_sha256']))
        self.assertEqual(set(current['source_sha256']) - set(base['source_sha256']), ADDED)
        self.assertEqual({path for path, expected in base['source_sha256'].items()
                          if current['source_sha256'][path] != expected}, CHANGED)
        for path, expected in base['source_sha256'].items():
            historical = RELOCATIONS.get(path, path)
            self.assertEqual(current['source_sha256'][historical], expected, path)
            self.assertEqual(candidate.digest(ROOT / historical), expected, path)
        for path, expected in candidate.COCKPIT_VARIANT_BASE_CONTRACT_HASHES.items():
            self.assertEqual(candidate.digest(ROOT / path), expected, path)
        for path in CHANGED:
            expected = base['source_sha256'][path]
            self.assertEqual(candidate.COCKPIT_VARIANT_PREVIOUS_SHA256[path], expected)
            self.assertEqual(migration['replaced_source_sha256'][path], {
                'previous_sha256': expected, 'sha256': candidate.digest(ROOT / path)})
            self.assertNotEqual(candidate.digest(ROOT / path), expected)
        for path in RUNTIME:
            self.assertEqual(migration['added_source_sha256'][path], candidate.digest(ROOT / path))
        preserved = json.loads((ROOT / candidate.PRESERVED_RUNTIME_PATH).read_text())['source_sha256']
        self.assertEqual(len(preserved), 497)
        self.assertEqual(len(candidate.PRESERVED_CRATE_SOURCE_PATHS), 492)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 499)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertEqual(len(capture.check.SOURCE_PATHS), 56)
        self.assertEqual(len(capture.BOUND_FILES), 4)
        self.assertEqual(current['contract'], 'full-two-aircraft-cockpit-variant-source-v1')
        self.assertEqual(current['scope'], candidate.SOURCE_ADMISSION_SCOPE)
        self.assertIs(current['release_authorized'], False)

    def test_descriptor_identity_base_and_source_only_authority_are_not_self_asserted(self):
        current = candidate.load_replay_contract(ROOT)
        base = json.loads((ROOT / candidate.COCKPIT_VARIANT_BASE_CONTRACT_PATH).read_text())
        positive = json.loads((ROOT / MIGRATION).read_text())
        def validate(value):
            candidate.validate_cockpit_variant_migration(
                value, current['source_sha256'], base['source_sha256'], ROOT)
        validate(positive)
        mutations = {
            'schema_version': True, 'identity': 'full-two-aircraft-cockpit-source-migration-v1',
            'base_source': '0' * 40, 'base_tree': '0' * 40,
            'base_source_kind': 'published-release', 'base_source_status': 'provisional-alpha23-parent',
            'previous_source_migration_sha256': candidate.COCKPIT_MIGRATION_SHA256,
            'preserved_runtime_sha256': '0' * 64,
            'scope': 'native appearance, rights and release approved', 'release_authorized': True,
        }
        for key, value in mutations.items():
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate({**positive, key: value})
        for key in positive:
            changed = copy.deepcopy(positive)
            del changed[key]
            with self.subTest(omission=key), self.assertRaises(ValueError):
                validate(changed)
        for value in (0, 1, None, 'false'):
            with self.subTest(authority=value), self.assertRaises(ValueError):
                validate({**positive, 'release_authorized': value})
        with self.assertRaises(ValueError):
            validate({**positive, 'native_accepted': True})
        for key in ('historical_relocations', 'base_contract_sha256',
                    'replaced_source_sha256', 'added_source_sha256'):
            for attack in ('omit', 'extra', 'alias', 'drift'):
                changed = copy.deepcopy(positive)
                path = sorted(changed[key])[0]
                value = changed[key][path]
                if attack == 'omit':
                    del changed[key][path]
                elif attack == 'extra':
                    changed[key][path + '.unreviewed'] = value
                elif attack == 'alias':
                    changed[key][path.upper()] = changed[key].pop(path)
                elif key == 'replaced_source_sha256':
                    changed[key][path]['previous_sha256'] = '0' * 64
                else:
                    changed[key][path] = '0' * 64
                with self.subTest(key=key, attack=attack), self.assertRaises(ValueError):
                    validate(changed)
        for path in CHANGED:
            for row in ({'previous_sha256': base['source_sha256'][path], 'sha256': '0' * 64},
                        {'previous_sha256': base['source_sha256'][path]},
                        {**positive['replaced_source_sha256'][path], 'approved': True}):
                changed = copy.deepcopy(positive)
                changed['replaced_source_sha256'][path] = row
                with self.subTest(path=path, row=row), self.assertRaises(ValueError):
                    validate(changed)

    def test_contract_omissions_extras_aliases_and_pin_drift_are_rejected(self):
        positive = candidate.load_replay_contract(ROOT)
        for path in sorted(CHANGED | ADDED):
            for attack in ('repin', 'omit', 'alias', 'extra'):
                changed = copy.deepcopy(positive)
                if attack == 'repin':
                    changed['source_sha256'][path] = '0' * 64
                elif attack == 'omit':
                    del changed['source_sha256'][path]
                elif attack == 'alias':
                    changed['source_sha256'][path.upper()] = changed['source_sha256'].pop(path)
                else:
                    changed['source_sha256'][path + '.unreviewed'] = '0' * 64
                with self.subTest(path=path, attack=attack), self.assertRaises(ValueError):
                    candidate.validate_replay_contract(changed)
        for key, value in (('contract', candidate.ALPHA23_REPLAY_CONTRACT_ID),
                           ('source_migration_sha256', candidate.ALPHA23_MIGRATION_SHA256),
                           ('base_reviewed_source', candidate.COCKPIT_VARIANT_BASE_SOURCE),
                           ('base_reviewed_runtime_tree', candidate.COCKPIT_VARIANT_BASE_TREE),
                           ('scope', 'native and release approved'), ('release_authorized', True),
                           ('release_authorized', 0)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                candidate.validate_replay_contract({**positive, key: value})
        base = json.loads((ROOT / candidate.COCKPIT_VARIANT_BASE_CONTRACT_PATH).read_text())
        with self.assertRaises(ValueError):
            candidate.validate_replay_contract(base)

    def test_positive_capture_and_committed_drift_need_no_alpha23_base_git_objects(self):
        repo, source = self.fixture()
        result = subprocess.run(['git', 'cat-file', '-e',
            candidate.COCKPIT_VARIANT_BASE_SOURCE + '^{commit}'], cwd=repo, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(source['replay_contract']['contract'], candidate.REPLAY_CONTRACT_ID)
        self.assertIs(source['replay_contract']['release_authorized'], False)
        candidate.validate_source_evidence(source, self.report(source))
        for relative in sorted(CHANGED | ADDED):
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b'\n')
            with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'canonical baseline changed'):
                capture.source_evidence(repo, self.commit(repo))
            path.write_bytes(original)
            capture.source_evidence(repo, self.commit(repo))

    def test_concealed_dirty_checkout_never_substitutes_for_canonical_source(self):
        repo, _ = self.fixture()
        expected = candidate.git(repo, 'rev-parse', 'HEAD')
        for flag, reset in (('--assume-unchanged', '--no-assume-unchanged'),
                            ('--skip-worktree', '--no-skip-worktree')):
            for relative in sorted(CHANGED | ADDED):
                path = repo / relative
                original = path.read_bytes()
                subprocess.run(['git', 'update-index', flag, relative], cwd=repo, check=True)
                try:
                    path.write_bytes(original + b'\n')
                    self.assertEqual(candidate.git(repo, 'status', '--porcelain', '--untracked-files=all'), '')
                    with self.subTest(flag=flag, path=relative), self.assertRaisesRegex(
                            ValueError, 'checkout differs from canonical'):
                        capture.source_evidence(repo, expected)
                finally:
                    path.write_bytes(original)
                    subprocess.run(['git', 'update-index', reset, relative], cwd=repo, check=True)
        capture.source_evidence(repo, expected)

    @staticmethod
    def report(source):
        return {'source_sha': source['source_sha'], 'replay_contract': candidate.REPLAY_CONTRACT_ID,
                'replay_contract_sha256': source['replay_contract_sha256']}

    def test_export_cannot_omit_or_spoof_canonical_checkout_or_resealed_hashes(self):
        _, source = self.fixture()
        candidate.validate_source_evidence(source, self.report(source))
        for relative in sorted(CHANGED | ADDED):
            for attack in ('omit-file', 'omit-evidence', 'alias-file', 'canonical', 'checkout',
                           'canonical-and-checkout', 'reseal-contract'):
                changed = copy.deepcopy(source)
                row = next(item for item in changed['files'] if item['path'] == relative)
                evidence = changed['reviewed_replay_source_evidence'][relative]
                if attack == 'omit-file':
                    changed['files'].remove(row)
                elif attack == 'omit-evidence':
                    del changed['reviewed_replay_source_evidence'][relative]
                elif attack == 'alias-file':
                    row['path'] = relative.upper()
                elif attack == 'canonical':
                    evidence['canonical_sha256'] = '0' * 64
                elif attack == 'checkout':
                    row['checkout_sha256'] = evidence['checkout_sha256'] = '0' * 64
                elif attack == 'canonical-and-checkout':
                    row['checkout_sha256'] = evidence['checkout_sha256'] = evidence['canonical_sha256'] = '0' * 64
                else:
                    changed['replay_contract']['source_sha256'][relative] = '0' * 64
                    changed['replay_contract_text'] = json.dumps(changed['replay_contract'])
                    data = changed['replay_contract_text'].encode()
                    digest = candidate.hashlib.sha256(data).hexdigest()
                    changed['replay_contract_sha256'] = digest
                    contract_row = next(item for item in changed['files']
                                        if item['path'] == candidate.REPLAY_CONTRACT_PATH)
                    contract_row['checkout_sha256'] = digest
                    contract_row['checkout_bytes'] = len(data)
                    row['checkout_sha256'] = evidence['checkout_sha256'] = evidence['canonical_sha256'] = '0' * 64
                with self.subTest(path=relative, attack=attack), self.assertRaises(ValueError):
                    candidate.validate_source_evidence(changed, self.report(changed))

    def test_new_module_boundaries_reject_helpers_auto_targets_and_root_case_aliases(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for path in sorted(CHANGED | RUNTIME):
            for changed in (positive - {path}, positive | {path.upper()}, positive | {path + '.rs'}):
                with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)
        extras = (
            'crates/flightsim-app/src/cockpit_layout/hidden.rs',
            'crates/flightsim-render/src/cockpit/variants/hidden.rs',
            'crates/flightsim-app/build.rs', 'crates/flightsim-render/build.rs',
            'crates/flightsim-app/src/bin/unreviewed.rs',
            'crates/flightsim-render/examples/unreviewed.rs',
            'crates/flightsim-app/benches/unreviewed.rs',
            'CRATES/flightsim-render/src/cockpit/unreviewed.rs',
        )
        for extra in extras:
            with self.subTest(extra=extra), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                candidate.validate_modified_source_boundaries(positive | {extra})
        repo, _ = self.fixture()
        expected = candidate.git(repo, 'rev-parse', 'HEAD')
        for relative in extras:
            path = repo / relative
            (repo / '.git/info/exclude').write_text('/' + relative + '\n')
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'// ignored unreviewed source\n')
            self.assertEqual(candidate.git(repo, 'status', '--porcelain', '--untracked-files=all'), '')
            with self.subTest(ignored=relative), self.assertRaisesRegex(
                    ValueError, 'unreviewed modified source checkout input|modified source root case alias'):
                capture.source_evidence(repo, expected)
            path.unlink()
        alias = repo / 'CRATES'
        if alias.exists() and not alias.samefile(repo / 'crates'):
            for child in sorted(alias.rglob('*'), key=lambda item: len(item.parts), reverse=True):
                if child.is_dir():
                    child.rmdir()
            alias.rmdir()
        (repo / '.git/info/exclude').write_text('')
        capture.source_evidence(repo, expected)
        for relative in extras:
            path = repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'// untracked, then committed unreviewed source\n')
            self.assertNotEqual(candidate.git(repo, 'status', '--porcelain', '--untracked-files=all'), '')
            with self.subTest(untracked=relative), self.assertRaisesRegex(ValueError, 'source checkout must be clean'):
                capture.source_evidence(repo, expected)
            with self.subTest(untracked_boundary=relative), self.assertRaisesRegex(
                    ValueError, 'unreviewed modified source checkout input|modified source root case alias'):
                candidate.validate_modified_source_checkout(repo)
            expected = self.commit(repo)
            with self.subTest(committed=relative), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                capture.source_evidence(repo, expected)
            path.unlink()
            expected = self.commit(repo)
        if alias.exists() and not alias.samefile(repo / 'crates'):
            for child in sorted(alias.rglob('*'), key=lambda item: len(item.parts), reverse=True):
                if child.is_dir():
                    child.rmdir()
            alias.rmdir()
        capture.source_evidence(repo, expected)

    def test_new_module_and_ancestor_symlink_and_reparse_guards(self):
        repo, _ = self.fixture()
        candidate.validate_modified_source_checkout(repo)
        guarded = {'crates', 'crates/flightsim-app', 'crates/flightsim-app/src',
                   'crates/flightsim-render', 'crates/flightsim-render/src',
                   'crates/flightsim-render/src/cockpit', *RUNTIME}
        original_lstat = Path.lstat
        for relative in sorted(guarded):
            target = repo / relative
            self.assertTrue(target.exists())
            with self.subTest(symlink=relative), patch.object(
                    Path, 'is_symlink', autospec=True, side_effect=lambda path: path == target):
                with self.assertRaisesRegex(ValueError, 'modified source (root|symlink)'):
                    candidate.validate_modified_source_checkout(repo)
            def reparse_details(path, *args, **kwargs):
                details = original_lstat(path, *args, **kwargs)
                if path == target:
                    return SimpleNamespace(st_mode=details.st_mode, st_file_attributes=0x400)
                return details
            with self.subTest(reparse=relative), patch.object(
                    Path, 'lstat', autospec=True, side_effect=reparse_details):
                with self.assertRaisesRegex(ValueError, 'reparse'):
                    candidate.validate_modified_source_checkout(repo)
        candidate.validate_modified_source_checkout(repo)

    def test_version_physics_replay_rights_and_original_witnesses_are_unchanged(self):
        current = candidate.load_replay_contract(ROOT)['source_sha256']
        base = json.loads((ROOT / candidate.COCKPIT_VARIANT_BASE_CONTRACT_PATH).read_text())['source_sha256']
        unchanged = {'Cargo.toml', 'Cargo.lock', candidate.PRESERVED_RUNTIME_PATH,
                     candidate.COMPONENT_TERMS_MIGRATION_PATH, candidate.TERRAIN_CENTROID_MIGRATION_PATH,
                     candidate.TERRAIN_STITCH_MIGRATION_PATH, candidate.ALPHA22_MIGRATION_PATH,
                     candidate.COCKPIT_MIGRATION_PATH, candidate.ALPHA23_MIGRATION_PATH,
                     *candidate.HISTORICAL_CONTRACT_HASHES, *candidate.ALPHA23_BASE_CONTRACT_HASHES,
                     *candidate.INDEPENDENT_REPLAY_HASHES}
        unchanged |= {path for path in base if path.startswith((
            'crates/flightsim-fdm/', 'crates/flightsim-sim/', 'assets/', 'docs/release/'))}
        for path in unchanged:
            expected = base.get(path, candidate.INDEPENDENT_REPLAY_HASHES.get(path))
            self.assertEqual(candidate.digest(ROOT / path), expected, path)
            if path in base:
                self.assertEqual(current[path], expected, path)

    def test_stale_alpha23_authorization_and_applicability_cannot_accept_changed_cockpits(self):
        # All authority in this isolated fixture is explicitly synthetic. Real
        # release receipts and completed reviews are never edited or generated.
        spec = importlib.util.spec_from_file_location(
            'cockpit_variant_applicability', ROOT / 'scripts/tests/test_ordinary_release_applicability.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        f = module.ApplicabilityFixture(self)
        q = module.q
        f.fixture.write('Cargo.toml', '[workspace.package]\nversion = "0.6.0-alpha.23"\n')
        package = f.fixture.inventory['packages'][0]
        package['id'], package['version'] = 'flightsim-app@0.6.0-alpha.23', '0.6.0-alpha.23'
        for path, historical in RELOCATIONS.items():
            f.fixture.write(path, (ROOT / historical).read_text())
        f.fixture.refresh_evidence()
        f.payload.captured.write_bytes(f.payload.original.read_bytes())
        f.comparison()
        f.review['basis']['original_inventory'] = q.capture.file_record(f.payload.original)
        f.review['dependency_review'] = q.capture.file_record(f.repo / q.release.DEPENDENCY_REVIEW)
        f.review['source_content_files'] = q.source_content(f.repo)
        f.save()
        self.assertIs(q.validate(f.repo, f.native)['conditions_matched'], True)
        preserved = {path: (f.repo / path).read_bytes() for path in (q.PATH, q.release.AUTHORIZATION)}
        for path in CHANGED | RUNTIME:
            f.fixture.write(path, (ROOT / path).read_text())
        f.fixture.commit()
        gate, _ = q.release.inspect(f.repo)
        self.assertFalse(gate['authorized'])
        self.assertIn('AUTHORIZATION_STALE_OR_INVALID', {row['code'] for row in gate['blockers']})
        with self.assertRaisesRegex(ValueError, 'authorization gate is blocked'):
            q.validate(f.repo, f.native)
        self.assertEqual(preserved, {path: (f.repo / path).read_bytes() for path in preserved})
        # Even a newly issued synthetic receipt cannot extend the old review.
        f.fixture.authorize_fixture()
        self.assertIs(q.release.inspect(f.repo)[0]['authorized'], True)
        with self.assertRaisesRegex(ValueError, 'reviewed source content changed'):
            q.validate(f.repo, f.native)
        self.assertEqual((f.repo / q.PATH).read_bytes(), preserved[q.PATH])


if __name__ == '__main__':
    unittest.main()
