#!/usr/bin/env python3
"""Read-only, separately selected analytical Swift build-evidence auditor.

This tool never builds, stages, runs an executable, approves rights or publishes.
A passing audit is a build-evidence result; all native/bundle/release gates remain
required and unexecuted. See docs/release/analytical-swift-recipe.md.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
import tomllib
from pathlib import Path
import re
import sys
import tempfile


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


tone = load('analytical_swift_tone', 'check-tonemapping-build.py')
candidate = load('analytical_swift_ordinary', 'check-swift-windows-candidate.py')
collector = load('analytical_swift_notices', 'collect-dependency-notices.py')
readiness = load('analytical_swift_readiness', 'check-commercial-readiness.py')
require, digest = tone.require, tone.digest
IDENTITY = tone.ANALYTICAL_SWIFT_RECIPE
TARGET, TOOLCHAIN = candidate.TARGET, candidate.TOOLCHAIN
FEATURES = ['analytic-tonemapping', 'commercial-staging']
CONTRACT = 'scripts/analytical-swift-source-contract.json'
SOURCE_PATHS = {
    'Cargo.toml', 'Cargo.lock', 'crates/flightsim-app/Cargo.toml',
    'crates/flightsim-render/Cargo.toml', 'crates/flightsim-app/src/main.rs',
    'crates/flightsim-app/src/distribution.rs', 'crates/flightsim-render/src/lib.rs',
    'crates/flightsim-render/src/tonemapping.rs',
    'crates/flightsim-render/tests/tonemapping_modes.rs',
    'scripts/check-analytical-swift-recipe.py', 'scripts/check-tonemapping-build.py',
    'scripts/collect-dependency-notices.py', 'scripts/check-commercial-readiness.py',
    'scripts/check-swift-windows-candidate.py', 'scripts/stage-commercial-candidate.py',
    'scripts/check-release-authorization.py', 'scripts/replay-candidate-contract.json',
    'scripts/tests/test_analytical_swift_recipe.py', 'scripts/tests/test_tonemapping_build.py',
    'docs/release/asset-rights-manifest.json', 'docs/release/dependency-notice-supplements.json',
    'docs/release/analytical-swift-recipe.md',
}
GATES = {
    'combined_feature_regressions': 'Combined release/MSVC app/render tests and Clippy; real render tonemapping_modes; ordinary controls and mixed/neither rejection',
    'exact_identity_and_legacy_tests': 'Existing fingerprint and three exact legacy tests, one executed passing test each under combined release/MSVC features',
    'extracted_bundle_isolation': 'Sole unchanged stager; Swift-only complete allowlist, hashes/archive/extraction, unrelated CWD and poisoned asset environment; no renamed unapproved bytes',
    'extracted_runtime_acceptance': 'Actual Swift model/7.12 m fit, decoded PNG/exit 0; absent Light exit 2; legacy mismatch exit 2; explicit no-model legacy PNG/exit 0 and partial-identity notice',
    'analytical_windows_appearance': 'Day/low sun/night, atmosphere/fog/clouds/water, cockpit/HUD, camera/map/lifecycle; accept darker Reinhard appearance',
    'dependency_and_platform_review': 'Genuine exact-inventory dependency/runtime review; constgebra/hexf unresolved; Fira, shaders, MSVC/native imports and other obligations retained',
    'publication_receipt': 'Genuine exact-inventory publication receipt; standing user publication permission is separate and is not withheld',
}


def commands(mode):
    require(mode in ('analytic', 'ordinary'), 'unknown capture mode')
    select = ['-p', 'flightsim-app']
    features = ['--no-default-features', '--features', ','.join(FEATURES)] if mode == 'analytic' else []
    metadata_features = ['--no-default-features', '--features', ','.join('flightsim-app/' + f for f in FEATURES)] if mode == 'analytic' else []
    return {
        'rustc': ['rustc', '+' + TOOLCHAIN, '-vV'],
        'graph': ['cargo', '+' + TOOLCHAIN, 'tree', '--locked', '--offline', '--target', TARGET,
                  *select, *features, '--edges', 'normal,build', '--prefix', 'none', '--format', '{p} features=[{f}]'],
        'build': ['cargo', '+' + TOOLCHAIN, 'build', '--locked', '--offline', '--release', '-j', '2',
                  '--target', TARGET, *select, *features, '--message-format=json'],
        'metadata': ['cargo', '+' + TOOLCHAIN, 'metadata', '--locked', '--offline', '--format-version', '1',
                     '--filter-platform', TARGET, *metadata_features],
    }


def recipe():
    return {'schema_version': 1, 'recipe': IDENTITY, 'status': 'prepared_unexecuted',
            'target': TARGET, 'toolchain': TOOLCHAIN, 'default_features': False,
            'features': FEATURES, 'region_downloads': False, 'release_authorized': False,
            'commands': {mode: commands(mode) for mode in ('analytic', 'ordinary')},
            'required_unexecuted_gates': GATES}


def validate_header(value, source_sha):
    expected = {key: recipe()[key] for key in ('schema_version', 'recipe', 'target', 'toolchain',
                                              'default_features', 'features', 'region_downloads', 'release_authorized')}
    require(all(value.get(k) == v for k, v in expected.items())
            and type(value.get('schema_version')) is int
            and value.get('default_features') is False and value.get('region_downloads') is False
            and value.get('release_authorized') is False,
            'wrong analytical Swift capture identity')
    require(re.fullmatch('[0-9a-f]{40}', source_sha) is not None and value.get('source_sha') == source_sha,
            'capture source differs from exact source')
    require(set(value.get('modes', {})) == {'analytic', 'ordinary'}, 'both isolated build captures are required')


def source_evidence(repo, expected):
    # This preserves all 160 reviewed source pins and 102 independent anchors.
    source = candidate.source_inputs(repo, expected)
    contract = json.loads((repo / CONTRACT).read_text())
    require(contract.get('schema_version') == 1 and contract.get('recipe') == IDENTITY
            and re.fullmatch('[0-9a-f]{40}', contract.get('reviewed_source', '')) is not None,
            'invalid analytical source contract')
    pins = contract.get('source_sha256')
    require(isinstance(pins, dict) and set(pins) == SOURCE_PATHS, 'analytical source boundary changed')
    files = {entry['path']: entry for entry in source['files']}
    require(CONTRACT in files, 'analytical source contract must be tracked')
    for relative in SOURCE_PATHS | {CONTRACT}:
        entry = files.get(relative)
        require(entry is not None, 'missing analytical source: ' + relative)
        # Hash canonical blob bytes as well as the LF checkout used by Cargo.
        import subprocess
        import hashlib
        blob = subprocess.check_output(['git', 'cat-file', 'blob', entry['canonical_git_blob']], cwd=repo)
        require(hashlib.sha256(blob).hexdigest() == entry['checkout_sha256'],
                'analytical checkout differs from Git: ' + relative)
        if relative != CONTRACT:
            require(pins[relative] == entry['checkout_sha256'], 'analytical source pin changed: ' + relative)
    return {**source, 'analytical_contract': contract, 'analytical_contract_sha256': digest(repo / CONTRACT)}


def graph_identity(name, value):
    return (name.rsplit('@', 1)[0], value['version'])


def reconcile(metadata, graph, repo, mode):
    require(Path(metadata['workspace_root']).resolve() == repo.resolve(), 'foreign metadata workspace')
    packages, nodes, ids = collector.closure(metadata, 'flightsim-app')
    lock = tomllib.loads((repo / 'Cargo.lock').read_text())
    locked = {(p['name'], p['version'], p.get('source')): p for p in lock['package']}
    for i in ids:
        package = packages[i]
        identity = (package['name'], package['version'], package.get('source'))
        require(identity in locked, 'metadata package is absent from exact lock: ' + package['name'])
        if (package.get('source') or '').startswith('registry+'):
            require(re.fullmatch('[0-9a-f]{64}', locked[identity].get('checksum', '')) is not None,
                    'registry checksum missing from pinned lock')
    identities = {(packages[i]['name'], packages[i]['version']): i for i in ids}
    require(len(identities) == len(ids), 'ambiguous source for package/version')
    exact = {graph_identity(name, value): value for name, value in graph.items()}
    require(len(exact) == len(graph) and set(exact) <= set(identities),
            'package graph is outside conservative metadata closure')
    if mode == 'analytic':
        require(not any('tonemapping_luts' in nodes[i]['features'] for i in ids),
                'metadata reactivates LUTs; stop and resolve collection design without editing evidence')
    differences = []
    for identity, i in identities.items():
        package, node = packages[i], nodes[i]
        require(Path(package['manifest_path']).is_file(), 'missing metadata package source')
        features = set(node['features'])
        if identity in exact:
            required = exact[identity]['features']
            require(required <= features, 'metadata omits exact graph features: ' + package['name'])
            if package['name'] == 'flightsim-app':
                require(features == required, 'metadata app selection differs from exact recipe')
            if features != required:
                differences.append({'package': package['name'], 'version': package['version'],
                                    'exact_features': sorted(required), 'metadata_features': sorted(features)})
    extra = [{'package': name, 'version': version} for name, version in sorted(set(identities) - set(exact))]
    return {'exact_graph_packages': len(exact), 'conservative_packages': len(ids),
            'extra_conservative_packages': extra, 'conservative_feature_differences': differences,
            'scope': 'Raw metadata retained unchanged; conservative extras retained in inventory; exact graph is separate compiled-scope evidence'}


def validate_inventory(metadata_path, inventory_path, graph, repo, mode):
    metadata = json.loads(metadata_path.read_text())
    difference = reconcile(metadata, graph, repo, mode)
    inventory = json.loads(inventory_path.read_text())
    # Recollect exact sources rather than trust mutable package/notice summaries.
    # The isolated temporary result never replaces the original inventory.
    with tempfile.TemporaryDirectory(prefix='analytical-swift-notices-') as temporary:
        fresh = collector.collect(metadata_path, repo, Path(temporary) / 'notices', TARGET, 'flightsim-app')
    require(inventory == fresh, 'inventory differs from fresh conservative collection')
    require(inventory.get('review_status') == 'not_reviewed', 'collection is not review approval')
    expected_assets = {'bevy-fira-mono'}
    if mode == 'ordinary':
        expected_assets |= {'bevy-tony-mc-mapface', 'bevy-agx-lut', 'bevy-blender-filmic-lut'}
    require({a['id'] for a in inventory['embedded_assets']} == expected_assets,
            'embedded asset applicability differs from exact mode')
    identities = {p['id'] for p in inventory['packages']}
    require({'constgebra@0.1.4', 'hexf-parse@0.2.1'} <= identities, 'remaining dependency obligations disappeared')
    report = readiness.check(repo, None, inventory_path)
    candidate.validate_readiness(report, 2)
    require(any(b['code'] == 'DEPENDENCY_REVIEW_REQUIRED' for b in report['blockers']),
            'missing genuine dependency review must remain blocked')
    return {'inventory_sha256': digest(inventory_path), 'metadata_sha256': digest(metadata_path),
            'reconciliation': difference, 'readiness': report}


def file_record(path):
    require(path.is_absolute() and path.is_file() and not path.is_symlink(), 'expected absolute regular artifact path')
    require(not any(p.is_symlink() for p in path.parents), 'artifact path traverses a symlink')
    return {'sha256': digest(path), 'bytes': path.stat().st_size}


def recapture(repo, target, mode):
    """Authoritative read-only Cargo collection; never compile or alter metadata.

    A receipt/hash supplied by a caller cannot establish metadata completeness.
    Re-run the exact locked/offline read commands in the same source/target.
    """
    overrides = ('RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_ENCODED_RUSTFLAGS',
                 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER')
    require(not any(os.environ.get(key) for key in overrides), 'unapproved compiler override in audit environment')
    env = {**os.environ, 'RUSTFLAGS': '-D warnings', 'CARGO_TARGET_DIR': str(target), 'CARGO_INCREMENTAL': '0'}
    return {name: subprocess.check_output(commands(mode)[name], cwd=repo, env=env, timeout=180)
            for name in ('rustc', 'graph', 'metadata')}


def release_profile(artifact):
    profile = artifact.get('profile', {})
    require(profile.get('opt_level') == '3' and type(profile.get('debuginfo')) is int
            and profile['debuginfo'] == 0 and profile.get('debug_assertions') is False
            and profile.get('overflow_checks') is False and profile.get('test') is False,
            'selected artifact is not the strict release profile')


def validate_capture(record, directory, repo, mode, source):
    require(record.get('cwd') == str(repo.resolve()) and record.get('source_sha') == source['source_sha']
            and record.get('source_tree') == source['source_tree'], 'command source/CWD mismatch')
    target = Path(record['target_dir'])
    require(target.is_absolute() and target.resolve() == target and not target.is_symlink(), 'invalid target directory')
    require(record.get('environment') == {'RUSTFLAGS': '-D warnings', 'CARGO_TARGET_DIR': str(target),
                                         'CARGO_INCREMENTAL': '0'}, 'unexpected captured compiler environment')
    require(record.get('commands') == commands(mode), 'build/graph/metadata command differs from recipe')
    outputs = {'rustc': 'rustc.txt', 'graph': 'graph.txt', 'build': 'messages.jsonl', 'metadata': 'metadata.json'}
    require(set(record.get('results', {})) == set(outputs), 'incomplete command results')
    for key, filename in outputs.items():
        status = record['results'][key]
        require(type(status.get('exit_code')) is int and status['exit_code'] == 0, 'capture command failed: ' + key)
        require(status.get('stdout') == file_record(directory / filename)
                and status.get('stderr') == file_record(directory / (key + '.stderr')), 'captured command output changed: ' + key)
    fresh = recapture(repo, target, mode)
    for name in ('rustc', 'graph', 'metadata'):
        require((directory / outputs[name]).read_bytes() == fresh[name],
                'captured ' + name + ' differs from authoritative locked/offline recapture')
    compiler = (directory / 'rustc.txt').read_text()
    require(compiler.startswith('rustc 1.93.0 ') and '\nhost: ' + TARGET + '\n' in compiler,
            'capture is not the pinned native Windows MSVC compiler')
    graph_path, messages_path = directory / 'graph.txt', directory / 'messages.jsonl'
    graph = tone.parse_graph(graph_path.read_text())
    selected_recipe = IDENTITY if mode == 'analytic' else None
    tone.validate_graph(graph, mode, 'app', recipe=selected_recipe)
    require(graph_path.read_text().splitlines()[0].startswith('flightsim-app v'), 'graph root is not the app')
    require('downloads' not in graph.get('flightsim-content', {}).get('features', set()), 'network content features are forbidden')
    messages = [json.loads(line) for line in messages_path.read_text().splitlines()]
    require([m for m in messages if m.get('reason') == 'build-finished'] == [{'reason': 'build-finished', 'success': True}],
            'contradictory or incomplete build completion')
    require(not any(m.get('reason') == 'compiler-message' and m.get('message', {}).get('level') in ('error', 'failure-note', 'warning')
                    for m in messages), 'build contains rejected compiler diagnostics')
    artifacts = [m for m in messages if m.get('reason') == 'compiler-artifact']
    require(len([m for m in artifacts if m.get('executable')]) == 1, 'unexpected executable roots')
    root, libraries = tone.select_artifacts(messages, graph, 'app')
    render = [a for a in artifacts if a['target']['name'] == 'flightsim_render' and a['target']['kind'] == ['lib']]
    require(len(render) == 1 and set(render[0]['features']) == graph['flightsim-render']['features'],
            'missing or mismatched compiled render routing')
    executable = target / TARGET / 'release/flightsim-app.exe'
    require(Path(root['executable']) == executable, 'wrong target/profile/executable path')
    for artifact in [root, *libraries.values(), render[0]]:
        release_profile(artifact)
    require(Path(root['target']['src_path']).resolve() == repo / 'crates/flightsim-app/src/main.rs', 'foreign app source')
    metadata = json.loads((directory / 'metadata.json').read_text())
    require(Path(metadata['target_directory']) == target, 'metadata target directory differs from captured build')
    packages, nodes, ids = collector.closure(metadata, 'flightsim-app')
    exact_identities = {graph_identity(name, item): item['features'] for name, item in graph.items()}
    for artifact in artifacts:
        package_id = artifact['package_id']
        require(package_id in packages and (packages[package_id]['name'], packages[package_id]['version']) in exact_identities,
                'compiled package is absent from exact app graph')
        require(set(artifact['features']) <= exact_identities[(packages[package_id]['name'], packages[package_id]['version'])],
                'compiled features are outside exact app graph')
        require(package_id in ids and set(artifact['features']) <= set(nodes[package_id]['features']),
                'compiled package/features outside conservative metadata')
        require(any(t['name'] == artifact['target']['name'] and t['kind'] == artifact['target']['kind']
                    and t['src_path'] == artifact['target']['src_path'] for t in packages[package_id]['targets']),
                'compiled source target differs from metadata')
    result = tone.audit(graph_path, messages_path, mode, 'app', recipe=selected_recipe)
    paths = [executable, Path(result['core_pipeline_depinfo']['path'])]
    for item in result['compiled']:
        paths.extend([Path(item['rlib']), Path(item['fingerprint'])])
    # Freeze the render routing library too, in addition to the six dependency libraries.
    render_libraries = [Path(p) for p in render[0]['filenames'] if p.endswith('.rlib')]
    require(len(render_libraries) == 1 and render_libraries[0].parent == executable.parent / 'deps',
            'render library is outside the exact target')
    paths.extend(render_libraries)
    render_suffix = render_libraries[0].stem.removeprefix('libflightsim_render-')
    require(render_suffix != render_libraries[0].stem, 'unrecognized render library name')
    render_fingerprint = executable.parent / '.fingerprint' / ('flightsim-render-' + render_suffix) / 'lib-flightsim_render.json'
    app_fingerprints = list((executable.parent / '.fingerprint').glob('flightsim-app-*/bin-flightsim-app.json'))
    require(len(app_fingerprints) == 1, 'expected one app fingerprint in the fresh target tree')
    for fingerprint, artifact in ((render_fingerprint, render[0]), (app_fingerprints[0], root)):
        value = json.loads(fingerprint.read_text())
        require(set(json.loads(value['features'])) == set(artifact['features']) and value.get('rustflags') == ['-D', 'warnings'],
                'app/render fingerprint differs from selected features or flags')
        paths.append(fingerprint)
    observed = {str(path): file_record(path) for path in paths}
    require(record.get('frozen_artifacts') == observed, 'frozen artifact hashes/bytes changed or incomplete')
    inventory = validate_inventory(directory / 'metadata.json', directory / 'notices/dependency-inventory.json', graph, repo, mode)
    require(record.get('inventory') == file_record(directory / 'notices/dependency-inventory.json'), 'inventory capture changed')
    return {'artifact': result, **inventory}


def audit(repo, expected, capture):
    repo, capture = repo.resolve(), capture.resolve()
    original_capture = (capture / 'capture.json').read_bytes()
    value = json.loads(original_capture)
    validate_header(value, expected)
    source = source_evidence(repo, expected)
    require(value.get('source_tree') == source['source_tree'], 'capture source tree changed')
    directories = [Path(value['modes'][mode]['target_dir']).resolve() for mode in ('analytic', 'ordinary')]
    require(not directories[0].is_relative_to(directories[1]) and not directories[1].is_relative_to(directories[0]),
            'ordinary control requires a disjoint target directory')
    results = {mode: validate_capture(value['modes'][mode], capture / mode, repo, mode, source)
               for mode in ('analytic', 'ordinary')}
    analytic, ordinary = (results[mode]['artifact'] for mode in ('analytic', 'ordinary'))
    require(analytic['executable_sha256'] != ordinary['executable_sha256'], 'ordinary control was substituted')
    require([(p['name'], p['source_sha256'], p['bytes']) for p in analytic['payloads']]
            == [(p['name'], p['source_sha256'], p['bytes']) for p in ordinary['payloads']], 'ordinary LUT source control changed')
    # Rehash every supplied input at the end; neither evidence nor source may drift.
    require((capture / 'capture.json').read_bytes() == original_capture, 'capture changed during audit')
    require(source_evidence(repo, expected) == source, 'source changed during audit')
    for mode in ('analytic', 'ordinary'):
        directory = capture / mode
        for name, filename in {'rustc': 'rustc.txt', 'graph': 'graph.txt', 'build': 'messages.jsonl', 'metadata': 'metadata.json'}.items():
            status = value['modes'][mode]['results'][name]
            require(file_record(directory / filename) == status['stdout']
                    and file_record(directory / (name + '.stderr')) == status['stderr'], 'input changed during audit')
        require(file_record(directory / 'notices/dependency-inventory.json') == value['modes'][mode]['inventory'],
                'inventory changed during audit')
        final_readiness = readiness.check(repo, None, directory / 'notices/dependency-inventory.json')
        candidate.validate_readiness(final_readiness, 2)
        require(final_readiness == results[mode]['readiness'], 'notice integrity/readiness changed during audit')
        for path, frozen in value['modes'][mode]['frozen_artifacts'].items():
            require(file_record(Path(path)) == frozen, 'artifact changed during audit')
    return {**recipe(), 'status': 'build_evidence_checked_native_and_distribution_unqualified',
            'source_sha': expected, 'source_tree': source['source_tree'],
            'analytical_contract_sha256': source['analytical_contract_sha256'],
            'replay_contract_sha256': source['replay_contract_sha256'],
            'capture_sha256': digest(capture / 'capture.json'), 'builds': results,
            'tone_attestation': 'Exact pinned source/command/graph/compiled features/fingerprints/dep-info/executable payloads; distribution-info has no tone field'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--describe', action='store_true')
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source-sha')
    parser.add_argument('--capture', type=Path)
    args = parser.parse_args()
    try:
        if args.describe:
            require(args.source_sha is None and args.capture is None, 'describe cannot attest a capture')
            result = recipe()
        else:
            require(args.source_sha is not None and args.capture is not None, 'audit requires source SHA and complete capture')
            result = audit(args.repo, args.source_sha, args.capture)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('analytical Swift build-evidence audit failed: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
