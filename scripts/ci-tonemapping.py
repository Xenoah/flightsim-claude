#!/usr/bin/env python3
"""Linux CI source/build regression; does not run or qualify an application.

Keep command status/logs and exact-build provenance outside the two build trees.
The existing artifact checker owns graph/fingerprint/dep-info/LUT validation.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys

TARGET = 'x86_64-unknown-linux-gnu'
SOURCE_RECIPE = 'bevy-0.18.1-tony-filmic-source-v1'


def inspect_source(source_root, upstream_archive):
    spec = importlib.util.spec_from_file_location('ci_tone_source', Path(__file__).with_name('check-tonemapping-build.py'))
    tone = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(tone)
    return tone.validate_source(source_root, upstream_archive)


def audit_command(mode, graph, messages, source_root, upstream_archive):
    return [sys.executable, 'scripts/check-tonemapping-build.py', '--mode', mode,
            '--kind', 'app', '--graph', str(graph), '--messages', str(messages),
            '--source-recipe', SOURCE_RECIPE, '--source-root', str(source_root),
            '--upstream-archive', str(upstream_archive), '--target', TARGET]

ENVIRONMENT = (
    'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET',
    'CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_DEV_DEBUG',
    'CARGO_PROFILE_TEST_DEBUG', 'RUSTUP_TOOLCHAIN', 'RUSTC', 'RUSTC_WRAPPER',
    'RUSTC_WORKSPACE_WRAPPER', 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER',
    'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS', 'CC', 'CXX', 'AR',
    'ImageOS', 'ImageVersion', 'RUNNER_ARCH', 'RUNNER_OS', 'GITHUB_REPOSITORY',
    'GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT',
)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def run(evidence, env, name, command, expected_diagnostic=None):
    """Record every real exit status; accept only the named compile rejection."""
    stdout = evidence / f'{name}.stdout'
    stderr = evidence / f'{name}.stderr'
    record = {
        'command': command, 'cwd': str(Path.cwd()),
        'target_dir': env.get('CARGO_TARGET_DIR'),
        'started_utc': datetime.now(timezone.utc).isoformat(),
        'exit_code': None, 'expected_diagnostic': expected_diagnostic,
    }
    record_path = evidence / f'{name}.command.json'
    write_json(record_path, record)
    print(f'{name}: {shlex.join(command)}', flush=True)
    with stdout.open('w') as out, stderr.open('w') as err:
        result = subprocess.run(command, env=env, stdout=out, stderr=err, check=False)
    record['exit_code'] = result.returncode
    record['finished_utc'] = datetime.now(timezone.utc).isoformat()
    write_json(record_path, record)
    accepted = result.returncode == 0
    if expected_diagnostic is not None:
        messages = [json.loads(line) for line in stdout.read_text().splitlines()]
        errors = [m['message']['message'] for m in messages
                  if m.get('reason') == 'compiler-message'
                  and m['message']['level'] == 'error']
        accepted = (
            result.returncode == 101
            and errors == [expected_diagnostic]
            and bool(messages)
            and messages[-1] == {'reason': 'build-finished', 'success': False}
        )
        record['matched_expected_rejection'] = accepted
        write_json(record_path, record)
    if not accepted:
        print(stdout.read_text(), file=sys.stderr)
        print(stderr.read_text(), file=sys.stderr)
        print(f'{name} failed (exit {result.returncode}); see {record_path}', file=sys.stderr)
        raise SystemExit(result.returncode if result.returncode > 0 else
                         128 - result.returncode if result.returncode < 0 else 1)
    print(f'{name}: passed (exit {result.returncode})', flush=True)
    return stdout


def canonical_directory_input(value):
    """Check the supplied spelling before resolving aliases away."""
    path = Path(value).absolute()
    if '..' in path.parts or path != path.resolve() or any(
            item.is_symlink() or getattr(item, 'is_junction', lambda: False)()
            for item in (path, *path.parents)):
        raise SystemExit('Refusing aliased CI directory input')
    return path


def create_final_target(evidence, build_root, mode, owned_qa):
    if mode not in ('ordinary', 'analytic') or owned_qa.get(str(build_root)) != directory_identity(build_root):
        raise SystemExit('Refusing final target under replaced or unowned build root')
    target = canonical_directory_input(build_root / mode)
    if target.exists():
        raise SystemExit('Final target must be fresh')
    target.mkdir(exist_ok=False)
    identity = directory_identity(target)
    write_json(evidence / (mode + '-final-target.json'),
               {'directory': str(target), 'directory_identity': identity, 'created_fresh': True})
    return target


def directory_identity(path):
    if path != path.resolve() or path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
        raise SystemExit('Refusing aliased QA directory')
    value = path.stat()
    return [value.st_dev, value.st_ino]


def create_qa_targets(evidence, build_root, environments):
    owned = {str(build_root): directory_identity(build_root)}
    for env in environments.values():
        path = Path(env['CARGO_TARGET_DIR'])
        path.mkdir(exist_ok=False)
        owned[str(path)] = directory_identity(path)
    write_json(evidence / 'qa-target-ownership.json', owned)
    return owned


def discard_qa_targets(evidence, build_root, environments, owned):
    """Discard only the exact fresh QA directories created by this invocation."""
    paths = [Path(env['CARGO_TARGET_DIR']) for env in environments.values()]
    expected = {build_root / ('qa-' + mode) for mode in ('ordinary', 'analytic')}
    if set(paths) != expected or set(owned) != {str(build_root), *(str(p) for p in expected)}:
        raise SystemExit('Refusing cleanup outside fresh QA scratch directories')
    for path in (build_root, *paths):
        if not path.is_dir() or owned[str(path)] != directory_identity(path):
            raise SystemExit('Refusing cleanup of replaced or unowned QA directories')
    record = {'directories': [str(p) for p in paths], 'owned_directory_identities': owned, 'completed': False}
    write_json(evidence / 'qa-target-cleanup.json', record)
    for path in paths:
        shutil.rmtree(path)
    record['completed'] = True
    write_json(evidence / 'qa-target-cleanup.json', record)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence', type=Path)
    parser.add_argument('build_root', type=Path)
    parser.add_argument('--upstream-archive', type=Path, required=True,
                        help='Checksum-bound private reference, outside source/evidence/build directories')
    args = parser.parse_args()
    evidence = canonical_directory_input(args.evidence)
    build_root = canonical_directory_input(args.build_root)
    source_root = Path.cwd().resolve()
    archive = args.upstream_archive.absolute()
    if any(archive.is_relative_to(path) for path in (source_root, evidence, build_root)):
        raise SystemExit('Private upstream archive must stay outside source/evidence/build directories')
    source = inspect_source(source_root, archive)
    if evidence.is_relative_to(build_root) or build_root.is_relative_to(evidence):
        raise SystemExit('Evidence and build roots must be disjoint')
    # Fresh evidence and targets cannot attest a previous or incomplete run.
    if evidence.exists() or build_root.exists():
        raise SystemExit('CI requires fresh evidence and build directories')
    evidence.mkdir(parents=True, exist_ok=False)
    build_root.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    provenance = {
        'scope': 'Linux source/build regression, not native/Windows/release qualification',
        'source_recipe': SOURCE_RECIPE, 'source_identity': source,
        'release_admitted': False, 'native_qualified': False,
        'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'source_tree': subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], text=True).strip(),
        'source_status': subprocess.check_output(['git', 'status', '--porcelain'], text=True),
        'cargo_lock_sha256': hashlib.sha256(Path('Cargo.lock').read_bytes()).hexdigest(),
        'rustc': subprocess.check_output(['rustc', '-vV'], text=True),
        'cargo': subprocess.check_output(['cargo', '-vV'], text=True),
        'python': sys.version, 'host': platform.uname()._asdict(),
        'os_release': Path('/etc/os-release').read_text(),
        'target': TARGET, 'profile': 'dev/test, debug=0, incremental=0',
        'environment': {key: env.get(key) for key in ENVIRONMENT},
        'target_directories': {mode: str(build_root / mode) for mode in ('ordinary', 'analytic')},
        'qa_target_directories': {mode: str(build_root / ('qa-' + mode)) for mode in ('ordinary', 'analytic')},
    }
    write_json(evidence / 'provenance.json', provenance)
    overrides = ('CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER',
                 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER', 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS')
    if any(env.get(key) for key in overrides):
        raise SystemExit('CI rejects compiler or rustflags overrides')
    if provenance['source_status'] or env.get('RUSTFLAGS') != '-D warnings':
        raise SystemExit('CI requires a clean source checkout and RUSTFLAGS=-D warnings')

    # Fetch first, then use the same locked offline target for every Cargo recipe.
    run(evidence, env, 'fetch', ['cargo', 'fetch', '--locked', '--target', TARGET])
    base = ['--locked', '--offline', '-j2', '--target', TARGET]
    analytic = ['--no-default-features', '--features', 'analytic-tonemapping']
    environments = {mode: {**env, 'CARGO_TARGET_DIR': str(build_root / ('qa-' + mode))}
                    for mode in ('ordinary', 'analytic')}
    owned_qa = create_qa_targets(evidence, build_root, environments)
    for package in ('flightsim-render', 'flightsim-app'):
        run(evidence, environments['analytic'], f'analytic-{package}-tests',
            ['cargo', 'test', *base, '-p', package, '--tests', *analytic])
    # --all-targets compiles sun_clock too; two app artifact audits suffice here.
    run(evidence, environments['analytic'], 'analytic-clippy',
        ['cargo', 'clippy', *base, '-p', 'flightsim-render', '-p', 'flightsim-app',
         '--all-targets', '--no-default-features', '--features',
         'flightsim-app/analytic-tonemapping', '--', '-D', 'warnings'])

    mixed = 'analytic-tonemapping requires --no-default-features; select one tone mode'
    guards = (
        ('app-mixed', 'ordinary', ['-p', 'flightsim-app', '--features', 'analytic-tonemapping'], mixed),
        ('render-mixed', 'ordinary', ['-p', 'flightsim-render', '--features', 'analytic-tonemapping'], mixed),
        ('app-neither', 'analytic', ['-p', 'flightsim-app', '--no-default-features'],
         'flightsim-app requires default or analytic-tonemapping; select one tone mode'),
        ('sun-clock-neither', 'analytic',
         ['-p', 'flightsim-render', '--example', 'sun_clock', '--no-default-features'],
         'sun_clock requires default or analytic-tonemapping; select one tone mode'),
    )
    for name, mode, flags, diagnostic in guards:
        run(evidence, environments[mode], name,
            ['cargo', 'check', *base, *flags, '--message-format=json'], diagnostic)
    run(evidence, environments['analytic'], 'render-library-neither',
        ['cargo', 'check', *base, '-p', 'flightsim-render', '--lib', '--no-default-features'])

    # Test/check/clippy roots have different fingerprints from the final build.
    # Free only our own fresh QA trees to keep two Bevy trees on disk at most.
    discard_qa_targets(evidence, build_root, environments, owned_qa)
    environments = {mode: {**env, 'CARGO_TARGET_DIR': str(build_root / mode)}
                    for mode in ('ordinary', 'analytic')}
    # Run each graph/build/audit together, with exactly one selected package.
    # Nothing writes to either target directory after its final artifact audit.
    for mode in ('ordinary', 'analytic'):
        create_final_target(evidence, build_root, mode, owned_qa)
        selected = ['-p', 'flightsim-app', *(analytic if mode == 'analytic' else [])]
        graph = run(evidence, environments[mode], f'{mode}-app-graph',
                    ['cargo', 'tree', '--locked', '--offline', '--target', TARGET,
                     *selected, '--edges', 'normal,build', '--prefix', 'none',
                     '--format', '{p} features=[{f}]'])
        messages = run(evidence, environments[mode], f'{mode}-app-build',
                       ['cargo', 'build', *base, *selected, '--message-format=json'])
        run(evidence, environments[mode], f'{mode}-app-audit',
            audit_command(mode, graph, messages, source_root, archive))


if __name__ == '__main__':
    main()
