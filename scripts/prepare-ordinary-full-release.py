#!/usr/bin/env python3
"""Prepare one authorized ordinary archive from one retained audited build.

Local Windows execution only. No compiler, GitHub write, review generation or
new authority. Every successful export is recomputed from retained private
source/build/runtime/archive/command bytes before it can be uploaded.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile
import zlib


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


native = load('project-ordinary-native-evidence')
applicability = load('check-ordinary-release-applicability')
archive = load('verify-ordinary-release-archive')
payload = applicability.payload
capture, release, require = payload.capture, payload.release, payload.require
PUBLIC = 'ordinary-release-evidence.json'
NOTES_TEMPLATE = 'docs/release/windows-component-release-notes.md'
NOTES = 'release-notes.md'
COMPONENT_FILES = tuple('docs/release/components/' + name for name in (
    'MICROSOFT-COMPONENT-TERMS.txt', 'MICROSOFT-COMPONENT-TERMS.ja.txt',
    'MICROSOFT-COMPONENT-NOTICE.txt'))
STAGES = frozenset(('source', 'capture', 'executable-independence', 'executable-copy', 'capture-revalidation',
                   'distribution-info', 'staging', 'runtime-collection', 'native-projection',
                   'applicability', 'archive', 'smoke', 'final-revalidation'))


def stage(name):
    require(name in STAGES, 'unknown ordinary preparation stage')
    print('Ordinary preparation stage: ' + name, file=sys.stderr, flush=True)


def independent_audited_executable(executable, expected):
    """Detach only Cargo's final name, preserving the exact audited bytes.

    Cargo may hardlink this file from release/deps. Do not relax the independent
    input/payload guards or modify the deps artifact. Revalidate before replacing
    this one name atomically; the original build audit remains byte-identical.
    """
    executable = payload.absolute(executable)
    require(executable.name == 'flightsim-app.exe' and executable.parent.name == 'release'
            and executable.parent.parent.name == release.TARGET, 'invalid audited executable layout')
    require(capture.valid_record(expected) and expected['bytes'] > 0
            and capture.file_record(executable) == expected, 'audited executable changed')
    before = executable.stat()
    if before.st_nlink == 1:
        payload.independent_file(executable)
        return
    require(before.st_nlink > 1, 'invalid executable link count')
    stage('executable-copy')
    descriptor, name = tempfile.mkstemp(prefix='.ordinary-audited-', suffix='.tmp', dir=executable.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, 'wb') as output, executable.open('rb') as source:
            shutil.copyfileobj(source, output)
            output.flush()
            os.fsync(output.fileno())
        copied = payload.independent_file(temporary)
        require(capture.file_record(temporary) == expected
                and capture.file_record(executable) == expected, 'audited executable copy changed')
        after = executable.stat()
        require((before.st_dev, before.st_ino, before.st_nlink, before.st_size, before.st_mtime_ns)
                == (after.st_dev, after.st_ino, after.st_nlink, after.st_size, after.st_mtime_ns),
                'audited executable changed during independent copy')
        os.replace(temporary, executable)
        installed = payload.independent_file(executable)
        require((copied.st_dev, copied.st_ino, copied.st_size, copied.st_mtime_ns)
                == (installed.st_dev, installed.st_ino, installed.st_size, installed.st_mtime_ns),
                'independent executable identity changed')
        require(capture.file_record(executable) == expected, 'independent executable differs from audit')
    finally:
        if temporary.exists():
            temporary.unlink()


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + '\n').encode('ascii')


def write_new(path, value):
    with path.open('xb') as stream:
        stream.write(canonical(value))


def paths(repo, build_private, build_text, private, output):
    roots = tuple(map(payload.absolute, (repo, build_private, build_text, private, output)))
    capture.disjoint(*roots)
    return roots


def context(repo, expected):
    source = capture.source_evidence(repo, expected)
    gate, plan = release.inspect(repo)
    require(gate['authorized'] is True and gate['status'] == 'authorized' and not gate['blockers'],
            'ordinary source/review/publication gate is blocked')
    require(applicability.PATH in release.source_inventory(repo)[1], 'committed real applicability review required')
    top = 'flightsim-claude-v' + gate['version'] + '-windows-x86_64'
    payload.member_name(top)
    return source, gate, plan, top


def publication_notes(repo, expected):
    """Resolve only the exact admitted source link, never a branch/latest URL."""
    require(isinstance(expected, str) and len(expected) == 40
            and all(c in '0123456789abcdef' for c in expected),
            'exact release source is required for component links')
    require(NOTES_TEMPLATE in release.source_inventory(repo)[1], 'component disclosure must be committed')
    raw = release.staging.safe_file(repo, NOTES_TEMPLATE).read_bytes()
    require(0 < len(raw) <= 32 * 1024 and raw.count(b'@SOURCE_SHA@') == 3,
            'unexpected component disclosure template')
    result = raw.replace(b'@SOURCE_SHA@', expected.encode('ascii'))
    require(b'@SOURCE_SHA@' not in result, 'unresolved component source link')
    return result


def project(repo, expected, build_private, build_text, private, output):
    """Read-only complete revalidation; original private inputs are mandatory."""
    stage('source')
    repo, build_private, build_text, private, output = paths(repo, build_private, build_text, private, output)
    source, gate, plan, top = context(repo, expected)
    staged, extracted = private / 'staged' / top, private / 'extracted' / top
    stage('native-projection')
    native_facts = native.project(repo, expected, build_private, build_text, staged,
                                  runtime_facts_private=private / 'runtime-facts')
    stage('applicability')
    conditions = applicability.validate(repo, native_facts)
    stage('archive')
    executable = build_private / 'target-ordinary' / release.TARGET / 'release/flightsim-app.exe'
    release.verify_bundle(extracted, plan, executable)
    staged_files, extracted_files = payload.snapshot_payload(staged), payload.snapshot_payload(extracted)
    require(staged_files == extracted_files, 'fresh extraction differs from staged authorized payload')
    frozen = output / (top + '.zip')
    archive_binding = archive.audit_archive(frozen, staged_files, top)
    stage('smoke')
    observation = archive.validate_smoke(extracted, private / 'smoke')
    require(observation['bundle_files'] == staged_files, 'smoke bundle differs from final archive')
    scenes = {}
    for identity, result in observation['scenes'].items():
        execution = result['execution']
        scenes[identity] = {'journal': result['journal'], 'screenshot': result['screenshot'],
                            'exit_code': execution['exit_code'], 'outcome': execution['outcome'],
                            'elapsed_seconds': execution['elapsed_seconds']}
    # The existing reviewed copy plan and applicability gate already cover
    # these exact inputs. This schedule exposes their same-build relationship;
    # it never creates recipient assent or a new component clearance.
    component_schedule = {
        'kind': 'incorporated_microsoft_component_exact_build_bindings',
        'source_sha': expected, 'source_tree': source['source_tree'],
        'documents': {name: staged_files[name] for name in COMPONENT_FILES},
        'executable': native_facts['payload_projection']['bindings']['native_executable'],
        'original_inventory': conditions['inventory_comparison']['original'],
        'captured_inventory': conditions['inventory_comparison']['captured'],
        'review_applicability': conditions['applicability_record'],
        'recipient_assent_collected': False, 'dialog_tested_by_smoke': False}
    result = {'schema_version': 1, 'kind': 'ordinary_final_same_build_evidence_not_authority',
              'source_sha': expected, 'source_tree': source['source_tree'],
              'version': gate['version'], 'source_inventory_sha256': gate['source_inventory_sha256'],
              'release_inventory_sha256': gate['release_inventory_sha256'],
              'authorization_sha256': gate['authorization_sha256'],
              'bindings': {'archive': archive_binding, 'native_projection': payload.record(canonical(native_facts)),
                           'runtime_facts': capture.file_record(private / 'runtime-facts' / native.runtime_facts.PROJECTION_NAME),
                           'smoke_manifest': capture.file_record(private / 'smoke/smoke.json'),
                           'publication_notes': payload.record(publication_notes(repo, expected))},
              'applicability': conditions, 'files': staged_files, 'scenes': scenes,
              'component_terms': component_schedule,
              'release_authorized': False, 'dependency_review_approved': False,
              'runtime_accepted': False, 'appearance_accepted': False,
              'limits': ['software_d3d12_only', 'physical_gpu_controller_audio_not_qualified',
                         'subjective_appearance_not_decided', 'publication_authority_is_existing_ordinary_receipt']}
    # Revalidate after smoke/private reads. Never authorize a cached snapshot.
    stage('final-revalidation')
    require(native.project(repo, expected, build_private, build_text, staged,
                           runtime_facts_private=private / 'runtime-facts') == native_facts,
            'same-build native facts changed during final revalidation')
    require(context(repo, expected) == (source, gate, plan, top)
            and payload.snapshot_payload(staged) == staged_files
            and payload.snapshot_payload(extracted) == staged_files
            and archive.audit_archive(frozen, staged_files, top) == archive_binding,
            'source or final archive changed during revalidation')
    require(archive.validate_smoke(extracted, private / 'smoke') == observation,
            'original smoke evidence changed during final native revalidation')
    require(len(canonical(result)) <= payload.MAX_JSON_BYTES, 'final evidence exceeds byte budget')
    return result, native_facts


def validate(repo, expected, build_private, build_text, private, output):
    stage('final-revalidation')
    output_before = payload.snapshot_payload(output)
    result, facts = project(repo, expected, build_private, build_text, private, output)
    require((output / PUBLIC).read_bytes() == canonical(result), 'final evidence changed')
    require((output / 'ordinary-native-review.json').read_bytes() == canonical(facts), 'native export changed')
    top = 'flightsim-claude-v' + result['version'] + '-windows-x86_64'
    checksum = result['bindings']['archive']['sha256'] + '  ' + top + '.zip'
    require((output / (top + '.zip.sha256')).read_bytes() == checksum.encode('ascii'), 'archive checksum changed')
    metadata = {'bundle': top + '.zip', 'sha256': result['bindings']['archive']['sha256'],
                'source_sha': expected, 'tag': 'v' + result['version'], 'version': result['version'],
                'release_inventory_sha256': result['release_inventory_sha256'],
                'authorization_sha256': result['authorization_sha256'],
                'evidence_sha256': payload.record(canonical(result))['sha256'],
                'notes_sha256': result['bindings']['publication_notes']['sha256']}
    require((output / 'release-metadata.json').read_bytes() == canonical(metadata), 'release metadata changed')
    wanted_output = {PUBLIC: payload.record(canonical(result)),
                     'ordinary-native-review.json': payload.record(canonical(facts)),
                     'release-metadata.json': payload.record(canonical(metadata)),
                     top + '.zip': result['bindings']['archive'],
                     top + '.zip.sha256': payload.record(checksum.encode('ascii')),
                     NOTES: result['bindings']['publication_notes']}
    require((output / NOTES).read_bytes() == publication_notes(repo, expected), 'component release disclosure changed')
    images = {'light-single-cockpit': 'flightsim-windows-smoke.png',
              'swift-sport-chase': 'flightsim-windows-smoke-swift.png'}
    tag = 'v' + result['version']
    for identity, name in images.items():
        destination = 'flightsim-claude-' + tag + ('-windows-smoke.png' if identity == 'light-single-cockpit' else '-windows-smoke-swift.png')
        wanted = {key: result['scenes'][identity]['screenshot'][key] for key in ('sha256', 'bytes')}
        require(capture.file_record(output / destination) == wanted
                and capture.file_record(private / 'smoke' / name) == wanted, 'exported smoke screenshot changed')
        wanted_output[destination] = wanted
    require(output_before == wanted_output and payload.snapshot_payload(output) == wanted_output,
            'unexpected or changed release-artifact member')
    return result


def prepare(repo, expected, build_private, build_text, private, output):
    require(sys.platform == 'win32', 'native Windows is required for final ordinary preparation')
    stage('source')
    repo, build_private, build_text, private, output = paths(repo, build_private, build_text, private, output)
    source, gate, plan, top = context(repo, expected)
    require(not private.exists() and not output.exists(), 'fresh preparation and output roots required')
    stage('capture')
    verified = capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    require(verified['status'] == capture.PASS, 'completed same-build audit required')
    executable = build_private / 'target-ordinary' / release.TARGET / 'release/flightsim-app.exe'
    require(capture.file_record(executable) == verified['builds']['ordinary']['executable'], 'audited executable changed')
    stage('executable-independence')
    independent_audited_executable(executable, verified['builds']['ordinary']['executable'])
    stage('capture-revalidation')
    require(capture.validate_export(build_text, repo=repo, expected=expected, private=build_private) == verified,
            'original build audit changed after independent copy')
    stage('distribution-info')
    release.verify_distribution_info(executable, gate['version'])
    stage('staging')
    private.mkdir(parents=True); output.mkdir(parents=True)
    staged = private / 'staged' / top
    staged.mkdir(parents=True)
    shutil.copyfile(executable, staged / 'flightsim-app.exe')
    for item in plan['files']:
        destination = staged / payload.member_name(item['path'])
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(release.staging.safe_file(repo, item['source']), destination)
    release.verify_bundle(staged, plan, executable)
    stage('runtime-collection')
    native.runtime_facts.collect_runtime_facts(private / 'runtime-facts', source_sha=expected,
        recipe_cfg_args=['-D', 'warnings'], linker_trace=build_private / 'capture/ordinary/build.stderr',
        audited_executable=executable)
    stage('native-projection')
    facts = native.project(repo, expected, build_private, build_text, staged,
                           runtime_facts_private=private / 'runtime-facts')
    stage('applicability')
    applicability.validate(repo, facts)
    stage('archive')
    frozen = output / (top + '.zip')
    files = archive.create_archive(staged, frozen, top)
    extracted = archive.extract_verified_archive(frozen, private / 'extracted', files, top)
    release.verify_bundle(extracted, plan, executable)
    stage('smoke')
    archive.run_smoke(extracted, private / 'smoke')
    stage('final-revalidation')
    result, facts = project(repo, expected, build_private, build_text, private, output)
    write_new(output / PUBLIC, result); write_new(output / 'ordinary-native-review.json', facts)
    with (output / NOTES).open('xb') as stream:
        stream.write(publication_notes(repo, expected))
    (output / (top + '.zip.sha256')).write_bytes((result['bindings']['archive']['sha256'] + '  ' + top + '.zip').encode('ascii'))
    write_new(output / 'release-metadata.json', {
        'bundle': top + '.zip', 'sha256': result['bindings']['archive']['sha256'], 'source_sha': expected,
        'tag': 'v' + gate['version'], 'version': gate['version'],
        'release_inventory_sha256': gate['release_inventory_sha256'], 'authorization_sha256': gate['authorization_sha256'],
        'evidence_sha256': payload.record(canonical(result))['sha256'],
        'notes_sha256': result['bindings']['publication_notes']['sha256']})
    for name, suffix in (('flightsim-windows-smoke.png', '-windows-smoke.png'),
                         ('flightsim-windows-smoke-swift.png', '-windows-smoke-swift.png')):
        shutil.copyfile(private / 'smoke' / name, output / ('flightsim-claude-v' + gate['version'] + suffix))
    return validate(repo, expected, build_private, build_text, private, output)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repo', 'build-private', 'build-text', 'private', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--expected-sha', required=True)
    parser.add_argument('--validate-only', action='store_true')
    args = parser.parse_args(argv)
    try:
        action = validate if args.validate_only else prepare
        action(args.repo, args.expected_sha, args.build_private, args.build_text, args.private, args.output)
    except (ValueError, OSError, KeyError, TypeError, AttributeError, UnicodeError, RecursionError,
            zipfile.BadZipFile, zlib.error, subprocess.SubprocessError):
        print('Ordinary final evidence blocked; private inputs were not exported.', file=sys.stderr)
        return 1
    print('Exact ordinary archive and two extracted smoke observations revalidated; no new authority was created.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
