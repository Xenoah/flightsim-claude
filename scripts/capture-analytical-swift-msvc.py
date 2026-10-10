#!/usr/bin/env python3
"""Capture the strict analytical Swift build recipe on native Windows MSVC.

Build/artifact evidence only. Never execute FlightSim, run acceptance/readback,
stage a bundle, publish binaries, or turn any remaining recipe gate into a pass.
Raw data remains in --private; --evidence contains one schema-checked JSON file.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import time


SPEC = importlib.util.spec_from_file_location('capture_recipe', Path(__file__).with_name('check-analytical-swift-recipe.py'))
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)
require = check.require
IDENTITY = 'analytical-swift-msvc-build-capture-v1'
CONTRACT = 'scripts/analytical-swift-capture-contract.json'
SOURCE_ADMISSION = 'analytical-swift-alpha22-version-capture-source-admission-v1'
REFERENCE = 'reference/bevy_core_pipeline-0.18.1.crate'
BOUND_FILES = {
    'scripts/capture-analytical-swift-msvc.py',
    'scripts/tests/test_analytical_swift_capture.py',
    '.github/workflows/analytical-swift-msvc-build.yml',
    'docs/release/analytical-swift-msvc-capture.md',
}
INHERITED_CONTRACTS = {'scripts/analytical-swift-source-contract.json': 'f018b328d06f464f7d30d3a272dba49ac00a98bd8ad96c7b846a6b09fc46a173', 'scripts/replay-candidate-contract.json': 'd5575c552cdf2522da4a2e8a50c9b41ff800632dae55138700bbed3b165e7412'}

MODES = ('analytic', 'ordinary')
OUTPUTS = {'rustc': 'rustc.txt', 'graph': 'graph.txt', 'build': 'messages.jsonl', 'metadata': 'metadata.json'}
COMMAND_IDS = ('fetch', *(f'{mode}-{name}' for mode in MODES for name in (*OUTPUTS, 'notices')), 'audit')
STAGES = ('preflight', *COMMAND_IDS, 'freeze', 'source-recheck', 'freeze-recheck', 'complete')
PASS = 'build_evidence_checked_native_and_distribution_unqualified'
EXPORT_NAME = 'build-evidence.json'
MAX_EXPORT_BYTES = 32768
MIN_START_FREE = 12 * 1024 ** 3
MIN_COMMAND_FREE = 2 * 1024 ** 3


def write_json(path, value):
    # Canonical JSON is ASCII with LF on every host. Text-mode writes translate
    # LF to CRLF on Windows, including the report the upload validator rejects.
    path.write_bytes((json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii'))


def no_links(path):
    """Reject symlinks and Windows junctions/reparse points before resolve()."""
    for part in (path, *path.parents):
        if part.exists() or part.is_symlink():
            details = part.lstat()
            require(not stat.S_ISLNK(details.st_mode)
                    and not getattr(details, 'st_file_attributes', 0) & 0x400,
                    'linked/reparse path is forbidden')


def disjoint(*paths):
    for i, first in enumerate(paths):
        for second in paths[i + 1:]:
            require(not first.is_relative_to(second) and not second.is_relative_to(first),
                    'source, private and export roots must be disjoint')


def reject_configuration(repo, environ):
    prefixes = ('CARGO_BUILD_', 'CARGO_PROFILE_', 'CARGO_TARGET_', 'CARGO_ENCODED_',
                'CARGO_REGISTRIES_', 'CARGO_SOURCE_', 'CARGO_ALIAS_', 'CC_', 'CXX_', 'AR_')
    names = {'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS',
             'RUSTDOC', 'RUSTDOCFLAGS', 'CARGO_INCREMENTAL', 'RUSTUP_TOOLCHAIN',
             'CARGO_ENCODED_RUSTDOCFLAGS', 'CC', 'CXX', 'AR', 'CFLAGS', 'CXXFLAGS',
             'CPPFLAGS', 'LDFLAGS', 'CL', '_CL_', 'LINK', '_LINK_'}
    require(not any(value and (name.upper() in names or name.upper().startswith(prefixes)
                              or (name.upper().startswith('CARGO_') and name.upper() not in ('CARGO_HOME', 'CARGO_TERM_COLOR')))
                    for name, value in environ.items()), 'inherited compiler/profile override')
    for ancestor in (repo, *repo.parents):
        for filename in ('config', 'config.toml'):
            path = ancestor / '.cargo' / filename
            require(not path.exists() and not path.is_symlink(), 'ancestor Cargo configuration is forbidden')


def verify_canonical_sources(repo, source):
    require(source['canonical_git_object_format'] in ('sha1', 'sha256'), 'unknown Git object format')
    # Git status can hide assume-unchanged/skip-worktree modifications. Compare
    # every tracked file to its canonical blob, not only the inherited pins.
    for record in source['files']:
        path = repo / record['path']
        no_links(path)
        value = hashlib.new(source['canonical_git_object_format'])
        value.update(('blob ' + str(path.stat().st_size) + '\0').encode('ascii'))
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                value.update(chunk)
        require(value.hexdigest() == record['canonical_git_blob'], 'tracked checkout differs from canonical source')


def source_evidence(repo, expected):
    source = check.source_evidence(repo, expected)
    verify_canonical_sources(repo, source)
    contract = json.loads((repo / CONTRACT).read_text(encoding='utf-8'))
    require(set(contract) == {'schema_version', 'identity', 'base_reviewed_source', 'base_reviewed_runtime_tree', 'source_migration_sha256', 'source_admission', 'source_recipe', 'inherited_contracts', 'source_sha256'}
            and type(contract['schema_version']) is int and contract['schema_version'] == 1
            and contract['identity'] == IDENTITY
            and contract['base_reviewed_source'] == check.candidate.REVIEWED_RUNTIME_SOURCE
            and contract['base_reviewed_runtime_tree'] == check.candidate.REVIEWED_RUNTIME_TREE
            and contract['source_migration_sha256'] == check.candidate.ALPHA22_MIGRATION_SHA256
            and contract['source_admission'] == SOURCE_ADMISSION
            and contract['source_recipe'] == check.SOURCE_RECIPE,
            'invalid additive capture source contract')
    require(contract['inherited_contracts'] == INHERITED_CONTRACTS, 'inherited contracts changed')
    require(set(contract['source_sha256']) == BOUND_FILES, 'capture boundary changed')
    files = {record['path']: record for record in source['files']}
    for relative in BOUND_FILES | {CONTRACT} | set(INHERITED_CONTRACTS):
        require(relative in files, 'capture input must be tracked')
        record = files[relative]
        no_links(repo / relative)
        blob = subprocess.check_output(['git', 'cat-file', 'blob', record['canonical_git_blob']], cwd=repo)
        actual = hashlib.sha256(blob).hexdigest()
        require(actual == record['checkout_sha256'], 'capture checkout differs from canonical source')
        if relative != CONTRACT:
            expected_hash = INHERITED_CONTRACTS.get(relative, contract['source_sha256'].get(relative))
            require(actual == expected_hash, 'capture source pin changed')
    source['capture_contract_sha256'] = check.digest(repo / CONTRACT)
    return source


def file_record(path):
    no_links(path)
    return check.file_record(path)


def snapshot_tree(root):
    no_links(root)
    require(root.is_dir(), 'missing frozen tree')
    result = {}
    for path in sorted(root.rglob('*')):
        no_links(path)
        if path.is_file():
            result[path.relative_to(root).as_posix()] = file_record(path)
        else:
            require(path.is_dir(), 'non-regular frozen tree entry')
    require(result, 'empty frozen tree')
    return result


def stop_process_tree(process):
    if os.name == 'nt':
        # Kill descendants too: a timed-out Cargo must not keep writing a tree.
        taskkill = Path(os.environ['SystemRoot']) / 'System32/taskkill.exe'
        killed = subprocess.run([str(taskkill), '/PID', str(process.pid), '/T', '/F'],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        require(killed.returncode == 0 or process.poll() is not None, 'could not terminate process tree')
    else:
        # Used only by real subprocess unit tests; production entry rejects Linux.
        os.killpg(process.pid, signal.SIGKILL)
    return process.wait(timeout=30)


def execute(command, *, cwd, env, stdout, stderr, journal, timeout):
    """Retain byte-exact streams and the observed status, including failures."""
    record = {'command': command, 'cwd': str(cwd), 'exit_code': None,
              'outcome': 'running', 'timeout_seconds': timeout}
    write_json(journal, record)
    started = time.monotonic()
    with stdout.open('xb') as out, stderr.open('xb') as err:
        try:
            options = {'creationflags': subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == 'nt' else {'start_new_session': True}
            process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err, **options)
        except OSError as error:
            record['outcome'] = 'launch_error'
            record['launch_error'] = str(error)  # private only, never projected
        else:
            try:
                record['exit_code'] = process.wait(timeout=timeout)
                record['outcome'] = 'succeeded' if record['exit_code'] == 0 else 'failed'
            except subprocess.TimeoutExpired:
                record['outcome'] = 'timed_out'
                try:
                    record['exit_code'] = stop_process_tree(process)
                except (ValueError, OSError, subprocess.SubprocessError) as error:
                    record['outcome'] = 'termination_failed'
                    record['exit_code'] = process.poll()
                    record['termination_error'] = str(error)  # private only
    record.update(elapsed_seconds=round(time.monotonic() - started, 3),
                  stdout=file_record(stdout), stderr=file_record(stderr))
    write_json(journal, record)
    return record


def frozen_artifacts(directory, target, mode, *, repo=None, upstream_archive=None):
    graph = check.tone.parse_graph((directory / 'graph.txt').read_text(encoding='utf-8'))
    messages = [json.loads(line) for line in (directory / 'messages.jsonl').read_text(encoding='utf-8').splitlines()]
    result = check.tone.audit(directory / 'graph.txt', directory / 'messages.jsonl', mode, 'app',
                              recipe=check.IDENTITY if mode == 'analytic' else None,
                              source_recipe=check.SOURCE_RECIPE, source_root=repo,
                              upstream_archive=upstream_archive, target=check.TARGET)
    root, libraries = check.tone.select_artifacts(messages, graph, 'app',
                                                  source_recipe=check.SOURCE_RECIPE, source_root=repo)
    render = [item for item in messages if item.get('reason') == 'compiler-artifact'
              and item['target']['name'] == 'flightsim_render' and item['target']['kind'] == ['lib']]
    require(len(render) == 1, 'expected one render artifact')
    for item in [root, *libraries.values(), render[0]]:
        check.release_profile(item)
    executable = target / check.TARGET / 'release/flightsim-app.exe'
    require(Path(root['executable']) == executable, 'wrong executable tree')
    paths = [executable, Path(result['core_pipeline_depinfo']['path'])]
    for item in result['compiled']:
        paths += [Path(item['rlib']), Path(item['fingerprint'])]
    render_libs = [Path(p) for p in render[0]['filenames'] if p.endswith('.rlib')]
    require(len(render_libs) == 1 and render_libs[0].parent == executable.parent / 'deps', 'wrong render library')
    library = render_libs[0]
    suffix = library.stem.removeprefix('libflightsim_render-')
    require(suffix != library.stem, 'wrong render library name')
    paths += [library, executable.parent / '.fingerprint' / ('flightsim-render-' + suffix) / 'lib-flightsim_render.json']
    app_fingerprints = list((executable.parent / '.fingerprint').glob('flightsim-app-*/bin-flightsim-app.json'))
    require(len(app_fingerprints) == 1, 'expected one fresh app fingerprint')
    paths += app_fingerprints
    require(all(path.is_relative_to(target) for path in paths), 'foreign frozen artifact')
    return {str(path): file_record(path) for path in paths}


def hex_string(value, length):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(length) + '}', value) is not None


def valid_record(value):
    return (isinstance(value, dict) and set(value) == {'sha256', 'bytes'}
            and hex_string(value['sha256'], 64) and type(value['bytes']) is int
            and 0 <= value['bytes'] <= 1024 ** 4)


def read_private_json(path):
    no_links(path)
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= 64 * 1024 ** 2,
            'private validation input must be independent bounded JSON')
    return json.loads(path.read_text(encoding='utf-8'))


def projected_builds(receipt, report):
    require(report['status'] == PASS and report['release_authorized'] is False
            and report['required_unexecuted_gates'] == check.GATES, 'wrong private audit status')
    source_recipe = receipt.get('source_recipe', check.tone.UPSTREAM_THREE_LUT_SOURCE)
    require(report.get('source_recipe', check.tone.UPSTREAM_THREE_LUT_SOURCE) == source_recipe,
            'audit and capture source recipes differ')
    if source_recipe == check.SOURCE_RECIPE:
        require(valid_record(receipt.get('upstream_archive'))
                and report.get('upstream_archive') == receipt['upstream_archive'],
                'audit and capture private reference bindings differ')
    builds = {}
    for mode in MODES:
        artifact = report['builds'][mode]['artifact']
        record = receipt['modes'][mode]
        require(artifact.get('source_recipe', check.tone.UPSTREAM_THREE_LUT_SOURCE) == source_recipe,
                'compiled source recipe differs')
        executable = record['frozen_artifacts'][artifact['executable']]
        require(file_record(Path(artifact['executable'])) == executable
                and artifact['executable_sha256'] == executable['sha256']
                and artifact['executable_bytes'] == executable['bytes'], 'audited executable changed')
        builds[mode] = {'executable': executable, 'inventory': record['inventory'],
                       'metadata': record['results']['metadata']['stdout'],
                       'frozen_artifact_count': len(record['frozen_artifacts']),
                       'lut_payloads_found': sum(item['full_bytes_offset'] >= 0 for item in artifact['payloads'])}
    return builds


def verify_command_journals(value, repo, private):
    for item in value['commands']:
        name = item['id']
        if name in ('fetch', 'audit'):
            directory = private
            stdout = 'fetch.stdout' if name == 'fetch' else 'audit.json'
            command = (['cargo', '+' + check.TOOLCHAIN, 'fetch', '--locked', '--target', check.TARGET] if name == 'fetch'
                       else [sys.executable, str(repo / 'scripts/check-analytical-swift-recipe.py'), '--repo', str(repo),
                             '--source-sha', value['source_sha'], '--capture', str(private / 'capture')])
            if name == 'audit' and value.get('source_recipe') == check.SOURCE_RECIPE:
                command += ['--upstream-archive', str(private / REFERENCE)]
        else:
            mode, operation = name.split('-')
            directory = private / 'capture' / mode
            stdout = OUTPUTS.get(operation, 'notices.stdout')
            command = (check.commands(mode)[operation] if operation != 'notices' else
                       [sys.executable, str(repo / 'scripts/collect-dependency-notices.py'), '--repo', str(repo),
                        '--metadata', str(directory / 'metadata.json'), '--target', check.TARGET,
                        '--root-package', 'flightsim-app', '--output', str(directory / 'notices')])
        journal = read_private_json(directory / (name + '.command.json'))
        require(journal['command'] == command and journal['cwd'] == str(repo), 'private command invocation differs')
        require({key: journal[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}
                == {key: item[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}, 'private command result differs')
        require(file_record(directory / stdout) == item['stdout']
                and file_record(directory / (name.split('-')[-1] + '.stderr')) == item['stderr'], 'private streams changed')


def validate_summary(value):
    require(isinstance(value, dict), 'export must be an object')
    modern = value.get('source_recipe') == check.SOURCE_RECIPE
    extra_fields = {'source_recipe'} if 'source_recipe' in value else set()
    require(value.get('source_recipe', check.tone.UPSTREAM_THREE_LUT_SOURCE)
            in (check.tone.UPSTREAM_THREE_LUT_SOURCE, check.SOURCE_RECIPE), 'unknown export source recipe')
    require(set(value) == {
        'schema_version', 'identity', 'recipe', 'status', 'stage', 'source_sha', 'source_tree',
        'release_authorized', 'native_runtime_qualified', 'distribution_qualified',
        'required_unexecuted_gates', 'commands', 'bindings', 'builds'} | extra_fields, 'unexpected export fields')
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['identity'] == IDENTITY and value['recipe'] == check.IDENTITY, 'wrong export identity')
    require(value['status'] in ('failed', PASS) and value['stage'] in STAGES, 'wrong export status')
    require(all(value[name] is False for name in ('release_authorized', 'native_runtime_qualified', 'distribution_qualified')),
            'export cannot grant qualification')
    require(value['required_unexecuted_gates'] == list(check.GATES), 'remaining gates changed')
    require(hex_string(value['source_sha'], 40) and (value['source_tree'] is None or hex_string(value['source_tree'], 40)),
            'invalid source binding')
    require(isinstance(value['commands'], list) and len(value['commands']) <= len(COMMAND_IDS), 'unbounded commands')
    for index, item in enumerate(value['commands']):
        require(isinstance(item, dict) and set(item) == {'id', 'outcome', 'exit_code', 'stdout', 'stderr'}
                and item['id'] == COMMAND_IDS[index], 'unexpected command record')
        require(item['outcome'] in ('succeeded', 'failed', 'timed_out', 'launch_error', 'termination_failed'), 'invalid command outcome')
        code = item['exit_code']
        require((item['outcome'] in ('launch_error', 'termination_failed') and code is None)
                or (type(code) is int and -(2 ** 32) <= code <= 2 ** 32
                    and (item['outcome'] in ('timed_out', 'termination_failed')
                         or (item['outcome'] == 'succeeded' and code == 0)
                         or (item['outcome'] == 'failed' and code != 0))), 'invalid actual command status')
        require(valid_record(item['stdout']) and valid_record(item['stderr']), 'invalid stream binding')
        require(index == len(value['commands']) - 1 or item['outcome'] == 'succeeded', 'execution continued after failure')
    require(isinstance(value['bindings'], dict) and set(value['bindings']) <= {
        'source', 'capture_contract', 'analytical_contract', 'replay_contract', 'lock', 'capture', 'audit', 'frozen_trees'}
            | ({'upstream_archive'} if modern else set()),
        'unexpected bindings')
    require(all(valid_record(record) for record in value['bindings'].values()), 'invalid binding')
    require(isinstance(value['builds'], dict) and set(value['builds']) <= set(MODES), 'unexpected builds')
    for mode, build in value['builds'].items():
        require(set(build) == {'executable', 'inventory', 'metadata', 'frozen_artifact_count', 'lut_payloads_found'},
                'unexpected build fields')
        require(all(valid_record(build[name]) for name in ('executable', 'inventory', 'metadata')), 'invalid build binding')
        require(type(build['frozen_artifact_count']) is int and build['frozen_artifact_count'] == 17
                and type(build['lut_payloads_found']) is int and build['lut_payloads_found'] == ((2 if modern else 3) if mode == 'ordinary' else 0),
                'wrong build proof counts')
    if value['status'] == PASS:
        require(value['stage'] == 'complete' and hex_string(value['source_tree'], 40)
                and len(value['commands']) == len(COMMAND_IDS)
                and all(item['outcome'] == 'succeeded' for item in value['commands'])
                and set(value['builds']) == set(MODES)
                and set(value['bindings']) == {'source', 'capture_contract', 'analytical_contract', 'replay_contract',
                                              'lock', 'capture', 'audit', 'frozen_trees'} | ({'upstream_archive'} if modern else set()), 'incomplete passing export')
    else:
        require(value['stage'] != 'complete' and not value['builds'], 'failure cannot claim audited builds')


def validate_export(directory, *, repo=None, expected=None, private=None):
    no_links(directory)
    require(directory.is_dir() and {path.name for path in directory.iterdir()} == {EXPORT_NAME}, 'export must contain exactly one allowed file')
    path = directory / EXPORT_NAME
    no_links(path)
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= MAX_EXPORT_BYTES,
            'export is not an independent bounded text file')
    raw = path.read_bytes()
    value = json.loads(raw.decode('ascii'))
    validate_summary(value)
    # A canonical encoding rejects duplicate keys, hidden whitespace payloads,
    # alternate encodings and bytes appended after otherwise valid JSON.
    require(raw == (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii'), 'noncanonical export bytes')
    if expected is not None:
        require(repo is not None and private is not None and hex_string(expected, 40)
                and value['source_sha'] == expected, 'export invocation differs')
        no_links(private)
        require(read_private_json(private / 'progress.json') == value,
                'export differs from private completed result')
        verify_command_journals(value, repo.resolve(), private.resolve())
        if value['status'] == PASS:
            source = source_evidence(repo.resolve(), expected)
            require(value.get('source_recipe', check.tone.UPSTREAM_THREE_LUT_SOURCE) == source['source_recipe'],
                    'export source recipe differs from admitted source')
            require(value['source_tree'] == source['source_tree']
                    and read_private_json(private / 'source.json') == source,
                    'export source differs from captured source')
            paths = {'source': private / 'source.json', 'capture_contract': repo / CONTRACT,
                     'analytical_contract': repo / check.CONTRACT, 'replay_contract': repo / check.candidate.REPLAY_CONTRACT_PATH,
                     'lock': repo / 'Cargo.lock', 'capture': private / 'capture/capture.json',
                     'audit': private / 'audit.json', 'frozen_trees': private / 'frozen-trees.json'}
            if value.get('source_recipe') == check.SOURCE_RECIPE:
                paths['upstream_archive'] = private / REFERENCE
                check.tone.validate_source(repo.resolve(), paths['upstream_archive'])
            require(all(file_record(path) == value['bindings'][key] for key, path in paths.items()), 'export bindings changed')
            frozen = read_private_json(private / 'frozen-trees.json')
            require(snapshot_tree(private / 'capture') == frozen['capture'], 'frozen capture changed before export')
            for mode in MODES:
                require(snapshot_tree(private / ('target-' + mode)) == frozen[mode], 'frozen target changed before export')
            receipt = read_private_json(private / 'capture/capture.json')
            report = read_private_json(private / 'audit.json')
            if value.get('source_recipe') == check.SOURCE_RECIPE:
                require(receipt.get('upstream_archive') == value['bindings']['upstream_archive'],
                        'export private reference binding differs from capture')
            require(value['builds'] == projected_builds(receipt, report), 'export build projection differs')
    return value


@contextmanager
def private_console(directory):
    """Keep imported tools' diagnostics and inherited subprocess output private."""
    sys.stdout.flush(); sys.stderr.flush()
    saved = (os.dup(1), os.dup(2))
    with (directory / 'runner.stdout').open('xb') as out, (directory / 'runner.stderr').open('xb') as err:
        try:
            os.dup2(out.fileno(), 1); os.dup2(err.fileno(), 2)
            yield
        finally:
            sys.stdout.flush(); sys.stderr.flush()
            os.dup2(saved[0], 1); os.dup2(saved[1], 2)
            os.close(saved[0]); os.close(saved[1])


