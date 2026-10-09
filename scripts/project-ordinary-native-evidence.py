#!/usr/bin/env python3
"""Full ordinary same-build facts; no review, runtime or publication approval.

This new read-only adapter composes the ordinary authorized copy-plan projector
with the historical native parsers. It never accepts the diagnostic full bundle
in place of the ordinary payload and does not modify any inherited contract.
"""
from __future__ import annotations

import argparse
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


base = load('ordinary_native_shared_parsers', 'project-analytical-native-evidence.py')
payload = load('ordinary_native_payload', 'project-ordinary-release-payload.py')
capture, check, require = base.capture, base.check, base.require
runtime_facts = base.runtime_facts
release = payload.release
token, relative, features = base.token, base.relative, base.features
headers, pe_imports = base.headers, base.pe_imports
SOURCE_RECIPE = 'bevy-0.18.1-tony-filmic-source-v1'
EMBEDDED_IDS = {'bevy-fira-mono', 'bevy-tony-mc-mapface', 'bevy-blender-filmic-lut'}
KIND = 'ordinary_native_review_projection_not_approval'
CONTENT_KIND = 'ordinary_native_content_view_not_approval'
CONTENT_VERSION = 2
MAX_BYTES = 8 * 1024 * 1024
MAX_TEXT_BYTES = 64 * 1024 * 1024
NATIVE_SUFFIXES = {'.exe', '.dll', '.lib', '.a', '.pdb', '.msi'}


def read_text(path, maximum=MAX_TEXT_BYTES, *, binding=None):
    details = payload.independent_file(path)
    require(details.st_size <= maximum, 'native text input exceeds byte budget')
    raw = path.read_bytes()
    require(len(raw) == details.st_size and b'\0' not in raw, 'native text changed or contains NUL')
    if binding is not None:
        require(payload.record(raw) == binding and capture.file_record(path) == binding, 'native text binding changed')
    return raw.decode('utf-8-sig')


def read_messages(path, *, binding=None):
    text = read_text(path, binding=binding)
    require(len(text.splitlines()) <= 16384, 'native message count exceeds budget')
    def pairs(rows):
        result = {}
        for key, value in rows:
            require(key not in result, 'duplicate native message key')
            result[key] = value
        return result
    messages = [json.loads(line, object_pairs_hook=pairs) for line in text.splitlines()]
    require(all(isinstance(row, dict) for row in messages), 'invalid native message')
    payload.canonical(messages)
    return messages


def validate_metadata_members(metadata):
    # The shared closure parser uses dictionaries: reject ambiguous source IDs
    # before they could be overwritten there.
    for rows in (metadata['packages'], metadata['resolve']['nodes']):
        require(isinstance(rows, list) and 0 < len(rows) <= 4096, 'unbounded Cargo metadata')
        ids = [row['id'] for row in rows]
        require(all(isinstance(key, str) for key in ids) and len(ids) == len(set(ids)),
                'duplicate Cargo metadata identity')


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


def validate_runtime_facts(private, repo, expected, build_private, build_text, *, build_bindings):
    """Replay collector validators without running any query or executable."""
    private = payload.absolute(private)
    value = runtime_facts.project(private)  # Rechecks original installed/build files.
    require(value['source_sha'] == expected and value['target'] == check.TARGET,
            'runtime facts source/target mismatch')
    captured = runtime_facts.compiler_identity(read_text(build_private / 'capture/ordinary/rustc.txt', 64 * 1024))
    require(captured['status'] == 'observed' and captured['matches_required_identity'] is True
            and value['rust']['identity'] == captured, 'runtime compiler differs from exact audited build')
    manifest, manifest_binding = payload.read_object(private / runtime_facts.PRIVATE_NAME)
    require(manifest['recipe_cfg_args'] == ['-D', 'warnings'] and manifest['linker_inputs'] == {
        'trace': str(build_private / 'capture/ordinary/build.stderr'),
        'executable': str(build_private / 'target-ordinary' / check.TARGET / 'release/flightsim-app.exe')},
        'runtime facts are not bound to this exact ordinary build trace/executable')
    require(value['private_manifest'] == manifest_binding, 'runtime private manifest changed')
    for name, binding in (('trace', build_bindings['build_trace']), ('executable', build_bindings['executable'])):
        observed = value['final_link'][name]
        require(observed is not None and observed['status'] == 'observed' and observed['record'] == binding,
                'runtime build input binding differs: ' + name)
    path = private / runtime_facts.PROJECTION_NAME
    details = payload.independent_file(path)
    require(details.st_size <= runtime_facts.MAX_PUBLIC and path.read_bytes() == runtime_facts.canonical(value),
            'runtime fact projection changed')
    return value


