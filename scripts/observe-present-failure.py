#!/usr/bin/env python3
"""Failure-only P2 observation after one strict validation by unchanged source S.

The validation mode runs S's full validator once and binds its exact inputs.
Observation/revalidation check those bindings, the current source/executable and
raw failure streams; they do not recollect platform facts or rehash target trees.
No build, simulator launch, timeout change or positive acceptance is provided.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys

POLICY_ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = 'fe7373e96396131ef4a1c2a9af9cfabdb8aab82b'
SOURCE_TREE = '6dc4d8ed190ccc43733182d03e089f84f0eaaef6'
RECEIPT = 'source-validation.json'
ORIGIN = 'present-origin.json'
EXCERPT = 'present-error.json'
MAX_RECEIPT = 65536
MAX_ORIGIN = 8192


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


capture = load('present_capture', POLICY_ROOT / 'scripts/capture-analytical-swift-msvc.py')
failure = load('present_failure', POLICY_ROOT / 'scripts/project-analytical-runtime-failure.py')
require = capture.require


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii')


def read_json(path, maximum):
    capture.no_links(path)
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= maximum, 'invalid bounded diagnostic JSON')
    raw = path.read_bytes(); value = json.loads(raw.decode('ascii'))
    require(raw == canonical(value), 'noncanonical diagnostic JSON')
    return value


def source_context(source_repo, expected, source_private, source_export, policy_sha, private):
    require(expected == SOURCE_SHA and capture.hex_string(policy_sha, 40), 'exact S/P identities required')
    roots = (source_repo, source_private, source_export, POLICY_ROOT, private)
    for path in roots:
        require(path.is_absolute(), 'absolute S/P roots required'); capture.no_links(path)
    capture.disjoint(*roots)
    policy = capture.source_evidence(POLICY_ROOT, policy_sha)
    source = capture.source_evidence(source_repo, expected)
    require(source['source_tree'] == SOURCE_TREE, 'wrong source tree')
    original = load('present_original_source', source_repo / 'scripts/qualify-analytical-swift-windows.py')
    inputs = {'source_repo': str(source_repo), 'source_private': str(source_private), 'source_export': str(source_export),
              'source_sha': expected, 'policy_sha': policy_sha}
    return original, source, policy, inputs


def report_bindings(original, report, source_repo, source_private, source_export, source):
    original.validate_shape(report)
    require(report['phase'] == 'runtime' and report['source_sha'] == SOURCE_SHA and report['source_tree'] in (None, SOURCE_TREE),
            'wrong original runtime identity')
    require(report == capture.read_private_json(source_private / 'result.json'), 'original private result changed')
    paths = {'qualification': source_export / original.EXPORT, 'private_result': source_private / 'result.json'}
    if 'source' in report['bindings']:
        require(capture.read_private_json(source_private / 'source.json') == source
                and capture.file_record(source_private / 'source.json') == report['bindings']['source'], 'original source receipt changed')
        paths['source'] = source_private / 'source.json'
    command_id = None
    if report['status'] == 'failed' and report['commands'] and report['commands'][-1]['id'] in failure.SCENES:
        command_id = report['commands'][-1]['id']
        require(report['native_projection'] is not None and report['runtime_facts'] is not None, 'native failure needs completed original build observations')
        build = source_private / 'build-private'; text = source_private / 'build-text'
        summary = read_json(text / capture.EXPORT_NAME, capture.MAX_EXPORT_BYTES)
        capture.validate_summary(summary)
        require(summary['status'] == capture.PASS and summary['source_sha'] == SOURCE_SHA and summary['source_tree'] == SOURCE_TREE
                and summary == capture.read_private_json(build / 'progress.json'), 'original successful build identity changed')
        executable = build / 'target-analytic' / capture.check.TARGET / 'release/flightsim-app.exe'
        bundle = source_private / 'extracted/swift-candidate'
        require(capture.file_record(executable) == summary['builds']['analytic']['executable'], 'audited executable changed')
        original.strict_bundle(bundle, executable, capture.check.digest(bundle / 'bundle-manifest.json'), source_repo,
                               build / 'capture/analytic/notices')
        paths.update(build_summary=text / capture.EXPORT_NAME, build_progress=build / 'progress.json',
                     frozen_trees=build / 'frozen-trees.json', build_source=build / 'source.json',
                     executable=executable, extracted_executable=bundle / 'flightsim-app.exe',
                     bundle_manifest=bundle / 'bundle-manifest.json', native_projection=source_export / 'native-review.json')
        require(capture.file_record(paths['native_projection']) == report['native_projection'], 'native projection changed')
        specs = original.command_specifications(source_repo, source_private, 'runtime', SOURCE_SHA)
        for command in report['commands']:
            name = command['id']; base = source_private / 'commands' / name
            journal = capture.read_private_json(base / 'journal.json')
            require(journal['command'] == specs[name] and all(journal[key] == command[key]
                    for key in ('outcome', 'exit_code', 'stdout', 'stderr')), 'original command binding changed')
            for stream in ('stdout', 'stderr'):
                require(capture.file_record(base / stream) == command[stream] and command[stream]['bytes'] <= 64 * 1024 * 1024,
                        'original command stream changed')
                paths['command_' + name + '_' + stream] = base / stream
            paths['command_' + name + '_journal'] = base / 'journal.json'
    return command_id, {name: capture.file_record(path) for name, path in paths.items()}


def validate_source(source_repo, expected, source_private, source_export, policy_sha, private):
    require(not private.exists(), 'fresh P2 private root required')
    original, source, policy, inputs = source_context(source_repo, expected, source_private, source_export, policy_sha, private)
    # This is the only full target-tree validation in this helper. It is S's
    # unchanged validator and runs once, before any observation is admitted.
    report = original.validate_export(source_export, source_repo, expected, source_private)
    command_id, bindings = report_bindings(original, report, source_repo, source_private, source_export, source)
    receipt = {'schema_version': 1, 'kind': 'original_source_validation_for_failure_observation',
               'source_sha': expected, 'source_tree': source['source_tree'], 'policy_sha': policy_sha,
               'policy_tree': policy['source_tree'], 'original_status': report['status'], 'original_source_tree': report['source_tree'],
               'native_failure_command': command_id, 'bindings': bindings,
               'runtime_accepted': False, 'release_authorized': False}
    require(len(canonical(receipt)) <= MAX_RECEIPT, 'source validation receipt too large')
    private.mkdir(parents=True)
    (private / 'inputs.json').write_bytes(canonical(inputs))
    (private / RECEIPT).write_bytes(canonical(receipt))
    return receipt


def bound_context(source_repo, expected, source_private, source_export, policy_sha, private):
    original, source, policy, inputs = source_context(source_repo, expected, source_private, source_export, policy_sha, private)
    require(read_json(private / 'inputs.json', 32768) == inputs, 'source observation roots changed')
    receipt = read_json(private / RECEIPT, MAX_RECEIPT)
    report = read_json(source_export / original.EXPORT, original.MAX_JSON)
    command_id, bindings = report_bindings(original, report, source_repo, source_private, source_export, source)
    expected_receipt = {'schema_version': 1, 'kind': 'original_source_validation_for_failure_observation',
                        'source_sha': expected, 'source_tree': source['source_tree'], 'policy_sha': policy_sha,
                        'policy_tree': policy['source_tree'], 'original_status': report['status'], 'original_source_tree': report['source_tree'],
                        'native_failure_command': command_id, 'bindings': bindings,
                        'runtime_accepted': False, 'release_authorized': False}
    require(receipt == expected_receipt, 'original validation bindings changed')
    return receipt, report


def project(source_repo, expected, source_private, source_export, policy_sha, private):
    receipt, report = bound_context(source_repo, expected, source_private, source_export, policy_sha, private)
    command_id = receipt['native_failure_command']
    require(command_id in failure.SCENES, 'no failed native scene to observe')
    observed = failure.project(source_private, expected, command_id)
    excerpt = load('present_excerpt', POLICY_ROOT / 'scripts/project-present-error-excerpt.py')
    selected = excerpt.project_stream(source_private / 'commands' / command_id / 'stderr',
                                     expected_record=report['commands'][-1]['stderr'])
    value = {'schema_version': 1, 'kind': 'present_failure_diagnostic_only', 'source_sha': expected,
             'source_tree': SOURCE_TREE, 'policy_sha': policy_sha, 'policy_tree': receipt['policy_tree'],
             'command_id': command_id, 'original_validation': capture.file_record(private / RECEIPT),
             'scope': 'failure_bindings_after_original_source_validation',
             'complete_build_audit_repeated': False, 'platform_facts_recollected': False,
             'runtime_failure': {'sha256': hashlib.sha256(canonical(observed)).hexdigest(), 'bytes': len(canonical(observed))},
             'present_error': {'sha256': hashlib.sha256(canonical(selected)).hexdigest(), 'bytes': len(canonical(selected))},
             'runtime_accepted': False, 'release_authorized': False}
    require(len(canonical(value)) <= MAX_ORIGIN, 'origin exceeds bound')
    return {ORIGIN: value, failure.EXPORT: observed, EXCERPT: selected, RECEIPT: receipt}


def observe(source_repo, expected, source_private, source_export, policy_sha, private, evidence):
    require(evidence.is_absolute() and not evidence.exists(), 'fresh absolute diagnostic export required')
    capture.no_links(evidence)
    capture.disjoint(source_repo, source_private, source_export, POLICY_ROOT, private, evidence)
    values = project(source_repo, expected, source_private, source_export, policy_sha, private)
    evidence.mkdir(parents=True)
    for name, value in values.items(): (evidence / name).write_bytes(canonical(value))
    return values[ORIGIN]


def validate_export(directory, source_repo, expected, source_private, source_export, policy_sha, private):
    capture.no_links(directory)
    values = project(source_repo, expected, source_private, source_export, policy_sha, private)
    require({path.name for path in directory.iterdir()} == set(values), 'unexpected P2 diagnostic artifact')
    limits = {ORIGIN: MAX_ORIGIN, RECEIPT: MAX_RECEIPT, failure.EXPORT: failure.MAX_PUBLIC, EXCERPT: 16384}
    for name, value in values.items():
        path = directory / name
        require(read_json(path, limits[name]) == value, 'diagnostic export differs from bound observations')
    return values[ORIGIN]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('source-repo', 'source-private', 'source-export', 'private'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--source-sha', required=True); parser.add_argument('--policy-sha', required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--validate-source', action='store_true'); mode.add_argument('--evidence', type=Path)
    mode.add_argument('--validate-evidence', type=Path)
    args = parser.parse_args()
    try:
        require(sys.platform == 'win32', 'native Windows evidence required')
        values = (args.source_repo, args.source_sha, args.source_private, args.source_export, args.policy_sha, args.private)
        if args.validate_source:
            result = validate_source(*values)
            # Only fixed values enter the workflow control file.
            if os.environ.get('GITHUB_OUTPUT'):
                with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
                    output.write('validated=true\n')
                    output.write('native_failure=' + ('true' if result['native_failure_command'] is not None else 'false') + '\n')
        elif args.validate_evidence: validate_export(args.validate_evidence, *values)
        else: observe(*values, args.evidence)
        print('P2 failure observation validated; runtime_accepted=false; release_authorized=false.')
        return 0
    except Exception:
        print('P2 diagnostic unavailable or invalid; original source evidence is unchanged.', file=sys.stderr)
        return 1


if __name__ == '__main__': raise SystemExit(main())
