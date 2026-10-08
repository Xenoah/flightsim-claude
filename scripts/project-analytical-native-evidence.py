#!/usr/bin/env python3
"""Bounded native review projection. Source material stays private; no approval.

PE parsing follows https://learn.microsoft.com/en-us/windows/win32/debug/pe-format
Only import DLL basenames are projected, never symbols, strings or binary bytes.
Cargo linker requests and compiled artifacts do not prove final static inclusion.
"""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import re
import struct

SPEC = importlib.util.spec_from_file_location('native_capture', Path(__file__).with_name('capture-analytical-swift-msvc.py'))
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)
check, require = capture.check, capture.require
MAX_BYTES = 2 * 1024 * 1024
HEADER_MANIFEST = 'docs/release/analytical-source-header-evidence.json'
MODIFIED_MANIFEST = 'docs/release/analytical-modified-source-provenance.json'
# Identities independently recorded in the original exact-package review. These
# are origins only, not grants or assertions that a modified tree is upstream.
REVIEWED_PATCH_ORIGINS = {
    'bevy_pbr@0.18.1': ('a5ab6944ffc6fd71604c0fbca68cc3e2a3654edfcdbfd232f9d8b88e3d20fdc0', 'f667c282dad2c1419afb5836ded22a3ec263970e'),
    'zune-jpeg@0.5.15': ('27bc9d5b815bc103f142aa054f561d9187d191692ec7c2d1e2b4737f8dbd7296', '31d81fed7551c8ccea456d9d8e2b1fd8bebb6995'),
}


def token(value, pattern=r'[A-Za-z0-9_.+@-]+', maximum=160):
    require(isinstance(value, str) and 0 < len(value) <= maximum and re.fullmatch(pattern, value), 'unbounded or unsafe evidence token')
    return value


def relative(value):
    token(value, r'[A-Za-z0-9_.+/@-]+', 320)
    require(not value.startswith('/') and all(part not in ('', '.', '..') for part in value.split('/')), 'unsafe source-relative evidence path')
    return value


def features(values):
    require(isinstance(values, list) and len(values) <= 256 and len(values) == len(set(values)), 'invalid feature list')
    return sorted(token(item, r'[A-Za-z0-9_.+:/@-]+') for item in values)


def modified_source(repo, package, metadata, source):
    """Path patches are neither registry bytes nor ordinary workspace members."""
    identity = package['name'] + '@' + package['version']
    path = repo / MODIFIED_MANIFEST
    if package.get('source') is not None or package['id'] in metadata['workspace_members']:
        if identity in REVIEWED_PATCH_ORIGINS and path.is_file():
            declared = capture.read_private_json(path)
            require(not any(row.get('id') == identity for row in declared.get('packages', [])),
                    'declared vendored package resolved as registry or workspace source')
        return None
    require(MODIFIED_MANIFEST in {item['path'] for item in source['files']}, 'path-patched crate needs tracked modified-source provenance')
    manifest = capture.read_private_json(path)
    require(set(manifest) == {'schema_version', 'packages'} and type(manifest['schema_version']) is int
            and manifest['schema_version'] == 1 and 0 < len(manifest['packages']) <= 16, 'invalid modified-source manifest')
    rows = [item for item in manifest['packages'] if item.get('id') == identity]
    require(len(rows) == 1, 'ambiguous or absent modified-source record')
    row = rows[0]
    require(set(row) == {'id', 'source_root', 'upstream', 'patch', 'files', 'modified_paths', 'retained_notices'},
            'unknown modified-source field')
    root = repo / relative(row['source_root'])
    require(row['source_root'] == 'vendor/' + package['name'], 'modified package is outside its reviewed canonical vendor location')
    native_manifest = Path(package['manifest_path'])
    capture.no_links(native_manifest)
    require(native_manifest.is_file() and native_manifest.resolve() == root.resolve() / 'Cargo.toml',
            'modified package must resolve to its exact canonical vendor/Cargo.toml')
    prefix = 'path+' + root.resolve().as_uri() + '#'
    require(package['id'] in (prefix + package['version'], prefix + identity),
            'modified package has a foreign Cargo path-source identity')
    upstream = row['upstream']
    require(set(upstream) == {'registry_checksum', 'revision'} and capture.hex_string(upstream['registry_checksum'], 64)
            and (upstream['revision'] is None or capture.hex_string(upstream['revision'], 40)), 'missing original registry identity')
    require(REVIEWED_PATCH_ORIGINS.get(identity) == (upstream['registry_checksum'], upstream['revision']),
            'path-patch origin differs from the separately reviewed original package identity')
    patch = row['patch']
    require(set(patch) == {'path', 'sha256', 'bytes'}, 'invalid reviewed replacement patch binding')
    patch_path = repo / relative(patch['path'])
    require(patch['path'] in {item['path'] for item in source['files']}
            and capture.file_record(patch_path) == {key: patch[key] for key in ('sha256', 'bytes')}, 'replacement patch bytes changed')
    require(isinstance(row['files'], list) and 0 < len(row['files']) <= 4096, 'unbounded vendored file manifest')
    wanted = {}
    for item in row['files']:
        require(set(item) == {'path', 'sha256', 'bytes'}, 'unknown vendored file field')
        name = relative(item['path'])
        require(name not in wanted and capture.valid_record({key: item[key] for key in ('sha256', 'bytes')}), 'invalid vendored file record')
        wanted[name] = {key: item[key] for key in ('sha256', 'bytes')}
        require((Path(row['source_root']) / name).as_posix() in {item['path'] for item in source['files']}, 'untracked vendored source')
    actual = capture.snapshot_tree(root)
    require(actual == wanted, 'vendored tree differs from complete reviewed manifest')
    for field in ('modified_paths', 'retained_notices'):
        require(isinstance(row[field], list) and 0 < len(row[field]) <= 4096
                and len(row[field]) == len(set(row[field])) and set(row[field]) <= set(wanted), 'invalid modified/notice path set')
    import hashlib
    tree = hashlib.sha256((json.dumps(actual, sort_keys=True, separators=(',', ':')) + '\n').encode('ascii')).hexdigest()
    return {'identity': 'modified_vendored_source', 'manifest': capture.file_record(path),
            'original_registry_checksum': upstream['registry_checksum'], 'original_upstream_revision': upstream['revision'],
            'replacement_patch': patch, 'source_root': row['source_root'], 'files': row['files'], 'tree_sha256': tree,
            'modified_paths': row['modified_paths'], 'retained_notices': row['retained_notices'],
            'upstream_registry_bytes_are_current_source': False, 'source_and_license_review_approved': False}


