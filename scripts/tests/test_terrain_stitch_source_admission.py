"""Bounded cap source migration; no native, appearance or release acceptance."""
import copy
import importlib.util
import json
import subprocess
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    'stitch_source_fixture', ROOT / 'scripts/tests/test_component_terms_source_admission.py')
fixture_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture_module)
capture = fixture_module.capture
candidate = capture.check.candidate
SEAMS = 'crates/flightsim-world/src/seams.rs'
GEOMETRY_TEST = 'crates/flightsim-world/tests/terrain_seam_geometry.rs'
CHANGED = {SEAMS, GEOMETRY_TEST}
MIGRATION = 'scripts/terrain-stitch-source-migration.json'
BASE_CONTRACT = 'scripts/history/5eaaff3-replay-candidate-contract.json'
RELOCATIONS = {
    SEAMS: 'scripts/history/5eaaff3-seams.rs',
    GEOMETRY_TEST: 'scripts/history/5eaaff3-terrain_seam_geometry.rs',
}
ADDED = {MIGRATION, BASE_CONTRACT, *RELOCATIONS.values()}


class TerrainStitchSourceAdmissionTests(unittest.TestCase):
    fixture = fixture_module.ComponentTermsSourceAdmissionTests.fixture
    commit = staticmethod(fixture_module.ComponentTermsSourceAdmissionTests.commit)

    def test_exact_delta_preserves_all_prior_pins_without_local_git_history(self):
        contract = candidate.load_replay_contract(ROOT)
        migration = json.loads((ROOT / MIGRATION).read_text())
        base = json.loads((ROOT / BASE_CONTRACT).read_text())
        self.assertEqual(candidate.TERRAIN_STITCH_SOURCE_PATHS, ADDED)
        self.assertEqual(candidate.TERRAIN_STITCH_HISTORICAL_RELOCATIONS, RELOCATIONS)
        self.assertEqual(migration['historical_relocations'], RELOCATIONS)
        self.assertEqual(set(migration['replaced_source_sha256']), CHANGED)
        self.assertEqual(migration['added_source_sha256'], {})
        self.assertEqual(migration['base_source_kind'], 'unpublished-local-checkpoint')
        self.assertEqual(migration['base_source'], '5eaaff379f19cc986fa5600619f1491213e9da0d')
        self.assertEqual(migration['base_tree'], 'a469ee76cd0d7aabf3dd2c07114d44c0bbdf5d37')
        self.assertEqual(migration['public_base_source'], '42a7ddeae36f3022c2c77e54d5dce33f71b11bca')
        self.assertEqual(migration['public_base_tree'], '625c39f9b182b16a0dada2dc4131434d178a0831')
        self.assertEqual(candidate.digest(ROOT / BASE_CONTRACT),
                         '1b72efcc010124b0a02708f0c4b75c07c79a272e352ac697b71934fc9aa20869')
        self.assertEqual(base['contract'], 'full-two-aircraft-terrain-centroid-source-v1')
        self.assertEqual(len(base['source_sha256']), 930)
        self.assertEqual(set(contract['source_sha256']) - set(base['source_sha256']), ADDED)
        changed = {path for path, expected in base['source_sha256'].items()
                   if contract['source_sha256'][path] != expected}
        self.assertEqual(changed, CHANGED)
        for path, expected in base['source_sha256'].items():
            historical_path = RELOCATIONS.get(path, path)
            self.assertEqual(contract['source_sha256'][historical_path], expected, path)
            self.assertEqual(candidate.digest(ROOT / historical_path), expected, path)
        for path in CHANGED:
            self.assertEqual(migration['replaced_source_sha256'][path], {
                'previous_sha256': base['source_sha256'][path],
                'sha256': candidate.digest(ROOT / path),
            })
        self.assertEqual(candidate.digest(ROOT / candidate.TERRAIN_CENTROID_MIGRATION_PATH),
                         '203d43b06ef860813fd3f21f482f4751d3ad0b625872086a7eb76d3b91d04db5')
        self.assertEqual(candidate.digest(ROOT / candidate.COMPONENT_TERMS_MIGRATION_PATH),
                         'b41c6e7d163f98d0ddc4bddb3063dbdff82ea3462064b2b887954367b457bcc9')
        self.assertEqual(len(candidate.REPLAY_CONTRACT_PATHS), 934)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 495)
        self.assertEqual(len(candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertEqual(len(capture.check.SOURCE_PATHS), 50)
        self.assertEqual(len(capture.BOUND_FILES), 4)
        self.assertIs(contract['release_authorized'], False)

    def test_every_prior_pin_and_new_member_rejects_repinning(self):
        positive = candidate.load_replay_contract(ROOT)
        for path in sorted(candidate.REPLAY_CONTRACT_PATHS):
            changed = copy.deepcopy(positive)
            changed['source_sha256'][path] = '0' * 64
            with self.subTest(path=path), self.assertRaises(ValueError):
                candidate.validate_replay_contract(changed)
        for identity in ('full-two-aircraft-terrain-centroid-source-v1',
                         'full-two-aircraft-component-terms-source-v1',
                         'full-two-aircraft-reviewed-source-v1', candidate.HISTORICAL_REPLAY_CONTRACT_ID):
            with self.subTest(identity=identity), self.assertRaises(ValueError):
                candidate.validate_replay_contract({**positive, 'contract': identity})
        with self.assertRaises(ValueError):
            candidate.validate_replay_contract({**positive,
                'source_migration_sha256': candidate.TERRAIN_CENTROID_MIGRATION_SHA256})

    def test_actual_capture_chain_rejects_committed_drift(self):
        repo, _ = self.fixture()
        # This fresh repository has none of the unpublished checkpoint objects.
        result = subprocess.run(['git', 'cat-file', '-e',
            '5eaaff379f19cc986fa5600619f1491213e9da0d^{commit}'], cwd=repo,
            capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        for relative in sorted(CHANGED | ADDED):
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b'\n')
            with self.subTest(path=relative), self.assertRaises(ValueError):
                capture.source_evidence(repo, self.commit(repo))
            path.write_bytes(original)
            capture.source_evidence(repo, self.commit(repo))

    def test_existing_crate_boundary_rejects_omission_alias_and_ignored_extra(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for relative in sorted(CHANGED):
            for changed in (positive - {relative}, positive | {relative.upper()},
                            positive | {'crates/flightsim-world/src/stitch_extra.rs'}):
                with self.subTest(path=relative), self.assertRaisesRegex(
                        ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)
        repo, _ = self.fixture()
        extra = 'crates/flightsim-world/examples/stitch_extra.rs'
        (repo / '.git/info/exclude').write_text('/' + extra + '\n')
        (repo / extra).parent.mkdir(parents=True, exist_ok=True)
        (repo / extra).write_text('fn main() {}\n')
        with self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input'):
            capture.source_evidence(repo, candidate.git(repo, 'rev-parse', 'HEAD'))

    def test_export_cannot_omit_substitute_or_reseal_stitch_inputs(self):
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
