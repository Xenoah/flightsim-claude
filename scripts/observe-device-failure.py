#!/usr/bin/env python3
"""Add P3 structured device facts to P2's unchanged bound failure observation.

One original strict S validation; no native invocation or repeated audit here.
The isolated workflow also requires original-evidence upload success first.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location('device_p2_observation', Path(__file__).with_name('observe-present-failure.py'))
p2 = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(p2)
device = p2.load('device_facts', Path(__file__).with_name('project-device-error-facts.py'))
ORIGIN, FACTS = 'device-origin.json', 'device-error.json'
require, canonical, capture = p2.require, p2.canonical, p2.capture


def record(value):
    raw = canonical(value)
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)}


def project(*args):
    values = p2.project(*args)
    original = values[p2.ORIGIN]
    source_private = args[2]
    command_id = original['command_id']
    stream_record = values[p2.failure.EXPORT]['streams']['stderr']['record']
    facts = device.project_stream(source_private / 'commands' / command_id / 'stderr', expected_record=stream_record)
    # Reconcile the new total against the unchanged old scanner, not a guessed
    # interpretation of its ERROR marker. Fatal and post-fatal counts are explicit.
    require(facts['marker_lines_total'] == values[p2.failure.EXPORT]['streams']['stderr']['error_lines'], 'old/new marker accounting differs')
    values[FACTS] = facts
    values[ORIGIN] = {'schema_version': 1, 'kind': 'structured_device_failure_diagnostic_only',
        **{key: original[key] for key in ('source_sha', 'source_tree', 'policy_sha', 'policy_tree', 'command_id', 'original_validation')},
        'present_observation': record(original), 'device_error': record(facts),
        'scope': 'same_bound_S_P_build_executable_stream_and_journal_as_present_observation',
        'complete_build_audit_repeated': False, 'platform_facts_recollected': False,
        'runtime_accepted': False, 'release_authorized': False}
    require(len(canonical(values[ORIGIN])) <= p2.MAX_ORIGIN, 'device origin exceeds bound')
    return values


def observe(*args, evidence):
    source_repo, _, source_private, source_export, _, private = args
    require(evidence.is_absolute() and not evidence.exists(), 'fresh absolute diagnostic export required')
    capture.no_links(evidence)
    capture.disjoint(source_repo, source_private, source_export, p2.POLICY_ROOT, private, evidence)
    values = project(*args)
    evidence.mkdir(parents=True)
    for name, value in values.items(): (evidence / name).write_bytes(canonical(value))
    return values[ORIGIN]


def validate_export(directory, *args):
    capture.no_links(directory)
    values = project(*args)
    require({path.name for path in directory.iterdir()} == set(values), 'unexpected P3 diagnostic artifact')
    limits = {p2.ORIGIN: p2.MAX_ORIGIN, p2.RECEIPT: p2.MAX_RECEIPT, p2.failure.EXPORT: p2.failure.MAX_PUBLIC,
              p2.EXCERPT: p2.MAX_ORIGIN * 2, ORIGIN: p2.MAX_ORIGIN, FACTS: device.MAX_PUBLIC}
    for name, value in values.items():
        require(p2.read_json(directory / name, limits[name]) == value, 'diagnostic export differs from bound observations')
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
            result = p2.validate_source(*values)
            if os.environ.get('GITHUB_OUTPUT'):
                with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
                    output.write('validated=true\n')
                    output.write('native_failure=' + ('true' if result['native_failure_command'] is not None else 'false') + '\n')
        elif args.validate_evidence: validate_export(args.validate_evidence, *values)
        else: observe(*values, evidence=args.evidence)
        print('P3 failure observation validated; runtime_accepted=false; release_authorized=false.')
        return 0
    except Exception:
        print('P3 diagnostic unavailable or invalid; original source evidence is unchanged.', file=sys.stderr)
        return 1


if __name__ == '__main__': raise SystemExit(main())
