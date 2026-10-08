#!/usr/bin/env python3
"""Read-only post-step diagnostics for clean source S and separately reviewed P.

Run S's unchanged qualification first. This helper never builds or launches the
simulator, rewrites S/original evidence, or creates qualification/approval.
"""
from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
import re
import sys

POLICY_ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = 'fe7373e96396131ef4a1c2a9af9cfabdb8aab82b'
SOURCE_TREE = '6dc4d8ed190ccc43733182d03e089f84f0eaaef6'
ORIGIN = 'diagnostic-origin.json'
MAX_ORIGIN = 8192


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec); spec.loader.exec_module(value)
    return value


capture = load('sidecar_capture', POLICY_ROOT / 'scripts/capture-analytical-swift-msvc.py')
failure = load('sidecar_failure', POLICY_ROOT / 'scripts/project-analytical-runtime-failure.py')
native = load('sidecar_native', POLICY_ROOT / 'scripts/project-analytical-native-evidence.py')
facts = native.runtime_facts
require = capture.require


def windows_observation():
    require(sys.platform == 'win32', 'native Windows post-step required')
    value = sys.getwindowsversion()
    names = ('major', 'minor', 'build', 'platform', 'service_pack_major', 'service_pack_minor', 'product_type')
    observed = {name: getattr(value, name) for name in names}
    observed['platform_version'] = list(value.platform_version)
    require(len(observed['platform_version']) == 3 and all(type(number) is int and 0 <= number <= 2**32 - 1
            for number in [*(observed[name] for name in names), *observed['platform_version']]), 'invalid native Windows version')
    env = {key.upper(): item for key, item in os.environ.items()}
    image = env.get('IMAGEOS')
    version = env.get('IMAGEVERSION')
    observed['runner_image_os'] = image if image in ('win19', 'win22', 'win25') else None
    observed['runner_image_version'] = (version if isinstance(version, str)
                                       and re.fullmatch(r'[0-9]{8}\.[0-9]{1,6}\.[0-9]{1,6}', version) else None)
    return observed


def inputs(source_repo, expected, source_private, source_export, policy_sha):
    require(expected == SOURCE_SHA and capture.hex_string(policy_sha, 40), 'this diagnostic is bound to exact fe source')
    for path in (source_repo, source_private, source_export, POLICY_ROOT):
        require(path.is_absolute(), 'absolute source/policy inputs required'); capture.no_links(path)
    capture.disjoint(source_repo, source_private, source_export, POLICY_ROOT)
    policy = capture.source_evidence(POLICY_ROOT, policy_sha)
    source = capture.source_evidence(source_repo, expected)
    require(source['source_tree'] == SOURCE_TREE, 'diagnostic source tree changed')
    # Use S's own strict validators. New P schemas never relax old S validation.
    original = load('original_qualification_for_diagnostics', source_repo / 'scripts/qualify-analytical-swift-windows.py')
    report = original.validate_export(source_export, source_repo, expected, source_private)
    require(report['phase'] == 'runtime' and report['source_tree'] == SOURCE_TREE, 'original exact native runtime evidence required')
    build_private, build_text = source_private / 'build-private', source_private / 'build-text'
    build = capture.validate_export(build_text, repo=source_repo, expected=expected, private=build_private)
    require(build['status'] == capture.PASS, 'diagnostic needs the actual completed original native build audit')
    return report, policy, source, build_private, build_text


