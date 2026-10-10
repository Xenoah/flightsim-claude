"""Real committed-source and export adversaries; never native build evidence."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


capture = load('component_terms_source_capture', 'capture-analytical-swift-msvc.py')
candidate = capture.check.candidate
MAIN = 'crates/flightsim-app/src/main.rs'
HISTORY = 'scripts/history/4d40f9a-flightsim-app-main.rs'
RUNTIME = {
    'crates/flightsim-app/src/component_terms.rs',
    'crates/flightsim-app/src/component_terms_dialog.ps1',
}
LICENSES = {'LICENSE-MIT', 'LICENSE-APACHE'}
TERMS = {
    'docs/release/components/MICROSOFT-COMPONENT-TERMS.txt',
    'docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt',
    'docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt',
}


class ComponentTermsSourceAdmissionTests(unittest.TestCase):
    def fixture(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        repo = Path(temporary.name)
        subprocess.run(['git', 'init', '-q', str(repo)], check=True)
        for key, value in (('gc.auto', '0'), ('maintenance.auto', 'false'), ('core.autocrlf', 'false')):
            subprocess.run(['git', 'config', key, value], cwd=repo, check=True)
        paths = (candidate.REPLAY_CONTRACT_PATHS | set(candidate.INDEPENDENT_REPLAY_HASHES)
                 | capture.check.SOURCE_PATHS | capture.BOUND_FILES | set(capture.INHERITED_CONTRACTS)
                 | {capture.CONTRACT, '.gitattributes', 'assets/aircraft/.gitattributes',
                    'docs/release/.gitattributes'})
        for relative in paths:
            destination = repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((ROOT / relative).read_bytes())
        commit = self.commit(repo)
        evidence = capture.source_evidence(repo, commit)
        self.assertEqual(evidence['source_sha'], commit)
        self.assertEqual(evidence['source_tree'], candidate.git(repo, 'rev-parse', 'HEAD^{tree}'))
        return repo, evidence

    @staticmethod
    def commit(repo):
        subprocess.run(['git', 'add', '.'], cwd=repo, check=True)
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                        'commit', '-qm', 'component terms source fixture'], cwd=repo, check=True)
        return candidate.git(repo, 'rev-parse', 'HEAD')

    def test_literal_additions_and_only_startup_replacement_preserve_old_bytes(self):
        contract = candidate.load_replay_contract(ROOT)
        migration = json.loads((ROOT / candidate.COMPONENT_TERMS_MIGRATION_PATH).read_text())
        self.assertEqual(candidate.COMPONENT_TERMS_RUNTIME_PATHS, RUNTIME)
        self.assertEqual(candidate.COMPONENT_TERMS_DOCUMENT_PATHS, TERMS)
        self.assertEqual(candidate.COMPONENT_TERMS_LICENSE_PATHS, LICENSES)
        self.assertEqual(migration['historical_relocations'], {MAIN: HISTORY})
        self.assertEqual(set(migration['replaced_source_sha256']), {MAIN})
        self.assertEqual(set(migration['added_source_sha256']), RUNTIME | TERMS | LICENSES)
        previous = '160cc900acbb0029452c4175fee9ba24bc26611ce41faaa57a97a8426b97811a'
        self.assertEqual(candidate.digest(ROOT / HISTORY), previous)
        self.assertEqual(migration['replaced_source_sha256'][MAIN]['previous_sha256'], previous)
        self.assertNotEqual(candidate.digest(ROOT / MAIN), previous)
        self.assertEqual(migration['replaced_source_sha256'][MAIN]['sha256'], candidate.digest(ROOT / MAIN))
        frozen = json.loads((ROOT / candidate.PRESERVED_RUNTIME_PATH).read_text())['source_sha256']
        self.assertEqual(len(frozen), 497)
        self.assertEqual(len(candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 495)
        for relative, expected in frozen.items():
            actual_path = candidate.HISTORICAL_RUNTIME_RELOCATIONS.get(relative, relative)
            self.assertEqual(contract['source_sha256'][actual_path], expected, relative)
            self.assertEqual(candidate.digest(ROOT / actual_path), expected, relative)
        for relative in {MAIN, HISTORY} | RUNTIME | TERMS | LICENSES:
            self.assertEqual(contract['source_sha256'][relative], candidate.digest(ROOT / relative))

    def test_new_identity_separates_base_provenance_from_final_source(self):
        replay = candidate.load_replay_contract(ROOT)
        analytical = json.loads((ROOT / capture.check.CONTRACT).read_text())
        captured = json.loads((ROOT / capture.CONTRACT).read_text())
        self.assertEqual(replay['contract'], 'full-two-aircraft-terrain-stitch-source-v1')
        self.assertEqual(analytical['source_admission'], 'analytical-swift-terrain-stitch-source-admission-v1')
        self.assertEqual(captured['source_admission'], 'analytical-swift-terrain-stitch-capture-source-admission-v1')
        for contract in (replay, analytical, captured):
            self.assertNotIn('reviewed_source', contract)
            self.assertNotIn('reviewed_runtime_tree', contract)
            self.assertEqual(contract['base_reviewed_source'], '960c3126e6a4bc22b8d4acc6e6737f8f2b473bef')
            self.assertEqual(contract['base_reviewed_runtime_tree'], 'b3bbd57bc3819b1b1eda35ce4f8b476c52d1b24e')
            self.assertEqual(contract['source_migration_sha256'], candidate.digest(ROOT / candidate.TERRAIN_STITCH_MIGRATION_PATH))
        self.assertIs(replay['release_authorized'], False)
        self.assertIs(analytical['release_authorized'], False)
        with self.assertRaises(ValueError):
            candidate.validate_replay_contract({**replay, 'contract': 'full-two-aircraft-reviewed-source-v1'})

    def test_repinning_any_added_or_relocated_file_is_rejected(self):
        positive = candidate.load_replay_contract(ROOT)
        for relative in {MAIN, HISTORY, candidate.COMPONENT_TERMS_MIGRATION_PATH} | RUNTIME | TERMS | LICENSES:
            changed = copy.deepcopy(positive)
            changed['source_sha256'][relative] = '0' * 64
            with self.subTest(relative=relative), self.assertRaises(ValueError):
                candidate.validate_replay_contract(changed)

    def test_actual_capture_chain_accepts_exact_fixture_and_rejects_committed_drift(self):
        repo, _ = self.fixture()
        for relative in [MAIN, HISTORY, candidate.COMPONENT_TERMS_MIGRATION_PATH, *sorted(RUNTIME | TERMS | LICENSES)]:
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b'\n')
            with self.subTest(relative=relative), self.assertRaises(ValueError):
                capture.source_evidence(repo, self.commit(repo))
            path.write_bytes(original)
            capture.source_evidence(repo, self.commit(repo))

    def test_closed_runtime_and_terms_members_reject_omission_alias_and_extra(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for relative in sorted(RUNTIME | TERMS):
            for changed in (positive - {relative}, positive | {relative.upper()},
                            positive | {str(Path(relative).parent / 'unreviewed-extra')}):
                with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)
        repo, _ = self.fixture()
        extra = 'docs/release/components/ignored-terms.txt'
        (repo / '.git/info/exclude').write_text('/' + extra + '\n')
        (repo / extra).write_text('unreviewed alternative')
        with self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input'):
            capture.source_evidence(repo, candidate.git(repo, 'rev-parse', 'HEAD'))

    def test_exported_source_cannot_omit_substitute_or_reseal_component_inputs(self):
        _, source = self.fixture()
        report = {'source_sha': source['source_sha'], 'replay_contract': candidate.REPLAY_CONTRACT_ID,
                  'replay_contract_sha256': source['replay_contract_sha256']}
        candidate.validate_source_evidence(source, report)
        for relative in [MAIN, HISTORY, *sorted(RUNTIME | TERMS | LICENSES)]:
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
                    digest = hashlib.sha256(altered['replay_contract_text'].encode()).hexdigest()
                    altered['replay_contract_sha256'] = digest
                    altered_report['replay_contract_sha256'] = digest
                with self.subTest(relative=relative, attack=attack), self.assertRaises(ValueError):
                    candidate.validate_source_evidence(altered, altered_report)


if __name__ == '__main__':
    unittest.main()