def project(repo, expected, build_private, build_text, bundle, *, runtime_facts_private):
    """Project exact source/native/runtime facts, with mandatory ordinary runtime roots.

    All roots must be canonical, absolute, pairwise disjoint paths. There is no
    caller-supplied receipt, skip-validation switch or UI capability requirement.
    """
    repo, build_private, build_text, bundle, runtime_facts_private = map(payload.absolute,
        (repo, build_private, build_text, bundle, runtime_facts_private))
    capture.disjoint(repo, build_private, build_text, bundle, runtime_facts_private)
    packet = payload.project_payload(repo, expected, build_private, build_text, bundle)
    verified, summary_binding = payload.read_object(build_text / capture.EXPORT_NAME)
    require(summary_binding == packet['bindings']['native_build_summary'], 'validated native summary changed')
    require(verified['status'] == capture.PASS and verified['source_sha'] == expected
            and verified.get('source_recipe') == SOURCE_RECIPE
            and verified['builds']['ordinary']['lut_payloads_found'] == 2,
            'completed same-build ordinary two-LUT audit required')
    source = capture.source_evidence(repo, expected)
    require(source['source_tree'] == packet['source_tree'] == verified['source_tree'], 'native source tree differs')
    directory = build_private / 'capture/ordinary'
    input_bindings = {key: capture.file_record(directory / name) for key, name in
                      (('graph', 'graph.txt'), ('messages', 'messages.jsonl'),
                       ('compiler', 'rustc.txt'), ('build_trace', 'build.stderr'))}
    metadata, metadata_binding = payload.read_object(directory / 'metadata.json')
    inventory, inventory_binding = payload.read_inventory(directory / 'notices/dependency-inventory.json')
    require(metadata_binding == packet['bindings']['captured_metadata']
            and inventory_binding == packet['inventory_comparison']['captured'],
            'projected native inventory/metadata changed')
    validate_metadata_members(metadata)
    packages, nodes, ids = check.collector.closure(metadata, 'flightsim-app')
    require(len(ids) <= 1024, 'native package count exceeds review budget')
    app = [package for package in inventory['packages'] if package['name'] == 'flightsim-app']
    require(len(app) == 1 and app[0]['features'] == ['default'], 'full inventory uses another application recipe')
    names = {key: token(packages[key]['name'] + '@' + packages[key]['version']) for key in ids}
    require(len(set(names.values())) == len(names), 'ambiguous package/version identities')
    graph = check.tone.parse_graph(read_text(directory / 'graph.txt', binding=input_bindings['graph']))
    exact = {check.graph_identity(name, value): value for name, value in graph.items()}
    require(len(exact) == len(graph) and set(exact) <= {(packages[key]['name'], packages[key]['version']) for key in ids},
            'exact graph is outside conservative metadata closure')
    records, edges = [], []
    by_id = {row['id']: row for row in inventory['packages']}
    require(set(by_id) == set(names.values()), 'inventory differs from complete package closure')
    for key in ids:
        package, node = packages[key], nodes[key]
        row = by_id[names[key]]
        require(row['source'] in ('workspace', 'registry+https://github.com/rust-lang/crates.io-index'), 'unreviewed dependency source kind')
        checksum = row['source_checksum']
        require((checksum is None if row['source'] == 'workspace' else capture.hex_string(checksum, 64)),
                'unexpected workspace or missing registry checksum')
        require(row['source'] == (package.get('source') or 'workspace'), 'inventory package source differs')
        expression = row['license_expression']
        if expression is not None: token(expression, r'[A-Za-z0-9_.+() /:-]+', 512)
        notices = []
        require(len(row['notices']) <= 64, 'unbounded package notices')
        for notice in row['notices']:
            path = relative(notice['path'])
            actual = capture.file_record(directory / 'notices' / path)
            require(capture.valid_record({name: notice[name] for name in ('sha256', 'bytes')})
                    and actual == {name: notice[name] for name in ('sha256', 'bytes')}, 'notice binding changed')
            notices.append({'path': path, 'upstream_path': relative(notice['upstream_path']), **actual})
        actual_graph = exact.get((package['name'], package['version']))
        require(row['name'] == package['name'] and row['version'] == package['version']
                and features(row['features']) == features(node['features']), 'inventory package identity/features differ')
        require(actual_graph is None or actual_graph['features'] <= set(node['features']),
                'metadata omits exact graph features')
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
            require(isinstance(dependency['dep_kinds'], list) and 0 < len(dependency['dep_kinds']) <= 128,
                    'missing or unbounded dependency kinds')
            for kind in dependency['dep_kinds']:
                require(kind['kind'] in (None, 'normal', 'build', 'dev'), 'unknown dependency kind')
                if kind['kind'] in (None, 'normal', 'build'):
                    require(dependency['pkg'] in names, 'edge outside conservative closure')
                    condition = kind['target']
                    if condition is not None: token(condition, r'[A-Za-z0-9_()=, ."+-]+', 512)
                    require(len(edges) < 8192, 'native edge budget exceeded')
                    edges.append({'from': names[key], 'to': names[dependency['pkg']],
                                  'kind': kind['kind'] or 'normal', 'condition': condition})
    require(len(edges) <= 8192, 'native edge budget exceeded')
    messages = read_messages(directory / 'messages.jsonl', binding=input_bindings['messages'])
    requests = []
    for message in messages:
        if message.get('reason') == 'build-script-executed':
            require(message['package_id'] in names, 'build-script package outside native closure')
            libraries = message.get('linked_libs', [])
            require(isinstance(libraries, list) and len(libraries) <= 128, 'unbounded build-script libraries')
            require(len(requests) < 512, 'unbounded build-script requests')
            requests.append({'package_id': names[message['package_id']],
                             'linked_libs': [token(item, r'[A-Za-z0-9_.+:=,-]+') for item in libraries]})
    require(len(requests) <= 512, 'unbounded build-script requests')
    executable = bundle / 'flightsim-app.exe'
    require(capture.file_record(executable) == verified['builds']['ordinary']['executable'], 'projected executable changed')
    shipped = packet['shipped_files']
    native = {row['path'] for row in shipped if Path(row['path']).suffix.lower() in NATIVE_SUFFIXES}
    require(native == {'flightsim-app.exe'}, 'unexpected redistributed native file')
    compiler = runtime_facts.compiler_identity(read_text(directory / 'rustc.txt', 64 * 1024, binding=input_bindings['compiler']))
    require(compiler['status'] == 'observed' and compiler['matches_required_identity'] is True,
            'captured compiler does not have the required exact native identity')
    embedded = []
    require(len(inventory['embedded_assets']) <= 16, 'unbounded embedded asset set')
    require({row['id'] for row in inventory['embedded_assets']} == EMBEDDED_IDS
            and len(inventory['embedded_assets']) == len(EMBEDDED_IDS), 'full embedded asset set must be exactly Fira/Tony/Filmic')
    for asset in inventory['embedded_assets']:
        require(asset['id'] in EMBEDDED_IDS and capture.hex_string(asset['sha256'], 64)
                and asset['observed_sha256'] == asset['sha256'],
                'full dependency asset identity changed')
        notices = []
        require(isinstance(asset['notices'], list) and len(asset['notices']) <= 64, 'unbounded embedded notices')
        for notice in asset['notices']:
            path = relative(notice['path']); actual = capture.file_record(directory / 'notices' / path)
            require(capture.valid_record({key: notice[key] for key in ('sha256', 'bytes')})
                    and actual == {key: notice[key] for key in ('sha256', 'bytes')}, 'embedded asset notice changed')
            notices.append({'path': path, **actual})
        embedded.append({'id': asset['id'], 'package': token(asset['package']), 'version': token(asset['version']),
                         'feature': token(asset['feature']), 'source_path': relative(asset['source_path']),
                         'sha256': asset['sha256'], 'notices': notices, 'review_status': 'not_reviewed'})
    result = {'schema_version': 1, 'kind': KIND, 'source_recipe': SOURCE_RECIPE, 'source_sha': expected,
              'source_tree': source['source_tree'], 'target': check.TARGET, 'rust_release': check.TOOLCHAIN,
              'rust_commit': compiler['commit'], 'rust_host': compiler['host'], 'profile': 'release',
              'app_features': ['default'], 'default_features': True, 'region_downloads': False,
              'release_authorized': False, 'dependency_review_approved': False,
              'bindings': {'lock': capture.file_record(repo / 'Cargo.lock'), 'metadata': capture.file_record(directory / 'metadata.json'),
                           'inventory': inventory_binding,
                           'graph': input_bindings['graph'], 'messages': input_bindings['messages'],
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
    result['runtime_accepted'] = False
    result['review_applicability_approved'] = False
    result['native_runtime_coverage_complete'] = False
    result['payload_projection'] = packet
    result['inventory_comparison'] = packet['inventory_comparison']
    result['bindings'].update(input_bindings)
    facts = validate_runtime_facts(runtime_facts_private, repo, expected, build_private, build_text,
                                   build_bindings=result['bindings'])
    result['runtime_facts'] = facts
    result['bindings']['runtime_facts'] = capture.file_record(runtime_facts_private / runtime_facts.PROJECTION_NAME)
    result['bindings']['runtime_manifest'] = facts['private_manifest']
    result['runtime_coverage'].update({
        'factual_packet': 'collected_review_required',
        'rust_sysroot_notice_and_library_hashes': 'see_bound_factual_packet',
        'resolved_linker_toolset_sdk_provenance': 'see_bound_factual_packet_actual_sdk_selection_unknown',
        'constructed_final_link_command': facts['final_link']['status'],
        'installed_candidates_are_actual_selection': False,
        'dynamic_module_closure': 'not_established',
    })
    result['source_content_files'] = source_content_files(repo, source, packet, records)
    result['content_view'] = content_view(result)
    # Replay native capture, source, gate, notices, bundle, installed runtime and
    # build-origin observations before returning; no cached caller grant wins.
    require(payload.canonical(payload.project_payload(repo, expected, build_private, build_text, bundle))
            == payload.canonical(packet), 'ordinary payload facts changed during projection')
    require(capture.source_evidence(repo, expected) == source, 'native source changed during projection')
    require(validate_runtime_facts(runtime_facts_private, repo, expected, build_private, build_text,
                                  build_bindings=result['bindings']) == facts,
            'runtime facts changed during projection')
    for key, path in {
        'graph': directory / 'graph.txt', 'messages': directory / 'messages.jsonl',
        'compiler': directory / 'rustc.txt', 'build_trace': directory / 'build.stderr',
        'runtime_facts': runtime_facts_private / runtime_facts.PROJECTION_NAME,
    }.items():
        require(capture.file_record(path) == result['bindings'][key], 'native fact input changed: ' + key)
    require(len((json.dumps(result, indent=2, sort_keys=True, allow_nan=False) + '\n').encode('ascii')) <= MAX_BYTES,
            'ordinary native projection exceeds export budget')
    return result


# This coverage is source content, not the inventory/review/authorization packet.
# Root source evidence still binds every tracked file; this explicit subset is
# what a separate applicability review must compare for implementation content.
SOURCE_PREFIXES = ('crates/', 'vendor/', 'assets/')
SOURCE_REQUIRED = {'Cargo.toml', 'Cargo.lock', release.ASSET_MANIFEST,
                   'scripts/tonemapping-two-lut-source.json'}
SOURCE_OPTIONAL = {base.HEADER_MANIFEST, base.MODIFIED_MANIFEST}
SOURCE_PACKET_EXCLUSIONS = {release.DEPENDENCY_INVENTORY, release.DEPENDENCY_REVIEW,
                            release.AUTHORIZATION}


def source_content_files(repo, source, packet, packages):
    require(isinstance(source['files'], list) and 0 < len(source['files']) <= 16384,
            'unbounded canonical source file set')
    by_path = {relative(row['path']): row for row in source['files']}
    require(len(by_path) == len(source['files']) and SOURCE_REQUIRED <= set(by_path),
            'missing or duplicate canonical source content')
    selected = SOURCE_REQUIRED | (SOURCE_OPTIONAL & set(by_path))
    selected |= {name for name in by_path if name.startswith(SOURCE_PREFIXES)}
    # Original shipped notices are a separately validated closed tree. Shipped
    # source/readme/attribution/license paths preserve their exact source bytes.
    selected |= {row['path'] for row in packet['shipped_files']
                 if row['path'] in by_path and row['path'] not in SOURCE_PACKET_EXCLUSIONS}
    for package in packages:
        modified = package['modified_source']
        if modified and 'replacement_patch' in modified:
            selected.add(relative(modified['replacement_patch']['path']))
    require(selected <= set(by_path), 'source content refers to an untracked file')
    result = []
    for name in sorted(selected):
        row = by_path[name]
        binding = capture.file_record(repo / name)
        require(binding == {'sha256': row['checkout_sha256'], 'bytes': row['checkout_bytes']}
                and row['git_mode'] in ('100644', '100755'), 'canonical source content changed')
        result.append({'path': name, 'git_mode': row['git_mode'], **binding})
    return result


OBSERVATION_FIELDS = {'status', 'record', 'evidence_id', 'file_version', 'product_version'}
RUST_SCALAR_FIELDS = ('identity', 'sysroot_status', 'target_rlibs_status', 'target_defaults',
                     'recipe_cfg_probe', 'recipe_settings_supplied', 'actual_compiler_invocation_observed',
                     'actual_build_panic_strategy', 'actual_build_crt_static', 'rlibs_prove_linked_inclusion')
MICROSOFT_SCALAR_FIELDS = ('installed_candidates_exhaustive', 'selected_linker', 'selected_sdk',
                          'static_contributions', 'licensed_product_entitlement',
                          'applicable_terms_acceptance', 'dynamic_module_coverage')
LINK_SCALAR_FIELDS = ('status', 'reason', 'constructed_final_command_observed',
                     'linker_process_execution_observed', 'requested_libraries', 'rlib_input_names',
                     'explicit_library_paths', 'actual_static_membership', 'implicit_sdk_selection')
CONTENT_SECTIONS = ('source', 'packages', 'graph', 'headers', 'embedded_assets',
                    'build_script_link_requests', 'platform', 'runtime_rust',
                    'runtime_microsoft', 'runtime_final_link', 'runtime_query_outcomes')
PROJECT_FIELDS = {
    'schema_version', 'kind', 'source_recipe', 'source_sha', 'source_tree', 'target',
    'rust_release', 'rust_commit', 'rust_host', 'profile', 'app_features', 'default_features',
    'region_downloads', 'release_authorized', 'dependency_review_approved', 'bindings',
    'packages', 'normal_build_edges', 'source_headers', 'embedded_assets',
    'build_script_link_requests', 'build_script_requests_prove_static_inclusion', 'pe',
    'shipped_files', 'redistributed_microsoft_native_files', 'os_prerequisites',
    'actual_static_contributions', 'runtime_coverage', 'unresolved_reviews', 'runtime_accepted',
    'review_applicability_approved', 'native_runtime_coverage_complete', 'payload_projection',
    'inventory_comparison', 'runtime_facts', 'source_content_files',
}


def observation_content(row, labels=(), *, build_input=False):
    """Remove only an observation's local ID and, for four build inputs, hash."""
    if row is None:
        require(not labels, 'labeled runtime observation is absent')
        return None
    require(set(row) == OBSERVATION_FIELDS | set(labels), 'unknown runtime observation content field')
    result = {key: row[key] for key in (*labels, 'status', 'file_version', 'product_version')}
    if not build_input:
        result['record'] = row['record']
    return result


def terms_content(rows):
    return [observation_content(row, ('location', 'name')) for row in rows]


def runtime_content(value):
    """Explicit schema projection; raw facts stay intact in runtime_facts.

    Packet-local bindings and trace total-line volume are excluded. Lists keep
    original ordering, file records and versions stay exact, and unknowns remain unknown. A changed or
    newly added field fails the upstream closed schema before any projection.
    """
    runtime_facts.validate_projection(value)
    rust, microsoft, link = value['rust'], value['microsoft'], value['final_link']
    rust_view = {key: copy.deepcopy(rust[key]) for key in RUST_SCALAR_FIELDS}
    rust_view['notices'] = [observation_content(row, ('id',)) for row in rust['notices']]
    rust_view['target_rlibs'] = [observation_content(row, ('name',)) for row in rust['target_rlibs']]
    ms_view = {key: microsoft[key] for key in MICROSOFT_SCALAR_FIELDS}
    ms_view['vswhere'] = observation_content(microsoft['vswhere'])
    ms_view['visual_studio_candidates'] = []
    candidate_reference = None
    for install in microsoft['visual_studio_candidates']:
        installation = {key: install[key] for key in ('product', 'version')}
        installation.update(default_toolset_hint=observation_content(install['default_toolset_hint']),
                            terms_candidates=terms_content(install['terms_candidates']), toolsets=[])
        for toolset in install['toolsets']:
            tool = {'version': toolset['version'],
                    'linkers': [observation_content(row, ('host', 'target')) for row in toolset['linkers']],
                    'runtime_libraries': [observation_content(row, ('name',)) for row in toolset['runtime_libraries']],
                    'terms_candidates': terms_content(toolset['terms_candidates'])}
            installation['toolsets'].append(tool)
            for linker in toolset['linkers']:
                if link['installed_candidate_evidence_id'] is not None and linker['evidence_id'] == link['installed_candidate_evidence_id']:
                    require(candidate_reference is None, 'ambiguous installed linker content identity')
                    candidate_reference = {'product': install['product'], 'installation_version': install['version'],
                        'toolset_version': toolset['version'], 'linker': observation_content(linker, ('host', 'target'))}
        ms_view['visual_studio_candidates'].append(installation)
    require((candidate_reference is None) == (link['installed_candidate_evidence_id'] is None),
            'unresolved installed linker content identity')
    ms_view['sdk_candidates'] = [
        {'version': sdk['version'], 'architecture': sdk['architecture'],
         'libraries': [observation_content(row, ('family', 'name')) for row in sdk['libraries']],
         'terms_candidates': terms_content(sdk['terms_candidates'])} for sdk in microsoft['sdk_candidates']]
    link_view = {key: copy.deepcopy(link[key]) for key in LINK_SCALAR_FIELDS}
    link_view['linker'] = observation_content(link['linker'])
    link_view['installed_candidate'] = candidate_reference
    link_view['build_input_observations'] = {
        key: observation_content(link[key], build_input=True)
        for key in ('trace', 'executable', 'fingerprint', 'link_output')}
    # Missing and present-null fields are distinct evidence availability states.
    for key in ('trace_observation', 'output_association'):
        if key in link:
            link_view[key] = copy.deepcopy(link[key])
    if isinstance(link_view.get('trace_observation'), dict):
        # The original closed validator above still checks this counter and all
        # its bounds. Ordinary compiler progress volume is not a code/notice
        # obligation. Other counters, availability and raw facts remain exact.
        del link_view['trace_observation']['total_lines']
    queries = [{key: row[key] for key in ('id', 'outcome', 'exit_code')} for row in value['query_bindings']]
    return {'runtime_rust': rust_view, 'runtime_microsoft': ms_view,
            'runtime_final_link': link_view, 'runtime_query_outcomes': queries}


def link_requests_content(rows):
    """Ignore only cross-package Cargo event order, never library order/content."""
    require(type(rows) is list and len(rows) <= 512, 'unbounded build-script request rows')
    for row in rows:
        require(type(row) is dict and set(row) == {'package_id', 'linked_libs'},
                'unknown build-script request field')
        token(row['package_id'])
        require(type(row['linked_libs']) is list and len(row['linked_libs']) <= 128,
                'unbounded linked library requests')
        for library in row['linked_libs']:
            token(library, r'[A-Za-z0-9_.+:=,-]+')
    # Stable package ordering preserves every field, row multiplicity, the
    # sequence of multiple events for one package, and each linked_libs order.
    # The raw packet is untouched.
    return copy.deepcopy(sorted(rows, key=lambda row: row['package_id']))


def content_view(projection):
    """Bounded explicit sections for review comparison, never an approval test."""
    require(set(projection) - {'content_view'} == PROJECT_FIELDS,
            'unknown ordinary native projection field')
    require(type(projection['schema_version']) is int and projection['schema_version'] == 1
            and projection['kind'] == KIND, 'invalid ordinary native projection identity')
    require(all(projection[key] is False for key in ('release_authorized', 'dependency_review_approved',
            'review_applicability_approved', 'runtime_accepted', 'native_runtime_coverage_complete')),
            'ordinary native projection cannot grant approval')
    sections = {
        'source': {'source_recipe': projection['source_recipe'], 'files': projection['source_content_files']},
        'packages': projection['packages'],
        'graph': {key: projection[key] for key in ('target', 'profile', 'app_features', 'default_features',
                                                  'region_downloads', 'normal_build_edges')},
        'headers': projection['source_headers'],
        'embedded_assets': projection['embedded_assets'],
        'build_script_link_requests': {
            'build_script_link_requests': link_requests_content(projection['build_script_link_requests']),
            'build_script_requests_prove_static_inclusion': projection['build_script_requests_prove_static_inclusion']},
        'platform': {key: projection[key] for key in ('rust_release', 'rust_commit', 'rust_host', 'pe',
            'redistributed_microsoft_native_files', 'os_prerequisites', 'actual_static_contributions',
            'runtime_coverage', 'unresolved_reviews')},
        **runtime_content(projection['runtime_facts']),
    }
    require(set(sections) == set(CONTENT_SECTIONS), 'ordinary content section set changed')
    result = {'schema_version': CONTENT_VERSION, 'kind': CONTENT_KIND, 'sections': copy.deepcopy(sections)}
    require(len(payload.canonical(result)) <= MAX_BYTES, 'ordinary content view exceeds byte budget')
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repo', 'build-private', 'build-text', 'bundle', 'runtime-facts-private'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--expected-sha', required=True)
    args = parser.parse_args(argv)
    try:
        result = project(args.repo, args.expected_sha, args.build_private, args.build_text, args.bundle,
                         runtime_facts_private=args.runtime_facts_private)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RecursionError, subprocess.SubprocessError):
        print('Ordinary native projection failed: input validation did not complete.', file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True, allow_nan=False))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
