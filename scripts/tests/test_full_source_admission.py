"""Source-only migration witnesses. No native or publication evidence is made."""
import copy
import hashlib
import importlib.util
import json
import subprocess
import tempfile
from unittest import mock
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

candidate = load('source_admission_candidate', 'check-swift-windows-candidate.py')

class FullSourceAdmissionTests(unittest.TestCase):
    def test_new_identity_preserves_historical_contracts_and_runtime(self):
        contract = candidate.load_replay_contract(ROOT)
        self.assertEqual(contract['contract'], 'full-two-aircraft-alpha23-version-source-v1')
        self.assertEqual(contract['base_reviewed_source'], '960c3126e6a4bc22b8d4acc6e6737f8f2b473bef')
        self.assertEqual(contract['base_reviewed_runtime_tree'], 'b3bbd57bc3819b1b1eda35ce4f8b476c52d1b24e')
        self.assertIs(contract['release_authorized'], False)
        for path, expected in candidate.HISTORICAL_CONTRACT_HASHES.items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)
        historical = json.loads((ROOT / 'scripts/history/d918943-replay-candidate-contract.json').read_text())
        self.assertEqual(historical['contract'], candidate.HISTORICAL_REPLAY_CONTRACT_ID)
        self.assertEqual(set(historical['source_sha256']), candidate.HISTORICAL_REPLAY_CONTRACT_PATHS)
        self.assertEqual(len(historical['source_sha256']), 404)
        preserved = json.loads((ROOT / candidate.PRESERVED_RUNTIME_PATH).read_text())['source_sha256']
        self.assertEqual(len(preserved), 497)
        self.assertEqual(len(candidate.INDEPENDENT_REPLAY_HASHES), 102)
        relocations = candidate.CURRENT_HISTORICAL_RUNTIME_RELOCATIONS
        for path, expected in preserved.items():
            self.assertEqual(candidate.digest(ROOT / relocations.get(path, path)), expected, path)
        for path, expected in candidate.INDEPENDENT_REPLAY_HASHES.items():
            self.assertEqual(candidate.digest(ROOT / path), expected, path)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertTrue(candidate.CORE_PIPELINE_SOURCE_PATHS <= set(contract['source_sha256']))

    def test_source_identity_authority_and_preserved_pin_mutations_rejected(self):
        positive = candidate.load_replay_contract(ROOT)
        for key, value in [('contract', candidate.HISTORICAL_REPLAY_CONTRACT_ID),
                           ('base_reviewed_source', '0' * 40), ('base_reviewed_runtime_tree', '0' * 40),
                           ('source_migration_sha256', '0' * 64),
                           ('release_authorized', True), ('release_authorized', 0),
                           ('scope', 'native and publication approved')]:
            bad = copy.deepcopy(positive); bad[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                candidate.validate_replay_contract(bad)
        for path in ('crates/flightsim-app/src/airport_drape_runtime.rs',
                     candidate.PRESERVED_RUNTIME_PATH, *candidate.HISTORICAL_CONTRACT_HASHES):
            bad = copy.deepcopy(positive); bad['source_sha256'][path] = '0' * 64
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'preserved runtime|historical contract'):
                candidate.validate_replay_contract(bad)

    def test_core_and_highwing_boundaries_reject_extra_or_omitted_inputs(self):
        positive = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(positive)
        for paths in (candidate.CORE_PIPELINE_SOURCE_PATHS, candidate.ORIGINAL_HIGHWING_SOURCE_PATHS):
            relative = sorted(paths)[0]
            for changed in (positive - {relative}, positive | {relative.upper()},
                            positive | {str(Path(relative).parent / 'unreviewed.rs')}):
                with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)

