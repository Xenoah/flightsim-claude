#!/usr/bin/env python3
"""Audit one exact build's graph, Cargo messages, fingerprints and LUT bytes.

Read-only engineering evidence; this does not authorize release or replace the
ordinary candidate gates. Capture graph/messages with the commands in
`docs/analytic-tonemapping.md` before reusing a target directory.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import mmap
from pathlib import Path
import re
import sys

ANALYTICAL_SWIFT_RECIPE = 'swift-only-analytical-windows-build-evidence-v1'

KEY_CRATES = ('bevy', 'bevy_internal', 'bevy_core_pipeline', 'bevy_image', 'ktx2', 'ruzstd')
LUTS = {
    'AgX-default_contrast.ktx2': '90fcdff22741698dbed7b3b2fd3133006847c63185d5dc42fbab6084f50e69cf',
    'Blender_-11_12.ktx2': 'a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7',
    'tony_mc_mapface.ktx2': '053e5adc519b1d733c819a625199e61cf138549db1358e533811d45df3227f84',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def parse_graph(text):
    graph = {}
    for line in text.splitlines():
        match = re.fullmatch(r'([\w-]+) v(\S+)(?: \([^\n]*\))? features=\[([^]]*)\](?: \(\*\))?', line)
        require(match is not None, f'unrecognized graph line: {line!r}')
        name, version, features = match.groups()
        key = name if name in KEY_CRATES or name.startswith('flightsim-') else f'{name}@{version}'
        entry = graph.setdefault(key, {'version': version, 'features': set()})
        require(entry['version'] == version, f'multiple versions of {name}; review graph explicitly')
        entry['features'].update(filter(None, features.split(',')))
    require(graph, 'empty graph')
    return graph


def validate_graph(graph, mode, kind, *, recipe=None):
    analytical = mode == 'analytic'
    root = 'flightsim-app' if kind == 'app' else 'flightsim-render'
    require(root in graph, f'missing root package {root}')
    require(recipe is None or (recipe == ANALYTICAL_SWIFT_RECIPE and mode == 'analytic' and kind == 'app'),
            'recipe requires the explicit analytical Swift app identity')
    expected = {'analytic-tonemapping'} if analytical else {'default'}
    root_features = expected | {'commercial-staging'} if recipe else expected
    require(graph[root]['features'] == root_features, 'unexpected root mode/features; use the exact documented build')
    for name in KEY_CRATES:
        require(name in graph, f'missing graph package {name}')
        if name.startswith('bevy'):
            require(graph[name]['version'] == '0.18.1', f'unreviewed {name} version')
    for name in ('bevy', 'bevy_internal', 'bevy_core_pipeline'):
        require(('tonemapping_luts' in graph[name]['features']) != analytical,
                f'wrong tonemapping_luts feature in {name}')
    if analytical:
        require(not any('tonemapping_luts' in p['features'] for p in graph.values()),
                'analytical graph reactivated tonemapping_luts')
        require('ktx2' in graph['bevy']['features'], 'analytical route must explicitly retain KTX2')
    require({'ktx2', 'zstd', 'zstd_rust'} <= graph['bevy_image']['features'],
            'required KTX2/zstd image decoding is missing')
    render_features = expected if kind == 'sun-clock' or analytical else set()
    require(graph['flightsim-render']['features'] == render_features, 'unexpected render feature routing')


def select_artifacts(messages, graph, kind):
    require(messages and messages[-1].get('reason') == 'build-finished'
            and messages[-1].get('success') is True, 'build did not finish successfully')
    artifacts = [m for m in messages if m.get('reason') == 'compiler-artifact']
    name, target_kind = ('flightsim-app', 'bin') if kind == 'app' else ('sun_clock', 'example')
    roots = [m for m in artifacts if m['target']['name'] == name
             and m['target']['kind'] == [target_kind] and m.get('executable')]
    require(len(roots) == 1, 'expected exactly one executable artifact')
    root = roots[0]
    require(root['executable'] in root['filenames'], 'executable is not in its Cargo artifact filenames')
    package = 'flightsim-app' if kind == 'app' else 'flightsim-render'
    # Cargo omits the name from a path-package fragment when its directory
    # already supplies that name (for example /flightsim-app#0.6.0-alpha.21).
    root_id = root['package_id']
    version = graph[package]['version']
    require(root_id.endswith(f'#{package}@{version}')
            or (root_id.startswith('path+file://') and root_id.endswith(f'/{package}#{version}')),
            'executable package/version differs from graph')
    require(set(root['features']) == graph[package]['features'], 'executable features differ from graph')
    require(root.get('profile', {}).get('test') is False, 'test executable is not a build artifact')
    selected = {}
    for name in KEY_CRATES:
        matches = [m for m in artifacts if m['target']['name'] == name
                   and m['target']['kind'] == ['lib']]
        require(len(matches) == 1, f'expected one compiled {name} library')
        require(set(matches[0]['features']) == graph[name]['features'], f'{name} compiled features differ from graph')
        require(matches[0]['package_id'].endswith(f'#{name}@' + graph[name]['version']),
                f'{name} compiled version differs from graph')
        selected[name] = matches[0]
    return root, selected


def audit(graph_path, messages_path, mode, kind, *, recipe=None):
    graph = parse_graph(graph_path.read_text())
    validate_graph(graph, mode, kind, recipe=recipe)
    messages = [json.loads(line) for line in messages_path.read_text().splitlines()]
    root, selected = select_artifacts(messages, graph, kind)
    executable = Path(root['executable'])
    require(executable.is_absolute(), 'expected the absolute executable path reported by Cargo')
    profile_dir = executable.parent if kind == 'app' else executable.parent.parent
    result = {'mode': mode, 'kind': kind, 'graph_sha256': digest(graph_path),
              'messages_sha256': digest(messages_path), 'executable': str(executable),
              'package_id': root['package_id'], 'features': root['features'], 'profile': root['profile'],
              'executable_sha256': digest(executable), 'executable_bytes': executable.stat().st_size,
              'compiled': [], 'payloads': []}
    for name, artifact in selected.items():
        libraries = [Path(p) for p in artifact['filenames'] if p.endswith('.rlib')]
        require(len(libraries) == 1, f'expected one linked {name} rlib')
        library = libraries[0]
        require(library.parent.resolve() == (profile_dir / 'deps').resolve(),
                f'{name} is from a different target directory than the executable')
        stem = library.stem.removeprefix('lib')
        suffix = stem.removeprefix(name + '-')
        require(suffix != stem, f'unrecognized rlib name {library}')
        fingerprint = library.parent.parent / '.fingerprint' / f'{name}-{suffix}' / f'lib-{name}.json'
        fp = json.loads(fingerprint.read_text())
        require(set(json.loads(fp['features'])) == graph[name]['features'], f'{name} fingerprint differs from graph')
        require(fp.get('rustflags') == ['-D', 'warnings'], f'{name} was not compiled with warnings denied')
        result['compiled'].append({'package': name, 'version': graph[name]['version'],
                                  'features': sorted(graph[name]['features']),
                                  'rlib': str(library), 'rlib_sha256': digest(library),
                                  'fingerprint': str(fingerprint), 'fingerprint_sha256': digest(fingerprint)})
        if name == 'bevy_core_pipeline':
            depinfo = library.with_name(stem + '.d')
            has_luts = '/tonemapping/luts/' in depinfo.read_text().replace('\\', '/')
            require(has_luts == (mode == 'ordinary'), 'core-pipeline source dependencies disagree with mode')
            result['core_pipeline_depinfo'] = {'path': str(depinfo), 'sha256': digest(depinfo), 'has_luts': has_luts}
    lut_dir = Path(selected['bevy_core_pipeline']['target']['src_path']).parent / 'tonemapping/luts'
    with executable.open('rb') as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        for name, expected in LUTS.items():
            path = lut_dir / name
            require(digest(path) == expected, f'unreviewed source LUT bytes: {path}')
            offset = data.find(path.read_bytes())
            require((offset >= 0) == (mode == 'ordinary'), f'{name} payload presence disagrees with mode')
            result['payloads'].append({'name': name, 'source_sha256': expected,
                                       'bytes': path.stat().st_size, 'full_bytes_offset': offset})
    if recipe is not None:
        result['recipe'] = recipe
    result['limits'] = 'Exact source LUT payload search supports graph/fingerprint/dep-info evidence; not universal content, visual, performance, Windows runtime or licensing qualification.'
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['ordinary', 'analytic'], required=True)
    parser.add_argument('--kind', choices=['app', 'sun-clock'], required=True)
    parser.add_argument('--graph', type=Path, required=True)
    parser.add_argument('--messages', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = audit(args.graph, args.messages, args.mode, args.kind)
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f'tonemapping audit failed: {error}', file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
