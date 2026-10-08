#!/usr/bin/env python3
"""Private, additive Windows engineering evidence; never release authorization.

Two independently fresh phases: regressions, then build/runtime capture. The
unchanged build auditor, stager and ordinary acceptance validators remain the
authorities for their existing scopes. No diagnostic executable is admitted.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time
import zipfile
import zlib


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


capture = load('qualification_capture', 'capture-analytical-swift-msvc.py')
check, require = capture.check, capture.require
candidate = check.candidate
native = load('qualification_native', 'project-analytical-native-evidence.py')
runtime_facts = load('qualification_runtime_facts', 'collect-analytical-runtime-facts.py')
ui_capabilities = load('qualification_ui_capabilities', 'observe-analytical-ui-capabilities.py')
write_json, record = capture.write_json, capture.file_record
IDENTITY = 'analytical-swift-windows-qualification-evidence-v1'
EXPORT = 'qualification.json'
MAX_JSON = 2 * 1024 * 1024
MAX_IMAGES = 64 * 1024 * 1024
PHASE_SECONDS = {'regressions': 3 * 3600, 'runtime': 350 * 60}
EXACT_TESTS = {
    'identity': 'distribution::tests::explicit_legacy_aircraft_retains_fingerprint_and_is_not_remapped',
    **candidate.REPLAY_ACCEPTANCE_TESTS,
}
MIXED = 'analytic-tonemapping requires --no-default-features; select one tone mode'
# Fixed image IDs and complete CLI selections. They are observations for review,
# never a subjective appearance pass or an interactive lifecycle witness.
SCENES = {
    'default-swift': [],
    'day-cockpit': ['--time', '12:00', '--view', 'cockpit', '--weather', 'clear'],
    'low-sun': ['--time', '05:30', '--weather', 'clear'],
    'night-cockpit': ['--time', '23:00', '--view', 'cockpit', '--weather', 'clear'],
    'fog-cockpit': ['--time', '12:00', '--view', 'cockpit', '--weather', 'fog', '--weather-seed', '1'],
    'cloud-high': ['--time', '12:00', '--weather', 'cloud', '--weather-seed', '1', '--cloud-quality', 'high'],
    'water-high': ['--time', '12:00', '--start', '35.40,139.80', '--water-quality', 'high'],
    'tower-day': ['--time', '12:00', '--view', 'tower', '--weather', 'clear'],
}
OPEN_REVIEWS = ['analytical_windows_appearance', 'dependency_and_platform_review', 'publication_receipt']
LIMITS = ['swift_only', 'reinhard_changes_appearance', 'offline_region_downloads_disabled',
          'legacy_partial_identity_only', 'interactive_map_camera_lifecycle_unexecuted',
          'physical_gpu_controller_audio_unexecuted', 'whole_target_rights_unreviewed',
          'reduced_release_choice_and_publication_receipt_required']
LINK_LOG_ENV = {'RUSTC_LOG': 'rustc_codegen_ssa::back::link=info', 'RUSTC_LOG_COLOR': 'never'}


def regression_plan():
    base = ['--locked', '--offline', '--release', '-j', '2', '--target', check.TARGET]
    both = ['-p', 'flightsim-app', '-p', 'flightsim-render']
    analytic = ['--no-default-features', '--features',
                'flightsim-app/analytic-tonemapping,flightsim-app/commercial-staging,flightsim-render/analytic-tonemapping']
    result = []

    def add(name, subcommand, flags, mode='analytic', diagnostic=None, exact=None):
        result.append({'id': name, 'command': ['cargo', '+' + check.TOOLCHAIN, subcommand, *base, *flags],
                       'target': mode, 'diagnostic': diagnostic, 'exact': exact})

    for mode in ('analytic', 'ordinary'):
        features = analytic if mode == 'analytic' else ['--features', 'flightsim-app/commercial-staging']
        add(mode + '-app-render-tests', 'test', [*both, '--tests', *features], mode)
        add(mode + '-clippy', 'clippy', [*both, '--all-targets', *features, '--', '-D', 'warnings'], mode)
        render_features = ['--no-default-features', '--features', 'analytic-tonemapping'] if mode == 'analytic' else []
        add(mode + '-tone-tests', 'test', ['-p', 'flightsim-render', '--test', 'tonemapping_modes', *render_features], mode)
    for name, test in EXACT_TESTS.items():
        add(name, 'test', ['-p', 'flightsim-app', '--no-default-features', '--features', ','.join(check.FEATURES),
                          '--bin', 'flightsim-app', test, '--', '--exact'], exact=test)
    guards = [
        ('app-mixed', ['-p', 'flightsim-app', '--features', ','.join(check.FEATURES)], MIXED),
        ('render-mixed', ['-p', 'flightsim-render', '--features', 'analytic-tonemapping'], MIXED),
        ('app-neither', ['-p', 'flightsim-app', '--no-default-features', '--features', 'commercial-staging'],
         'flightsim-app requires default or analytic-tonemapping; select one tone mode'),
        ('sun-clock-neither', ['-p', 'flightsim-render', '--example', 'sun_clock', '--no-default-features'],
         'sun_clock requires default or analytic-tonemapping; select one tone mode'),
    ]
    for name, flags, diagnostic in guards:
        add(name, 'check', [*flags, '--message-format=json'], 'ordinary' if name.endswith('-mixed') else 'analytic', diagnostic)
    add('render-library-neither', 'check', ['-p', 'flightsim-render', '--lib', '--no-default-features'])
    return result


def check_test_output(log, exact=None):
    plain = candidate.ANSI.sub('', log)
    results = re.findall(r'test result: ok\. (\d+) passed; 0 failed;', plain)
    require(results and sum(map(int, results)) > 0, 'test command executed no passing tests')
    if exact:
        require(results == ['1'] and re.search(r'(?m)^running 1 test\r?$', plain)
                and re.search(r'(?m)^test ' + re.escape(exact) + r' \.\.\. ok\r?$', plain),
                'exact acceptance test did not execute exactly once')


def check_rejection(raw, code, diagnostic):
    messages = [json.loads(line) for line in raw.decode('utf-8').splitlines()]
    errors = [item['message']['message'] for item in messages
              if item.get('reason') == 'compiler-message' and item['message']['level'] == 'error']
    require(code == 101 and errors == [diagnostic] and messages
            and messages[-1] == {'reason': 'build-finished', 'success': False},
            'compiler failed for a different reason or accepted forbidden features')


def strict_bundle(bundle, executable, manifest_hash, repo, notices, review_path=None):
    """Keep the stager authoritative and check its full copy plan, not filenames."""
    capture.snapshot_tree(bundle)  # Reject junctions/links too.
    candidate.verify_bundle(bundle, executable, manifest_hash)
    stager = load('qualification_stager', 'stage-commercial-candidate.py')
    inventory = json.loads((notices / stager.INVENTORY).read_text(encoding='utf-8'))
    review = capture.read_private_json(review_path) if review_path is not None else None
    notice_paths = stager.notice_files(notices, inventory, review)
    copied = {name: repo / name for name in stager.SOURCE_FILES}
    copied.update({'third-party/' + name: notices / name for name in notice_paths})
    copied['flightsim-app.exe'] = executable
    if review_path is not None:
        copied['docs/release/dependency-review.json'] = review_path
    generated = {'bundle-manifest.json', 'distribution-info.json', 'commercial-readiness.json', 'LOCAL-CANDIDATE.txt'}
    actual = {path.relative_to(bundle).as_posix() for path in bundle.rglob('*') if path.is_file()}
    require(actual == set(copied) | generated, 'staged bundle differs from sole stager copy plan')
    excluded = record(repo / 'assets/aircraft/light_single.glb')
    for relative, source in copied.items():
        require(record(bundle / relative) == record(source), 'renamed or changed staged source bytes')
    for path in bundle.rglob('*'):
        if path.is_file():
            require(record(path) != excluded, 'excluded model bytes under another name')


class Runner:
    def __init__(self, repo, private, env, phase):
        self.repo, self.private, self.env, self.phase = repo, private, env, phase
        self.deadline = time.monotonic() + PHASE_SECONDS[phase]
        self.commands = []
        (private / 'commands').mkdir()

    def run(self, name, command, *, cwd=None, env=None, timeout=3600, accepted=(0,), diagnostic=None, exact=None, tests=False):
        require(re.fullmatch('[a-z0-9_-]+', name) and name not in [item['id'] for item in self.commands], 'invalid command ID')
        require(shutil.disk_usage(self.private).free >= capture.MIN_COMMAND_FREE, 'private disk floor reached')
        remaining = int(self.deadline - time.monotonic())
        require(remaining > 0, 'phase time budget exhausted')
        directory = self.private / 'commands' / name
        directory.mkdir()
        actual = capture.execute([str(item) for item in command], cwd=cwd or self.repo, env=env or self.env,
                                 stdout=directory / 'stdout', stderr=directory / 'stderr', journal=directory / 'journal.json',
                                 timeout=min(timeout, remaining))
        self.commands.append({'id': name, **{key: actual[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}})
        write_json(self.private / 'command-progress.json', self.commands)
        require(all(actual[key]['bytes'] <= 64 * 1024 * 1024 for key in ('stdout', 'stderr')), 'command output budget exceeded')
        require(actual['outcome'] in ('succeeded', 'failed') and actual['exit_code'] in accepted,
                'command failed or timed out')
        raw = (directory / 'stdout').read_bytes()
        log = (raw + b'\n' + (directory / 'stderr').read_bytes()).decode('utf-8', errors='replace')
        if diagnostic:
            check_rejection(raw, actual['exit_code'], diagnostic)
        if tests or exact:
            check_test_output(log, exact)
        return actual, raw, log


def regressions(runner, source):
    runner.run('fetch', ['cargo', '+' + check.TOOLCHAIN, 'fetch', '--locked', '--target', check.TARGET], timeout=1200)
    for name in ('replay_identity_reference', 'replay_v3_reference'):
        runner.run(name, [sys.executable, str(runner.repo / 'docs/qa' / (name + '.py'))], timeout=120)
    for step in regression_plan():
        env = {**runner.env, 'CARGO_TARGET_DIR': str(runner.private / ('test-target-' + step['target']))}
        runner.run(step['id'], step['command'], env=env, timeout=7200,
                   accepted=(101,) if step['diagnostic'] else (0,), diagnostic=step['diagnostic'],
                   exact=step['exact'], tests=step['id'].endswith('-tests'))
    return {'combined_feature_regressions': 'observed_pass', 'exact_identity_and_legacy_tests': 'observed_pass'}, {}


def runtime(runner, source):
    repo, private = runner.repo, runner.private
    build_private, build_text = private / 'build-private', private / 'build-text'
    # This command inherits the original clean environment. The unchanged capture
    # creates its own private Cargo home and rejects inherited compiler overrides.
    runner.run('build-capture', [sys.executable, repo / 'scripts/capture-analytical-swift-msvc.py',
               '--repo', repo, '--source-sha', source['source_sha'], '--private', build_private, '--evidence', build_text],
               env={**os.environ, **LINK_LOG_ENV}, timeout=300 * 60)
    build = capture.validate_export(build_text, repo=repo, expected=source['source_sha'], private=build_private)
    require(build['status'] == capture.PASS, 'build/artifact audit incomplete')
    executable = build_private / 'target-analytic' / check.TARGET / 'release/flightsim-app.exe'
    collect_runtime_observations(runner, source, build_private, build_text)
    notices = build_private / 'capture/analytic/notices'
    staged = private / 'swift-candidate'
    result, _, _ = runner.run('stage', [sys.executable, repo / 'scripts/stage-commercial-candidate.py', '--source-root', repo,
                             '--executable', executable, '--dependency-notices', notices, '--output', staged], accepted=(0, 1), timeout=180)
    readiness = json.loads((staged / 'commercial-readiness.json').read_text(encoding='utf-8'))
    candidate.validate_readiness(readiness, 2 if result['exit_code'] == 1 else 0)
    return check_staged_runtime(runner, source, build_private, build_text, executable, notices, staged)


def validate_runtime_facts(private, repo, expected, build_private, build_text):
    return native.validate_runtime_facts(private / 'runtime-facts', repo, expected, build_private, build_text)


def collect_runtime_observations(runner, source, build_private, build_text):
    """Facts first; unavailable UI capability never erases independent evidence."""
    private, repo = runner.private, runner.repo
    command = command_specifications(repo, private, 'runtime', source['source_sha'],
                                     build_private=build_private, build_text=build_text)['runtime-facts']
    runner.run('runtime-facts', command, timeout=600)
    validate_runtime_facts(private, repo, source['source_sha'], build_private, build_text)
    ui_capabilities.collect(private / 'ui-capabilities', repo, source['source_sha'])


def check_staged_runtime(runner, source, build_private, build_text, executable, notices, staged, review_path=None):
    """Run the identical outcomes on the exact staged bytes, including supplements."""
    repo, private = runner.repo, runner.private
    manifest_hash = check.digest(staged / 'bundle-manifest.json')
    strict_bundle(staged, executable, manifest_hash, repo, notices, review_path)
    archive = private / 'swift-candidate.zip'
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as out:
        for path in sorted(staged.rglob('*')):
            if path.is_file():
                out.write(path, 'swift-candidate/' + path.relative_to(staged).as_posix())
    with zipfile.ZipFile(archive) as zipped:
        require(zipped.testzip() is None, 'archive CRC failure')
        expected_paths = {'swift-candidate/' + path.relative_to(staged).as_posix() for path in staged.rglob('*') if path.is_file()}
        require(len(zipped.namelist()) == len(expected_paths) and set(zipped.namelist()) == expected_paths, 'archive membership mismatch')
        zipped.extractall(private / 'extracted')
    bundle = private / 'extracted/swift-candidate'
    strict_bundle(bundle, executable, manifest_hash, repo, notices, review_path)
    app = bundle / 'flightsim-app.exe'
    unrelated = private / 'unrelated-cwd'; unrelated.mkdir()
    env = {**runner.env, 'WGPU_BACKEND': 'dx12', 'WGPU_FORCE_FALLBACK_ADAPTER': '1',
           'BEVY_ASSET_ROOT': str(repo), 'CARGO_MANIFEST_DIR': str(repo), 'RUST_LOG': 'info'}
    first, second = None, None
    for name in ('distribution-first', 'distribution-second'):
        _, raw, _ = runner.run(name, [app, '--distribution-info'], cwd=unrelated, env=env, timeout=30)
        if first is None: first = raw
        else: second = raw
    require(first == second, 'distribution handshake changed')
    candidate.validate_distribution(json.loads(first), json.loads((bundle / 'distribution-info.json').read_text(encoding='utf-8')))
    review_flags = ['--dependency-review', bundle / 'docs/release/dependency-review.json'] if review_path is not None else []
    result, raw, _ = runner.run('extracted-readiness', [sys.executable, repo / 'scripts/check-commercial-readiness.py', '--repo', repo,
                               '--bundle', bundle, '--dependency-inventory', bundle / 'third-party/dependency-inventory.json', '--json', *review_flags],
                               accepted=(0,) if review_path is not None else (0, 2), timeout=180)
    candidate.validate_readiness(json.loads(raw), result['exit_code'])
    write_json(private / 'native-review.json', native.project(repo, source['source_sha'], build_private, build_text, bundle,
               runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities'))
    images = {}
    for name, flags in SCENES.items():
        image = private / (name + '.png')
        command = candidate.capture_command(app, image)
        # A scene's explicit view replaces the one baseline view flag.
        if '--view' in flags:
            command = command[:-2]
        result, _, log = runner.run(name, [*command, *flags], cwd=unrelated, env=env, timeout=candidate.CAPTURE_TIMEOUT_SECONDS)
        candidate.validate_smoke(candidate.sanitize(log, repo, private), result['exit_code'], model=True)
        images[name + '.png'] = candidate.validate_png(image)
    result, _, log = runner.run('absent-light-single', [app, '--aircraft', 'light-single'], cwd=unrelated, env=env, accepted=(2,), timeout=30)
    require('selected aircraft model is missing: aircraft/light_single.glb' in log and 'aircraft model fitted:' not in log,
            'absent model failed for wrong reason')
    fixture = private / 'legacy.fsreplay'
    fixture_env = {**runner.env, 'CARGO_HOME': str(build_private / 'cargo-home'), 'CARGO_TARGET_DIR': str(private / 'fixture-target')}
    runner.run('legacy-fixture', ['cargo', '+' + check.TOOLCHAIN, 'run', '--locked', '--offline', '--release', '-j', '2',
               '--target', check.TARGET, '-p', 'flightsim-sim', '--example', 'record_takeoff', '--', fixture], env=fixture_env, timeout=1200)
    candidate.legacy_identity(fixture)
    result, _, log = runner.run('default-rejects-legacy', [app, '--replay', fixture], cwd=unrelated, env=env, accepted=(2,), timeout=30)
    candidate.validate_legacy_rejection(log, result['exit_code'])
    legacy_png = private / 'legacy-no-model.png'
    result, _, log = runner.run('legacy-no-model', candidate.legacy_capture_command(app, fixture, legacy_png),
                              cwd=unrelated, env=env, timeout=candidate.CAPTURE_TIMEOUT_SECONDS)
    candidate.validate_legacy_smoke(log, result['exit_code'])
    candidate.validate_png(legacy_png)  # stays private; no excluded-aircraft PNG export
    strict_bundle(bundle, executable, manifest_hash, repo, notices, review_path)
    capture.validate_export(build_text, repo=repo, expected=source['source_sha'], private=build_private)
    write_json(private / 'runtime-bindings.json', {'executable': record(executable), 'archive': record(archive),
               'bundle_manifest': record(bundle / 'bundle-manifest.json'), 'build_summary': record(build_text / capture.EXPORT_NAME),
               'legacy_fixture': record(fixture), 'legacy_png': record(legacy_png)})
    return {'extracted_bundle_isolation': 'observed_pass', 'extracted_runtime_acceptance': 'observed_pass'}, images


def expected_ids(phase):
    initial = ['rustc']
    if phase == 'regressions':
        return initial + ['fetch', 'replay_identity_reference', 'replay_v3_reference'] + [step['id'] for step in regression_plan()]
    return initial + ['build-capture', 'runtime-facts', 'stage', 'distribution-first', 'distribution-second', 'extracted-readiness',
                      *SCENES, 'absent-light-single', 'legacy-fixture', 'default-rejects-legacy', 'legacy-no-model']


def accepted_codes(name):
    if name in {'app-mixed', 'render-mixed', 'app-neither', 'sun-clock-neither'}: return (101,)
    if name in {'absent-light-single', 'default-rejects-legacy'}: return (2,)
    if name == 'stage': return (0, 1)
    if name == 'extracted-readiness': return (0, 2)
    return (0,)


def command_specifications(repo, private, phase, expected, *, build_private=None, build_text=None, notices=None, review_path=None):
    commands = {'rustc': ['rustc', '+' + check.TOOLCHAIN, '-vV']}
    if phase == 'regressions':
        commands['fetch'] = ['cargo', '+' + check.TOOLCHAIN, 'fetch', '--locked', '--target', check.TARGET]
        commands.update({name: [sys.executable, str(repo / 'docs/qa' / (name + '.py'))]
                         for name in ('replay_identity_reference', 'replay_v3_reference')})
        commands.update({step['id']: step['command'] for step in regression_plan()})
        return commands
    build_private = build_private or private / 'build-private'
    build_text = build_text or private / 'build-text'
    notices = notices or build_private / 'capture/analytic/notices'
    executable = build_private / 'target-analytic' / check.TARGET / 'release/flightsim-app.exe'
    bundle = private / 'extracted/swift-candidate'; app = bundle / 'flightsim-app.exe'
    fixture = private / 'legacy.fsreplay'
    commands.update({
        'build-capture': [sys.executable, repo / 'scripts/capture-analytical-swift-msvc.py', '--repo', repo,
                          '--source-sha', expected, '--private', build_private, '--evidence', build_text],
        'runtime-facts': [sys.executable, repo / 'scripts/collect-analytical-runtime-facts.py', '--private', private / 'runtime-facts',
                          '--source-sha', expected, '--recipe-warnings-only', '--linker-trace', build_private / 'capture/analytic/build.stderr',
                          '--audited-executable', executable],
        'stage': [sys.executable, repo / 'scripts/stage-commercial-candidate.py', '--source-root', repo,
                  '--executable', executable, '--dependency-notices', notices,
                  '--output', private / 'swift-candidate', *(['--dependency-review', review_path] if review_path is not None else [])],
        'distribution-first': [app, '--distribution-info'], 'distribution-second': [app, '--distribution-info'],
        'extracted-readiness': [sys.executable, repo / 'scripts/check-commercial-readiness.py', '--repo', repo,
                                '--bundle', bundle, '--dependency-inventory', bundle / 'third-party/dependency-inventory.json', '--json',
                                *(['--dependency-review', bundle / 'docs/release/dependency-review.json'] if review_path is not None else [])],
        'absent-light-single': [app, '--aircraft', 'light-single'],
        'legacy-fixture': ['cargo', '+' + check.TOOLCHAIN, 'run', '--locked', '--offline', '--release', '-j', '2',
                           '--target', check.TARGET, '-p', 'flightsim-sim', '--example', 'record_takeoff', '--', fixture],
        'default-rejects-legacy': [app, '--replay', fixture],
        'legacy-no-model': candidate.legacy_capture_command(app, fixture, private / 'legacy-no-model.png'),
    })
    for name, flags in SCENES.items():
        command = candidate.capture_command(app, private / (name + '.png'))
        if '--view' in flags: command = command[:-2]
        commands[name] = [*command, *flags]
    return {key: [str(item) for item in command] for key, command in commands.items()}


def validate_runtime_observations(repo, private, expected, *, build_private=None, build_text=None, notices=None, review_path=None):
    """Recheck all durable production outcomes without rerunning the application."""
    build_private = build_private or private / 'build-private'
    build_text = build_text or private / 'build-text'
    notices = notices or build_private / 'capture/analytic/notices'
    executable = build_private / 'target-analytic' / check.TARGET / 'release/flightsim-app.exe'
    bundle = private / 'extracted/swift-candidate'
    bindings = capture.read_private_json(private / 'runtime-bindings.json')
    paths = {'executable': executable, 'archive': private / 'swift-candidate.zip',
             'bundle_manifest': bundle / 'bundle-manifest.json',
             'build_summary': build_text / capture.EXPORT_NAME,
             'legacy_fixture': private / 'legacy.fsreplay', 'legacy_png': private / 'legacy-no-model.png'}
    require(set(bindings) == set(paths) and all(record(path) == bindings[key] for key, path in paths.items()), 'changed durable runtime inputs')
    strict_bundle(bundle, executable, bindings['bundle_manifest']['sha256'], repo, notices, review_path)
    capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)

    def command(name):
        directory = private / 'commands' / name
        actual = capture.read_private_json(directory / 'journal.json')
        stdout = (directory / 'stdout').read_bytes()
        log = (stdout + b'\n' + (directory / 'stderr').read_bytes()).decode('utf-8', errors='replace')
        return actual, stdout, log

    first = command('distribution-first')[1]; second = command('distribution-second')[1]
    require(first == second, 'changed deterministic handshake')
    candidate.validate_distribution(json.loads(first), json.loads((bundle / 'distribution-info.json').read_text(encoding='utf-8')))
    actual, stdout, _ = command('extracted-readiness')
    candidate.validate_readiness(json.loads(stdout), actual['exit_code'])
    if review_path is not None: require(actual['exit_code'] == 0, 'final reviewed bundle readiness did not pass')
    for name, flags in SCENES.items():
        actual, _, log = command(name)
        image = private / (name + '.png')
        expected_command = candidate.capture_command(bundle / 'flightsim-app.exe', image)
        if '--view' in flags: expected_command = expected_command[:-2]
        require(actual['command'] == [*expected_command, *flags], 'changed scene invocation')
        candidate.validate_smoke(candidate.sanitize(log, repo, private), actual['exit_code'], model=True)
        candidate.validate_png(image)
    actual, _, log = command('absent-light-single')
    require(actual['exit_code'] == 2 and 'selected aircraft model is missing: aircraft/light_single.glb' in log
            and 'aircraft model fitted:' not in log, 'changed absent-model outcome')
    candidate.legacy_identity(private / 'legacy.fsreplay')
    actual, _, log = command('default-rejects-legacy'); candidate.validate_legacy_rejection(log, actual['exit_code'])
    actual, _, log = command('legacy-no-model'); candidate.validate_legacy_smoke(log, actual['exit_code'])
    candidate.validate_png(private / 'legacy-no-model.png')


def validate_shape(value):
    require(set(value) == {'schema_version', 'identity', 'phase', 'source_sha', 'source_tree', 'status', 'release_authorized',
                         'native_runtime_qualified', 'distribution_qualified', 'appearance_accepted', 'checks', 'commands',
                         'images', 'open_reviews', 'limits', 'bindings', 'native_projection', 'runtime_facts', 'ui_capabilities'}, 'unexpected qualification export fields')
    require(type(value['schema_version']) is int and value['schema_version'] == 1 and value['identity'] == IDENTITY, 'wrong export identity')
    require(value['phase'] in PHASE_SECONDS and capture.hex_string(value['source_sha'], 40)
            and (value['source_tree'] is None or capture.hex_string(value['source_tree'], 40)), 'wrong source/phase')
    require(all(value[key] is False for key in ('release_authorized', 'native_runtime_qualified', 'distribution_qualified', 'appearance_accepted')),
            'engineering evidence cannot authorize qualification or publication')
    require(value['open_reviews'] == OPEN_REVIEWS and value['limits'] == LIMITS, 'required review or limitation disappeared')
    require(value['status'] in ('failed', 'engineering_evidence_complete_reviews_required'), 'invalid evidence status')
    ids = expected_ids(value['phase'])
    require(type(value['commands']) is list and len(value['commands']) <= len(ids), 'too many commands')
    for index, item in enumerate(value['commands']):
        require(set(item) == {'id', 'outcome', 'exit_code', 'stdout', 'stderr'} and item['id'] == ids[index], 'unexpected command')
        require(item['outcome'] in ('succeeded', 'failed', 'timed_out', 'termination_failed', 'launch_error'), 'unexpected outcome')
        code = item['exit_code']
        require((type(code) is int and -(2**32) <= code <= 2**32) or
                (code is None and item['outcome'] in ('launch_error', 'termination_failed')), 'invalid status')
        require(item['outcome'] not in ('succeeded', 'failed') or
                (item['outcome'] == 'succeeded') == (code == 0), 'outcome contradicts actual status')
        if value['status'] != 'failed' or index < len(value['commands']) - 1:
            require(item['outcome'] in ('succeeded', 'failed') and code in accepted_codes(item['id']),
                    'execution continued after unaccepted command failure')
        require(all(capture.valid_record(item[key]) for key in ('stdout', 'stderr')), 'invalid stream binding')
    require(set(value['checks']) <= set(check.GATES) and all(status == 'observed_pass' for status in value['checks'].values()), 'unexpected engineering check')
    require(set(value['bindings']) <= {'source', 'runtime'} and all(capture.valid_record(item) for item in value['bindings'].values()), 'invalid bindings')
    require(value['native_projection'] is None or (value['phase'] == 'runtime' and capture.valid_record(value['native_projection'])
            and value['native_projection']['bytes'] <= native.MAX_BYTES), 'invalid native projection binding')
    for field, maximum in (('runtime_facts', runtime_facts.MAX_PUBLIC), ('ui_capabilities', ui_capabilities.MAX_PUBLIC)):
        require(value[field] is None or (value['phase'] == 'runtime' and capture.valid_record(value[field])
                and value[field]['bytes'] <= maximum), 'invalid native fact/capability binding')
    require(set(value['images']) <= {name + '.png' for name in SCENES}, 'unexpected image export')
    for item in value['images'].values():
        require(set(item) == {'width', 'height', 'sha256'} and capture.hex_string(item['sha256'], 64)
                and type(item['width']) is int and 640 <= item['width'] <= 4096
                and type(item['height']) is int and 360 <= item['height'] <= 4096, 'invalid image binding')
    if value['status'] != 'failed':
        required = {'combined_feature_regressions', 'exact_identity_and_legacy_tests'} if value['phase'] == 'regressions' else {'extracted_bundle_isolation', 'extracted_runtime_acceptance'}
        require(len(value['commands']) == len(ids) and set(value['checks']) == required and value['source_tree'] is not None, 'incomplete successful phase')
        require(set(value['images']) == ({name + '.png' for name in SCENES} if value['phase'] == 'runtime' else set()), 'missing successful phase images')
        require(set(value['bindings']) == ({'source', 'runtime'} if value['phase'] == 'runtime' else {'source'}), 'missing success bindings')
        require((value['native_projection'] is not None) == (value['phase'] == 'runtime'), 'missing native projection')
        require(all((value[key] is not None) == (value['phase'] == 'runtime') for key in ('runtime_facts', 'ui_capabilities')),
                'missing factual runtime/capability observations')
    else:
        require(not value['checks'] and not value['images'], 'failed phase cannot claim complete checks or export images')


def validate_export(directory, repo, expected, private):
    for path in (directory, private, repo): capture.no_links(path)
    path = directory / EXPORT
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= MAX_JSON, 'invalid qualification JSON')
    raw = path.read_bytes(); value = json.loads(raw.decode('ascii'))
    validate_shape(value)
    require(raw == (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii'), 'noncanonical export')
    require(value == capture.read_private_json(private / 'result.json') and value['source_sha'] == expected, 'private result/invocation mismatch')
    optional = {name for field, name in (('native_projection', 'native-review.json'), ('runtime_facts', runtime_facts.PROJECTION_NAME),
                                        ('ui_capabilities', ui_capabilities.EXPORT)) if value[field] is not None}
    require({p.name for p in directory.iterdir()} == {EXPORT, *value['images'], *optional}, 'unexpected exported file')
    specifications = command_specifications(repo, private, value['phase'], expected)
    for item in value['commands']:
        base = private / 'commands' / item['id']; journal = capture.read_private_json(base / 'journal.json')
        require(journal['command'] == specifications[item['id']], 'private exact command changed')
        require(all(item[key] == journal[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')),
                'command differs from actual journal')
        require(all(record(base / stream) == item[stream] for stream in ('stdout', 'stderr')), 'private output changed')
        require(all(item[key]['bytes'] <= 64 * 1024 * 1024 for key in ('stdout', 'stderr')), 'private output budget exceeded')
        require(journal['cwd'] == str(private / 'unrelated-cwd' if item['id'] in
                {'distribution-first', 'distribution-second', *SCENES, 'absent-light-single', 'default-rejects-legacy', 'legacy-no-model'} else repo),
                'private command CWD changed')
        if value['phase'] == 'regressions':
            step = next((step for step in regression_plan() if step['id'] == item['id']), None)
            if step is not None:
                require(journal['command'] == step['command'], 'regression command changed')
                if value['status'] != 'failed':
                    raw = (base / 'stdout').read_bytes()
                    log = (raw + b'\n' + (base / 'stderr').read_bytes()).decode('utf-8', errors='replace')
                    if step['diagnostic']: check_rejection(raw, item['exit_code'], step['diagnostic'])
                    if step['exact'] or item['id'].endswith('-tests'): check_test_output(log, step['exact'])
    size = 0
    for name, binding in value['images'].items():
        image = directory / name; capture.no_links(image)
        require(image.is_file() and image.stat().st_nlink == 1, 'linked image')
        require(candidate.validate_png(image) == binding and record(image) == record(private / name), 'changed image')
        size += image.stat().st_size
    require(size <= MAX_IMAGES, 'image export budget exceeded')
    if value['runtime_facts'] is not None:
        facts = validate_runtime_facts(private, repo, expected, private / 'build-private', private / 'build-text')
        path = directory / runtime_facts.PROJECTION_NAME; capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and record(path) == value['runtime_facts']
                and path.read_bytes() == runtime_facts.canonical(facts), 'runtime fact export changed')
    if value['ui_capabilities'] is not None:
        caps = ui_capabilities.project(private / 'ui-capabilities', repo, expected)
        path = directory / ui_capabilities.EXPORT; capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and record(path) == value['ui_capabilities']
                and path.read_bytes() == (json.dumps(caps, indent=2, sort_keys=True) + '\n').encode('ascii'), 'capability export changed')
    if value['native_projection']:
        path = directory / 'native-review.json'; capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= native.MAX_BYTES, 'invalid native projection file')
        require(record(path) == value['native_projection'] == record(private / 'native-review.json'), 'changed native projection')
        bundle = private / 'extracted/swift-candidate'
        strict_bundle(bundle, private / 'build-private/target-analytic' / check.TARGET / 'release/flightsim-app.exe',
                      check.digest(bundle / 'bundle-manifest.json'), repo, private / 'build-private/capture/analytic/notices')
        projection = native.project(repo, expected, private / 'build-private', private / 'build-text', bundle,
                                    runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities')
        require(path.read_bytes() == (json.dumps(projection, indent=2, sort_keys=True) + '\n').encode('ascii'), 'native projection differs from private authoritative inputs')
    if value['status'] != 'failed':
        source = capture.source_evidence(repo, expected)
        require(source == capture.read_private_json(private / 'source.json') and source['source_tree'] == value['source_tree']
                and record(private / 'source.json') == value['bindings']['source'], 'changed final source')
        if value['phase'] == 'runtime':
            validate_runtime_observations(repo, private, expected)
            require(record(private / 'runtime-bindings.json') == value['bindings']['runtime'], 'changed runtime binding')
    return value


def qualify(repo, expected, private, evidence, phase):
    require(capture.hex_string(expected, 40), 'full source SHA required')
    for path in (repo, private, evidence):
        require(path.is_absolute(), 'absolute paths required'); capture.no_links(path)
    repo, private, evidence = (path.resolve() for path in (repo, private, evidence))
    capture.disjoint(repo, private, evidence)
    require(not private.exists() and not evidence.exists(), 'fresh private and export roots required')
    private.mkdir(parents=True)
    value = {'schema_version': 1, 'identity': IDENTITY, 'phase': phase, 'source_sha': expected, 'source_tree': None,
             'status': 'failed', 'release_authorized': False, 'native_runtime_qualified': False,
             'distribution_qualified': False, 'appearance_accepted': False, 'checks': {}, 'commands': [], 'images': {},
             'open_reviews': OPEN_REVIEWS, 'limits': LIMITS, 'bindings': {}, 'native_projection': None,
             'runtime_facts': None, 'ui_capabilities': None}
    with capture.private_console(private):
        runner = None
        try:
            require(sys.platform == 'win32' and platform.machine().lower() in ('amd64', 'x86_64'), 'native Windows x64 required')
            capture.reject_configuration(repo, os.environ)
            require(shutil.disk_usage(private).free >= capture.MIN_START_FREE, 'insufficient initial disk')
            source = capture.source_evidence(repo, expected)
            value['source_tree'] = source['source_tree']; write_json(private / 'source.json', source)
            value['bindings']['source'] = record(private / 'source.json')
            cargo_home = private / 'cargo-home'; cargo_home.mkdir()
            env = {**os.environ, 'CARGO_HOME': str(cargo_home), 'RUSTFLAGS': '-D warnings', 'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never'}
            runner = Runner(repo, private, env, phase)
            _, raw, _ = runner.run('rustc', ['rustc', '+' + check.TOOLCHAIN, '-vV'], timeout=60)
            compiler = raw.decode('utf-8').splitlines()
            require(compiler[0].startswith('rustc 1.93.0 ') and ('host: ' + check.TARGET) in compiler, 'wrong native compiler')
            checks, images = (regressions if phase == 'regressions' else runtime)(runner, source)
            require(capture.source_evidence(repo, expected) == source, 'source changed during qualification')
            if phase == 'runtime': value['bindings']['runtime'] = record(private / 'runtime-bindings.json')
            value.update(checks=checks, images=images, status='engineering_evidence_complete_reviews_required')
        except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError, zipfile.BadZipFile, zlib.error) as error:
            write_json(private / 'failure.json', {'type': type(error).__name__, 'message': str(error)})
        if runner is not None: value['commands'] = runner.commands
        if (private / 'native-review.json').is_file(): value['native_projection'] = record(private / 'native-review.json')
        for field, path in (('runtime_facts', private / 'runtime-facts' / runtime_facts.PROJECTION_NAME),
                            ('ui_capabilities', private / 'ui-capabilities' / ui_capabilities.EXPORT)):
            if path.is_file(): value[field] = record(path)
        write_json(private / 'result.json', value)
    validate_shape(value)
    evidence.mkdir(parents=True)
    write_json(evidence / EXPORT, value)
    for name in value['images']: shutil.copyfile(private / name, evidence / name)
    if value['native_projection']: shutil.copyfile(private / 'native-review.json', evidence / 'native-review.json')
    if value['runtime_facts']: shutil.copyfile(private / 'runtime-facts' / runtime_facts.PROJECTION_NAME, evidence / runtime_facts.PROJECTION_NAME)
    if value['ui_capabilities']: shutil.copyfile(private / 'ui-capabilities' / ui_capabilities.EXPORT, evidence / ui_capabilities.EXPORT)
    validate_export(evidence, repo, expected, private)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--describe', action='store_true')
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source-sha')
    parser.add_argument('--phase', choices=tuple(PHASE_SECONDS))
    parser.add_argument('--private', type=Path)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--validate-evidence', type=Path)
    args = parser.parse_args()
    if args.describe:
        print(json.dumps({'identity': IDENTITY, 'status': 'prepared_unexecuted', 'regressions': regression_plan(),
                          'scenes': SCENES, 'phase_seconds': PHASE_SECONDS, 'release_authorized': False,
                          'required_gates': check.GATES, 'open_reviews': OPEN_REVIEWS, 'limits': LIMITS}, indent=2))
        return 0
    try:
        require(args.source_sha is not None and args.private is not None, 'source and private root required')
        if args.validate_evidence:
            require(args.evidence is None, 'unexpected capture destination')
            validate_export(args.validate_evidence, args.repo.resolve(), args.source_sha, args.private.resolve())
            print('Bounded qualification evidence validated; release_authorized=false.')
            return 0
        require(args.phase is not None and args.evidence is not None, 'phase and export root required')
        result = qualify(args.repo, args.source_sha, args.private, args.evidence, args.phase)
        print('Analytical Windows evidence: ' + result['status'] + '; release_authorized=false.')
        return 0 if result['status'] != 'failed' else 1
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print('Qualification/export failed; raw details remain private.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
