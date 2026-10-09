#!/usr/bin/env python3
"""Exact ordinary two-aircraft native facts, never a review or publication grant.

Reuse the admitted dual capture's ordinary side and reviewed low-level parsers.
Historical analytical projections and their acceptance semantics stay unchanged.
"""
from __future__ import annotations
import importlib.util
import json
from pathlib import Path
import re
import tomllib

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module

base = load('full_native_shared_parsers', 'project-analytical-native-evidence.py')
capture, check, require = base.capture, base.check, base.require
runtime_facts, ui_capabilities = base.runtime_facts, base.ui_capabilities
token, relative, features = base.token, base.relative, base.features
headers, pe_imports = base.headers, base.pe_imports
MAX_BYTES = base.MAX_BYTES
SOURCE_RECIPE = 'bevy-0.18.1-tony-filmic-source-v1'
EMBEDDED_IDS = {'bevy-fira-mono', 'bevy-tony-mc-mapface', 'bevy-blender-filmic-lut'}
FULL_BUNDLE_KIND = 'local-full-two-aircraft-windows-candidate-v1'
release = load('full_native_release_payload', 'check-release-authorization.py')


def validate_bundle(repo, bundle, verified, *, original_notices):
    """Recompute the exact full payload; a declared member list is not authority."""
    repo, bundle = Path(repo), Path(bundle)
    require(repo.is_absolute() and bundle.is_absolute() and '..' not in repo.parts
            and '..' not in bundle.parts, 'canonical absolute bundle/source paths required')
    capture.no_links(bundle)
    manifest = capture.read_private_json(bundle / 'bundle-manifest.json')
    expected_fields = {'schema_version', 'kind', 'release_authorized', 'source_binary_correspondence',
                       'distribution', 'dependency_target', 'rights_status', 'files', 'inventory_excludes_itself'}
    require(set(manifest) == expected_fields and type(manifest['schema_version']) is int
            and manifest['schema_version'] == 1 and manifest['kind'] == FULL_BUNDLE_KIND
            and manifest['release_authorized'] is False and manifest['inventory_excludes_itself'] is True
            and manifest['source_binary_correspondence'] == 'requires_exact_ordinary_capture_revalidation'
            and manifest['dependency_target'] == check.TARGET
            and manifest['rights_status'] in ('blocked', 'review_consumed_final_acceptance_required'),
            'wrong full bundle identity')
    require(isinstance(manifest['files'], list) and 0 < len(manifest['files']) <= 2048,
            'unbounded full bundle member set')
    wanted, folded = {}, set()
    for row in manifest['files']:
        require(isinstance(row, dict) and set(row) == {'path', 'sha256', 'bytes'}, 'invalid bundle member record')
        name = relative(row['path'])
        require(name != 'bundle-manifest.json' and name.casefold() not in folded
                and capture.valid_record({key: row[key] for key in ('sha256', 'bytes')}),
                'duplicate, colliding or malformed bundle member')
        folded.add(name.casefold()); wanted[name] = {key: row[key] for key in ('sha256', 'bytes')}
    actual = capture.snapshot_tree(bundle)
    actual.pop('bundle-manifest.json')
    require(actual == wanted, 'full bundle has absent, undeclared or changed members')
    require(actual.get('flightsim-app.exe') == verified['builds']['ordinary']['executable'],
            'full bundle executable differs from ordinary capture')
    inventory_path = bundle / 'third-party/dependency-inventory.json'
    require(capture.file_record(inventory_path) == verified['builds']['ordinary']['inventory'],
            'full bundle inventory differs from original ordinary capture')
    inventory = capture.read_private_json(inventory_path)
    review_path = bundle / release.DEPENDENCY_REVIEW
    review = capture.read_private_json(review_path) if review_path.exists() else None
    notice_names = release.staging.notice_files(inventory_path.parent, inventory, review)
    original_notices = Path(original_notices)
    require(original_notices.is_absolute() and '..' not in original_notices.parts,
            'canonical original notices required')
    capture.no_links(original_notices)
    require(capture.file_record(original_notices / 'dependency-inventory.json')
            == verified['builds']['ordinary']['inventory'], 'original notice inventory changed')
    original_names = release.staging.notice_files(original_notices, inventory)
    for name in original_names:
        path = release.staging.safe_file(original_notices, name)
        require(path.stat().st_nlink == 1
                and wanted.get('third-party/' + name) == capture.file_record(path),
                'bundled original notice differs from captured bytes')

    asset_manifest = capture.read_private_json(repo / release.ASSET_MANIFEST)
    source_names = set(release.SOURCE_FILES) | set(asset_manifest['required_bundle_files'])
    expected_names = source_names | {'third-party/' + name for name in notice_names}
    expected_names |= {'flightsim-app.exe', 'distribution-info.json', 'full-readiness.json'}
    if review is not None: expected_names.add(release.DEPENDENCY_REVIEW)
    require(set(wanted) == expected_names, 'declared bundle payload is outside the full source/notice allowlist')
    for name in source_names:
        require(wanted[name] == capture.file_record(release.staging.safe_file(repo, name)),
                'full bundled source differs from admitted source')
    report = release.readiness.check(repo, None, inventory_path, review_path if review is not None else None)
    blockers = [row for row in report['blockers'] if row['code'] != 'BUNDLE_NOT_CHECKED']
    require(all(row.get('category') == 'review' and row['code'] not in
                ('UNRESOLVED_ASSET_RIGHTS', 'UNAPPROVED_GEODATA') for row in blockers),
            'source or dependency integrity blocks full bundle revalidation')
    require(review is not None or bool(blockers), 'missing review cannot be readiness-complete')
    expected_readiness = {
                'schema_version': 1, 'status': 'blocked' if blockers else 'source_and_notice_checks_passed',
                'blockers': blockers, 'whole_target_review_present': review is not None,
                'release_authorized': False, 'runtime_accepted': False}
    require(json.dumps(capture.read_private_json(bundle / 'full-readiness.json'), sort_keys=True, allow_nan=False)
            == json.dumps(expected_readiness, sort_keys=True, allow_nan=False)
            and manifest['rights_status'] == ('blocked' if blockers else 'review_consumed_final_acceptance_required'),
            'full readiness differs from actual source and notice checks')
    distribution = capture.read_private_json(bundle / 'distribution-info.json')
    version = tomllib.loads((repo / 'Cargo.toml').read_text())['workspace']['package']['version']
    expected = {'schema_version': 1, 'package': 'flightsim-app', 'package_version': version,
                'profile': 'development', 'region_downloads': False, 'default_aircraft': 'light-single',
                'default_model': 'aircraft/light_single.glb', 'bundled_aircraft': ['light-single', 'swift-sport'],
                'target_os': 'windows', 'target_arch': 'x86_64', 'target_env': 'msvc', 'release_authorized': False}
    require(isinstance(distribution, dict) and set(distribution) == set(expected)
            and distribution == manifest['distribution'] and all(distribution.get(k) == v for k, v in expected.items())
            and type(distribution['schema_version']) is int and distribution['region_downloads'] is False
            and distribution['release_authorized'] is False, 'full bundle distribution identity changed')
    return manifest


