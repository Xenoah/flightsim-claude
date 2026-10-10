"""Exact cockpit source admission; no native, rights or release acceptance."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    'cockpit_source_fixture', ROOT / 'scripts/tests/test_component_terms_source_admission.py')
fixture_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture_module)
capture = fixture_module.capture
candidate = capture.check.candidate
CHANGED = {
    'crates/flightsim-app/src/main.rs',
    'crates/flightsim-app/src/aircraft_scene.rs',
    'crates/flightsim-render/src/cockpit.rs',
    'crates/flightsim-ui/src/instruments.rs',
}
RUNTIME = {
    'crates/flightsim-app/src/cockpit_runtime.rs',
    'crates/flightsim-render/src/cockpit/texture.rs',
}
RELOCATIONS = {
    'crates/flightsim-app/src/main.rs': 'scripts/history/be587384-flightsim-app-main.rs',
    'crates/flightsim-app/src/aircraft_scene.rs': 'scripts/history/be587384-aircraft_scene.rs',
    'crates/flightsim-render/src/cockpit.rs': 'scripts/history/be587384-cockpit.rs',
    'crates/flightsim-ui/src/instruments.rs': 'scripts/history/be587384-instruments.rs',
}
CONTRACTS = {
    'scripts/history/be587384-replay-candidate-contract.json',
    'scripts/history/be587384-analytical-swift-source-contract.json',
    'scripts/history/be587384-analytical-swift-capture-contract.json',
}
ADDED = RUNTIME | set(RELOCATIONS.values()) | CONTRACTS | {'scripts/cockpit-source-migration.json'}


class CockpitSourceAdmissionTests(unittest.TestCase):
    fixture = fixture_module.ComponentTermsSourceAdmissionTests.fixture
    commit = staticmethod(fixture_module.ComponentTermsSourceAdmissionTests.commit)

    def test_exact_delta_preserves_all_940_prior_pins_and_seven_witnesses(self):
        candidate.load_replay_contract(ROOT)
        current = json.loads((ROOT / candidate.ALPHA23_BASE_CONTRACT_PATH).read_text())
        candidate.validate_cockpit_replay_contract(current)
        base = json.loads((ROOT / candidate.COCKPIT_BASE_CONTRACT_PATH).read_text())
        migration = json.loads((ROOT / candidate.COCKPIT_MIGRATION_PATH).read_text())
        candidate.validate_alpha22_replay_contract(base)
        self.assertEqual(candidate.COCKPIT_SOURCE_PATHS, ADDED)
        self.assertEqual(candidate.COCKPIT_RUNTIME_PATHS, RUNTIME)
        self.assertEqual(candidate.COCKPIT_HISTORICAL_RELOCATIONS, RELOCATIONS)
        self.assertEqual(set(candidate.COCKPIT_BASE_CONTRACT_HASHES), CONTRACTS)
        self.assertEqual(len(CONTRACTS | set(RELOCATIONS.values())), 7)
        self.assertEqual(set(migration['replaced_source_sha256']), CHANGED)
        self.assertEqual(set(migration['added_source_sha256']), RUNTIME)
        self.assertEqual(migration['historical_relocations'], RELOCATIONS)
        self.assertEqual(migration['base_source'], 'be5873840dc86eb551971bad6e7290840eaf9cd2')
        self.assertEqual(migration['base_tree'], '49479664f838700cf52ed7621a727db7732752e2')
        self.assertEqual(migration['base_source_kind'], 'published-commit')
        self.assertEqual(migration['previous_source_migration_sha256'], candidate.ALPHA22_MIGRATION_SHA256)
        self.assertEqual(migration['preserved_runtime_sha256'], candidate.PRESERVED_RUNTIME_SHA256)
        self.assertEqual(len(base['source_sha256']), 940)
        self.assertEqual(len(current['source_sha256']), 950)
        self.assertEqual(set(current['source_sha256']) - set(base['source_sha256']), ADDED)
        self.assertEqual({path for path, expected in base['source_sha256'].items()
                          if current['source_sha256'][path] != expected}, CHANGED)
        for path, expected in base['source_sha256'].items():
            historical = RELOCATIONS.get(path, path)
            self.assertEqual(current['source_sha256'][historical], expected, path)
            self.assertEqual(candidate.digest(ROOT / candidate.ALPHA23_HISTORICAL_RELOCATIONS.get(historical, historical)), expected, path)
        for path in CHANGED:
            self.assertEqual(migration['replaced_source_sha256'][path], {
                'previous_sha256': base['source_sha256'][path], 'sha256': candidate.digest(ROOT / path)})
        for path in RUNTIME:
            self.assertEqual(migration['added_source_sha256'][path], candidate.digest(ROOT / path))
        self.assertEqual(len(candidate.PRESERVED_CRATE_SOURCE_PATHS), 492)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 497)
        self.assertEqual(len(candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertEqual(len(capture.check.SOURCE_PATHS), 55)
        self.assertEqual(len(capture.BOUND_FILES), 4)
        self.assertIs(current['release_authorized'], False)

    def test_every_current_or_historical_pin_rejects_repinning(self):
        positive = candidate.load_replay_contract(ROOT)
        for path in sorted(candidate.REPLAY_CONTRACT_PATHS):
            changed = copy.deepcopy(positive)
            changed['source_sha256'][path] = '0' * 64
            with self.subTest(path=path), self.assertRaises(ValueError):
                candidate.validate_replay_contract(changed)

    def test_old_identity_and_migration_never_become_cockpit_evidence(self):
        positive = candidate.load_replay_contract(ROOT)
        base = json.loads((ROOT / candidate.COCKPIT_BASE_CONTRACT_PATH).read_text())
        for changed in (base, {**positive, 'contract': candidate.ALPHA22_REPLAY_CONTRACT_ID},
                        {**positive, 'source_migration_sha256': candidate.ALPHA22_MIGRATION_SHA256},
                        {**positive, 'release_authorized': True},
                        {**positive, 'scope': 'native cockpit and publication qualified'}):
            with self.assertRaises(ValueError):
                candidate.validate_replay_contract(changed)
        for historical, key, value in (
            ('scripts/history/be587384-analytical-swift-source-contract.json', 'source_admission', capture.check.SOURCE_ADMISSION),
            ('scripts/history/be587384-analytical-swift-capture-contract.json', 'source_admission', capture.SOURCE_ADMISSION),
        ):
            self.assertNotEqual(json.loads((ROOT / historical).read_text())[key], value)

    def test_main_retains_both_startup_and_pre_cockpit_witnesses(self):
        main = 'crates/flightsim-app/src/main.rs'
        earlier = json.loads((ROOT / candidate.COMPONENT_TERMS_MIGRATION_PATH).read_text())
        self.assertEqual(candidate.HISTORICAL_RUNTIME_RELOCATIONS[main], candidate.COMPONENT_TERMS_HISTORY_PATH)
        self.assertEqual(candidate.CURRENT_HISTORICAL_RUNTIME_RELOCATIONS[main], candidate.COMPONENT_TERMS_HISTORY_PATH)
        self.assertEqual(candidate.digest(ROOT / RELOCATIONS[main]),
                         earlier['replaced_source_sha256'][main]['sha256'])
        self.assertEqual(candidate.digest(ROOT / candidate.COMPONENT_TERMS_HISTORY_PATH),
                         earlier['replaced_source_sha256'][main]['previous_sha256'])
        self.assertNotEqual(candidate.digest(ROOT / main), candidate.digest(ROOT / RELOCATIONS[main]))

    def test_no_version_physics_replay_or_prior_migration_repin(self):
        current = candidate.load_replay_contract(ROOT)['source_sha256']
        base = json.loads((ROOT / candidate.COCKPIT_BASE_CONTRACT_PATH).read_text())['source_sha256']
        paths = {'Cargo.toml', 'Cargo.lock', candidate.PRESERVED_RUNTIME_PATH,
                 candidate.COMPONENT_TERMS_MIGRATION_PATH, candidate.TERRAIN_CENTROID_MIGRATION_PATH,
                 candidate.TERRAIN_STITCH_MIGRATION_PATH, candidate.ALPHA22_MIGRATION_PATH,
                 *candidate.INDEPENDENT_REPLAY_HASHES}
        paths |= {path for path in base if path.startswith(('crates/flightsim-fdm/', 'crates/flightsim-sim/'))}
        for path in paths:
            expected = base.get(path, candidate.INDEPENDENT_REPLAY_HASHES.get(path))
            historical = candidate.ALPHA23_HISTORICAL_RELOCATIONS.get(path, path)
            self.assertEqual(candidate.digest(ROOT / historical), expected, path)
            if path in base:
                self.assertEqual(current[historical], expected, path)

    def test_missing_aliased_and_extra_contract_members_are_rejected(self):
        positive = candidate.load_replay_contract(ROOT)
        for path in sorted(CHANGED | ADDED):
            for attack in ('omit', 'alias', 'extra'):
                changed = copy.deepcopy(positive)
                if attack == 'omit':
                    del changed['source_sha256'][path]
                elif attack == 'alias':
                    changed['source_sha256'][path.upper()] = changed['source_sha256'].pop(path)
                else:
                    changed['source_sha256'][path + '.unreviewed'] = '0' * 64
                with self.subTest(path=path, attack=attack), self.assertRaises(ValueError):
                    candidate.validate_replay_contract(changed)

    def test_actual_capture_chain_needs_no_base_git_object_and_rejects_drift(self):
        repo, _ = self.fixture()
        result = subprocess.run(['git', 'cat-file', '-e',
            'be5873840dc86eb551971bad6e7290840eaf9cd2^{commit}'], cwd=repo, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        for relative in sorted(CHANGED | ADDED):
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b'\n')
            with self.subTest(path=relative), self.assertRaises(ValueError):
                capture.source_evidence(repo, self.commit(repo))
            path.write_bytes(original)
            capture.source_evidence(repo, self.commit(repo))

    def test_crate_closure_rejects_omission_case_alias_and_ignored_helpers(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for relative in sorted(CHANGED | RUNTIME):
            for changed in (positive - {relative}, positive | {relative.upper()},
                            positive | {relative + '.rs'}):
                with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)
        repo, _ = self.fixture()
        for relative in ('crates/flightsim-app/build.rs',
                         'crates/flightsim-app/src/cockpit_extra.rs',
                         'crates/flightsim-render/src/cockpit/extra.rs'):
            path = repo / relative
            (repo / '.git/info/exclude').write_text('/' + relative + '\n')
            path.write_text('// ignored and unreviewed\n')
            with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input'):
                capture.source_evidence(repo, candidate.git(repo, 'rev-parse', 'HEAD'))
            path.unlink()
        (repo / '.git/info/exclude').write_text('')
        capture.source_evidence(repo, candidate.git(repo, 'rev-parse', 'HEAD'))

    def test_export_cannot_omit_substitute_or_reseal_cockpit_inputs(self):
        _, source = self.fixture()
        report = {'source_sha': source['source_sha'], 'replay_contract': candidate.REPLAY_CONTRACT_ID,
                  'replay_contract_sha256': source['replay_contract_sha256']}
        candidate.validate_source_evidence(source, report)
        for relative in sorted(CHANGED | ADDED):
            for attack in ('omit-file', 'omit-evidence', 'canonical', 'checkout', 'reseal-contract'):
                altered = copy.deepcopy(source)
                altered_report = dict(report)
                if attack == 'omit-file':
                    altered['files'] = [row for row in altered['files'] if row['path'] != relative]
                elif attack == 'omit-evidence':
                    del altered['reviewed_replay_source_evidence'][relative]
                elif attack == 'canonical':
                    altered['reviewed_replay_source_evidence'][relative]['canonical_sha256'] = '0' * 64
                elif attack == 'checkout':
                    next(row for row in altered['files'] if row['path'] == relative)['checkout_sha256'] = '0' * 64
                    altered['reviewed_replay_source_evidence'][relative]['checkout_sha256'] = '0' * 64
                else:
                    altered['replay_contract']['source_sha256'][relative] = '0' * 64
                    altered['replay_contract_text'] = json.dumps(altered['replay_contract'])
                    digest = candidate.hashlib.sha256(altered['replay_contract_text'].encode()).hexdigest()
                    altered['replay_contract_sha256'] = digest
                    altered_report['replay_contract_sha256'] = digest
                with self.subTest(path=relative, attack=attack), self.assertRaises(ValueError):
                    candidate.validate_source_evidence(altered, altered_report)


if __name__ == '__main__':
    unittest.main()
