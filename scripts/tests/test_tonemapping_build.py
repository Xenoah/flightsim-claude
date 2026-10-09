"""Negative witnesses for the separate build evidence checker; no Cargo needed."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest import mock
import tempfile

SPEC = importlib.util.spec_from_file_location('tonemapping_build', Path(__file__).parents[1] / 'check-tonemapping-build.py')
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


def graph(mode='analytic', kind='app'):
    values = {name: {'version': '0.18.1', 'features': set()} for name in audit.KEY_CRATES}
    values['ktx2']['version'] = '0.4.0'
    values['ruzstd']['version'] = '0.8.2'
    values['bevy_image']['features'] = {'ktx2', 'zstd', 'zstd_rust'}
    features = {'analytic-tonemapping'} if mode == 'analytic' else {'default'}
    values['flightsim-render'] = {'version': '0.6.0-alpha.21', 'features': features if mode == 'analytic' or kind == 'sun-clock' else set()}
    if kind == 'app':
        values['flightsim-app'] = {'version': '0.6.0-alpha.21', 'features': features}
    if mode == 'analytic':
        values['bevy']['features'] = {'ktx2'}
    else:
        for name in ('bevy', 'bevy_internal', 'bevy_core_pipeline'):
            values[name]['features'].add('tonemapping_luts')
    return values


def messages(values, kind='app'):
    result = [{'reason': 'compiler-artifact', 'target': {'name': name, 'kind': ['lib']},
               'features': sorted(values[name]['features']),
               'package_id': f'registry+https://github.com/rust-lang/crates.io-index#{name}@{values[name]["version"]}'}
              for name in audit.KEY_CRATES]
    package = 'flightsim-app' if kind == 'app' else 'flightsim-render'
    result.append({'reason': 'compiler-artifact', 'target': {'name': 'flightsim-app' if kind == 'app' else 'sun_clock',
                   'kind': ['bin' if kind == 'app' else 'example']}, 'executable': 'example-output',
                   'filenames': ['example-output'],
                   'package_id': f'path+file:///source#{package}@{values[package]["version"]}',
                   'features': sorted(values[package]['features']), 'profile': {'test': False}})
    result.append({'reason': 'build-finished', 'success': True})
    return result


class TonemappingBuildTests(unittest.TestCase):
    def artifact_fixture(self, directory, mode):
        values = graph(mode)
        captured = messages(values)
        graph_path = directory / 'graph.txt'
        graph_path.write_text('\n'.join(f'{name} v{item["version"]} features=[{",".join(sorted(item["features"]))}]' for name, item in values.items()) + '\n')
        source = directory / 'src'
        source.mkdir()
        luts = source / 'tonemapping/luts'
        luts.mkdir(parents=True)
        payload = b'reviewed-exact-LUT-payload'
        (luts / 'one.ktx2').write_bytes(payload)
        for artifact in captured[:-2]:
            name = artifact['target']['name']
            library = directory / 'debug/deps' / f'lib{name}-1234.rlib'
            library.parent.mkdir(parents=True, exist_ok=True)
            library.write_bytes(b'compiled-library')
            artifact['filenames'] = [str(library)]
            artifact['target']['src_path'] = str(source / 'lib.rs')
            fingerprint = directory / 'debug/.fingerprint' / f'{name}-1234' / f'lib-{name}.json'
            fingerprint.parent.mkdir(parents=True)
            fingerprint.write_text(json.dumps({'features': json.dumps(artifact['features']), 'rustflags': ['-D', 'warnings']}))
            library.with_name(f'{name}-1234.d').write_text('/source/tonemapping/luts/one.ktx2' if mode == 'ordinary' else '/source/tonemapping/mod.rs')
        executable = directory / 'debug/app'
        executable.write_bytes(b'executable' + (payload if mode == 'ordinary' else b''))
        captured[-2]['executable'] = str(executable)
        captured[-2]['filenames'] = [str(executable)]
        messages_path = directory / 'build.jsonl'
        messages_path.write_text('\n'.join(json.dumps(m) for m in captured) + '\n')
        return graph_path, messages_path, {'one.ktx2': hashlib.sha256(payload).hexdigest()}

    def test_artifact_corruption_cannot_be_reported_as_exclusion(self):
        for mode in ('ordinary', 'analytic'):
            for corruption in ('none', 'wrong-fingerprint', 'warnings-allowed', 'wrong-depinfo', 'wrong-payload', 'changed-source', 'different-target-root'):
                with self.subTest(mode=mode, corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                    directory = Path(temporary)
                    graph_path, messages_path, luts = self.artifact_fixture(directory, mode)
                    fingerprint = directory / 'debug/.fingerprint/bevy_core_pipeline-1234/lib-bevy_core_pipeline.json'
                    if corruption == 'wrong-fingerprint':
                        data = json.loads(fingerprint.read_text()); data['features'] = '["unexpected"]'; fingerprint.write_text(json.dumps(data))
                    elif corruption == 'warnings-allowed':
                        data = json.loads(fingerprint.read_text()); data['rustflags'] = []; fingerprint.write_text(json.dumps(data))
                    elif corruption == 'wrong-depinfo':
                        (directory / 'debug/deps/bevy_core_pipeline-1234.d').write_text('/tonemapping/mod.rs' if mode == 'ordinary' else '/tonemapping/luts/one.ktx2')
                    elif corruption == 'wrong-payload':
                        (directory / 'debug/app').write_bytes(b'ordinary-missing' if mode == 'ordinary' else b'reactivated-reviewed-exact-LUT-payload')
                    elif corruption == 'changed-source':
                        (directory / 'src/tonemapping/luts/one.ktx2').write_bytes(b'changed')
                    elif corruption == 'different-target-root':
                        captured = [json.loads(line) for line in messages_path.read_text().splitlines()]
                        elsewhere = directory / 'different-target/app'
                        elsewhere.parent.mkdir()
                        elsewhere.write_bytes((directory / 'debug/app').read_bytes())
                        captured[-2]['executable'] = str(elsewhere)
                        captured[-2]['filenames'] = [str(elsewhere)]
                        messages_path.write_text('\n'.join(json.dumps(m) for m in captured) + '\n')
                    with mock.patch.object(audit, 'LUTS', luts):
                        if corruption == 'none':
                            result = audit.audit(graph_path, messages_path, mode, 'app')
                            self.assertEqual(result['payloads'][0]['full_bytes_offset'] >= 0, mode == 'ordinary')
                        else:
                            with self.assertRaises(ValueError):
                                audit.audit(graph_path, messages_path, mode, 'app')

    def test_all_supported_graph_and_artifact_modes(self):
        for mode in ('ordinary', 'analytic'):
            for kind in ('app', 'sun-clock'):
                with self.subTest(mode=mode, kind=kind):
                    values = graph(mode, kind)
                    audit.validate_graph(values, mode, kind)
                    audit.select_artifacts(messages(values, kind), values, kind)

    def test_cargo_path_package_version_only_fragment(self):
        for kind, package in (('app', 'flightsim-app'), ('sun-clock', 'flightsim-render')):
            values = graph(kind=kind)
            captured = messages(values, kind)
            captured[-2]['package_id'] = f'path+file:///workspace/crates/{package}#0.6.0-alpha.21'
            audit.select_artifacts(captured, values, kind)

    def test_parser_preserves_each_dependency_version_and_unions_duplicate_rows(self):
        values = audit.parse_graph('winnow v0.7.15 features=[std]\nwinnow v0.5.40 features=[alloc]\nbevy v0.18.1 features=[std]\nbevy v0.18.1 features=[ktx2] (*)\n')
        self.assertEqual(values['bevy']['features'], {'std', 'ktx2'})
        self.assertEqual(values['winnow@0.5.40']['features'], {'alloc'})
        self.assertEqual(len(values), 3)

    def test_parser_rejects_missing_or_malformed_evidence(self):
        for text in ('', 'error: Cargo failed', 'bevy v0.18.1', 'bevy v0.18.1 features=[]\nbevy v0.19.0 features=[]'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                audit.parse_graph(text)

    def test_lut_reactivation_is_rejected_even_outside_routing_crates(self):
        for name in (*audit.KEY_CRATES, 'other'):
            values = graph()
            values.setdefault(name, {'version': '1.0', 'features': set()})['features'].add('tonemapping_luts')
            with self.subTest(package=name), self.assertRaises(ValueError):
                audit.validate_graph(values, 'analytic', 'app')

    def test_decoder_and_mode_regressions_fail(self):
        cases = []
        for name in audit.KEY_CRATES:
            values = graph(); del values[name]; cases.append(values)
        for feature in ('ktx2', 'zstd', 'zstd_rust'):
            values = graph(); values['bevy_image']['features'].remove(feature); cases.append(values)
        for features in (set(), {'default'}, {'default', 'analytic-tonemapping'}, {'analytic-tonemapping', 'commercial-staging'}):
            values = graph(); values['flightsim-app']['features'] = features; cases.append(values)
        values = graph(); values['bevy']['version'] = '0.19.0'; cases.append(values)
        values = graph(); values['flightsim-render']['features'] = {'default'}; cases.append(values)
        for values in cases:
            with self.subTest(graph=values), self.assertRaises(ValueError):
                audit.validate_graph(values, 'analytic', 'app')

    def test_ordinary_requires_luts(self):
        for name in ('bevy', 'bevy_internal', 'bevy_core_pipeline'):
            values = graph('ordinary'); values[name]['features'].clear()
            with self.subTest(package=name), self.assertRaises(ValueError):
                audit.validate_graph(values, 'ordinary', 'app')

    def test_failed_incomplete_or_test_builds_cannot_supply_evidence(self):
        values = graph()
        original = messages(values)
        cases = [[], original[:-1], original[:-2] + [original[-1]]]
        changed = copy.deepcopy(original); changed[-1]['success'] = False; cases.append(changed)
        changed = copy.deepcopy(original); changed[-2]['profile']['test'] = True; cases.append(changed)
        changed = copy.deepcopy(original); changed[-2]['filenames'] = ['different-executable']; cases.append(changed)
        changed = copy.deepcopy(original); changed[-2]['target']['kind'] = ['custom-build']; cases.append(changed)
        changed = copy.deepcopy(original); changed[-2]['package_id'] = 'path+file:///other#wrong@0.6.0-alpha.21'; cases.append(changed)
        changed = copy.deepcopy(original); changed.insert(-1, changed[-2]); cases.append(changed)
        for captured in cases:
            with self.subTest(messages=captured), self.assertRaises(ValueError):
                audit.select_artifacts(captured, values, 'app')

    def test_compiled_features_version_and_library_presence_must_match(self):
        values = graph()
        for index in range(len(audit.KEY_CRATES)):
            original = messages(values)
            cases = [original[:index] + original[index+1:]]
            changed = copy.deepcopy(original); changed[index]['features'].append('unexpected'); cases.append(changed)
            changed = copy.deepcopy(original); changed[index]['package_id'] = 'wrong#0.19.0'; cases.append(changed)
            for captured in cases:
                with self.subTest(index=index, messages=captured), self.assertRaises(ValueError):
                    audit.select_artifacts(captured, values, 'app')


class TwoLutSourceBuildTests(unittest.TestCase):
    """Synthetic artifacts exercise admission; they are never native/build proof."""

    def fixture(self, directory, mode='ordinary', target=audit.TARGETS[0]):
        import io
        import shutil
        import tarfile
        repository = Path(__file__).resolve().parents[2]
        source = directory / 'source'
        vendor = source / 'vendor/bevy_core_pipeline'
        shutil.copytree(repository / 'vendor/bevy_core_pipeline', vendor)
        for name in ('Cargo.toml', 'Cargo.lock'):
            shutil.copyfile(repository / name, source / name)
        shutil.copytree(repository / 'docs/release/licenses', source / 'docs/release/licenses')
        provenance_path = vendor / 'FLIGHTSIM-UPSTREAM-SOURCE.json'
        provenance = json.loads(provenance_path.read_text())
        originals = {}
        for entry in provenance['original_files']:
            name = entry['path']
            originals[name] = ((b'synthetic-reference-agx-not-shipped' if name.endswith(audit.AGX) else b'original-vcs')
                               if not (vendor / name).exists() else (vendor / name).read_bytes())
        originals['src/tonemapping/mod.rs'] = b'synthetic-original-unmodified-module'
        archive = directory / 'private.crate'
        with tarfile.open(archive, 'w:gz') as package:
            for name, payload in sorted(originals.items()):
                member = tarfile.TarInfo('bevy_core_pipeline-0.18.1/' + name)
                member.size = len(payload)
                package.addfile(member, io.BytesIO(payload))
        provenance['upstream_archive_sha256'] = audit.digest(archive)
        provenance['original_files'] = [{'path': name, 'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest()}
                                        for name, payload in sorted(originals.items())]
        provenance_path.write_text(json.dumps(provenance))
        manifest = json.loads(audit.SOURCE_MANIFEST.read_text())
        manifest['files'] = [{'path': p.relative_to(vendor).as_posix(), 'bytes': p.stat().st_size,
                              'sha256': audit.digest(p)} for p in sorted(vendor.rglob('*')) if p.is_file()]
        manifest['upstream_archive_sha256'] = audit.digest(archive)
        manifest_path = directory / 'source-manifest.json'
        manifest_path.write_text(json.dumps(manifest))
        lut_hashes = dict(audit.LUTS)
        lut_hashes[audit.AGX] = hashlib.sha256(originals['src/tonemapping/luts/' + audit.AGX]).hexdigest()
        values = graph(mode)
        captured = messages(values)
        render = {'reason': 'compiler-artifact', 'target': {'name': 'flightsim_render', 'kind': ['lib']},
                  'features': sorted(values['flightsim-render']['features'])}
        captured.insert(-2, render)
        profile_dir = directory / 'build' / target / 'debug'
        for item in captured[:-2]:
            name = item['target']['name']
            package_name = name.replace('_', '-') if name == 'flightsim_render' else name
            src = vendor / 'src/lib.rs' if name == 'bevy_core_pipeline' else source / ('crates/' + package_name + '/src/lib.rs')
            src.parent.mkdir(parents=True, exist_ok=True)
            if not src.exists():
                src.write_text('synthetic rust source')
            if name in ('bevy_core_pipeline', 'flightsim_render'):
                item['package_id'] = 'path+' + src.parent.parent.as_uri() + '#' + values[package_name]['version']
            item['target']['src_path'] = str(src)
            item['profile'] = {'test': False}
            library = profile_dir / 'deps' / ('lib' + name + '-1234.rlib')
            library.parent.mkdir(parents=True, exist_ok=True)
            library.write_bytes(b'synthetic-compiled-library')
            item['filenames'] = [str(library)]
            fingerprint = profile_dir / '.fingerprint' / (package_name + '-1234') / ('lib-' + name + '.json')
            fingerprint.parent.mkdir(parents=True, exist_ok=True)
            fingerprint.write_text(json.dumps({'features': json.dumps(item['features']), 'rustflags': ['-D', 'warnings']}))
        dependencies = [vendor / 'src/lib.rs', vendor / 'src/tonemapping/mod.rs']
        if mode == 'ordinary':
            dependencies.extend(vendor / 'src/tonemapping/luts' / name for name in audit.RETAINED_LUTS)
        depinfo = profile_dir / 'deps/bevy_core_pipeline-1234.d'
        depinfo.write_text(str(depinfo) + ': ' + ' '.join(str(p).replace(' ', '\\ ') for p in dependencies) + '\n')
        root = captured[-2]
        root_source = source / 'crates/flightsim-app/src/main.rs'
        root_source.parent.mkdir(parents=True, exist_ok=True)
        root_source.write_text('synthetic app source')
        root['target']['src_path'] = str(root_source)
        root['package_id'] = 'path+' + root_source.parent.parent.as_uri() + '#0.6.0-alpha.21'
        executable = profile_dir / ('flightsim-app.exe' if target.endswith('msvc') else 'flightsim-app')
        header = bytearray(128)
        if target.endswith('msvc'):
            header[:2] = b'MZ'; header[60:64] = (64).to_bytes(4, 'little')
            header[64:70] = b'PE\0\0\x64\x86'; header[88:90] = b'\x0b\x02'
        else:
            header[:6] = b'\x7fELF\x02\x01'; header[18:20] = b'\x3e\x00'
        payloads = {name: originals['src/tonemapping/luts/' + name] for name in audit.LUTS}
        executable.write_bytes(bytes(header) + b''.join(payloads[n] for n in audit.RETAINED_LUTS) if mode == 'ordinary' else bytes(header))
        root['executable'] = str(executable); root['filenames'] = [str(executable)]
        root_fp = profile_dir / '.fingerprint/flightsim-app-4321/bin-flightsim-app.json'
        root_fp.parent.mkdir(parents=True)
        root_fp.write_text(json.dumps({'features': json.dumps(root['features']), 'rustflags': ['-D', 'warnings']}))
        graph_path = directory / 'graph.txt'
        graph_path.write_text('\n'.join(f'{name} v{value["version"]}' +
                                       (f' ({vendor})' if name == 'bevy_core_pipeline' else
                                        f' ({source / "crates" / name})' if name in ('flightsim-app', 'flightsim-render') else '') +
                                       f' features=[{",".join(sorted(value["features"]))}]'
                                       for name, value in values.items()) + '\n')
        messages_path = directory / 'messages.jsonl'
        messages_path.write_text('\n'.join(json.dumps(m) for m in captured) + '\n')
        return {'source': source, 'vendor': vendor, 'archive': archive, 'manifest': manifest_path,
                'manifest_hash': audit.digest(manifest_path), 'lut_hashes': lut_hashes,
                'graph': graph_path, 'messages': messages_path, 'captured': captured, 'mode': mode,
                'target': target, 'executable': executable, 'depinfo': depinfo, 'root_fp': root_fp,
                'payloads': payloads, 'profile_dir': profile_dir}

    def run_fixture(self, fixture):
        with mock.patch.object(audit, 'SOURCE_MANIFEST', fixture['manifest']), \
             mock.patch.object(audit, 'SOURCE_MANIFEST_SHA256', fixture['manifest_hash']), \
             mock.patch.object(audit, 'LUTS', fixture['lut_hashes']):
            return audit.audit(fixture['graph'], fixture['messages'], fixture['mode'], 'app',
                               source_recipe=audit.TWO_LUT_SOURCE, source_root=fixture['source'],
                               upstream_archive=fixture['archive'], target=fixture['target'])

    def write_messages(self, fixture):
        fixture['messages'].write_text('\n'.join(json.dumps(m) for m in fixture['captured']) + '\n')

    def test_explicit_two_lut_modes_and_target_formats(self):
        for mode in ('ordinary', 'analytic'):
            for target in audit.TARGETS:
                with self.subTest(mode=mode, target=target), tempfile.TemporaryDirectory() as temporary:
                    fixture = self.fixture(Path(temporary), mode, target)
                    result = self.run_fixture(fixture)
                    self.assertEqual(result['source']['vendor_file_count'], 55)
                    self.assertEqual(len(result['compiled']), 7)
                    self.assertEqual(result['source_recipe'], audit.TWO_LUT_SOURCE)
                    self.assertFalse(result['release_admitted'])
                    self.assertFalse(result['native_qualified'])
                    self.assertEqual({p['name'] for p in result['payloads'] if p['full_bytes_offset'] >= 0},
                                     set(audit.RETAINED_LUTS) if mode == 'ordinary' else set())

    def test_exact_package_uri_accepts_both_cargo_fragment_forms(self):
        root = Path('/source/vendor/bevy_core_pipeline')
        from pathlib import PureWindowsPath
        windows = PureWindowsPath('C:/reviewed source/vendor/bevy_core_pipeline')
        for fragment in ('0.18.1', 'bevy_core_pipeline@0.18.1'):
            audit.path_package_identity('path+file:///C:/reviewed%20source/vendor/bevy_core_pipeline#' + fragment,
                                        windows, 'bevy_core_pipeline', '0.18.1')
            audit.path_package_identity('path+' + root.as_uri() + '#' + fragment, root, 'bevy_core_pipeline', '0.18.1')
        for identity in ('registry+https://github.com/rust-lang/crates.io-index#bevy_core_pipeline@0.18.1',
                         'path+file:///other/bevy_core_pipeline#0.18.1',
                         'path+file:///source/vendor/bevy_core_pipeline#0.19.0',
                         'path+file:///source/vendor/bevy_core_pipeline#other@0.18.1',
                         'path+file://host/source/vendor/bevy_core_pipeline#0.18.1',
                         'path+file:///source/vendor/bevy_core_pipeline?spoof=1#0.18.1',
                         'path+file:///source/vendor/%62evy_core_pipeline#0.18.1'):
            with self.subTest(identity=identity), self.assertRaises(ValueError):
                audit.path_package_identity(identity, root, 'bevy_core_pipeline', '0.18.1')

    def test_source_paths_with_spaces_keep_exact_cargo_and_depinfo_identity(self):
        with tempfile.TemporaryDirectory(prefix='tone source ') as temporary:
            f = self.fixture(Path(temporary))
            value = f['depinfo'].read_text()
            lhs, rhs = value.split(': ', 1)
            f['depinfo'].write_text(lhs.replace(' ', '\\ ') + ': ' + rhs)
            self.assertEqual(self.run_fixture(f)['source_recipe'], audit.TWO_LUT_SOURCE)

    def test_source_graph_and_cargo_message_spoofing(self):
        for corruption in ('registry-package', 'foreign-source', 'foreign-app', 'wrong-graph', 'ambiguous-graph',
                           'missing-library', 'duplicate-library', 'test-library', 'extra-executable',
                           'contradictory-completion', 'compiled-features', 'render-features', 'foreign-root-filename', 'compiler-warning'):
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary)); core = f['captured'][2]
                if corruption == 'registry-package':
                    core['package_id'] = 'registry+https://github.com/rust-lang/crates.io-index#bevy_core_pipeline@0.18.1'
                elif corruption == 'foreign-source': core['target']['src_path'] = '/other/bevy_core_pipeline/src/lib.rs'
                elif corruption == 'foreign-app': f['captured'][-2]['package_id'] = 'path+file:///other/flightsim-app#0.6.0-alpha.21'
                elif corruption == 'wrong-graph': f['graph'].write_text(f['graph'].read_text().replace(str(f['vendor']), '/foreign'))
                elif corruption == 'ambiguous-graph':
                    f['graph'].write_text(f['graph'].read_text() + 'bevy_core_pipeline v0.18.1 features=[tonemapping_luts]\n')
                elif corruption == 'missing-library': del f['captured'][2]
                elif corruption == 'duplicate-library': f['captured'].insert(2, copy.deepcopy(core))
                elif corruption == 'test-library': core['profile']['test'] = True
                elif corruption == 'extra-executable':
                    extra = copy.deepcopy(f['captured'][-2]); extra['target']['name'] = 'other'; f['captured'].insert(-1, extra)
                elif corruption == 'contradictory-completion': f['captured'].insert(0, {'reason': 'build-finished', 'success': False})
                elif corruption == 'compiled-features': core['features'] = []
                elif corruption == 'render-features': f['captured'][-3]['features'] = ['default']
                elif corruption == 'foreign-root-filename': f['captured'][-2]['filenames'].append('/other/executable')
                elif corruption == 'compiler-warning': f['captured'].insert(-1, {'reason': 'compiler-message', 'message': {'level': 'warning'}})
                self.write_messages(f)
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_binary_payload_presence_is_exact_not_two_or_three(self):
        cases = [('ordinary', name, action) for name in audit.RETAINED_LUTS for action in ('omit', 'mutate')]
        cases += [('ordinary', audit.AGX, 'insert')]
        cases += [('analytic', name, 'insert') for name in audit.LUTS]
        for mode, name, action in cases:
            with self.subTest(mode=mode, name=name, action=action), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary), mode)
                payload = f['payloads'][name]; binary = f['executable'].read_bytes()
                if action == 'insert': binary += payload
                elif action == 'omit': binary = binary.replace(payload, b'')
                else: binary = binary.replace(payload, payload[:-1] + bytes([payload[-1] ^ 1]))
                f['executable'].write_bytes(binary)
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_source_boundary_payload_notices_and_patch_route_mutations(self):
        for corruption in ('extra-file', 'missing-file', 'extra-directory', 'source-symlink', 'shader',
                           'module', 'tony', 'vendor-notice', 'outer-notice', 'patch-route', 'registry-lock', 'manifest'):
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary)); v = f['vendor']
                if corruption == 'extra-file': (v / 'extra.txt').write_text('extra')
                elif corruption == 'extra-directory': (v / 'extra').mkdir()
                elif corruption == 'missing-file': (v / 'src/tonemapping/node.rs').unlink()
                elif corruption == 'source-symlink':
                    node = v / 'src/tonemapping/node.rs'; data = node.read_bytes(); node.unlink()
                    other = Path(temporary) / 'node.rs'; other.write_bytes(data); node.symlink_to(other)
                elif corruption == 'shader': (v / 'src/tonemapping/tonemapping_shared.wgsl').write_text('changed')
                elif corruption == 'module': (v / 'src/tonemapping/mod.rs').write_text('changed')
                elif corruption == 'tony': (v / 'src/tonemapping/luts/tony_mc_mapface.ktx2').write_bytes(b'changed')
                elif corruption == 'vendor-notice': (v / 'third-party-notices/TonyMcMapface-LICENSE-MIT').write_text('changed')
                elif corruption == 'outer-notice': (f['source'] / 'docs/release/licenses/TonyMcMapface-LICENSE-MIT').write_text('changed')
                elif corruption == 'patch-route':
                    p = f['source'] / 'Cargo.toml'; p.write_text(p.read_text().replace('vendor/bevy_core_pipeline', 'wrong/path'))
                elif corruption == 'registry-lock':
                    p = f['source'] / 'Cargo.lock'; p.write_text(p.read_text().replace('name = "bevy_core_pipeline"\n', 'name = "bevy_core_pipeline"\nsource = "registry+spoof"\n'))
                elif corruption == 'manifest': f['manifest'].write_text(f['manifest'].read_text() + ' ')
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_graph_app_and_render_routes_must_match_exact_source(self):
        for name in ('flightsim-app', 'flightsim-render'):
            for mutation in ('foreign', 'missing', 'duplicate-foreign'):
                with self.subTest(name=name, mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                    f = self.fixture(Path(temporary))
                    original = f['graph'].read_text()
                    expected = str(f['source'] / 'crates' / name)
                    if mutation == 'foreign':
                        changed = original.replace(' (' + expected + ')', ' (/foreign/' + name + ')')
                    elif mutation == 'missing':
                        changed = original.replace(' (' + expected + ')', '')
                    else:
                        line = next(line for line in original.splitlines() if line.startswith(name + ' '))
                        changed = original + line.replace(expected, '/foreign/' + name) + '\n'
                    self.assertNotEqual(changed, original)
                    f['graph'].write_text(changed)
                    with self.assertRaisesRegex(ValueError, 'graph ' + name + ' route differs'):
                        self.run_fixture(f)

    def test_checkout_relative_depinfo_is_bound_to_admitted_source_root(self):
        for mode in ('ordinary', 'analytic'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary), mode)
                value = f['depinfo'].read_text().replace(str(f['source']) + '/', '')
                f['depinfo'].write_text(value)
                self.assertEqual(self.run_fixture(f)['mode'], mode)
                for token in ('../source/vendor/bevy_core_pipeline/src/lib.rs', 'foreign/src/lib.rs'):
                    f['depinfo'].write_text(value.replace('vendor/bevy_core_pipeline/src/lib.rs', token))
                    with self.assertRaises(ValueError): self.run_fixture(f)

    def test_depinfo_exact_payload_and_admitted_source_paths(self):
        for mode, corruption in (('ordinary', 'missing-tony'), ('ordinary', 'agx'), ('analytic', 'tony'),
                                 ('ordinary', 'foreign-source'), ('ordinary', 'wrong-artifact'),
                                 ('ordinary', 'missing-lib'), ('ordinary', 'inconsistent-rules')):
            with self.subTest(mode=mode, corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary), mode); p = f['depinfo']; value = p.read_text()
                if corruption == 'missing-tony': value = value.replace(' ' + str(f['vendor'] / 'src/tonemapping/luts/tony_mc_mapface.ktx2'), '')
                elif corruption in ('agx', 'tony'):
                    name = audit.AGX if corruption == 'agx' else 'tony_mc_mapface.ktx2'
                    value = value.rstrip() + ' ' + str(f['vendor'] / 'src/tonemapping/luts' / name) + '\n'
                elif corruption == 'foreign-source': value = value.rstrip() + ' /foreign/src/hidden.rs\n'
                elif corruption == 'wrong-artifact': value = value.replace(str(p) + ':', '/other/stale.d:')
                elif corruption == 'missing-lib': value = value.replace(' ' + str(f['vendor'] / 'src/lib.rs'), '')
                else: value += str(p) + ': ' + str(f['vendor'] / 'src/lib.rs') + '\n'
                p.write_text(value)
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_artifact_target_fingerprint_ambiguity_and_flags(self):
        for corruption in ('foreign-library', 'foreign-filename', 'wrong-header', 'wrong-target', 'root-features',
                           'root-flags', 'ambiguous-root-fingerprint', 'render-flags', 'symlink-executable'):
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary)); root = f['root_fp']
                if corruption in ('foreign-library', 'foreign-filename'):
                    library = Path(f['captured'][2]['filenames'][0]); foreign = Path(temporary) / library.name
                    foreign.write_bytes(library.read_bytes())
                    if corruption == 'foreign-library': f['captured'][2]['filenames'] = [str(foreign)]
                    else: f['captured'][2]['filenames'].append(str(foreign.with_suffix('.rmeta')))
                    self.write_messages(f)
                elif corruption == 'wrong-header': f['executable'].write_bytes(b'wrong-format' + f['executable'].read_bytes()[12:])
                elif corruption == 'wrong-target': f['target'] = audit.TARGETS[1]
                elif corruption in ('root-features', 'root-flags'):
                    data = json.loads(root.read_text()); data['features' if corruption == 'root-features' else 'rustflags'] = '[]' if corruption == 'root-features' else []
                    root.write_text(json.dumps(data))
                elif corruption == 'ambiguous-root-fingerprint':
                    other = root.parent.parent / 'flightsim-app-5678' / root.name; other.parent.mkdir(); other.write_bytes(root.read_bytes())
                elif corruption == 'render-flags':
                    p = f['profile_dir'] / '.fingerprint/flightsim-render-1234/lib-flightsim_render.json'
                    data = json.loads(p.read_text()); data['rustflags'] = []; p.write_text(json.dumps(data))
                else:
                    exe = f['executable']; other = Path(temporary) / 'other-exe'; other.write_bytes(exe.read_bytes()); exe.unlink(); exe.symlink_to(other)
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_private_reference_is_mandatory_bounded_and_outside_source(self):
        for corruption in ('missing-option', 'source-contained', 'wrong-hash', 'truncated', 'symlink', 'oversized'):
            with self.subTest(corruption=corruption), tempfile.TemporaryDirectory() as temporary:
                f = self.fixture(Path(temporary))
                if corruption == 'missing-option':
                    with self.assertRaises(ValueError):
                        audit.audit(f['graph'], f['messages'], 'ordinary', 'app', source_recipe=audit.TWO_LUT_SOURCE)
                    continue
                if corruption == 'source-contained':
                    p = f['source'] / 'private.crate'; p.write_bytes(f['archive'].read_bytes()); f['archive'] = p
                elif corruption == 'wrong-hash': f['archive'].write_bytes(f['archive'].read_bytes() + b'changed')
                elif corruption == 'truncated': f['archive'].write_bytes(f['archive'].read_bytes()[:100])
                elif corruption == 'symlink':
                    p = Path(temporary) / 'alias.crate'; p.symlink_to(f['archive']); f['archive'] = p
                else:
                    with f['archive'].open('wb') as stream: stream.truncate(audit.MAX_ARCHIVE_BYTES + 1)
                with self.assertRaises(ValueError): self.run_fixture(f)

    def test_historical_ordinary_control_still_requires_all_three_payloads(self):
        old = TonemappingBuildTests()
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            graph_path, messages_path, _ = old.artifact_fixture(directory, 'ordinary')
            lut_dir = directory / 'src/tonemapping/luts'
            payloads = {name: ('historical-control-' + name).encode() for name in audit.LUTS}
            for name, data in payloads.items(): (lut_dir / name).write_bytes(data)
            hashes = {name: hashlib.sha256(data).hexdigest() for name, data in payloads.items()}
            executable = directory / 'debug/app'; executable.write_bytes(b''.join(payloads.values()))
            with mock.patch.object(audit, 'LUTS', hashes):
                self.assertEqual(len(audit.audit(graph_path, messages_path, 'ordinary', 'app')['payloads']), 3)
                executable.write_bytes(b''.join(payloads[name] for name in audit.RETAINED_LUTS))
                with self.assertRaises(ValueError): audit.audit(graph_path, messages_path, 'ordinary', 'app')


class PrivateReferenceArchiveTests(unittest.TestCase):
    def make_archive(self, directory, entries):
        import io
        import tarfile
        path = directory / 'reference.crate'
        with tarfile.open(path, 'w:gz') as archive:
            for name, value, kind in entries:
                info = tarfile.TarInfo(name)
                info.type = kind; info.size = len(value)
                if kind == tarfile.SYMTYPE: info.linkname = '/outside'
                archive.addfile(info, io.BytesIO(value))
        return path

    def test_archive_inventory_and_unsafe_member_rejections(self):
        import tarfile
        prefix = 'bevy_core_pipeline-0.18.1/'
        inventory = [{'path': 'one', 'bytes': 7, 'sha256': hashlib.sha256(b'payload').hexdigest()}]
        cases = {'valid': [(prefix + 'one', b'payload', tarfile.REGTYPE)],
                 'traversal': [(prefix + '../one', b'payload', tarfile.REGTYPE)],
                 'symlink': [(prefix + 'one', b'', tarfile.SYMTYPE)],
                 'duplicate': [(prefix + 'one', b'payload', tarfile.REGTYPE)] * 2,
                 'wrong-root': [('foreign/one', b'payload', tarfile.REGTYPE)],
                 'missing': [], 'extra': [(prefix + 'one', b'payload', tarfile.REGTYPE), (prefix + 'two', b'other', tarfile.REGTYPE)],
                 'mutated': [(prefix + 'one', b'changed', tarfile.REGTYPE)]}
        for kind, entries in cases.items():
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                path = self.make_archive(Path(temporary), entries)
                if kind == 'valid':
                    self.assertEqual(audit.read_reference_archive(path, audit.digest(path), inventory), {'one': b'payload'})
                    with mock.patch.object(audit, 'MAX_EXPANDED_BYTES', 16), self.assertRaises(ValueError):
                        audit.read_reference_archive(path, audit.digest(path), inventory)
                else:
                    with self.assertRaises(ValueError): audit.read_reference_archive(path, audit.digest(path), inventory)


if __name__ == '__main__':
    unittest.main()
