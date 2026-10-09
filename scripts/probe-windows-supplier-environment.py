#!/usr/bin/env python3
"""Export environment-only installed supplier facts, never application evidence.

Reuse the runtime collector and its unchanged public projection. No linker
trace, build fingerprint or executable is supplied. The warnings-only cfg
query is hypothetical, not an application invocation. Raw paths, command output
and notice bytes stay private. The source SHA identifies the probe scripts.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location(
    'runtime_facts', Path(__file__).with_name('collect-analytical-runtime-facts.py'))
facts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(facts)

NAME = 'environment-only.json'
KIND = 'windows_supplier_environment_not_approval'
MAX_RECEIPT = 4096
BASE_SHA = '55094fa928fbb8f907a2103740ee6824171c26e7'


def image_identity(env):
    """Only public runner image tokens; never a runner name or other environment."""
    selected = {}
    for key, value in env.items():
        if key.upper() in ('IMAGEOS', 'IMAGEVERSION'):
            key = key.upper()
            facts.require(key not in selected or selected[key] == value, 'ambiguous runner image identity')
            selected[key] = value
    image_os, image_version = selected.get('IMAGEOS'), selected.get('IMAGEVERSION')
    if image_os is not None:
        # Official image token in the 20260925.250 Windows VS2026 image.
        # Admit this one literal suffix, never arbitrary image metadata.
        facts.safe_token(image_os, 32, r'(?:win[0-9]{2,4}|win25-vs2026)')
    if image_version is not None:
        facts.version(image_version)
    return {'image_os': image_os, 'image_version': image_version}


def validate_runtime(runtime, source_sha):
    facts.validate_projection(runtime)
    facts.require(isinstance(source_sha, str) and re.fullmatch('[0-9a-f]{40}', source_sha)
                  and runtime['source_sha'] == source_sha, 'invalid probe source identity')
    facts.require(runtime['rust']['recipe_settings_supplied'] is True,
                  'expected hypothetical warnings-only cfg query')
    facts.require(runtime['final_link'] == facts.final_link_facts(None),
                  'environment probe must not contain application build inputs')
    return runtime


def projection(private, source_sha):
    # Replays source-defined discovery against original installed files and
    # private receipts. Do not substitute an offline-only receipt validation.
    runtime = validate_runtime(facts.project(private), source_sha)
    manifest = json.loads((Path(private) / facts.PRIVATE_NAME).read_bytes())
    facts.require(manifest['recipe_cfg_args'] == ['-D', 'warnings'],
                  'only the source-defined hypothetical cfg is allowed')
    raw = facts.canonical(runtime)
    receipt = {
        'schema_version': 1, 'kind': KIND, 'scope': 'environment_only',
        'probe_source_sha': source_sha, 'probe_base_sha': BASE_SHA,
        'application_build_inputs': 'not_supplied',
        'requested_runner_label': 'windows-latest', 'runner_image': image_identity(os.environ),
        'runtime_facts': {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)},
    }
    facts.require(len(facts.canonical(receipt)) <= MAX_RECEIPT, 'environment receipt budget exceeded')
    return {facts.PROJECTION_NAME: raw, NAME: facts.canonical(receipt)}


def separate_directories(private, evidence):
    private, evidence = Path(os.path.abspath(private)), Path(os.path.abspath(evidence))
    facts.no_links(private)
    facts.no_links(evidence)
    facts.require(not private.is_relative_to(evidence) and not evidence.is_relative_to(private),
                  'private and public directories must be separate')
    return private, evidence


def export(private, evidence, source_sha):
    private, evidence = separate_directories(private, evidence)
    facts.require(not evidence.exists(), 'fresh public directory required')
    outputs = projection(private, source_sha)
    evidence.mkdir(parents=True)
    for name, raw in outputs.items():
        (evidence / name).write_bytes(raw)
    return validate_export(private, evidence, source_sha)


def validate_export(private, evidence, source_sha):
    private, evidence = separate_directories(private, evidence)
    facts.require(evidence.is_dir() and {path.name for path in facts.children(evidence)}
                  == {NAME, facts.PROJECTION_NAME}, 'public directory must contain exactly the two JSON files')
    expected = projection(private, source_sha)
    for name, raw in expected.items():
        maximum = MAX_RECEIPT if name == NAME else facts.MAX_PUBLIC
        record = facts.file_record(evidence / name, maximum)
        facts.require((evidence / name).read_bytes() == raw,
                      'public export differs from private environment evidence')
        facts.require(facts.file_record(evidence / name, maximum) == record,
                      'export changed during validation')
    return expected



def error_code(error):
    """Fixed source-defined labels only; never echo exception text or paths."""
    if isinstance(error, UnicodeError):
        return 'encoding_error'
    if isinstance(error, ValueError):
        return {
            'unsafe identity': 'invalid_identity',
            'file budget or type rejected': 'file_budget_or_type',
            'aggregate file budget exceeded': 'aggregate_file_budget',
            'linked or reparse path rejected': 'linked_path',
            'directory entry budget exceeded': 'directory_budget',
            'installed version budget exceeded': 'version_budget',
            'query output budget exceeded': 'query_budget',
            'native Windows is required': 'native_windows_required',
        }.get(str(error), 'invalid_value')
    if isinstance(error, OSError):
        return 'io_error'
    if isinstance(error, KeyError):
        return 'missing_field'
    if isinstance(error, TypeError):
        return 'invalid_type'
    return 'subprocess_error'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--private', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--validate', action='store_true')
    args = parser.parse_args(argv)
    stage = 'directories'
    try:
        private, evidence = separate_directories(args.private, args.evidence)
        stage = 'image_identity'
        image_identity(os.environ)
        if args.validate:
            stage = 'export_validation'
            validate_export(private, evidence, args.source_sha)
        else:
            stage = 'runtime_collection'
            facts.require(not evidence.exists(), 'fresh public directory required')
            facts.collect_runtime_facts(private, source_sha=args.source_sha, recipe_cfg_args=['-D', 'warnings'])
            stage = 'public_export'
            export(private, evidence, args.source_sha)
        print('Environment-only supplier facts validated; no application build or release approval.')
        return 0
    except (ValueError, OSError, KeyError, TypeError, UnicodeError, subprocess.SubprocessError) as error:
        # Exceptions can include a private path or untrusted tool output.
        print('Environment probe failed [' + stage + ':' + error_code(error)
              + ']; no public upload is allowed.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