def projected_origin(source_repo, expected, source_private, source_export, policy_sha, private):
    report, policy, source, build_private, build_text = inputs(source_repo, expected, source_private, source_export, policy_sha)
    projected_facts = native.validate_runtime_facts(private / 'runtime-facts', source_repo, expected, build_private, build_text)
    value = {'schema_version': 1, 'kind': 'separate_native_diagnostic_sidecars',
             'source_sha': expected, 'source_tree': source['source_tree'], 'policy_sha': policy_sha,
             'policy_tree': policy['source_tree'], 'windows': windows_observation(),
             'original_status': report['status'], 'release_authorized': False, 'runtime_accepted': False,
             'bindings': {'original_qualification': capture.file_record(source_export / 'qualification.json'),
                          'original_build': capture.file_record(build_text / capture.EXPORT_NAME),
                          'executable': capture.file_record(build_private / 'target-analytic' / capture.check.TARGET / 'release/flightsim-app.exe'),
                          'runtime_facts': capture.file_record(private / 'runtime-facts' / facts.PROJECTION_NAME)},
             'failure_projection': None}
    failed = None
    if report['status'] == 'failed' and report['commands'] and report['commands'][-1]['id'] in failure.COMMANDS:
        failed = failure.project(source_private, expected, report['commands'][-1]['id'])
        data = failure.canonical(failed)
        import hashlib
        value['failure_projection'] = {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}
    require(len(failure.canonical(value)) <= MAX_ORIGIN, 'diagnostic origin exceeds export bound')
    return value, projected_facts, failed


def validate_export(directory, source_repo, expected, source_private, source_export, policy_sha, private):
    for path in (directory, private): capture.no_links(path)
    origin, projected_facts, failed = projected_origin(source_repo, expected, source_private, source_export, policy_sha, private)
    allowed = {ORIGIN, facts.PROJECTION_NAME} | ({failure.EXPORT} if failed is not None else set())
    require({path.name for path in directory.iterdir()} == allowed, 'unexpected diagnostic artifact')
    for name, value, limit in ((ORIGIN, origin, MAX_ORIGIN), (facts.PROJECTION_NAME, projected_facts, facts.MAX_PUBLIC),
                               *([(failure.EXPORT, failed, failure.MAX_PUBLIC)] if failed is not None else [])):
        path = directory / name; capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= limit
                and path.read_bytes() == failure.canonical(value), 'diagnostic artifact differs from exact private observations')
    return origin


def observe(source_repo, expected, source_private, source_export, policy_sha, private, evidence):
    require(sys.platform == 'win32', 'native Windows observation required')
    for path in (private, evidence):
        require(path.is_absolute(), 'absolute diagnostic roots required'); capture.no_links(path)
        require(not path.exists(), 'fresh diagnostic roots required')
    capture.disjoint(source_repo, source_private, source_export, POLICY_ROOT, private, evidence)
    _, _, _, build_private, build_text = inputs(source_repo, expected, source_private, source_export, policy_sha)
    private.mkdir(parents=True)
    facts.collect_runtime_facts(private / 'runtime-facts', source_sha=expected, recipe_cfg_args=['-D', 'warnings'],
        linker_trace=build_private / 'capture/analytic/build.stderr',
        audited_executable=build_private / 'target-analytic' / capture.check.TARGET / 'release/flightsim-app.exe')
    origin, projected_facts, failed = projected_origin(source_repo, expected, source_private, source_export, policy_sha, private)
    evidence.mkdir(parents=True)
    for name, value in ((ORIGIN, origin), (facts.PROJECTION_NAME, projected_facts),
                         *([(failure.EXPORT, failed)] if failed is not None else [])):
        (evidence / name).write_bytes(failure.canonical(value))
    return validate_export(evidence, source_repo, expected, source_private, source_export, policy_sha, private)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('source-repo', 'source-private', 'source-export', 'private'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--source-sha', required=True); parser.add_argument('--policy-sha', required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--evidence', type=Path); mode.add_argument('--validate-evidence', type=Path)
    args = parser.parse_args()
    try:
        values = (args.source_repo, args.source_sha, args.source_private, args.source_export, args.policy_sha, args.private)
        if args.validate_evidence:
            validate_export(args.validate_evidence, *values)
        else:
            observe(*values, args.evidence)
        print('Separate diagnostic sidecars verified; runtime_accepted=false; release_authorized=false.')
        return 0
    except Exception:
        # Never expose private source paths or raw validation error text.
        print('Diagnostic sidecars unavailable or invalid; original qualification evidence is unchanged.', file=sys.stderr)
        return 1


if __name__ == '__main__': raise SystemExit(main())
