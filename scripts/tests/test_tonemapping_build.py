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


if __name__ == '__main__':
    unittest.main()