def pe_imports(path):
    capture.no_links(path)
    require(path.is_file() and 128 <= path.stat().st_size <= 512 * 1024 * 1024, 'invalid PE file size')
    data = path.read_bytes()

    def unpack(fmt, offset):
        require(0 <= offset <= len(data) - struct.calcsize(fmt), 'PE structure outside file')
        return struct.unpack_from(fmt, data, offset)

    require(data[:2] == b'MZ', 'missing DOS header')
    pe, = unpack('<I', 0x3c)
    require(data[pe:pe + 4] == b'PE\0\0', 'missing PE signature')
    machine, count, _, _, _, optional_size, _ = unpack('<HHIIIHH', pe + 4)
    require(machine == 0x8664 and 1 <= count <= 96, 'not bounded native x64 PE')
    optional = pe + 24
    require(optional_size >= 240 and unpack('<H', optional)[0] == 0x20b, 'not native PE32+')
    require(unpack('<I', optional + 108)[0] == 16, 'unexpected PE data-directory count')
    subsystem, = unpack('<H', optional + 68)
    require(subsystem in (2, 3), 'unsupported executable subsystem')
    header_size, = unpack('<I', optional + 60)
    require(optional + optional_size + count * 40 <= header_size <= len(data), 'invalid PE header range')
    sections = []
    for index in range(count):
        offset = optional + optional_size + index * 40
        virtual_size, rva, raw_size, raw_offset = unpack('<IIII', offset + 8)
        require(raw_offset + raw_size <= len(data), 'section raw bytes outside PE')
        sections.append((rva, max(raw_size, virtual_size), raw_offset, raw_size))

    def at(rva, length):
        require(rva > 0 and length > 0, 'invalid RVA')
        if rva + length <= header_size:
            return rva
        matches = [(raw + rva - start, raw_size - (rva - start))
                   for start, size, raw, raw_size in sections if start <= rva < start + size]
        require(len(matches) == 1 and length <= matches[0][1], 'ambiguous or unbacked PE RVA')
        return matches[0][0]

    def dll(rva):
        letters = bytearray()
        for index in range(128):
            byte = data[at(rva + index, 1)]
            if byte == 0:
                name = token(letters.decode('ascii'), r'[A-Za-z0-9_.-]+', 127).lower()
                require(name.endswith('.dll'), 'import name is not a DLL basename')
                return name
            letters.append(byte)
        raise ValueError('unterminated DLL basename')

    def directory(index, delay=False):
        rva, size = unpack('<II', optional + 112 + index * 8)
        if rva == size == 0:
            return []
        width = 32 if delay else 20
        require(rva > 0 and width <= size <= 256 * width, 'unbounded import directory')
        names = []
        for index in range(size // width):
            offset = at(rva + index * width, width)
            fields = unpack('<' + 'I' * (width // 4), offset)
            if not any(fields):
                return sorted(set(names))
            if delay:
                require(fields[0] == 1, 'unsupported delay-import address mode')
            names.append(dll(fields[1] if delay else fields[3]))
        raise ValueError('import directory lacks a bounded terminator')

    return {'machine': 'amd64', 'format': 'PE32+', 'subsystem': 'windows_gui' if subsystem == 2 else 'windows_console',
            'ordinary_imports': directory(1), 'delay_imports': directory(13, True),
            'dynamic_loads': 'not_established', 'static_contributions': 'not_established'}


def headers(repo, packages, source):
    manifest = repo / HEADER_MANIFEST
    if not manifest.exists():
        return {'status': 'not_established', 'manifest': None, 'records': []}
    require(HEADER_MANIFEST in {item['path'] for item in source['files']}, 'header evidence manifest must be tracked')
    value = capture.read_private_json(manifest)
    require(set(value) == {'schema_version', 'records'} and type(value['schema_version']) is int
            and value['schema_version'] == 1 and 0 < len(value['records']) <= 512, 'invalid reviewed header evidence manifest')
    by_id = {package['name'] + '@' + package['version']: package for package in packages.values()}
    results = []
    for row in value['records']:
        require(set(row) == {'package_id', 'source_relative_path', 'source_sha256', 'start_byte', 'byte_length', 'excerpt_sha256'},
                'unknown source-header evidence field')
        package = by_id[token(row['package_id'])]
        root = Path(package['manifest_path']).parent
        path = root / relative(row['source_relative_path']); capture.no_links(path)
        require(path.is_file() and path.stat().st_size <= 8 * 1024 * 1024 and check.digest(path) == row['source_sha256'], 'source-header bytes changed')
        start, length = row['start_byte'], row['byte_length']
        require(type(start) is int and type(length) is int and start >= 0 and 0 < length <= 64 * 1024
                and start + length <= path.stat().st_size, 'invalid header excerpt range')
        import hashlib
        excerpt = path.read_bytes()[start:start + length]
        require(hashlib.sha256(excerpt).hexdigest() == row['excerpt_sha256'], 'source-header excerpt changed')
        results.append(row)
    return {'status': 'exact_spans_observed_not_exhaustive', 'manifest': capture.file_record(manifest), 'records': results}



def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


runtime_facts = load("native_runtime_facts", "collect-analytical-runtime-facts.py")
ui_capabilities = load("native_ui_capabilities", "observe-analytical-ui-capabilities.py")


def validate_runtime_facts(private, repo, expected, build_private, build_text):
    build = capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    require(build['status'] == capture.PASS, 'runtime facts require exact frozen native build evidence')
    directory = private
    value = runtime_facts.project(directory)
    require(value['source_sha'] == expected and value['target'] == check.TARGET, 'runtime facts source/target mismatch')
    captured = (build_private / 'capture/analytic/rustc.txt').read_text(encoding='utf-8')
    identity = runtime_facts.compiler_identity(captured)
    if value['rust']['identity']['status'] == 'observed':
        require(value['rust']['identity'] == identity, 'runtime compiler differs from exact audited build')
    manifest = json.loads((directory / runtime_facts.PRIVATE_NAME).read_text(encoding='utf-8'))
    require(manifest['recipe_cfg_args'] == ['-D', 'warnings'] and manifest['linker_inputs'] == {
        'trace': str(build_private / 'capture/analytic/build.stderr'),
        'executable': str(build_private / 'target-analytic' / check.TARGET / 'release/flightsim-app.exe')},
        'runtime facts are not bound to this exact build trace/executable')
    path = directory / runtime_facts.PROJECTION_NAME
    require(path.read_bytes() == runtime_facts.canonical(value), 'runtime fact projection changed')
    return value

def project(repo, expected, build_private, build_text, bundle, *, runtime_facts_private=None, ui_capabilities_private=None):
    verified = capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    require(verified['status'] == capture.PASS, 'native projection requires completed exact build audit')
    source = capture.source_evidence(repo, expected)
    directory = build_private / 'capture/analytic'
    metadata = json.loads((directory / 'metadata.json').read_text(encoding='utf-8'))
    inventory = json.loads((directory / 'notices/dependency-inventory.json').read_text(encoding='utf-8'))
    packages, nodes, ids = check.collector.closure(metadata, 'flightsim-app')
    require(len(ids) <= 1024, 'native package count exceeds review budget')
    names = {key: token(packages[key]['name'] + '@' + packages[key]['version']) for key in ids}
    graph = check.tone.parse_graph((directory / 'graph.txt').read_text(encoding='utf-8'))
    exact = {check.graph_identity(name, value): value for name, value in graph.items()}
    records, edges = [], []
    by_id = {row['id']: row for row in inventory['packages']}
    for key in ids:
        package, node = packages[key], nodes[key]
        row = by_id[names[key]]
        require(row['source'] in ('workspace', 'registry+https://github.com/rust-lang/crates.io-index'), 'unreviewed dependency source kind')
        checksum = row['source_checksum']
        require((row['source'] == 'workspace' and checksum is None) or capture.hex_string(checksum, 64), 'missing registry checksum')
        expression = row['license_expression']
        if expression is not None: token(expression, r'[A-Za-z0-9_.+() /:-]+', 512)
        notices = []
        require(len(row['notices']) <= 64, 'unbounded package notices')
        for notice in row['notices']:
            path = relative(notice['path'])
            actual = capture.file_record(directory / 'notices' / path)
            require(actual == {name: notice[name] for name in ('sha256', 'bytes')}, 'notice binding changed')
            notices.append({'path': path, 'upstream_path': relative(notice['upstream_path']), **actual})
        actual_graph = exact.get((package['name'], package['version']))
        revision = row['source_revision']
        require(revision is None or capture.hex_string(revision, 40), 'invalid source revision')
        modification = modified_source(repo, package, metadata, source)
        if modification is not None:
            require(revision in (None, modification['original_upstream_revision']), 'unexplained retained upstream VCS marker')
        records.append({'id': names[key], 'name': token(package['name']), 'version': token(package['version']),
                        'source_kind': 'modified_vendored' if modification else ('workspace' if row['source'] == 'workspace' else 'crates_io'),
                        'registry_checksum': checksum, 'source_revision': None if modification else revision,
                        'modified_source': modification, 'declared_license': expression,
                        'conservative_features': features(node['features']),
                        'exact_features': features(sorted(actual_graph['features'])) if actual_graph else None,
                        'notices': notices, 'review_status': 'not_reviewed'})
        for dependency in node['deps']:
            for kind in dependency['dep_kinds']:
                if kind['kind'] in (None, 'normal', 'build'):
                    require(dependency['pkg'] in names, 'edge outside conservative closure')
                    condition = kind['target']
                    if condition is not None: token(condition, r'[A-Za-z0-9_()=, ."+-]+', 512)
                    edges.append({'from': names[key], 'to': names[dependency['pkg']],
                                  'kind': kind['kind'] or 'normal', 'condition': condition})
    require(len(edges) <= 8192, 'native edge budget exceeded')
    messages = [json.loads(line) for line in (directory / 'messages.jsonl').read_text(encoding='utf-8').splitlines()]
    requests = []
    for message in messages:
        if message.get('reason') == 'build-script-executed':
            require(message['package_id'] in names, 'build-script package outside native closure')
            libraries = message.get('linked_libs', [])
            require(len(libraries) <= 128, 'unbounded build-script libraries')
            requests.append({'package_id': names[message['package_id']],
                             'linked_libs': [token(item, r'[A-Za-z0-9_.+:=,-]+') for item in libraries]})
    require(len(requests) <= 512, 'unbounded build-script requests')
    executable = bundle / 'flightsim-app.exe'
    require(capture.file_record(executable) == verified['builds']['analytic']['executable'], 'projected executable changed')
    native = [path for path in bundle.rglob('*') if path.is_file() and path.suffix.lower() in ('.exe', '.dll', '.lib', '.a', '.pdb', '.msi')]
    # The unchanged sole stager has no redistributed Microsoft runtime package.
    # Any future added native file requires a separate reviewed packaging route.
    require(set(native) == {executable}, 'unexpected redistributed native file')
    manifest = json.loads((bundle / 'bundle-manifest.json').read_text(encoding='utf-8'))
    require(len(manifest['files']) <= 2048, 'unbounded shipped file set')
    shipped = []
    for item in manifest['files']:
        path = relative(item['path']); actual = capture.file_record(bundle / path)
        require(actual == {key: item[key] for key in ('sha256', 'bytes')}, 'shipped file differs from manifest')
        shipped.append({'path': path, **actual})
    compiler = (directory / 'rustc.txt').read_text(encoding='utf-8')
    commit = re.findall(r'(?m)^commit-hash: ([0-9a-f]{40})$', compiler)
    require(len(commit) == 1, 'missing exact compiler revision')
    embedded = []
    require(len(inventory['embedded_assets']) <= 16, 'unbounded embedded asset set')
    for asset in inventory['embedded_assets']:
        require(asset['id'] == 'bevy-fira-mono' and asset['observed_sha256'] == asset['sha256'],
                'analytical dependency asset identity changed')
        notices = []
        for notice in asset['notices']:
            path = relative(notice['path']); actual = capture.file_record(directory / 'notices' / path)
            require(actual == {key: notice[key] for key in ('sha256', 'bytes')}, 'embedded asset notice changed')
            notices.append({'path': path, **actual})
        embedded.append({'id': asset['id'], 'package': token(asset['package']), 'version': token(asset['version']),
                         'feature': token(asset['feature']), 'source_path': relative(asset['source_path']),
                         'sha256': asset['sha256'], 'notices': notices, 'review_status': 'not_reviewed'})
    result = {'schema_version': 1, 'kind': 'native_review_projection_not_approval', 'source_sha': expected,
              'source_tree': source['source_tree'], 'target': check.TARGET, 'rust_release': check.TOOLCHAIN,
              'rust_commit': commit[0], 'rust_host': check.TARGET, 'profile': 'release',
              'app_features': check.FEATURES, 'default_features': False, 'region_downloads': False,
              'release_authorized': False, 'dependency_review_approved': False,
              'bindings': {'lock': capture.file_record(repo / 'Cargo.lock'), 'metadata': capture.file_record(directory / 'metadata.json'),
                           'inventory': capture.file_record(directory / 'notices/dependency-inventory.json'),
                           'graph': capture.file_record(directory / 'graph.txt'), 'messages': capture.file_record(directory / 'messages.jsonl'),
                           'executable': capture.file_record(executable), 'build_summary': capture.file_record(build_text / capture.EXPORT_NAME)},
              'packages': records, 'normal_build_edges': edges,
              'source_headers': headers(repo, {key: packages[key] for key in ids}, source),
              'embedded_assets': embedded,
              'build_script_link_requests': requests, 'build_script_requests_prove_static_inclusion': False,
              'pe': pe_imports(executable), 'shipped_files': shipped, 'redistributed_microsoft_native_files': [],
              'os_prerequisites': 'imports_observed_origin_and_minimum_version_require_review',
              'actual_static_contributions': 'not_established',
              'runtime_coverage': {'status': 'not_established',
                                   'conservative_complete_notices_can_suffice_without_exact_member_map': True,
                                   'rust_sysroot_notice_and_library_hashes': 'not_established',
                                   'resolved_linker_toolset_sdk_provenance': 'not_established'},
              'unresolved_reviews': ['nested_source_notice_completeness', 'runtime_and_platform_coverage', 'whole_target_dependency_review']}
    require((runtime_facts_private is None) == (ui_capabilities_private is None), 'incomplete runtime observation roots')
    if runtime_facts_private is not None:
        facts = validate_runtime_facts(runtime_facts_private, repo, expected, build_private, build_text)
        caps = ui_capabilities.project(ui_capabilities_private, repo, expected)
        result['bindings']['runtime_facts'] = capture.file_record(runtime_facts_private / runtime_facts.PROJECTION_NAME)
        result['bindings']['ui_capabilities'] = capture.file_record(ui_capabilities_private / ui_capabilities.EXPORT)
        require((ui_capabilities_private / ui_capabilities.EXPORT).read_bytes() ==
                (json.dumps(caps, indent=2, sort_keys=True) + '\n').encode('ascii'), 'capability projection changed')
        result['runtime_coverage']['factual_packet'] = 'collected_review_required'
        result['runtime_coverage']['rust_sysroot_notice_and_library_hashes'] = 'see_bound_factual_packet'
        result['runtime_coverage']['resolved_linker_toolset_sdk_provenance'] = 'see_bound_factual_packet_actual_sdk_selection_unknown'
        result['runtime_coverage']['constructed_final_link_command'] = facts['final_link']['status']
        result['runtime_coverage']['installed_candidates_are_actual_selection'] = False
        result['runtime_coverage']['dynamic_module_closure'] = 'not_established'
        result['ui_capability_states'] = {key: row['state'] for key, row in caps['probes'].items()}
    encoded = (json.dumps(result, indent=2, sort_keys=True) + '\n').encode('ascii')
    require(len(encoded) <= MAX_BYTES, 'native projection exceeds export budget')
    return result
