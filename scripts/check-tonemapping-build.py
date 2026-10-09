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
import io
import gzip
from pathlib import Path
import re
import sys
import tarfile
import tomllib
from urllib.parse import urlsplit, unquote

ANALYTICAL_SWIFT_RECIPE = 'swift-only-analytical-windows-build-evidence-v1'
UPSTREAM_THREE_LUT_SOURCE = 'bevy-0.18.1-upstream-three-lut-source-v1'
TWO_LUT_SOURCE = 'bevy-0.18.1-tony-filmic-source-v1'
SOURCE_MANIFEST = Path(__file__).with_name('tonemapping-two-lut-source.json')
SOURCE_MANIFEST_SHA256 = '8d98eac4df13df864cf9aea40422b153b8c41cc1f7bcb6c1cb23a87c2737cb51'
MAX_ARCHIVE_BYTES = 2 * 1024 * 1024
MAX_EXPANDED_BYTES = 4 * 1024 * 1024
MAX_EVIDENCE_BYTES = 64 * 1024 * 1024
MAX_BINARY_BYTES = 2 * 1024 * 1024 * 1024
TARGETS = ('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc')
AGX = 'AgX-default_contrast.ktx2'
RETAINED_LUTS = ('Blender_-11_12.ktx2', 'tony_mc_mapface.ktx2')

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


def regular_file(path, maximum):
    """Reject aliases/symlinks/special files and bound every new-recipe read."""
    path = Path(path)
    require(path.is_absolute() and path == path.resolve(), f'noncanonical or symlink path: {path}')
    require(path.is_file() and 0 < path.stat().st_size <= maximum, f'nonregular, empty or oversized file: {path}')
    require(not any(item.is_symlink() for item in (path, *path.parents)), f'symlink path: {path}')
    return path


