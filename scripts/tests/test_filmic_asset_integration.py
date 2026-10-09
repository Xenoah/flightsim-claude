"""Asset-local evidence integrity and explicitly synthetic collector fixtures.

These checks do not create a dependency review or grant release authorization.
"""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
DECISION = Path('docs/release/filmic-asset-source-decision.json')
MANIFEST = Path('docs/release/asset-rights-manifest.json')
HISTORICAL = Path('docs/release/history/mesh-and-lut-boundary-d918943.json')
PROOF = Path('docs/release/licenses/review-evidence/bevy-blender-filmic-lut')
IDENTITY = 'bevy-blender-filmic-lut'
AGX_IDENTITY = 'bevy-agx-lut'
TONY_IDENTITY = 'bevy-tony-mc-mapface'
LIGHT_PATH = 'assets/aircraft/light_single.glb'
ORIGINAL_LIGHT_HASH = 'b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc'
HISTORICAL_LIGHT_HASH = '8fc91894ea3f54d4226c3a30545de1947fa8a9fb0e013bb4bfec0b5e298effd9'
LUT_HASH = 'a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7'
PRIOR_HASH = '64635e2d9bd322de6a6282daf7a83f7de1e69bfcc11f1c7c92265021557a8087'
PACKET_MANIFEST_HASH = 'f79bcb6bf7fcc86ce5eefb0bb15b66c3237e65b921dd9e2ce8178ee342f71d57'
LICENSE_HASH = 'e39e97c2149a821e3e3b3c07839f8cadf6a98fb0a816b8ed69f16dd8817066fc'
NOTICES = {'docs/release/licenses/Blender-Filmic-OpenColorIO-LICENSE',
           'docs/release/licenses/Blender-Filmic-NOTICE.txt'}


