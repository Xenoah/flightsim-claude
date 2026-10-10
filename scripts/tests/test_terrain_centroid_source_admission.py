"""Exact terrain-only source migration; never native appearance or approval."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    'terrain_source_fixture', ROOT / 'scripts/tests/test_component_terms_source_admission.py')
fixture_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture_module)
capture = fixture_module.capture
candidate = capture.check.candidate
DETAIL = 'crates/flightsim-render/src/terrain_detail.rs'
HISTORY = 'scripts/history/42a7dde-terrain_detail.rs'
FORWARD = 'vendor/bevy_pbr/src/render/forward_io.wgsl'
TEST = 'crates/flightsim-render/src/terrain_interpolation_tests.rs'
CHANGED = {
    DETAIL, FORWARD,
    'vendor/bevy_pbr/FLIGHTSIM-MODIFICATION-NOTICE.txt',
    'vendor/bevy_pbr/FLIGHTSIM-PATCHES.md',
    'docs/release/analytical-modified-source-provenance.json',
    'docs/release/modified-source-patches/bevy_pbr-added-vendor.patch',
}


class TerrainCentroidSourceAdmissionTests(unittest.TestCase):
    fixture = fixture_module.ComponentTermsSourceAdmissionTests.fixture
    commit = staticmethod(fixture_module.ComponentTermsSourceAdmissionTests.commit)

    def test_bounded_migration_preserves_earlier_source_identities(self):
        contract = candidate.load_replay_contract(ROOT)
        migration = json.loads((ROOT / candidate.TERRAIN_CENTROID_MIGRATION_PATH).read_text())
        self.assertEqual(set(candidate.TERRAIN_CENTROID_PREVIOUS_SHA256), CHANGED)
        self.assertEqual(set(migration['replaced_source_sha256']), CHANGED)
        self.assertEqual(set(migration['added_source_sha256']), {TEST})
        self.assertEqual(candidate.TERRAIN_CENTROID_RUNTIME_PATHS, {TEST})
        self.assertEqual(migration['historical_relocations'], {DETAIL: HISTORY})
        self.assertEqual(candidate.digest(ROOT / HISTORY),
                         '6670c630aa58391ae05d5b70b847bf2df1f2b01c3805dc594628053b3a317aa5')
        for path in CHANGED:
            row = migration['replaced_source_sha256'][path]
            self.assertEqual(row['previous_sha256'], candidate.TERRAIN_CENTROID_PREVIOUS_SHA256[path])
            self.assertNotEqual(row['previous_sha256'], row['sha256'])
            self.assertEqual(row['sha256'], candidate.digest(ROOT / path))
        self.assertEqual(candidate.digest(ROOT / candidate.COMPONENT_TERMS_MIGRATION_PATH),
                         'b41c6e7d163f98d0ddc4bddb3063dbdff82ea3462064b2b887954367b457bcc9')
        frozen = json.loads((ROOT / candidate.PRESERVED_RUNTIME_PATH).read_text())['source_sha256']
        self.assertEqual(len(frozen), 497)
        for path, expected in frozen.items():
            relative = candidate.CURRENT_HISTORICAL_RUNTIME_RELOCATIONS.get(path, path)
            self.assertEqual(contract['source_sha256'][relative], expected)
            self.assertEqual(candidate.digest(ROOT / relative), expected)
        self.assertEqual(len(candidate.COCKPIT_REPLAY_CONTRACT_PATHS - candidate.TERRAIN_STITCH_SOURCE_PATHS
                             - candidate.ALPHA22_SOURCE_PATHS - candidate.COCKPIT_SOURCE_PATHS), 930)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 497)
        self.assertEqual(len(candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertIs(contract['release_authorized'], False)

    def test_each_new_or_changed_pin_and_old_identity_rejected(self):
        positive = candidate.load_replay_contract(ROOT)
        paths = CHANGED | {TEST, HISTORY, candidate.TERRAIN_CENTROID_MIGRATION_PATH}
        for path in paths:
            changed = copy.deepcopy(positive)
            changed['source_sha256'][path] = '0' * 64
            with self.subTest(path=path), self.assertRaises(ValueError):
                candidate.validate_replay_contract(changed)
        for identity in ('full-two-aircraft-component-terms-source-v1',
                         'full-two-aircraft-reviewed-source-v1', candidate.HISTORICAL_REPLAY_CONTRACT_ID):
            with self.subTest(identity=identity), self.assertRaises(ValueError):
                candidate.validate_replay_contract({**positive, 'contract': identity})

    def test_actual_capture_chain_rejects_committed_drift(self):
        repo, _ = self.fixture()
        for relative in sorted(CHANGED | {TEST, HISTORY, candidate.TERRAIN_CENTROID_MIGRATION_PATH}):
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b'\n')
            with self.subTest(path=relative), self.assertRaises(ValueError):
                capture.source_evidence(repo, self.commit(repo))
            path.write_bytes(original)
            capture.source_evidence(repo, self.commit(repo))

    def test_new_runtime_input_keeps_closed_boundary(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for changed in (positive - {TEST}, positive | {TEST.upper()},
                        positive | {'crates/flightsim-render/src/terrain_extra.rs'}):
            with self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                candidate.validate_modified_source_boundaries(changed)
        repo, _ = self.fixture()
        extra = 'crates/flightsim-render/src/terrain_extra.rs'
        (repo / '.git/info/exclude').write_text('/' + extra + '\n')
        (repo / extra).write_text('// ignored input must not be admitted\n')
        with self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input'):
            capture.source_evidence(repo, candidate.git(repo, 'rev-parse', 'HEAD'))

    def test_export_cannot_omit_substitute_or_reseal_terrain_inputs(self):
        _, source = self.fixture()
        report = {'source_sha': source['source_sha'], 'replay_contract': candidate.REPLAY_CONTRACT_ID,
                  'replay_contract_sha256': source['replay_contract_sha256']}
        candidate.validate_source_evidence(source, report)
        for relative in sorted(CHANGED | {TEST, HISTORY, candidate.TERRAIN_CENTROID_MIGRATION_PATH}):
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

    def test_complete_vendor_patch_recreates_current_exact_tree(self):
        provenance = json.loads((ROOT / 'docs/release/analytical-modified-source-provenance.json').read_text())
        row = next(p for p in provenance['packages'] if p['id'] == 'bevy_pbr@0.18.1')
        self.assertIn('src/render/forward_io.wgsl', row['modified_paths'])
        self.assertEqual(candidate.digest(ROOT / row['patch']['path']), row['patch']['sha256'])
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            subprocess.run(['git', 'apply', '--binary', str(ROOT / row['patch']['path'])],
                           cwd=destination, check=True, capture_output=True)
            actual = capture.snapshot_tree(destination / row['source_root'])
            expected = {entry['path']: {k: entry[k] for k in ('bytes', 'sha256')} for entry in row['files']}
            self.assertEqual(actual, expected)
            self.assertEqual(actual, capture.snapshot_tree(ROOT / row['source_root']))


if __name__ == '__main__':
    unittest.main()