def validate_build(repo, expected, build_private, build_text):
    result = capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    require(result['status'] == capture.PASS and result.get('source_recipe') == SOURCE_RECIPE,
            'full native projection requires explicit admitted two-LUT build')
    require(result['builds']['ordinary']['lut_payloads_found'] == 2,
            'ordinary build must contain exactly the two reviewed LUT payloads')
    return result


def modified_source(repo, package, metadata, source, upstream_archive):
    if package['name'] != 'bevy_core_pipeline':
        return base.modified_source(repo, package, metadata, source)
    require(package['version'] == '0.18.1' and package.get('source') is None
            and package['id'] not in metadata['workspace_members'], 'full core pipeline must be the exact reviewed path patch')
    root = repo / 'vendor/bevy_core_pipeline'
    path = Path(package['manifest_path']); capture.no_links(path)
    require(path.resolve() == root / 'Cargo.toml', 'core-pipeline manifest belongs to another source root')
    check.tone.path_package_identity(package['id'], root, 'bevy_core_pipeline', '0.18.1')
    observed = check.tone.validate_source(repo, upstream_archive)
    require(observed['source_recipe'] == SOURCE_RECIPE and observed['vendor_file_count'] == 55,
            'full core source audit differs from admitted recipe')
    tracked = {row['path'] for row in source['files']}
    require(all('vendor/bevy_core_pipeline/' + row['path'] in tracked for row in observed['vendor_files']),
            'untracked full core source')
    return {'identity': 'modified_vendored_source', 'source_recipe': SOURCE_RECIPE,
            'source_root': 'vendor/bevy_core_pipeline', 'files': observed['vendor_files'],
            'source_manifest_sha256': observed['manifest_sha256'],
            'original_registry_checksum': observed['upstream_archive_sha256'],
            'original_upstream_revision': 'f667c282dad2c1419afb5836ded22a3ec263970e',
            'provenance': capture.file_record(root / 'FLIGHTSIM-UPSTREAM-SOURCE.json'),
            'modification_notice': capture.file_record(root / 'FLIGHTSIM-MODIFICATION-NOTICE.txt'),
            'agx_payload_absent_from_exact_source': True,
            'upstream_registry_bytes_are_current_source': False, 'source_and_license_review_approved': False}