class AutoDiscoveredCrateInputTests(unittest.TestCase):
    def fixture(self):
        spec = importlib.util.spec_from_file_location('crate_boundary_fixtures', ROOT / 'scripts/tests/test_swift_windows_candidate.py')
        module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        fixture = module.CandidateAcceptanceTests(); fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        repo, _ = fixture.source_fixture()
        candidate.source_inputs(repo, candidate.git(repo, 'rev-parse', 'HEAD'))
        return repo, fixture.commit_source_fixture

    def test_crate_member_set_is_exactly_the_preserved_492_paths(self):
        preserved = json.loads((ROOT / candidate.PRESERVED_RUNTIME_PATH).read_text())['source_sha256']
        expected = {p for p in preserved if p.startswith('crates/')}
        self.assertEqual(len(expected), 492)
        self.assertEqual(candidate.PRESERVED_CRATE_SOURCE_PATHS, expected)
        self.assertEqual(candidate.CURRENT_CRATE_SOURCE_PATHS,
                         expected | candidate.COMPONENT_TERMS_RUNTIME_PATHS
                         | candidate.TERRAIN_CENTROID_RUNTIME_PATHS | candidate.COCKPIT_RUNTIME_PATHS)
        self.assertEqual(len(candidate.CURRENT_CRATE_SOURCE_PATHS), 497)
        candidate.validate_modified_source_boundaries(set(candidate.REPLAY_CONTRACT_PATHS))

    def test_committed_auto_discovered_targets_aliases_and_omission_are_rejected(self):
        repo, commit = self.fixture()
        for relative in ('crates/flightsim-app/build.rs', 'crates/flightsim-app/src/bin/unreviewed.rs',
                         'crates/flightsim-app/examples/unreviewed.rs', 'crates/flightsim-app/benches/unreviewed.rs',
                         'CRATES/flightsim-app/build.rs'):
            path = repo / relative; path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fn main() {}\n')
            expected = commit(repo)
            with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'modified source boundary changed: crates/'):
                candidate.source_inputs(repo, expected)
            path.unlink(); commit(repo)
        # Remove the now-empty root alias before restoring a clean live fixture.
        alias = repo / 'CRATES'
        if alias.exists() and not alias.samefile(repo / 'crates'):
            (alias / 'flightsim-app').rmdir(); alias.rmdir()
        path = repo / 'crates/flightsim-app/Cargo.toml'; original = path.read_bytes(); path.unlink()
        with self.assertRaisesRegex(ValueError, 'modified source boundary changed: crates/'):
            candidate.source_inputs(repo, commit(repo))
        path.write_bytes(original)
        candidate.source_inputs(repo, commit(repo))

    def test_ignored_and_untracked_crate_inputs_cannot_escape_live_admission(self):
        repo, commit = self.fixture()
        expected = candidate.git(repo, 'rev-parse', 'HEAD')
        paths = ('crates/flightsim-app/build.rs', 'crates/flightsim-app/examples/unreviewed.rs',
                 'crates/flightsim-app/benches/unreviewed.rs', 'CRATES/flightsim-app/build.rs')
        (repo / '.git/info/exclude').write_text('\n'.join('/' + p for p in paths) + '\n')
        for relative in paths:
            path = repo / relative; path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fn main() {}\n')
            with self.subTest(ignored=relative), self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input|modified source root case alias'):
                candidate.validate_modified_source_checkout(repo)
            with self.assertRaises(ValueError): candidate.source_inputs(repo, expected)
            path.unlink()
        alias = repo / 'CRATES'
        if alias.exists() and not alias.samefile(repo / 'crates'):
            (alias / 'flightsim-app').rmdir(); alias.rmdir()
        (repo / '.git/info/exclude').write_text('')
        path = repo / 'crates/flightsim-app/build.rs'; path.write_bytes(b'fn main() {}\n')
        with self.assertRaisesRegex(ValueError, 'source checkout must be clean'):
            candidate.source_inputs(repo, expected)
        with self.assertRaisesRegex(ValueError, 'unreviewed modified source checkout input'):
            candidate.validate_modified_source_checkout(repo)
        path.unlink()
        candidate.source_inputs(repo, expected)