def read_reference_archive(path, expected_sha256, inventory):
    """Read a complete private reference into bounded memory; never extract it."""
    path = regular_file(path, MAX_ARCHIVE_BYTES)
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == expected_sha256, 'unexpected original crate archive hash')
    # Limit decompression before tarfile parses attacker-controlled metadata.
    with gzip.GzipFile(fileobj=io.BytesIO(raw)) as compressed:
        expanded = compressed.read(MAX_EXPANDED_BYTES + 1)
    require(len(expanded) <= MAX_EXPANDED_BYTES, 'original crate archive expands beyond bound')
    originals = {}
    prefix = 'bevy_core_pipeline-0.18.1/'
    with tarfile.open(fileobj=io.BytesIO(expanded), mode='r:') as archive:
        for count, member in enumerate(archive, 1):
            require(count <= 64, 'too many original archive members')
            require(member.isfile() and not member.issym() and not member.islnk(), 'unsafe original archive member')
            require(member.name.startswith(prefix), 'wrong original archive root')
            name = member.name[len(prefix):]
            require(name and not any(part in ('', '.', '..') for part in name.split('/'))
                    and '\\' not in name and ':' not in name, 'unsafe original archive path')
            require(name not in originals, 'duplicate original archive member')
            require(0 <= member.size <= MAX_ARCHIVE_BYTES, 'oversized original archive member')
            originals[name] = archive.extractfile(member).read(member.size + 1)
            require(len(originals[name]) == member.size, 'truncated original archive member')
    observed = [{'path': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                for name, data in sorted(originals.items())]
    require(observed == inventory, 'original archive inventory mismatch')
    return originals


def source_inventory(source_root):
    source_root = Path(source_root)
    require(source_root.is_absolute() and source_root == source_root.resolve(), 'source root must be canonical and absolute')
    require(not any(p.is_symlink() for p in (source_root, *source_root.parents)), 'symlink source root')
    regular_file(SOURCE_MANIFEST, MAX_ARCHIVE_BYTES)
    require(digest(SOURCE_MANIFEST) == SOURCE_MANIFEST_SHA256, 'unreviewed source identity manifest')
    manifest = json.loads(SOURCE_MANIFEST.read_text())
    require(manifest['schema_version'] == 1 and manifest['source_recipe'] == TWO_LUT_SOURCE
            and manifest['file_count'] == 55 and len(manifest['files']) == 55,
            'unexpected two-LUT source identity')
    vendor = source_root / 'vendor/bevy_core_pipeline'
    require(vendor.is_dir() and not vendor.is_symlink(), 'missing or aliased vendor root')
    observed = []
    entries = []
    for count, path in enumerate(vendor.rglob('*'), 1):
        require(count <= 128, 'oversized vendor boundary')
        entries.append(path)
    expected_dirs = {parent.as_posix() for item in manifest['files']
                     for parent in Path(item['path']).parents if parent != Path('.')}
    require({p.relative_to(vendor).as_posix() for p in entries if p.is_dir()} == expected_dirs,
            'unexpected directory in source boundary')
    for path in sorted(entries):
        require(not path.is_symlink(), f'symlink in source boundary: {path}')
        if path.is_dir():
            continue
        regular_file(path, MAX_ARCHIVE_BYTES)
        observed.append({'path': path.relative_to(vendor).as_posix(), 'bytes': path.stat().st_size,
                         'sha256': digest(path)})
    require(observed == manifest['files'], 'exact 55-file vendor source boundary differs from admitted identity')
    root_manifest = regular_file(source_root / 'Cargo.toml', MAX_EVIDENCE_BYTES)
    cargo = tomllib.loads(root_manifest.read_text())
    require(cargo['patch']['crates-io']['bevy_core_pipeline'] == {'path': 'vendor/bevy_core_pipeline'},
            'wrong core-pipeline Cargo patch route')
    lock = regular_file(source_root / 'Cargo.lock', MAX_EVIDENCE_BYTES)
    packages = [p for p in tomllib.loads(lock.read_text())['package'] if p['name'] == 'bevy_core_pipeline']
    require(len(packages) == 1 and packages[0]['version'] == '0.18.1'
            and 'source' not in packages[0] and 'checksum' not in packages[0],
            'lockfile disguises local core-pipeline source')
    package = tomllib.loads((vendor / 'Cargo.toml').read_text())
    require(package['package']['name'] == 'bevy_core_pipeline' and package['package']['version'] == '0.18.1',
            'wrong local package identity')
    for name in ('TonyMcMapface-LICENSE-MIT', 'Blender-Filmic-OpenColorIO-LICENSE', 'Blender-Filmic-NOTICE.txt'):
        notice = regular_file(source_root / 'docs/release/licenses' / name, MAX_ARCHIVE_BYTES)
        require(notice.read_bytes() == (vendor / 'third-party-notices' / name).read_bytes(), 'changed asset notice')
    return manifest, vendor


def validate_source(source_root, upstream_archive):
    """Attest the exact local source identity, with no build or release claim."""
    manifest, vendor = source_inventory(source_root)
    archive = Path(upstream_archive)
    require(not archive.is_relative_to(Path(source_root)), 'private reference archive must be outside shipped source')
    provenance = json.loads((vendor / 'FLIGHTSIM-UPSTREAM-SOURCE.json').read_text())
    require(provenance['upstream_archive_sha256'] == manifest['upstream_archive_sha256'], 'archive identity mismatch')
    originals = read_reference_archive(archive, manifest['upstream_archive_sha256'], provenance['original_files'])
    agx = originals['src/tonemapping/luts/' + AGX]
    require(hashlib.sha256(agx).hexdigest() == LUTS[AGX], 'unexpected AgX reference payload')
    for item in manifest['files']:
        path = vendor / item['path']
        data = path.read_bytes()
        require(agx not in data, 'excluded AgX payload present in source: ' + item['path'])
        if item['path'] in originals and item['path'] != 'src/tonemapping/mod.rs':
            require(data == originals[item['path']], 'retained original source changed')
    return {'source_recipe': TWO_LUT_SOURCE, 'source_root': str(source_root), 'vendor_root': str(vendor),
            'manifest_sha256': SOURCE_MANIFEST_SHA256, 'vendor_files': manifest['files'],
            'vendor_file_count': len(manifest['files']),
            'cargo_toml_sha256': digest(Path(source_root) / 'Cargo.toml'),
            'cargo_lock_sha256': digest(Path(source_root) / 'Cargo.lock'), 'upstream_archive_sha256': manifest['upstream_archive_sha256'],
            'upstream_archive_bytes': archive.stat().st_size, 'original_member_count': len(originals),
            'agx_full_payload_absent': True, 'release_admitted': False, 'native_qualified': False}


def path_package_identity(package_id, directory, name, version):
    """Accept Cargo's named or directory-implied fragment, only at this root."""
    require(package_id.startswith('path+file://'), 'core/app package is not the admitted Cargo path package')
    parsed = urlsplit(package_id[len('path+'):])
    require(parsed.scheme == 'file' and not parsed.netloc and not parsed.query, 'invalid Cargo path package URI')
    expected = directory.as_uri()[len('file://'):]
    require(parsed.path == expected and unquote(parsed.path) == unquote(expected), 'Cargo path package root differs from admitted source')
    fragments = {f'{name}@{version}'}
    if directory.name == name:
        fragments.add(version)
    require(parsed.fragment in fragments, 'Cargo path package name/version differs from graph')


def validate_source_graph(graph, source_root):
    paths = {'bevy_core_pipeline': source_root / 'vendor/bevy_core_pipeline',
             'flightsim-app': source_root / 'crates/flightsim-app',
             'flightsim-render': source_root / 'crates/flightsim-render'}
    for name, path in paths.items():
        # App is absent when the selected root is the render example. Every
        # local package actually present must still name its exact source.
        if name in graph:
            require(graph[name].get('sources') == {str(path)},
                    f'graph {name} route differs from admitted local source')


def depinfo_sources(depinfo, library, vendor, mode):
    text = regular_file(depinfo, MAX_EVIDENCE_BYTES).read_text().replace('\\\n', '')
    rules = []
    for line in text.splitlines():
        if ': ' not in line:
            continue
        lhs, rhs = line.split(': ', 1)
        if not rhs.strip():
            continue
        # Make escaping, including Windows drive paths, without shell evaluation.
        tokens = re.findall(r'(?:\\[ \t#:\\]|[^ \t])+', rhs)
        values = [re.sub(r'\\([ \t#:\\])', r'\1', token) for token in tokens]
        target = re.sub(r'\\([ \t#:\\])', r'\1', lhs)
        allowed = {str(depinfo), str(library), str(library.with_suffix('.rmeta'))}
        require(target in allowed, 'dep-info belongs to a different artifact')
        rules.append(set(values))
    require(rules and all(values == rules[0] for values in rules), 'missing or inconsistent dep-info rules')
    # Cargo emits checkout-relative paths for the local patched dependency.
    # Bind them to the admitted checkout, never the auditor's ambient CWD.
    source_root = vendor.parents[1]
    dependencies = set()
    for value in rules[0]:
        path = Path(value)
        require('..' not in path.parts, 'traversal in core-pipeline source dependency')
        path = path if path.is_absolute() else source_root / path
        require(path.is_relative_to(vendor), 'foreign core-pipeline source dependency')
        require(path.is_file() and path.resolve() == path, 'missing or aliased core-pipeline source dependency')
        dependencies.add(str(path))
    require(str(vendor / 'src/lib.rs') in dependencies and str(vendor / 'src/tonemapping/mod.rs') in dependencies,
            'dep-info does not compile admitted core-pipeline source')
    expected = {str(vendor / 'src/tonemapping/luts' / name) for name in RETAINED_LUTS} if mode == 'ordinary' else set()
    observed = {p for p in dependencies if '/tonemapping/luts/' in p.replace('\\', '/')}
    require(observed == expected, 'exact LUT dep-info differs from selected mode/source recipe')
    return sorted(observed)


def validate_executable_target(executable, target):
    require(target in TARGETS, 'unsupported or missing exact build target')
    with executable.open('rb') as stream:
        header = stream.read(64)
        if target == 'x86_64-unknown-linux-gnu':
            require(len(header) >= 20 and header[:6] == b'\x7fELF\x02\x01' and header[18:20] == b'\x3e\x00',
                    'executable is not the expected Linux x86-64 ELF')
        else:
            require(len(header) == 64 and header[:2] == b'MZ', 'executable is not a Windows PE')
            offset = int.from_bytes(header[60:64], 'little')
            require(64 <= offset <= min(executable.stat().st_size - 26, MAX_EVIDENCE_BYTES), 'invalid PE header offset')
            stream.seek(offset)
            pe = stream.read(26)
            require(pe[:6] == b'PE\x00\x00\x64\x86' and pe[24:26] == b'\x0b\x02',
                    'executable is not the expected Windows x86-64 PE32+')


def parse_graph(text):
    graph = {}
    for line in text.splitlines():
        match = re.fullmatch(r'([\w-]+) v(\S+)(?: \(([^\n]*)\))? features=\[([^]]*)\](?: \(\*\))?', line)
        require(match is not None, f'unrecognized graph line: {line!r}')
        name, version, source, features = match.groups()
        key = name if name in KEY_CRATES or name.startswith('flightsim-') else f'{name}@{version}'
        entry = graph.setdefault(key, {'version': version, 'features': set()})
        require(entry['version'] == version, f'multiple versions of {name}; review graph explicitly')
        entry['features'].update(filter(None, features.split(',')))
        entry.setdefault('sources', set()).add(source)
    require(graph, 'empty graph')
    return graph


def validate_graph(graph, mode, kind, *, recipe=None):
    require(mode in ('ordinary', 'analytic') and kind in ('app', 'sun-clock'), 'unknown tone mode or target kind')
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


def select_artifacts(messages, graph, kind, *, source_recipe=UPSTREAM_THREE_LUT_SOURCE, source_root=None):
    require(source_recipe in (UPSTREAM_THREE_LUT_SOURCE, TWO_LUT_SOURCE), 'unknown source recipe')
    require(source_recipe == TWO_LUT_SOURCE or source_root is None, 'source root requires explicit two-LUT source recipe')
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
    modern = source_recipe == TWO_LUT_SOURCE
    if modern:
        require(source_root is not None, 'two-LUT recipe requires source root')
        source_root = Path(source_root)
        path_package_identity(root_id, source_root / 'crates' / package, package, version)
        expected_src = source_root / ('crates/flightsim-app/src/main.rs' if kind == 'app' else 'crates/flightsim-render/examples/sun_clock.rs')
        require(Path(root['target']['src_path']) == expected_src, 'foreign executable source route')
        executable_path = Path(root['executable'])
        require(all(Path(p) == executable_path or
                    (Path(p).parent == executable_path.parent and Path(p).suffix in ('.pdb', '.dwp'))
                    for p in root['filenames']), 'unexpected or cross-target executable filenames')
        require(len([a for a in artifacts if a.get('executable')]) == 1, 'unexpected or ambiguous executable artifact')
        require(not any(m.get('reason') == 'compiler-message' and
                        m.get('message', {}).get('level') in ('error', 'failure-note', 'warning') for m in messages),
                'build contains rejected compiler diagnostics')
        require([m for m in messages if m.get('reason') == 'build-finished'] ==
                [{'reason': 'build-finished', 'success': True}], 'contradictory build completion')
    selected = {}
    for name in (*KEY_CRATES, *(['flightsim_render'] if modern else [])):
        graph_name = name.replace('_', '-') if name == 'flightsim_render' else name
        matches = [m for m in artifacts if m['target']['name'] == name
                   and m['target']['kind'] == ['lib']]
        require(len(matches) == 1, f'expected one compiled {name} library')
        require(set(matches[0]['features']) == graph[graph_name]['features'], f'{name} compiled features differ from graph')
        if modern and name in ('bevy_core_pipeline', 'flightsim_render'):
            directory = source_root / ('vendor/bevy_core_pipeline' if name == 'bevy_core_pipeline' else 'crates/flightsim-render')
            path_package_identity(matches[0]['package_id'], directory, graph_name, graph[graph_name]['version'])
            require(Path(matches[0]['target']['src_path']) == directory / 'src/lib.rs', 'foreign compiled source route')
        else:
            require(matches[0]['package_id'].endswith(f'#{name}@' + graph[graph_name]['version']),
                    f'{name} compiled version differs from graph')
        if modern:
            require(matches[0].get('profile', {}).get('test') is False, 'test library is not a build artifact')
        selected[name] = matches[0]
    return root, selected


def audit(graph_path, messages_path, mode, kind, *, recipe=None,
          source_recipe=UPSTREAM_THREE_LUT_SOURCE, source_root=None, upstream_archive=None, target=None):
    require(source_recipe in (UPSTREAM_THREE_LUT_SOURCE, TWO_LUT_SOURCE), 'unknown source recipe')
    modern = source_recipe == TWO_LUT_SOURCE
    source_report = None
    if modern:
        require(source_root is not None and upstream_archive is not None and target in TARGETS,
                'two-LUT recipe requires source root, private upstream archive and exact target')
        source_root, upstream_archive = Path(source_root), Path(upstream_archive)
        source_report = validate_source(source_root, upstream_archive)
        regular_file(graph_path, MAX_EVIDENCE_BYTES)
        regular_file(messages_path, MAX_EVIDENCE_BYTES)
    else:
        require(source_root is None and upstream_archive is None and target is None,
                'source/target arguments require the explicit two-LUT recipe')
    graph = parse_graph(graph_path.read_text())
    validate_graph(graph, mode, kind, recipe=recipe)
    messages = [json.loads(line) for line in messages_path.read_text().splitlines()]
    if modern:
        validate_source_graph(graph, source_root)
    root, selected = select_artifacts(messages, graph, kind, source_recipe=source_recipe, source_root=source_root)
    executable = Path(root['executable'])
    require(executable.is_absolute(), 'expected the absolute executable path reported by Cargo')
    profile_dir = executable.parent if kind == 'app' else executable.parent.parent
    if modern:
        regular_file(executable, MAX_BINARY_BYTES)
        require(profile_dir.parent.name == target and profile_dir.name in ('debug', 'release'), 'executable belongs to a different target/profile')
        validate_executable_target(executable, target)
    result = {'mode': mode, 'kind': kind, 'graph_sha256': digest(graph_path),
              'messages_sha256': digest(messages_path), 'executable': str(executable),
              'package_id': root['package_id'], 'features': root['features'], 'profile': root['profile'],
              'executable_sha256': digest(executable), 'executable_bytes': executable.stat().st_size,
              'compiled': [], 'payloads': []}
    for name, artifact in selected.items():
        graph_name = name.replace('_', '-') if name == 'flightsim_render' else name
        libraries = [Path(p) for p in artifact['filenames'] if p.endswith('.rlib')]
        require(len(libraries) == 1, f'expected one linked {name} rlib')
        library = libraries[0]
        if modern:
            regular_file(library, MAX_BINARY_BYTES)
            require(all(Path(p).is_absolute() and Path(p).parent == library.parent
                        for p in artifact['filenames']), 'cross-target library artifact filenames')
        require(library.parent.resolve() == (profile_dir / 'deps').resolve(),
                f'{name} is from a different target directory than the executable')
        stem = library.stem.removeprefix('lib')
        suffix = stem.removeprefix(name + '-')
        if modern:
            require(re.fullmatch(r'[0-9a-f]+', suffix) is not None, 'invalid library artifact hash')
        require(suffix != stem, f'unrecognized rlib name {library}')
        fingerprint = library.parent.parent / '.fingerprint' / f'{graph_name}-{suffix}' / f'lib-{name}.json'
        if modern:
            regular_file(fingerprint, MAX_EVIDENCE_BYTES)
        fp = json.loads(fingerprint.read_text())
        require(set(json.loads(fp['features'])) == graph[graph_name]['features'], f'{name} fingerprint differs from graph')
        require(fp.get('rustflags') == ['-D', 'warnings'], f'{name} was not compiled with warnings denied')
        result['compiled'].append({'package': name, 'version': graph[graph_name]['version'],
                                  'features': sorted(graph[graph_name]['features']),
                                  'rlib': str(library), 'rlib_sha256': digest(library),
                                  'fingerprint': str(fingerprint), 'fingerprint_sha256': digest(fingerprint)})
        if modern:
            result['compiled'][-1].update(package_id=artifact['package_id'], source_path=artifact['target']['src_path'])
        if name == 'bevy_core_pipeline':
            depinfo = library.with_name(stem + '.d')
            if modern:
                dependencies = depinfo_sources(depinfo, library, source_root / 'vendor/bevy_core_pipeline', mode)
            has_luts = '/tonemapping/luts/' in depinfo.read_text().replace('\\', '/')
            require(has_luts == (mode == 'ordinary'), 'core-pipeline source dependencies disagree with mode')
            result['core_pipeline_depinfo'] = {'path': str(depinfo), 'sha256': digest(depinfo), 'has_luts': has_luts}
    lut_dir = Path(selected['bevy_core_pipeline']['target']['src_path']).parent / 'tonemapping/luts'
    if modern:
        root_package = 'flightsim-app' if kind == 'app' else 'flightsim-render'
        root_name = 'bin-flightsim-app' if kind == 'app' else 'example-sun_clock'
        root_fps = list((profile_dir / '.fingerprint').glob(root_package + '-*/' + root_name + '.json'))
        require(len(root_fps) == 1, 'expected exactly one executable fingerprint')
        fp_path = regular_file(root_fps[0], MAX_EVIDENCE_BYTES)
        fp = json.loads(fp_path.read_text())
        require(set(json.loads(fp['features'])) == graph[root_package]['features'] and fp.get('rustflags') == ['-D', 'warnings'],
                'executable fingerprint differs from graph/flags')
        result['executable_fingerprint'] = {'path': str(fp_path), 'sha256': digest(fp_path)}
        result['source'] = source_report
        result['source_recipe'] = source_recipe
        result['target'] = target
        result['release_admitted'] = False
        result['native_qualified'] = False
        result['core_pipeline_depinfo']['lut_dependencies'] = dependencies
        provenance = json.loads((source_root / 'vendor/bevy_core_pipeline/FLIGHTSIM-UPSTREAM-SOURCE.json').read_text())
        originals = read_reference_archive(upstream_archive, source_report['upstream_archive_sha256'], provenance['original_files'])
    with executable.open('rb') as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        for name, expected in LUTS.items():
            path = lut_dir / name
            payload = originals['src/tonemapping/luts/' + name] if modern and name == AGX else path.read_bytes()
            require(hashlib.sha256(payload).hexdigest() == expected, f'unreviewed source LUT bytes: {name}')
            offset = data.find(payload)
            present = mode == 'ordinary' and (not modern or name in RETAINED_LUTS)
            require((offset >= 0) == present, f'{name} payload presence disagrees with mode/source recipe')
            result['payloads'].append({'name': name, 'source_sha256': expected,
                                       'bytes': len(payload), 'full_bytes_offset': offset})
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
    parser.add_argument('--source-recipe', choices=[UPSTREAM_THREE_LUT_SOURCE, TWO_LUT_SOURCE], default=UPSTREAM_THREE_LUT_SOURCE)
    parser.add_argument('--source-root', type=Path)
    parser.add_argument('--upstream-archive', type=Path, help='Private exact upstream .crate, outside shipped source')
    parser.add_argument('--target', choices=TARGETS)
    args = parser.parse_args()
    try:
        result = audit(args.graph, args.messages, args.mode, args.kind, source_recipe=args.source_recipe,
                       source_root=args.source_root, upstream_archive=args.upstream_archive, target=args.target)
    except (ValueError, OSError, KeyError, TypeError, EOFError, tarfile.TarError) as error:
        print(f'tonemapping audit failed: {error}', file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