def read(path):
    return json.loads(path.read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


def checked_record(root, item):
    relative = item['path']
    if Path(relative).is_absolute() or any(p in ('', '.', '..') for p in relative.split('/')):
        raise ValueError('unsafe evidence path')
    path = root / relative
    if path.is_symlink() or not path.is_file():
        raise ValueError('missing regular evidence')
    if sha(path) != item['sha256'] or path.stat().st_size != item['bytes']:
        raise ValueError('changed evidence')
    return path


def verify_candidate(root):
    """Test-only integrity contract for this specific reviewed evidence set."""
    decision, manifest = read(root / DECISION), read(root / MANIFEST)
    if decision['kind'] != 'single-embedded-asset-source-rights-decision':
        raise ValueError('scope changed')
    for key in ('whole_target_dependency_review_complete', 'native_platform_review_complete',
                'runtime_appearance_accepted', 'user_variant_choice_recorded', 'release_authorized'):
        if decision[key] is not False:
            raise ValueError('scope or authority expanded')
    assets = {item['id']: item for item in manifest['dependency_assets']}
    if set(assets) != {'bevy-fira-mono', TONY_IDENTITY, IDENTITY} or len(manifest['dependency_assets']) != 3:
        raise ValueError('current embedded-asset boundary changed')
    asset = assets[IDENTITY]
    expected = {'id': IDENTITY, 'package': 'bevy_core_pipeline', 'version': '0.18.1',
                'source_path': 'src/tonemapping/luts/Blender_-11_12.ktx2',
                'feature': 'tonemapping_luts', 'sha256': LUT_HASH, 'bytes': 308905}
    if decision['asset'] != expected or any(asset[k] != v for k, v in expected.items()):
        raise ValueError('asset identity changed')
    if asset['review_state'] != 'licensed_with_notices' or asset['license'] != 'BSD-3-Clause':
        raise ValueError('asset-local decision changed')
    if set(asset['notice_files']) != NOTICES:
        raise ValueError('notice coverage changed')
    notices = decision['determination']['notices']
    if {n['path'] for n in notices} != NOTICES or len(notices) != 2:
        raise ValueError('decision notice coverage changed')
    for item in notices:
        checked_record(root, item)
    if sha(root / 'docs/release/licenses/Blender-Filmic-OpenColorIO-LICENSE') != LICENSE_HASH:
        raise ValueError('complete upstream notice changed')
    proof_manifest = checked_record(root, decision['evidence']['packet_manifest'])
    if sha(proof_manifest) != PACKET_MANIFEST_HASH:
        raise ValueError('original proof packet resealed')
    for item in read(proof_manifest)['files']:
        checked_record(proof_manifest.parent, item)
    for name in ('source_bindings', 'exact_comparison', 'independent_source_review'):
        checked_record(root, decision['evidence'][name])
    prior_path = checked_record(root, decision['source_observation']['prior_native_inventory'])
    if sha(prior_path) != PRIOR_HASH:
        raise ValueError('historical native inventory changed')
    prior = read(prior_path)
    historical = read(root / HISTORICAL)
    historical_assets = manifest['historical_excluded_dependency_assets']
    if historical_assets != [historical['agx_dependency_asset']]:
        raise ValueError('historical AgX decision changed')
    for item in prior['embedded_assets']:
        if item['id'] != IDENTITY:
            original = {k: v for k, v in item.items() if k not in ('observed_sha256', 'notices')}
            record = historical_assets[0] if item['id'] == AGX_IDENTITY else assets[item['id']]
            if record != original:
                raise ValueError('another embedded-asset decision changed')
    legacy = manifest['historical_excluded_assets']
    if (legacy != [historical['light_single_asset']] or legacy[0]['review_state'] != 'unresolved'
            or legacy[0]['sha256'] != HISTORICAL_LIGHT_HASH):
        raise ValueError('historical Light Single decision changed')
    current = [a for a in manifest['assets'] if a['path'] == LIGHT_PATH]
    if (len(current) != 1 or current[0]['review_state'] != 'original_source_recorded'
            or current[0]['sha256'] != ORIGINAL_LIGHT_HASH or current[0]['bytes'] != 140840
            or current[0]['license'] != 'MIT OR Apache-2.0'):
        raise ValueError('current original Light Single decision changed')
    checked_record(root, current[0])
    for identity in (TONY_IDENTITY, IDENTITY):
        item = assets[identity]
        checked_record(root, {**item, 'path': 'vendor/bevy_core_pipeline/' + item['source_path']})
    if (root / 'vendor/bevy_core_pipeline' / historical_assets[0]['source_path']).exists():
        raise ValueError('historical AgX payload restored')
    if (root / 'docs/release/dependency-review.json').exists():
        raise ValueError('unexpected whole-target review')


def load(name):
    spec = importlib.util.spec_from_file_location(name, REPO / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


collector = load('collect-dependency-notices')
gate = load('check-commercial-readiness')
stager = load('stage-commercial-candidate')


class ActualCandidateEvidence(unittest.TestCase):
    def test_actual_scoped_records_and_complete_proof(self):
        verify_candidate(REPO)

    def test_mutated_notice_proof_identity_or_scope_rejected(self):
        for mutation in ('license', 'comparison', 'inventory', 'authority', 'identity', 'notice-list',
                         'historical-agx', 'historical-meshy', 'current-light', 'current-tony', 'active-agx'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                shutil.copytree(REPO / 'docs/release', root / 'docs/release')
                current_manifest = read(REPO / MANIFEST)
                current_files = [LIGHT_PATH] + [
                    'vendor/bevy_core_pipeline/' + a['source_path']
                    for a in current_manifest['dependency_assets'] if a['id'] in (TONY_IDENTITY, IDENTITY)]
                for relative in current_files:
                    target = root / relative
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(REPO / relative, target)
                if mutation == 'license':
                    (root / next(n for n in NOTICES if n.endswith('LICENSE'))).write_text('Truncated notice')
                elif mutation == 'comparison':
                    (root / PROOF / 'final-comparison.json').write_text('{}')
                elif mutation == 'inventory':
                    (root / PROOF / 'prior-native-inventory.json').write_text('{}')
                elif mutation == 'authority':
                    d = read(root / DECISION); d['release_authorized'] = True; write(root / DECISION, d)
                else:
                    m = read(root / MANIFEST)
                    a = next(a for a in m['dependency_assets'] if a['id'] == IDENTITY)
                    if mutation == 'identity': a['sha256'] = '0' * 64
                    if mutation == 'notice-list': a['notice_files'].pop()
                    if mutation == 'historical-agx': m['historical_excluded_dependency_assets'][0]['review_state'] = 'licensed_with_notices'
                    if mutation == 'historical-meshy': m['historical_excluded_assets'][0]['review_state'] = 'original_source_recorded'
                    if mutation == 'current-light': next(a for a in m['assets'] if a['path'] == LIGHT_PATH)['sha256'] = HISTORICAL_LIGHT_HASH
                    if mutation == 'current-tony': next(a for a in m['dependency_assets'] if a['id'] == TONY_IDENTITY)['sha256'] = '0' * 64
                    if mutation == 'active-agx': m['dependency_assets'].append(m['historical_excluded_dependency_assets'][0])
                    write(root / MANIFEST, m)
                with self.assertRaises(ValueError):
                    verify_candidate(root)

    def test_historical_native_inventory_stays_stale_and_blocked(self):
        report = gate.check(REPO, None, REPO / PROOF / 'prior-native-inventory.json')
        codes = {item['code'] for item in report['blockers']}
        self.assertEqual(report['status'], 'blocked')
        self.assertTrue({'BUNDLE_NOT_CHECKED', 'DEPENDENCY_ASSET_MANIFEST_CHANGED',
                         'DEPENDENCY_REVIEW_REQUIRED', 'DEPENDENCY_UNRESOLVED',
                         'DEPENDENCY_NOTICE_CHANGED'} <= codes)
        unresolved = [b['message'] for b in report['blockers'] if b['code'] == 'DEPENDENCY_UNRESOLVED']
        for identity in ('constgebra@0.1.4', 'hexf-parse@0.2.1', 'bevy-agx-lut', IDENTITY):
            self.assertTrue(any(identity in message for message in unresolved))


class SyntheticHistoricalAgxCollectorIntegration(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name); self.repo = self.root / 'fixture-repo'; self.repo.mkdir()
        (self.repo / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "flightsim-app"\nversion = "0.0.0"\n')
        for name in ('LICENSE-MIT', 'LICENSE-APACHE'):
            (self.repo / name).write_text('Synthetic package notice; no real review\n')
        actual = read(REPO / MANIFEST)
        # Recreate the historical Filmic-cleared/AgX-unresolved boundary explicitly;
        # the current source recipe no longer embeds the historical AgX payload.
        assets = [copy.deepcopy(next(a for a in actual['dependency_assets'] if a['id'] == IDENTITY)),
                  copy.deepcopy(read(REPO / HISTORICAL)['agx_dependency_asset'])]
        source = self.repo / 'synthetic-engine'; source.mkdir()
        (source / 'Cargo.toml').write_text('Synthetic metadata fixture')
        (source / 'LICENSE').write_text('Synthetic engine notice')
        for a in assets:
            path = source / a['source_path']; path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(('Synthetic fixture: ' + a['id']).encode())
            a['sha256'] = sha(path); a['bytes'] = path.stat().st_size
            for name in a['notice_files']:
                target = self.repo / name; target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(REPO / name, target)
        write(self.repo / MANIFEST, {'schema_version': 1, 'assets': [], 'dependency_assets': assets,
                                    'required_bundle_files': [], 'commercial_external_assets': []})
        packages = [{'id': 'fixture-app', 'name': 'flightsim-app', 'version': '0.0.0',
                     'manifest_path': str(self.repo / 'Cargo.toml'), 'license': 'MIT', 'source': None},
                    {'id': 'fixture-engine', 'name': 'bevy_core_pipeline', 'version': '0.18.1',
                     'manifest_path': str(source / 'Cargo.toml'), 'license': 'MIT', 'source': 'registry+synthetic'}]
        self.nodes = [{'id': 'fixture-app', 'features': ['commercial-staging', 'default'],
                       'deps': [{'pkg': 'fixture-engine', 'dep_kinds': [{'kind': None}]}]},
                      {'id': 'fixture-engine', 'features': ['tonemapping_luts'], 'deps': []}]
        self.metadata = {'workspace_root': str(self.repo), 'workspace_members': ['fixture-app'],
                         'packages': packages, 'resolve': {'nodes': self.nodes}}
        self.source = source; self.output = self.root / 'notices'

    def collect(self):
        path = self.root / 'metadata.json'; write(path, self.metadata)
        return collector.collect(path, self.repo, self.output, 'x86_64-pc-windows-msvc', 'flightsim-app')

    def test_conditional_notices_copy_and_whole_review_stays_required(self):
        result = self.collect()
        self.assertEqual(result['review_status'], 'not_reviewed')
        self.assertEqual({x['id'] for x in result['unresolved']}, {'bevy-agx-lut'})
        asset = next(a for a in result['embedded_assets'] if a['id'] == IDENTITY)
        self.assertEqual(len(asset['notices']), 2)
        for item in asset['notices']:
            self.assertEqual(sha(self.output / item['path']), item['sha256'])
        self.assertIn('licenses/supplemental/Blender-Filmic-OpenColorIO-LICENSE', stager.notice_files(self.output, result))
        report = gate.check(self.repo, None, self.output / 'dependency-inventory.json')
        self.assertTrue(any(b['code'] == 'DEPENDENCY_REVIEW_REQUIRED' for b in report['blockers']))
        self.assertTrue(any(b['code'] == 'DEPENDENCY_UNRESOLVED' and 'bevy-agx-lut' in b['message'] for b in report['blockers']))

    def test_feature_disabled_does_not_copy_filmic_notices(self):
        self.nodes[1]['features'] = []
        result = self.collect()
        self.assertEqual(result['embedded_assets'], [])
        self.assertFalse((self.output / 'licenses/supplemental').exists())
        self.assertEqual(result['review_status'], 'not_reviewed')

    def test_changed_asset_remains_unresolved(self):
        (self.source / 'src/tonemapping/luts/Blender_-11_12.ktx2').write_bytes(b'Changed synthetic LUT')
        self.assertIn(IDENTITY, {x['id'] for x in self.collect()['unresolved']})

    def test_missing_notice_rejected(self):
        (self.repo / 'docs/release/licenses/Blender-Filmic-NOTICE.txt').unlink()
        with self.assertRaises(ValueError):
            self.collect()

    def test_collected_notice_mutation_rejected_by_stager(self):
        result = self.collect()
        (self.output / 'licenses/supplemental/Blender-Filmic-NOTICE.txt').write_text('Changed fixture notice')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            stager.notice_files(self.output, result)


if __name__ == '__main__':
    unittest.main()