class CanonicalBatchTests(unittest.TestCase):
    def test_real_git_binary_blobs_match_prior_reader_without_cross_commit_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=repo)
            git('init', '-q')
            git('config', 'gc.auto', '0')
            git('config', 'maintenance.auto', 'false')
            payloads = {'binary': b'\0first\r\n\xff\n', 'empty': b'', 'text': b'exact LF\n'}
            for name, data in payloads.items(): (repo / name).write_bytes(data)
            def commit():
                git('-c', 'core.autocrlf=false', 'add', '.')
                git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture')
                ids = {name: git('rev-parse', 'HEAD:' + name).decode().strip() for name in payloads}
                sizes = {oid: len(git('cat-file', 'blob', oid)) for oid in ids.values()}
                batch = candidate.canonical_blobs(repo, sizes)
                for oid in ids.values(): self.assertEqual(batch[oid], git('cat-file', 'blob', oid))
                return ids, batch
            first, first_blobs = commit()
            (repo / 'binary').write_bytes(b'new\0committed\r\nbytes')
            second, second_blobs = commit()
            self.assertNotEqual(first['binary'], second['binary'])
            self.assertNotEqual(first_blobs[first['binary']], second_blobs[second['binary']])
            self.assertEqual(first_blobs[first['empty']], b'')

    def test_complete_frames_reject_bad_identity_type_size_truncation_and_extra_objects(self):
        first, second = 'a' * 40, 'b' * 40
        sizes = {first: 4, second: 0}
        positive = (first + ' blob 4\n').encode() + b'\0\r\nX\n' + (second + ' blob 0\n\n').encode()
        def read(raw):
            def run(*args, **kwargs):
                self.assertEqual(args[0], ['git', 'cat-file', '--batch'])
                self.assertEqual(kwargs['input'], (first + '\n' + second + '\n').encode())
                kwargs['stdout'].write(raw)
                return subprocess.CompletedProcess(args[0], 0)
            with mock.patch.object(candidate.subprocess, 'run', side_effect=run):
                return candidate.canonical_blobs(ROOT, sizes)
        self.assertEqual(read(positive), {first: b'\0\r\nX', second: b''})
        mutations = {
            'mismatched ID': positive.replace(first.encode(), b'c' * 40, 1),
            'wrong type': positive.replace(b' blob ', b' tree ', 1),
            'wrong size': positive.replace(b' blob 4', b' blob 5', 1),
            'malformed size': positive.replace(b' blob 4', b' blob x', 1),
            'header newline': positive.replace(b'4\n', b'4X', 1),
            'payload separator': positive.replace(b'X\n', b'XX', 1),
            'truncated header': positive[:20],
            'truncated payload': positive[:-2],
            'extra object': positive + (first + ' blob 0\n\n').encode(),
            'extra byte': positive + b'X',
            'swapped objects': (second + ' blob 0\n\n').encode() + (first + ' blob 4\n').encode() + b'\0\r\nX\n',
        }
        for name, raw in mutations.items():
            with self.subTest(mutation=name), self.assertRaisesRegex(ValueError, 'canonical batch'):
                read(raw)

    def test_oversized_output_is_rejected_before_any_in_memory_read(self):
        real_temporary_file = candidate.tempfile.TemporaryFile
        reads = []
        class WatchedSpool:
            def __init__(self): self.file = real_temporary_file()
            def __enter__(self): return self
            def __exit__(self, *args): self.file.close()
            def __getattr__(self, name): return getattr(self.file, name)
            def read(self, count):
                reads.append(count)
                return self.file.read(count)
        def oversized(*args, **kwargs):
            # Sparse output simulates a grossly oversized child response without
            # allocating it in the test or loading it into the parent process.
            kwargs['stdout'].seek(candidate.MAX_CANONICAL_BATCH_BYTES + 1)
            kwargs['stdout'].write(b'X')
            return subprocess.CompletedProcess(args[0], 0)
        with mock.patch.object(candidate.tempfile, 'TemporaryFile', side_effect=WatchedSpool), \
                mock.patch.object(candidate.subprocess, 'run', side_effect=oversized):
            with self.assertRaisesRegex(ValueError, 'extra bytes'):
                candidate.canonical_blobs(ROOT, {'a' * 40: 1})
        self.assertEqual(reads, [])

    def test_limits_and_invalid_requests_reject_before_git(self):
        oid = 'a' * 40
        bad = [{}, {oid: -1}, {oid: True}, {oid: '1'}, {'invalid': 1},
               {oid: candidate.MAX_CANONICAL_BLOB_BYTES + 1},
               {format(i, '040x'): 0 for i in range(candidate.MAX_CANONICAL_BATCH_OBJECTS + 1)},
               {format(i, '040x'): candidate.MAX_CANONICAL_BLOB_BYTES for i in range(5)}]
        for sizes in bad:
            with self.subTest(sizes_count=len(sizes)), mock.patch.object(candidate.subprocess, 'run') as run:
                with self.assertRaises(ValueError): candidate.canonical_blobs(ROOT, sizes)
                run.assert_not_called()

    def test_git_failure_and_timeout_are_not_reinterpreted_as_source(self):
        for error in (subprocess.CalledProcessError(128, ['git']), subprocess.TimeoutExpired(['git'], 60)):
            with self.subTest(error=type(error).__name__), mock.patch.object(candidate.subprocess, 'run', side_effect=error):
                with self.assertRaises(type(error)):
                    candidate.canonical_blobs(ROOT, {'a' * 40: 1})

if __name__ == '__main__':
    unittest.main()