def validate_runtime_facts(private, repo, expected, build_private, build_text):
    build = validate_build(repo, expected, build_private, build_text)
    require(build['status'] == capture.PASS, 'runtime facts require exact frozen native build evidence')
    directory = private
    value = runtime_facts.project(directory)
    require(value['source_sha'] == expected and value['target'] == check.TARGET, 'runtime facts source/target mismatch')
    captured = (build_private / 'capture/ordinary/rustc.txt').read_text(encoding='utf-8')
    identity = runtime_facts.compiler_identity(captured)
    if value['rust']['identity']['status'] == 'observed':
        require(value['rust']['identity'] == identity, 'runtime compiler differs from exact audited build')
    manifest = json.loads((directory / runtime_facts.PRIVATE_NAME).read_text(encoding='utf-8'))
    require(manifest['recipe_cfg_args'] == ['-D', 'warnings'] and manifest['linker_inputs'] == {
        'trace': str(build_private / 'capture/ordinary/build.stderr'),
        'executable': str(build_private / 'target-ordinary' / check.TARGET / 'release/flightsim-app.exe')},
        'runtime facts are not bound to this exact build trace/executable')
    path = directory / runtime_facts.PROJECTION_NAME
    require(path.read_bytes() == runtime_facts.canonical(value), 'runtime fact projection changed')
    return value

def project(repo, expected, build_private, build_text, bundle, *, runtime_facts_private=None, ui_capabilities_private=None):
    verified = validate_build(repo, expected, build_private, build_text)
    require(verified['status'] == capture.PASS, 'native projection requires completed exact build audit')
    source = capture.source_evidence(repo, expected)
    validate_bundle(repo, bundle, verified, original_notices=build_private / 'capture/ordinary/notices')
    directory = build_private / 'capture/ordinary'
    metadata = json.loads((directory / 'metadata.json').read_text(encoding='utf-8'))
    inventory = json.loads((directory / 'notices/dependency-inventory.json').read_text(encoding='utf-8'))
    packages, nodes, ids = check.collector.closure(metadata, 'flightsim-app')
    require(len(ids) <= 1024, 'native package count exceeds review budget')
    app = [package for package in inventory['packages'] if package['name'] == 'flightsim-app']
    require(len(app) == 1 and app[0]['features'] == ['default'], 'full inventory uses another application recipe')
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
        modification = modified_source(repo, package, metadata, source, build_private / 'reference/bevy_core_pipeline-0.18.1.crate')
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
    require(capture.file_record(executable) == verified['builds']['ordinary']['executable'], 'projected executable changed')
    native = [path for path in bundle.rglob('*') if path.is_file() and path.suffix.lower() in ('.exe', '.dll', '.lib', '.a', '.pdb', '.msi')]
    # The named full stager has no separately redistributed Microsoft native package.
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
    require({row['id'] for row in inventory['embedded_assets']} == EMBEDDED_IDS
            and len(inventory['embedded_assets']) == len(EMBEDDED_IDS), 'full embedded asset set must be exactly Fira/Tony/Filmic')
    for asset in inventory['embedded_assets']:
        require(asset['id'] in EMBEDDED_IDS and asset['observed_sha256'] == asset['sha256'],
                'full dependency asset identity changed')
        notices = []
        for notice in asset['notices']:
            path = relative(notice['path']); actual = capture.file_record(directory / 'notices' / path)
            require(actual == {key: notice[key] for key in ('sha256', 'bytes')}, 'embedded asset notice changed')
            notices.append({'path': path, **actual})
        embedded.append({'id': asset['id'], 'package': token(asset['package']), 'version': token(asset['version']),
                         'feature': token(asset['feature']), 'source_path': relative(asset['source_path']),
                         'sha256': asset['sha256'], 'notices': notices, 'review_status': 'not_reviewed'})
    result = {'schema_version': 1, 'kind': 'full_native_review_projection_not_approval', 'source_recipe': SOURCE_RECIPE, 'source_sha': expected,
              'source_tree': source['source_tree'], 'target': check.TARGET, 'rust_release': check.TOOLCHAIN,
              'rust_commit': commit[0], 'rust_host': check.TARGET, 'profile': 'release',
              'app_features': ['default'], 'default_features': True, 'region_downloads': False,
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
