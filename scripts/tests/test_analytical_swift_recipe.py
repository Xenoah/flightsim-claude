"""Adversarial synthetic build-evidence fixtures; never execute Cargo or binaries."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('analytical_swift', Path(__file__).parents[1] / 'check-analytical-swift-recipe.py')
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fixture(root):
    repo, capture = root / 'repo', root / 'capture'
    repo.mkdir(); capture.mkdir()
    (repo / 'LICENSE-MIT').write_text('fixture license')
    (repo / 'LICENSE-APACHE').write_text('fixture license')
    source = {'source_sha': 'a' * 40, 'source_tree': 'b' * 40, 'analytical_contract_sha256': 'c' * 64,
              'replay_contract_sha256': 'd' * 64, 'source_recipe': check.tone.UPSTREAM_THREE_LUT_SOURCE}
    versions = {name: '0.18.1' for name in check.tone.KEY_CRATES}
    versions.update({'ktx2': '0.4.0', 'ruzstd': '0.8.2', 'flightsim-app': '0.6.0-alpha.21',
                     'flightsim-render': '0.6.0-alpha.21', 'bevy_text': '0.18.1',
                     'constgebra': '0.1.4', 'hexf-parse': '0.2.1', 'bevy_gizmos': '0.18.1', 'metadata_only': '1.0.0'})
    packages = []
    for name, version in versions.items():
        workspace = name.startswith('flightsim-')
        directory = repo / ('crates' if workspace else 'registry') / name
        directory.mkdir(parents=True)
        (directory / 'Cargo.toml').write_text(f'[package]\nname="{name}"\nversion="{version}"\nlicense="MIT"\n')
        target = {'name': 'flightsim-app' if name == 'flightsim-app' else name.replace('-', '_'),
                  'kind': ['bin' if name == 'flightsim-app' else 'lib'],
                  'src_path': str(directory / ('src/main.rs' if name == 'flightsim-app' else 'src/lib.rs'))}
        Path(target['src_path']).parent.mkdir(); Path(target['src_path']).write_text('// fixture\n')
        if name not in ('constgebra', 'hexf-parse'):
            (directory / 'LICENSE').write_text('fixture license')
        packages.append({'id': ('path+file://' + str(directory) if workspace else 'registry+https://github.com/rust-lang/crates.io-index') + '#' + name + '@' + version,
                         'name': name, 'version': version, 'source': None if workspace else 'registry+https://github.com/rust-lang/crates.io-index',
                         'manifest_path': str(directory / 'Cargo.toml'), 'license': 'MIT', 'targets': [target]})
    lock = '\n'.join('[[package]]\nname="' + p['name'] + '"\nversion="' + p['version'] + '"\n' +
                     ('source="' + p['source'] + '"\nchecksum="' + 'e' * 64 + '"\n' if p['source'] else '') for p in packages)
    (repo / 'Cargo.lock').write_text(lock)
    lut_hashes, assets = {}, []
    for name, identity in [('AgX-default_contrast.ktx2', 'bevy-agx-lut'), ('Blender_-11_12.ktx2', 'bevy-blender-filmic-lut'),
                           ('tony_mc_mapface.ktx2', 'bevy-tony-mc-mapface')]:
        payload = ('fixture:' + name).encode(); lut_hashes[name] = sha(payload)
        path = repo / 'registry/bevy_core_pipeline/src/tonemapping/luts' / name
        path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(payload)
        assets.append({'id': identity, 'package': 'bevy_core_pipeline', 'version': '0.18.1',
                       'source_path': 'src/tonemapping/luts/' + name, 'feature': 'tonemapping_luts',
                       'sha256': sha(payload), 'bytes': len(payload), 'review_state': 'unresolved', 'reason': 'fixture review required'})
    font = repo / 'registry/bevy_text/src/font.ttf'; font.write_bytes(b'fixture-font')
    assets.append({'id': 'bevy-fira-mono', 'package': 'bevy_text', 'version': '0.18.1', 'source_path': 'src/font.ttf',
                   'feature': 'default_font', 'sha256': sha(b'fixture-font'), 'bytes': 12, 'review_state': 'licensed_with_notices'})
    write(repo / 'docs/release/asset-rights-manifest.json', {'schema_version': 1, 'assets': [], 'dependency_assets': assets})
    write(repo / 'docs/release/dependency-notice-supplements.json', {'entries': []})
    receipt = {key: check.recipe()[key] for key in ('schema_version', 'recipe', 'target', 'toolchain', 'features', 'default_features', 'region_downloads', 'release_authorized')}
    receipt.update(source_recipe=check.tone.UPSTREAM_THREE_LUT_SOURCE, source_sha=source['source_sha'], source_tree=source['source_tree'], modes={})
    for mode in ('analytic', 'ordinary'):
        directory = capture / mode; directory.mkdir()
        target = root / ('target-' + mode); profile = target / check.TARGET / 'release'; profile.mkdir(parents=True)
        features = {name: set() for name in versions}
        features['bevy_image'] = {'ktx2', 'zstd', 'zstd_rust'}
        features['bevy_text'] = {'default_font'}
        features['flightsim-app'] = set(check.FEATURES) if mode == 'analytic' else {'default'}
        features['flightsim-render'] = {'analytic-tonemapping'} if mode == 'analytic' else set()
        if mode == 'analytic': features['bevy'] = {'ktx2'}
        else:
            for name in ('bevy', 'bevy_internal', 'bevy_core_pipeline'): features[name] = {'tonemapping_luts'}
        app = next(p for p in packages if p['name'] == 'flightsim-app')
        ordered = [app, *[p for p in packages if p != app and p['name'] not in ('bevy_gizmos', 'metadata_only')]]
        graph = '\n'.join(f'{p["name"]} v{p["version"]} features=[{",".join(sorted(features[p["name"]]))}]' for p in ordered) + '\n'
        (directory / 'graph.txt').write_text(graph)
        executable = profile / 'flightsim-app.exe'
        executable.write_bytes(b'fixture-executable' + (b''.join(('fixture:' + name).encode() for name in lut_hashes) if mode == 'ordinary' else b'analytic'))
        artifacts, frozen = [], {}
        for package in ordered:
            name = package['name']; is_app = name == 'flightsim-app'
            artifact = {'reason': 'compiler-artifact', 'package_id': package['id'], 'target': package['targets'][0],
                        'features': sorted(features[name]), 'profile': {'opt_level': '3', 'debuginfo': 0, 'debug_assertions': False, 'overflow_checks': False, 'test': False},
                        'executable': str(executable) if is_app else None}
            if is_app:
                artifact['filenames'] = [str(executable)]
                frozen[str(executable)] = check.file_record(executable)
                fingerprint = profile / '.fingerprint/flightsim-app-1234/bin-flightsim-app.json'
                write(fingerprint, {'features': json.dumps(sorted(features[name])), 'rustflags': ['-D', 'warnings']})
                frozen[str(fingerprint)] = check.file_record(fingerprint)
            else:
                library = profile / 'deps' / ('lib' + name.replace('-', '_') + '-1234.rlib')
                library.parent.mkdir(exist_ok=True); library.write_bytes(('fixture-lib:' + mode + name).encode())
                artifact['filenames'] = [str(library)]
                if name in check.tone.KEY_CRATES or name == 'flightsim-render':
                    fingerprint = profile / '.fingerprint' / (name + '-1234') / ('lib-' + name.replace('-', '_') + '.json')
                    write(fingerprint, {'features': json.dumps(sorted(features[name])), 'rustflags': ['-D', 'warnings']})
                    frozen[str(fingerprint)] = check.file_record(fingerprint)
                if name in check.tone.KEY_CRATES or name == 'flightsim-render': frozen[str(library)] = check.file_record(library)
                if name == 'bevy_core_pipeline':
                    depinfo = library.with_name('bevy_core_pipeline-1234.d')
                    depinfo.write_text('/src/tonemapping/luts/one.ktx2' if mode == 'ordinary' else '/src/tonemapping/mod.rs')
                    frozen[str(depinfo)] = check.file_record(depinfo)
            artifacts.append(artifact)
        artifacts.append({'reason': 'build-finished', 'success': True})
        (directory / 'messages.jsonl').write_text('\n'.join(json.dumps(m) for m in artifacts) + '\n')
        nodes = [{'id': p['id'], 'features': sorted(features[p['name']] | ({'metadata-extra'} if p['name'] in ('bevy_gizmos', 'bevy_text') else set())),
                  'deps': [{'pkg': q['id'], 'dep_kinds': [{'kind': None}]} for q in packages if q != app] if p == app else []} for p in packages]
        metadata = {'workspace_root': str(repo), 'target_directory': str(target), 'workspace_members': [p['id'] for p in packages if p['source'] is None],
                    'packages': packages, 'resolve': {'nodes': nodes}}
        write(directory / 'metadata.json', metadata)
        check.collector.collect(directory / 'metadata.json', repo, directory / 'notices', check.TARGET, 'flightsim-app')
        (directory / 'rustc.txt').write_text('rustc 1.93.0 (fixture)\nhost: ' + check.TARGET + '\n')
        results = {}
        for key, filename in {'graph': 'graph.txt', 'build': 'messages.jsonl', 'metadata': 'metadata.json', 'rustc': 'rustc.txt'}.items():
            (directory / (key + '.stderr')).write_text('')
            results[key] = {'exit_code': 0, 'stdout': check.file_record(directory / filename), 'stderr': check.file_record(directory / (key + '.stderr'))}
        receipt['modes'][mode] = {'cwd': str(repo), 'source_sha': source['source_sha'], 'source_tree': source['source_tree'], 'target_dir': str(target),
                                  'environment': {'RUSTFLAGS': '-D warnings', 'CARGO_TARGET_DIR': str(target), 'CARGO_INCREMENTAL': '0'},
                                  'commands': check.commands(mode), 'results': results, 'frozen_artifacts': frozen,
                                  'inventory': check.file_record(directory / 'notices/dependency-inventory.json')}
    write(capture / 'capture.json', receipt)
    return repo, capture, receipt, source, lut_hashes


class AnalyticalSwiftRecipeTests(unittest.TestCase):
    def audit_fixture(self, root, mutation=None, *, late_notice=False):
        repo, capture, receipt, source, luts = fixture(root)
        authoritative = {mode: {name: (capture / mode / filename).read_bytes() for name, filename in
                         {'rustc': 'rustc.txt', 'graph': 'graph.txt', 'metadata': 'metadata.json'}.items()}
                         for mode in ('analytic', 'ordinary')}
        if mutation: mutation(repo, capture, receipt)
        write(capture / 'capture.json', receipt)
        original_validator = check.validate_capture
        def validate(*args):
            result = original_validator(*args)
            if late_notice and args[3] == 'ordinary':
                inventory_path = capture / 'analytic/notices/dependency-inventory.json'
                inventory = json.loads(inventory_path.read_text())
                notice = next(n for p in inventory['packages'] for n in p['notices'])
                (inventory_path.parent / notice['path']).write_text('changed after first-mode validation')
            return result
        with mock.patch.object(check, 'validate_capture', side_effect=validate), \
             mock.patch.object(check, 'source_evidence', return_value=source), mock.patch.object(check.tone, 'LUTS', luts), \
             mock.patch.object(check, 'recapture', side_effect=lambda repo, target, mode: authoritative[mode]):
            return check.audit(repo, source['source_sha'], capture)

    def test_inventory_asset_applicability_is_versioned_by_source_recipe(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, captured, receipt, source, luts = fixture(Path(temporary))
            directory = captured / 'ordinary'
            graph = check.tone.parse_graph((directory / 'graph.txt').read_text())
            historical_inventory = directory / 'notices/dependency-inventory.json'
            check.validate_inventory(directory / 'metadata.json', historical_inventory, graph, repo, 'ordinary',
                                     source_recipe=check.tone.UPSTREAM_THREE_LUT_SOURCE)
            with self.assertRaisesRegex(ValueError, 'embedded asset applicability'):
                check.validate_inventory(directory / 'metadata.json', historical_inventory, graph, repo, 'ordinary',
                                         source_recipe=check.SOURCE_RECIPE)
            manifest_path = repo / 'docs/release/asset-rights-manifest.json'
            manifest = json.loads(manifest_path.read_text())
            manifest['dependency_assets'] = [a for a in manifest['dependency_assets'] if a['id'] != 'bevy-agx-lut']
            write(manifest_path, manifest)
            current_notices = Path(temporary) / 'current-notices'
            check.collector.collect(directory / 'metadata.json', repo, current_notices, check.TARGET, 'flightsim-app')
            current_inventory = current_notices / 'dependency-inventory.json'
            check.validate_inventory(directory / 'metadata.json', current_inventory, graph, repo, 'ordinary',
                                     source_recipe=check.SOURCE_RECIPE)
            with self.assertRaisesRegex(ValueError, 'embedded asset applicability'):
                check.validate_inventory(directory / 'metadata.json', current_inventory, graph, repo, 'ordinary',
                                         source_recipe=check.tone.UPSTREAM_THREE_LUT_SOURCE)

    def test_historical_fixture_requires_its_explicit_source_recipe(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = self.audit_fixture(Path(temporary))
            self.assertEqual(result['source_recipe'], check.tone.UPSTREAM_THREE_LUT_SOURCE)
        for changed in (None, check.tone.TWO_LUT_SOURCE, 'unknown-source'):
            def mutate(repo, capture, receipt):
                if changed is None: receipt.pop('source_recipe')
                else: receipt['source_recipe'] = changed
            with self.subTest(source_recipe=changed), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaisesRegex(ValueError, 'source recipe'):
                    self.audit_fixture(Path(temporary), mutate)

    def test_late_notice_mutation_cannot_retain_inventory_integrity(self):
        with tempfile.TemporaryDirectory() as temporary, self.assertRaisesRegex(ValueError, 'integrity blocker'):
            self.audit_fixture(Path(temporary), late_notice=True)

    def test_positive_control_and_analytical_audit_retain_all_unexecuted_gates(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = self.audit_fixture(Path(temporary))
            self.assertEqual(result['status'], 'build_evidence_checked_native_and_distribution_unqualified')
            self.assertIs(result['release_authorized'], False)
            self.assertEqual(result['required_unexecuted_gates'], check.GATES)
            self.assertEqual(len(result['builds']['analytic']['artifact']['compiled']), 6)
            for mode in ('analytic', 'ordinary'):
                self.assertTrue(all((p['full_bytes_offset'] >= 0) == (mode == 'ordinary') for p in result['builds'][mode]['artifact']['payloads']))
                self.assertEqual(result['builds'][mode]['readiness']['status'], 'blocked')

    def test_receipt_identity_commands_targets_and_control_cannot_be_substituted(self):
        mutations = [
            lambda r, c, v: v.update(recipe=check.candidate.IDENTITY),
            lambda r, c, v: v.update(default_features=0),
            lambda r, c, v: v.update(release_authorized=0),
            lambda r, c, v: v.update(region_downloads=0),
            lambda r, c, v: v.update(features=['analytic-tonemapping']),
            lambda r, c, v: v.update(source_sha='e' * 40),
            lambda r, c, v: v.update(source_tree='e' * 40),
            lambda r, c, v: v['modes']['analytic']['commands']['build'].append('--workspace'),
            lambda r, c, v: v['modes']['analytic']['environment'].update(RUSTFLAGS=''),
            lambda r, c, v: v['modes']['analytic']['results']['build'].update(exit_code=101),
            lambda r, c, v: v['modes']['analytic']['results']['build'].update(exit_code=False),
            lambda r, c, v: v['modes']['analytic'].update(source_sha='e' * 40),
            lambda r, c, v: v['modes']['ordinary'].update(target_dir=v['modes']['analytic']['target_dir']),
            lambda r, c, v: v['modes']['analytic'].update(frozen_artifacts={}),
            lambda r, c, v: v['modes']['analytic'].update(inventory={'sha256': 'e' * 64, 'bytes': 1}),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutations.index(mutation)), tempfile.TemporaryDirectory() as temporary, self.assertRaises(ValueError):
                self.audit_fixture(Path(temporary), mutation)

    def mutate_output(self, mode, filename, transform, key):
        def mutate(repo, capture, receipt):
            path = capture / mode / filename
            transform(path)
            receipt['modes'][mode]['results'][key]['stdout'] = check.file_record(path)
        return mutate

    def test_rehashed_malicious_build_messages_still_fail(self):
        changes = [
            lambda m: m[-1].update(success=False),
            lambda m: m[0]['profile'].update(test=True),
            lambda m: m[0]['profile'].update(opt_level='0'),
            lambda m: m[0]['profile'].update(debug_assertions=True),
            lambda m: m[0]['profile'].update(debuginfo=2),
            lambda m: m[0]['target'].update(src_path='/foreign/main.rs'),
            lambda m: m[0].update(executable='/foreign/flightsim-app.exe'),
            lambda m: m[0].update(features=['analytic-tonemapping']),
            lambda m: m[1].update(package_id='registry+foreign#bevy_internal@0.18.1'),
            lambda m: m.append(copy.deepcopy(m[0])),
            lambda m: m.pop(1),
            lambda m: m.insert(0, {'reason': 'build-finished', 'success': False}),
            lambda m: m.insert(0, {'reason': 'compiler-message', 'message': {'level': 'error', 'message': 'failed'}}),
        ]
        for change in changes:
            def transform(path):
                messages = [json.loads(line) for line in path.read_text().splitlines()]
                change(messages); path.write_text('\n'.join(json.dumps(m) for m in messages) + '\n')
            with self.subTest(change=changes.index(change)), tempfile.TemporaryDirectory() as temporary, self.assertRaises((ValueError, KeyError)):
                self.audit_fixture(Path(temporary), self.mutate_output('analytic', 'messages.jsonl', transform, 'build'))

    def test_conservative_features_cannot_authorize_additional_compiled_features(self):
        def transform(path):
            messages = [json.loads(line) for line in path.read_text().splitlines()]
            artifact = next(m for m in messages if m.get('target', {}).get('name') == 'bevy_text')
            artifact['features'].append('metadata-extra')
            path.write_text('\n'.join(json.dumps(m) for m in messages) + '\n')
        with tempfile.TemporaryDirectory() as temporary, self.assertRaisesRegex(ValueError, 'outside exact app graph'):
            self.audit_fixture(Path(temporary), self.mutate_output('analytic', 'messages.jsonl', transform, 'build'))

    def test_every_selected_library_rejects_profile_tampering(self):
        for name in (*check.tone.KEY_CRATES, 'flightsim_render'):
            for field, value in [('opt_level', '0'), ('debuginfo', 2), ('debug_assertions', True), ('overflow_checks', True), ('test', True)]:
                def transform(path):
                    messages = [json.loads(line) for line in path.read_text().splitlines()]
                    artifact = next(m for m in messages if m.get('target', {}).get('name') == name)
                    artifact['profile'][field] = value
                    path.write_text('\n'.join(json.dumps(m) for m in messages) + '\n')
                with self.subTest(name=name, field=field), tempfile.TemporaryDirectory() as temporary, self.assertRaisesRegex(ValueError, 'release profile'):
                    self.audit_fixture(Path(temporary), self.mutate_output('analytic', 'messages.jsonl', transform, 'build'))

    def test_rehashed_graph_lut_decoder_roots_and_mode_changes_fail(self):
        changes = [
            lambda s: s.replace('analytic-tonemapping,commercial-staging', 'analytic-tonemapping,commercial-staging,default'),
            lambda s: s.replace('analytic-tonemapping,commercial-staging', 'analytic-tonemapping,commercial-staging,region-downloads'),
            lambda s: s.replace('bevy_core_pipeline v0.18.1 features=[]', 'bevy_core_pipeline v0.18.1 features=[tonemapping_luts]'),
            lambda s: s.replace('ktx2,zstd,zstd_rust', 'ktx2,zstd'),
            lambda s: 'flightsim-render v0.6.0-alpha.21 features=[analytic-tonemapping]\n' + s,
            lambda s: s + 'flightsim-tilegen v0.6.0-alpha.21 features=[]\n',
        ]
        for change in changes:
            with self.subTest(change=changes.index(change)), tempfile.TemporaryDirectory() as temporary, self.assertRaises(ValueError):
                self.audit_fixture(Path(temporary), self.mutate_output('analytic', 'graph.txt', lambda p: p.write_text(change(p.read_text())), 'graph'))

    def test_payload_library_fingerprint_depinfo_and_source_substitution_fail(self):
        for mode in ('analytic', 'ordinary'):
            for corruption in ('payload', 'library', 'fingerprint', 'app-fingerprint', 'render-fingerprint', 'depinfo', 'source-lut'):
                def mutate(repo, capture, receipt):
                    profile = Path(receipt['modes'][mode]['target_dir']) / check.TARGET / 'release'
                    paths = {'payload': profile / 'flightsim-app.exe', 'library': profile / 'deps/libbevy-1234.rlib',
                             'fingerprint': profile / '.fingerprint/bevy-1234/lib-bevy.json',
                             'app-fingerprint': profile / '.fingerprint/flightsim-app-1234/bin-flightsim-app.json',
                             'render-fingerprint': profile / '.fingerprint/flightsim-render-1234/lib-flightsim_render.json',
                             'depinfo': profile / 'deps/bevy_core_pipeline-1234.d',
                             'source-lut': repo / 'registry/bevy_core_pipeline/src/tonemapping/luts/AgX-default_contrast.ktx2'}
                    paths[corruption].write_bytes(b'corrupted')
                with self.subTest(mode=mode, corruption=corruption), tempfile.TemporaryDirectory() as temporary, self.assertRaises(ValueError):
                    self.audit_fixture(Path(temporary), mutate)

    def test_inventory_tampering_missing_notice_and_stale_inputs_fail(self):
        for corruption in ('omit-unresolved', 'omit-package', 'omit-font', 'pretend-reviewed', 'lock', 'manifest', 'notice'):
            def mutate(repo, capture, receipt):
                path = capture / 'analytic/notices/dependency-inventory.json'
                inventory = json.loads(path.read_text())
                if corruption == 'omit-unresolved': inventory['unresolved'] = []
                elif corruption == 'omit-package': inventory['packages'] = [p for p in inventory['packages'] if p['name'] != 'constgebra']
                elif corruption == 'omit-font': inventory['embedded_assets'] = []
                elif corruption == 'pretend-reviewed': inventory['review_status'] = 'reviewed'
                elif corruption == 'lock': (repo / 'Cargo.lock').write_text((repo / 'Cargo.lock').read_text() + '\n# stale\n')
                elif corruption == 'manifest': (repo / 'docs/release/asset-rights-manifest.json').write_text('{}')
                else:
                    notice = next(n for p in inventory['packages'] for n in p['notices'])
                    (path.parent / notice['path']).write_text('changed')
                write(path, inventory)
                receipt['modes']['analytic']['inventory'] = check.file_record(path)
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary, self.assertRaises((ValueError, KeyError)):
                self.audit_fixture(Path(temporary), mutate)

    def test_forged_metadata_recollection_cannot_omit_conservative_evidence(self):
        for corruption in ('remove-conservative-package', 'strip-conservative-feature', 'foreign-source'):
            def mutate(repo, capture, receipt):
                path = capture / 'analytic/metadata.json'; metadata = json.loads(path.read_text())
                if corruption == 'remove-conservative-package':
                    removed = next(p['id'] for p in metadata['packages'] if p['name'] == 'bevy_gizmos')
                    metadata['packages'] = [p for p in metadata['packages'] if p['id'] != removed]
                    metadata['resolve']['nodes'] = [n for n in metadata['resolve']['nodes'] if n['id'] != removed]
                    for node in metadata['resolve']['nodes']:
                        node['deps'] = [d for d in node['deps'] if d['pkg'] != removed]
                elif corruption == 'strip-conservative-feature':
                    # Valid-looking subset changed independently of source/lock.
                    node = next(n for n in metadata['resolve']['nodes'] if 'bevy_gizmos@' in n['id'])
                    node['features'] = []
                else:
                    metadata['packages'][0]['source'] = 'registry+https://unreviewed.invalid'
                write(path, metadata)
                # Recollect and rehash all forged evidence, the former bypass.
                import shutil
                shutil.rmtree(capture / 'analytic/notices')
                check.collector.collect(path, repo, capture / 'analytic/notices', check.TARGET, 'flightsim-app')
                receipt['modes']['analytic']['results']['metadata']['stdout'] = check.file_record(path)
                receipt['modes']['analytic']['inventory'] = check.file_record(capture / 'analytic/notices/dependency-inventory.json')
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary, self.assertRaisesRegex(ValueError, 'authoritative'):
                self.audit_fixture(Path(temporary), mutate)

    def test_metadata_supersets_retained_and_lut_reactivation_is_blocked(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, capture, receipt, source, luts = fixture(Path(temporary))
            path = capture / 'analytic/metadata.json'; metadata = json.loads(path.read_text())
            graph = check.tone.parse_graph((capture / 'analytic/graph.txt').read_text())
            metadata['resolve']['nodes'][0]['features'].append('metadata-only-fixture')
            report = check.reconcile(metadata, graph, repo, 'analytic')
            self.assertEqual(len(report['conservative_feature_differences']), 2)
            metadata['resolve']['nodes'][0]['features'].append('tonemapping_luts')
            with self.assertRaisesRegex(ValueError, 'collection design'):
                check.reconcile(metadata, graph, repo, 'analytic')

    def test_runtime_facts_helper_and_witness_reject_committed_drift(self):
        source = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary)
            subprocess.run(['git', 'init', '-q', str(repo)], check=True)
            for name, value in (('core.autocrlf', 'false'), ('gc.auto', '0'),
                                ('maintenance.auto', 'false')):
                subprocess.run(['git', 'config', name, value], cwd=repo, check=True)
            paths = (check.candidate.REPLAY_CONTRACT_PATHS
                     | set(check.candidate.INDEPENDENT_REPLAY_HASHES)
                     | check.SOURCE_PATHS | {check.CONTRACT,
                        '.gitattributes', 'assets/aircraft/.gitattributes',
                        'docs/release/.gitattributes'})
            for relative in paths:
                target = repo / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((source / relative).read_bytes())

            def commit():
                subprocess.run(['git', 'add', '.'], cwd=repo, check=True)
                subprocess.run(['git', '-c', 'user.name=Fixture', '-c',
                                'user.email=fixture@example.invalid',
                                'commit', '-qm', 'analytical source fixture'],
                               cwd=repo, check=True)
                return check.candidate.git(repo, 'rev-parse', 'HEAD')

            check.source_evidence(repo, commit())
            mutations = (
                ('scripts/collect-analytical-runtime-facts.py',
                 b"require(sys.platform == 'win32', 'native Windows is required')",
                 b"require(True, 'native Windows is required')"),
                ('scripts/tests/test_analytical_runtime_facts.py',
                 b'def test_new_association_retains_unique_fingerprint_requirement(self):',
                 b'def disabled_new_association_retains_unique_fingerprint_requirement(self):'),
            )
            for relative, old, new in mutations:
                self.assertIn(relative, check.SOURCE_PATHS)
                path = repo / relative
                original = path.read_bytes()
                self.assertEqual(original.count(old), 1, relative)
                path.write_bytes(original.replace(old, new))
                with self.subTest(relative=relative), self.assertRaisesRegex(
                        ValueError, 'analytical source pin changed') as failure:
                    check.source_evidence(repo, commit())
                self.assertIn(relative, str(failure.exception))
                path.write_bytes(original)
                check.source_evidence(repo, commit())

    def test_old_minimal_audit_and_ordinary_constants_are_unchanged(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, capture, receipt, source, luts = fixture(Path(temporary))
            graph = check.tone.parse_graph((capture / 'analytic/graph.txt').read_text())
            with self.assertRaises(ValueError): check.tone.validate_graph(graph, 'analytic', 'app')
            check.tone.validate_graph(graph, 'analytic', 'app', recipe=check.IDENTITY)
            for mode, kind, recipe in [('ordinary', 'app', check.IDENTITY), ('analytic', 'sun-clock', check.IDENTITY), ('analytic', 'app', 'unknown')]:
                with self.assertRaises(ValueError): check.tone.validate_graph(graph, mode, kind, recipe=recipe)
        self.assertEqual(check.candidate.FEATURES, ['commercial-staging'])
        self.assertEqual(len(check.candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(check.SOURCE_PATHS), 50)
        self.assertEqual(check.recipe()['source_recipe'], check.tone.TWO_LUT_SOURCE)
        self.assertEqual(len(check.candidate.INDEPENDENT_REPLAY_HASHES), 102)


if __name__ == '__main__':
    unittest.main()
