#!/usr/bin/env python3
"""Observe UI prerequisites without blocking independent native fact collection.

No simulator launch, desktop capture, installation or security-policy change.
Operationally missing/timed-out probes remain factual unavailable/unknown rows;
they never create appearance/lifecycle/publication acceptance.
"""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location('ui_capability_capture', Path(__file__).with_name('capture-analytical-swift-msvc.py'))
capture = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(capture)
require = capture.require
EXPORT = 'ui-capabilities.json'
MAX_PUBLIC = 32768
NAMES = ('ocr', 'desktop')
OUTCOMES = ('succeeded', 'failed', 'timed_out', 'launch_error', 'termination_failed')


def commands(repo):
    system = Path(os.environ.get('SystemRoot', 'C:/Windows'))
    require(system.is_absolute(), 'native SystemRoot must be absolute')
    return {
        'ocr': [str(system / 'System32/WindowsPowerShell/v1.0/powershell.exe'), '-NoProfile', '-NonInteractive',
                '-File', str(repo / 'scripts/windows-ui-ocr.ps1'), '-Probe'],
        'desktop': [sys.executable, str(repo / 'scripts/observe-analytical-windows-ui.py'), '--probe-desktop'],
    }


def state(name, result, raw):
    if result['outcome'] == 'failed': return 'unavailable'
    if result['outcome'] != 'succeeded': return 'unknown'
    if name == 'desktop':
        return ('available' if raw.strip() == b'Default interactive Windows desktop available; no simulator launched.' else 'unknown')
    try:
        value = json.loads(raw.decode('utf-8-sig'))
    except (UnicodeError, ValueError):
        return 'unknown'
    return ('available' if isinstance(value, dict) and set(value) == {'schema_version', 'engine', 'language', 'available'}
            and type(value['schema_version']) is int and value['schema_version'] == 1
            and value['engine'] == 'Windows.Media.Ocr' and value['language'] == 'en-US' and value['available'] is True else 'unknown')


def validate_projection(value):
    require(isinstance(value, dict) and set(value) == {'schema_version', 'kind', 'source_sha', 'probes',
            'appearance_accepted', 'lifecycle_accepted', 'release_authorized'}, 'unknown capability projection field')
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['kind'] == 'ui_capability_observations_not_qualification'
            and capture.hex_string(value['source_sha'], 40), 'wrong capability identity')
    require(all(value[key] is False for key in ('appearance_accepted', 'lifecycle_accepted', 'release_authorized')),
            'capability observations cannot approve qualification')
    require(isinstance(value['probes'], dict) and set(value['probes']) == set(NAMES), 'missing capability observation')
    for row in value['probes'].values():
        require(set(row) == {'state', 'outcome', 'exit_code', 'stdout', 'stderr'}
                and row['state'] in ('available', 'unavailable', 'unknown') and row['outcome'] in OUTCOMES,
                'unknown capability outcome')
        code = row['exit_code']
        require((type(code) is int and -(2**32) <= code <= 2**32) or
                (code is None and row['outcome'] in ('launch_error', 'termination_failed')), 'invalid capability exit status')
        require(row['outcome'] not in ('succeeded', 'failed') or (row['outcome'] == 'succeeded') == (code == 0),
                'capability outcome and exit disagree')
        require(all(capture.valid_record(row[key]) and row[key]['bytes'] <= 1024 * 1024 for key in ('stdout', 'stderr')),
                'unbounded capability stream record')
        require(row['state'] != 'available' or (row['outcome'] == 'succeeded' and code == 0), 'unobserved capability availability')
    require(len((json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii')) <= MAX_PUBLIC, 'capability export budget exceeded')


def project(private, repo, expected):
    capture.no_links(private)
    receipt = capture.read_private_json(private / 'capture.json')
    require(set(receipt) == {'source_sha', 'cwd', 'commands'} and receipt['source_sha'] == expected
            and receipt['cwd'] == str(repo) and receipt['commands'] == commands(repo), 'capability invocation changed')
    value = {'schema_version': 1, 'kind': 'ui_capability_observations_not_qualification', 'source_sha': expected,
             'appearance_accepted': False, 'lifecycle_accepted': False, 'release_authorized': False, 'probes': {}}
    for name in NAMES:
        journal = capture.read_private_json(private / (name + '.command.json'))
        require(journal['command'] == receipt['commands'][name] and journal['cwd'] == str(repo), 'capability command changed')
        row = {key: journal[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}
        for stream in ('stdout', 'stderr'):
            require(row[stream] == capture.file_record(private / (name + '.' + stream))
                    and row[stream]['bytes'] <= 1024 * 1024, 'capability raw stream changed or oversized')
        row['state'] = state(name, row, (private / (name + '.stdout')).read_bytes())
        value['probes'][name] = row
    validate_projection(value)
    return value


def collect(private, repo, expected, *, env=None):
    require(sys.platform == 'win32' and capture.hex_string(expected, 40), 'native Windows and exact source required')
    capture.no_links(private); require(not private.exists(), 'fresh capability evidence root required')
    private.mkdir(parents=True)
    planned = commands(repo)
    capture.write_json(private / 'capture.json', {'source_sha': expected, 'cwd': str(repo), 'commands': planned})
    for name in NAMES:
        capture.execute(planned[name], cwd=repo, env=env or os.environ.copy(), stdout=private / (name + '.stdout'),
                        stderr=private / (name + '.stderr'), journal=private / (name + '.command.json'), timeout=30)
    value = project(private, repo, expected)
    capture.write_json(private / EXPORT, value)
    return value