def capture(repo, expected, private, evidence, *, upstream_archive=None):
    require(hex_string(expected, 40), 'source must be a full lowercase commit SHA')
    for path in (repo, private, evidence):
        require(path.is_absolute(), 'absolute paths required')
        no_links(path)
    repo, private, evidence = (path.resolve() for path in (repo, private, evidence))
    disjoint(repo, private, evidence)
    require(not private.exists() and not evidence.exists(), 'capture/export roots must be fresh')
    private.mkdir(parents=True)
    summary = {'schema_version': 1, 'identity': IDENTITY, 'recipe': check.IDENTITY,
               'source_recipe': check.SOURCE_RECIPE, 'status': 'failed',
               'stage': 'preflight', 'source_sha': expected, 'source_tree': None,
               'release_authorized': False, 'native_runtime_qualified': False, 'distribution_qualified': False,
               'required_unexecuted_gates': list(check.GATES), 'commands': [], 'bindings': {}, 'builds': {}}
    with private_console(private):
        try:
            require(sys.platform == 'win32' and platform.machine().lower() in ('amd64', 'x86_64'), 'native Windows x64 required')
            require(shutil.disk_usage(private).free >= MIN_START_FREE, 'insufficient private build disk')
            reject_configuration(repo, os.environ)
            source = source_evidence(repo, expected)
            require(upstream_archive is not None, 'two-LUT capture requires private upstream archive')
            upstream_archive = Path(upstream_archive)
            no_links(upstream_archive)
            require(upstream_archive.is_absolute() and upstream_archive.resolve() == upstream_archive,
                    'private upstream reference must be absolute and unaliased')
            check.tone.validate_source(repo, upstream_archive)
            reference = private / REFERENCE
            reference.parent.mkdir()
            shutil.copyfile(upstream_archive, reference)
            check.tone.validate_source(repo, reference)
            summary['source_tree'] = source['source_tree']
            write_json(private / 'source.json', source)
            summary['bindings'] = {'source': file_record(private / 'source.json'),
                                   'capture_contract': file_record(repo / CONTRACT),
                                   'analytical_contract': file_record(repo / check.CONTRACT),
                                   'replay_contract': file_record(repo / check.candidate.REPLAY_CONTRACT_PATH),
                                   'lock': file_record(repo / 'Cargo.lock'), 'upstream_archive': file_record(reference)}
            cargo_home = private / 'cargo-home'; cargo_home.mkdir()
            env = {**os.environ, 'CARGO_HOME': str(cargo_home), 'CARGO_TERM_COLOR': 'never',
                   'RUSTFLAGS': '-D warnings', 'CARGO_INCREMENTAL': '0', 'PYTHONUTF8': '1'}
            capture_root = private / 'capture'; capture_root.mkdir()
            receipt = {key: check.recipe()[key] for key in ('schema_version', 'recipe', 'target', 'toolchain',
                       'features', 'default_features', 'region_downloads', 'release_authorized', 'source_recipe')}
            receipt['upstream_archive'] = file_record(reference)
            receipt.update(source_sha=expected, source_tree=source['source_tree'], modes={})

            def run(name, command, directory, stdout, timeout, run_env=env):
                summary['stage'] = name
                require(shutil.disk_usage(private).free >= MIN_COMMAND_FREE, 'private build disk floor reached')
                actual = execute(command, cwd=repo, env=run_env, stdout=directory / stdout,
                                 stderr=directory / (name.split('-')[-1] + '.stderr'),
                                 journal=directory / (name + '.command.json'), timeout=timeout)
                summary['commands'].append({'id': name, **{key: actual[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}})
                write_json(private / 'progress.json', summary)
                require(actual['outcome'] == 'succeeded', 'capture command did not succeed')
                return {key: actual[key] for key in ('exit_code', 'stdout', 'stderr')}

            # Only dependency acquisition is online; every captured recipe command
            # is byte-for-byte the existing auditor's pinned locked/offline recipe.
            run('fetch', ['cargo', '+' + check.TOOLCHAIN, 'fetch', '--locked', '--target', check.TARGET],
                private, 'fetch.stdout', 1200)
            frozen = {}
            for mode in MODES:
                directory = capture_root / mode; directory.mkdir()
                target = private / ('target-' + mode)
                require(not target.exists(), 'target must be fresh')
                mode_env = {**env, 'CARGO_TARGET_DIR': str(target)}
                record = {'cwd': str(repo), 'source_sha': expected, 'source_tree': source['source_tree'],
                          'target_dir': str(target), 'environment': {name: mode_env[name] for name in
                          ('RUSTFLAGS', 'CARGO_TARGET_DIR', 'CARGO_INCREMENTAL')}, 'commands': check.commands(mode), 'results': {}}
                receipt['modes'][mode] = record
                for name, filename in OUTPUTS.items():
                    record['results'][name] = run(mode + '-' + name, record['commands'][name], directory,
                                                   filename, 7200 if name == 'build' else 300, mode_env)
                    write_json(capture_root / 'capture.json', receipt)
                    if name == 'rustc':
                        compiler = (directory / filename).read_text(encoding='utf-8')
                        require(compiler.startswith('rustc 1.93.0 ') and '\nhost: ' + check.TARGET + '\n' in compiler,
                                'not the pinned native Windows MSVC compiler')
                run(mode + '-notices', [sys.executable, str(repo / 'scripts/collect-dependency-notices.py'),
                    '--repo', str(repo), '--metadata', str(directory / 'metadata.json'), '--target', check.TARGET,
                    '--root-package', 'flightsim-app', '--output', str(directory / 'notices')],
                    directory, 'notices.stdout', 300, mode_env)
                record['inventory'] = file_record(directory / 'notices/dependency-inventory.json')
                summary['stage'] = 'freeze'
                record['frozen_artifacts'] = frozen_artifacts(directory, target, mode, repo=repo, upstream_archive=reference)
                frozen[mode] = snapshot_tree(target)
                write_json(capture_root / 'capture.json', receipt)
                # No further build/test commands ever target this frozen tree.
            frozen['capture'] = snapshot_tree(capture_root)
            write_json(private / 'frozen-trees.json', frozen)
            summary['bindings']['capture'] = file_record(capture_root / 'capture.json')
            summary['bindings']['frozen_trees'] = file_record(private / 'frozen-trees.json')
            run('audit', [sys.executable, str(repo / 'scripts/check-analytical-swift-recipe.py'),
                '--repo', str(repo), '--source-sha', expected, '--capture', str(capture_root),
                '--upstream-archive', str(reference)], private, 'audit.json', 1800)
            report = json.loads((private / 'audit.json').read_text(encoding='utf-8'))
            require(report['status'] == PASS and report['release_authorized'] is False
                    and report['required_unexecuted_gates'] == check.GATES
                    and report['source_sha'] == expected and report['source_tree'] == source['source_tree']
                    and report['source_recipe'] == check.SOURCE_RECIPE
                    and report['upstream_archive'] == file_record(reference),
                    'auditor did not retain unqualified status and gates')
            summary['stage'] = 'source-recheck'
            require(source_evidence(repo, expected) == source, 'source changed during capture')
            summary['stage'] = 'freeze-recheck'
            for mode in MODES:
                require(snapshot_tree(private / ('target-' + mode)) == frozen[mode], 'frozen target changed')
            require(snapshot_tree(capture_root) == frozen['capture'], 'frozen capture changed')
            summary['bindings']['audit'] = file_record(private / 'audit.json')
            summary['builds'] = projected_builds(receipt, report)
            summary.update(status=PASS, stage='complete')
        except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
            summary['builds'] = {}
            write_json(private / 'failure.json', {'type': type(error).__name__, 'message': str(error)})
        write_json(private / 'progress.json', summary)
    validate_summary(summary)
    evidence.mkdir(parents=True)
    write_json(evidence / EXPORT_NAME, summary)
    validate_export(evidence)
    return summary


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source-sha')
    parser.add_argument('--private', type=Path)
    parser.add_argument('--upstream-archive', type=Path)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--validate-evidence', type=Path)
    args = parser.parse_args(argv)
    try:
        if args.validate_evidence is not None:
            require(args.source_sha is not None and args.private is not None and args.evidence is None,
                    'validation requires exact invocation and private result')
            validate_export(args.validate_evidence, repo=args.repo, expected=args.source_sha, private=args.private)
            print('Bounded text-only build evidence validated.')
            return 0
        require(args.source_sha is not None and args.private is not None and args.evidence is not None, 'capture arguments required')
        result = capture(args.repo, args.source_sha, args.private, args.evidence, upstream_archive=args.upstream_archive)
        print('Analytical MSVC build capture: ' + result['status'] + '; remaining gates unexecuted; release_authorized=false.')
        return 0 if result['status'] == PASS else 1
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        # Do not expose private exception text, paths, tool output, or URLs.
        print('Analytical MSVC build capture/export validation failed; raw evidence remains private.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
