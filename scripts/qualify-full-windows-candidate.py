#!/usr/bin/env python3
"""Bind ordinary two-aircraft Windows observations to independent source/policy pins.

Consume an already completed same-host dual capture. No build, source-CI approval,
rights decision, appearance acceptance, or publication authority is created here.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import struct
import subprocess
import sys
import zipfile
import zlib


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


stage = load('full_qualification_stage', 'stage-full-windows-candidate.py')
native, capture, require = stage.native, stage.capture, stage.require
candidate, runtime_facts, capabilities = native.check.candidate, native.runtime_facts, native.ui_capabilities
device = load('full_qualification_device', 'project-device-error-facts.py')
originals = load('full_qualification_original_notices', 'export-full-original-notices.py')
POLICY_ROOT = Path(__file__).absolute().parents[1]
IDENTITY = 'full-two-aircraft-windows-observations-v1'
EXPORT = 'full-qualification.json'
ARCHIVE = 'full-windows-candidate.zip'
BUNDLE = 'full-windows-candidate'
MAX_JSON = 128 * 1024
MAX_STREAM = 64 * 1024 * 1024
MAX_ARCHIVE = 2 * 1024 * 1024 * 1024
MAX_MEMBERS = 2048
MAX_UNPACKED = MAX_ARCHIVE
TIMEOUT = 180
WARP = {'WGPU_BACKEND': 'dx12', 'WGPU_FORCE_FALLBACK_ADAPTER': '1'}
SCENES = {'light-single-cockpit': ('light-single', 'cockpit', 'flightsim-windows-smoke.png'),
          'swift-sport-chase': ('swift-sport', 'chase', 'flightsim-windows-smoke-swift.png')}
FOCUSED_ID = 'light-single-exterior'
FOCUSED_SCENE = ('light-single', 'chase', 'flightsim-light-single-exterior.png')
ALL_SCENES = {**SCENES, FOCUSED_ID: FOCUSED_SCENE}
FLAGS = ('release_authorized', 'runtime_accepted', 'appearance_accepted', 'distribution_qualified')
LIMITS = ['software_d3d12_only', 'physical_gpu_controller_audio_unexecuted',
          'appearance_and_rights_require_separate_review', 'source_ci_requires_external_exact_head_check',
          'policy_and_run_identity_require_external_verification']
OUTCOMES = ('succeeded', 'failed', 'timed_out', 'termination_failed', 'launch_error')
write_json = capture.write_json


def record(path):
    capture.no_links(path)
    require(path.is_file() and path.stat().st_nlink == 1, 'independent regular file required')
    return capture.file_record(path)


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii')


def path_root(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'canonical absolute root required')
    capture.no_links(path)
    require(path == path.resolve(), 'aliased root forbidden')
    return path


def roots(repo, build_private, build_text, private, evidence, assembly=None, review=None):
    paths = [path_root(p) for p in (POLICY_ROOT, repo, build_private, build_text, private, evidence)]
    capture.disjoint(*paths)
    require((assembly is None) == (review is None), 'assembly and review must be paired')
    if assembly is not None:
        extras = [path_root(assembly), path_root(review)]
        capture.disjoint(*paths, *extras)


def git(*args):
    return subprocess.check_output(['git', *args], cwd=POLICY_ROOT)


def policy_evidence(source, expected_sha, expected_tree):
    """Caller pins policy identity; canonical P must preserve every inherited S blob."""
    require(capture.hex_string(expected_sha, 40) and capture.hex_string(expected_tree, 40), 'full policy commit/tree required')
    require(git('rev-parse', 'HEAD').decode().strip() == expected_sha
            and git('rev-parse', 'HEAD^{tree}').decode().strip() == expected_tree, 'policy identity differs from external pins')
    require(not git('status', '--porcelain', '--untracked-files=all'), 'policy checkout is not clean')
    algorithm = git('rev-parse', '--show-object-format').decode().strip()
    require(algorithm in ('sha1', 'sha256'), 'unknown policy object format')
    rows = {}
    for item in git('ls-tree', '-rz', 'HEAD').split(b'\0'):
        if not item: continue
        metadata, name = item.split(b'\t', 1)
        mode, kind, blob = metadata.decode('ascii').split()
        name = name.decode('utf-8'); native.relative(name)
        require(mode in ('100644', '100755') and kind == 'blob', 'linked or non-regular policy source')
        path = POLICY_ROOT / name; actual = record(path)
        value = hashlib.new(algorithm)
        value.update(('blob ' + str(actual['bytes']) + '\0').encode('ascii'))
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b''): value.update(chunk)
        require(value.hexdigest() == blob, 'policy bytes differ from canonical commit')
        rows[name] = actual
    require('scripts/qualify-full-windows-candidate.py' in rows, 'wrapper is not a committed policy input')
    for item in source['files']:
        require(item['path'] in rows and rows[item['path']]['sha256'] == item['checkout_sha256'],
                'policy changed inherited source bytes')
    return {'source_sha': source['source_sha'], 'source_tree': source['source_tree'],
            'policy_sha': expected_sha, 'policy_tree': expected_tree, 'files': rows,
            'inherited_source_files_byte_identical': True, 'release_authorized': False}


def command(app, image, name):
    aircraft, view, _ = ALL_SCENES[name]
    # Exact release.yml smoke selections and order, including synthetic traffic.
    return [str(app), '--screenshot', str(image), '--screenshot-delay', '5', '--exit-after-screenshot',
            '--aircraft', aircraft, '--view', view, '--traffic', 'synthetic']


def smoke(log, actual, name):
    require(actual['outcome'] == 'succeeded' and actual['exit_code'] == 0, 'screenshot did not exit zero')
    elapsed = actual['elapsed_seconds']
    require(type(elapsed) in (int, float) and math.isfinite(elapsed) and 0 <= elapsed <= TIMEOUT,
            'screenshot exceeded 180 seconds')
    plain = candidate.ANSI.sub('', log)
    for proof in ('aircraft model fitted:', '(' + ALL_SCENES[name][0] + ')', 'Screenshot saved to',
                  'Batch capture complete: status 0'):
        require(proof in plain, 'missing ordinary release smoke proof')
    require(not re.search(r'(?m)(^|\s)ERROR(\s|:|$)|(?im:thread .+ panicked at|panic(?:ked)? at|Failed to load asset|unregistered type)', plain),
            'ordinary release smoke logged a fatal error')


def relative(value):
    require(isinstance(value, str) and 0 < len(value) <= 512 and '\\' not in value and ':' not in value,
            'unsafe member path')
    parts = value.split('/')
    require(all(part and part not in ('.', '..') and part == part.rstrip(' .')
                and not any(ord(c) < 32 or ord(c) == 127 for c in part)
                and not re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', part)
                for part in parts), 'unsafe Windows member path')
    return value


def audit_archive(archive, files):
    """Compare each compressed member's actual bytes to the final extracted copy.

    Never extract, execute or rewrite. Names, duplicates, links and bombs fail
    before decompression; streaming caps remain enforced during decompression.
    """
    before = record(archive)
    require(before['bytes'] <= MAX_ARCHIVE, 'archive exceeds budget')
    expected = {BUNDLE + '/' + relative(name): item for name, item in files.items()}
    with zipfile.ZipFile(archive) as zipped:
        rows = zipped.infolist()
        require(0 < len(rows) <= MAX_MEMBERS and len(rows) == len(expected), 'archive member count changed')
        names = [relative(row.filename) for row in rows]
        require(len(set(names)) == len(names) and len({name.casefold() for name in names}) == len(names)
                and set(names) == set(expected), 'archive membership differs from frozen bundle')
        require(not zipped.comment and sum(row.file_size for row in rows) <= MAX_UNPACKED, 'archive budget/comment rejected')
        cursor = 0
        for row in rows:
            mode = row.external_attr >> 16
            require(not row.is_dir() and not row.flag_bits & ~0x800 and not row.comment and not row.extra
                    and stat.S_IFMT(mode) in (0, stat.S_IFREG)
                    and row.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), 'unsupported archive member')
            require(row.header_offset == cursor, 'archive has a prefix, gap or reordered local header')
            zipped.fp.seek(cursor)
            local = zipped.fp.read(30)
            require(len(local) == 30, 'truncated local header')
            signature, _, flags, compression, _, _, crc, compressed, unpacked, name_size, extra_size = struct.unpack('<4s5H3I2H', local)
            name = row.filename.encode('utf-8' if flags & 0x800 else 'cp437')
            require(signature == b'PK\x03\x04' and flags == row.flag_bits and compression == row.compress_type
                    and crc == row.CRC and compressed == row.compress_size and unpacked == row.file_size
                    and name_size == len(name) and extra_size == 0 and zipped.fp.read(name_size) == name,
                    'local/central archive header differs or has hidden extra bytes')
            cursor += 30 + name_size + compressed
            wanted = expected[row.filename]
            require(row.file_size == wanted['bytes'], 'archive member length changed')
            count, digest, crc_actual = 0, hashlib.sha256(), 0
            remaining = compressed
            decoder = zlib.decompressobj(-15) if compression == zipfile.ZIP_DEFLATED else None
            while remaining:
                chunk = zipped.fp.read(min(1024 * 1024, remaining))
                require(chunk, 'truncated compressed member')
                remaining -= len(chunk)
                while chunk:
                    output = decoder.decompress(chunk, min(1024 * 1024, wanted['bytes'] - count + 1)) if decoder else chunk
                    count += len(output)
                    require(count <= wanted['bytes'], 'archive decompression exceeded frozen length')
                    digest.update(output); crc_actual = zlib.crc32(output, crc_actual)
                    require(decoder is None or not decoder.unused_data, 'hidden bytes after DEFLATE end-of-stream')
                    pending = decoder.unconsumed_tail if decoder else b''
                    require(not pending or len(pending) < len(chunk) or output, 'DEFLATE made no progress')
                    chunk = pending
            # A capped zlib call can retain output internally even with no
            # unconsumed input. Drain that output with the same strict budget.
            while decoder is not None and not decoder.eof:
                output = decoder.decompress(b'', min(1024 * 1024, wanted['bytes'] - count + 1))
                count += len(output)
                require(count <= wanted['bytes'], 'archive decompression exceeded frozen length')
                digest.update(output); crc_actual = zlib.crc32(output, crc_actual)
                require(output or decoder.eof, 'incomplete DEFLATE stream')
                require(not decoder.unused_data and not decoder.unconsumed_tail, 'unconsumed DEFLATE bytes')
            require(decoder is None or (decoder.eof and not decoder.unused_data and not decoder.unconsumed_tail),
                    'incomplete or padded DEFLATE stream')
            require(count == wanted['bytes'] and digest.hexdigest() == wanted['sha256'] and crc_actual == crc,
                    'archived bytes differ from executed bundle')
        require(cursor == zipped.start_dir, 'hidden bytes before archive directory')
        central_size = sum(46 + len(row.filename.encode('utf-8' if row.flag_bits & 0x800 else 'cp437')) for row in rows)
        require(cursor + central_size + 22 == before['bytes'], 'archive has trailing or unaccounted bytes')
        zipped.fp.seek(cursor + central_size)
        end = zipped.fp.read(22)
        require(len(end) == 22 and struct.unpack('<4s4H2IH', end) ==
                (b'PK\x05\x06', 0, 0, len(rows), len(rows), central_size, cursor, 0), 'unexpected archive end record')
    require(record(archive) == before, 'archive changed during verification')
    return before


def archive_members(archive, expected, destination=None):
    audit_archive(archive, expected)
    if destination is not None:
        require(not destination.exists(), 'fresh extraction root required')
        destination.mkdir()
        with zipfile.ZipFile(archive) as zipped:
            # The strict audit admitted every exact path and compressed byte.
            zipped.extractall(destination)
        require(capture.snapshot_tree(destination / BUNDLE) == expected, 'extracted bytes changed')
        audit_archive(archive, expected)


def freeze(repo, verified, private, build_private):
    staged = private / BUNDLE
    native.validate_bundle(repo, staged, verified, original_notices=build_private / 'capture/ordinary/notices')
    snapshot = capture.snapshot_tree(staged)
    require(sum(row['bytes'] for row in snapshot.values()) <= MAX_ARCHIVE, 'bundle exceeds archive budget')
    archive = private / ARCHIVE
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as zipped:
        for name in sorted(snapshot): zipped.write(staged / name, BUNDLE + '/' + name)
    require(capture.snapshot_tree(staged) == snapshot, 'staged bytes changed during archive creation')
    archive_members(archive, snapshot, private / 'extracted')
    bundle = private / 'extracted' / BUNDLE
    native.validate_bundle(repo, bundle, verified, original_notices=build_private / 'capture/ordinary/notices')
    return snapshot


def sidecar(private, row, source, policy, verified):
    """Only reviewed P4 output; arbitrary private log text is never copied."""
    facts = {stream: device.project_stream(private / 'commands' / row['id'] / stream,
                                          expected_record=row[stream]) for stream in ('stdout', 'stderr')}
    # One deterministic stream, at most four reviewed/redacted 512-byte excerpts
    # in the entire sidecar. Both complete stream hashes remain in command.
    selected = 'stderr' if facts['stderr']['marker_lines_total'] else 'stdout'
    return {'schema_version': 1, 'kind': 'full_runtime_failure_observations_not_approval',
            'source_sha': source['source_sha'], 'source_tree': source['source_tree'],
            'policy_sha': policy['policy_sha'], 'policy_tree': policy['policy_tree'],
            'build_summary': verified, 'command': row, 'stream': selected, 'observations': facts[selected],
            'producer_authenticated': False, 'runtime_accepted': False, 'release_authorized': False}


def validate_command(row, name):
    require(set(row) == {'id', 'outcome', 'exit_code', 'elapsed_seconds', 'stdout', 'stderr'} and row['id'] == name,
        'unexpected runtime command')
    require(row['outcome'] in OUTCOMES and (type(row['exit_code']) is int and -(2**32) <= row['exit_code'] <= 2**32
        or row['exit_code'] is None and row['outcome'] in ('launch_error', 'termination_failed')), 'invalid runtime status')
    require(row['outcome'] not in ('succeeded', 'failed') or (row['outcome'] == 'succeeded') == (row['exit_code'] == 0),
        'contradictory exit status')
    require(type(row['elapsed_seconds']) in (int, float) and math.isfinite(row['elapsed_seconds'])
        and 0 <= row['elapsed_seconds'] <= TIMEOUT + 120, 'invalid elapsed runtime')
    require(all(capture.valid_record(row[key]) and row[key]['bytes'] <= MAX_STREAM for key in ('stdout', 'stderr')),
        'unbounded runtime stream')


def validate_shape(value):
    expected = {'schema_version', 'identity', 'source_sha', 'source_tree', 'policy_sha', 'policy_tree',
                'source_recipe', 'status', 'limits', 'commands', 'images', 'bindings', 'failure_sidecar', 'focused_exterior', *FLAGS}
    require(isinstance(value, dict) and set(value) == expected, 'unexpected qualification fields')
    require(type(value['schema_version']) is int and value['schema_version'] == 1 and value['identity'] == IDENTITY,
            'wrong qualification identity')
    require(all(capture.hex_string(value[key], 40) for key in ('source_sha', 'source_tree', 'policy_sha', 'policy_tree')),
            'invalid source or policy identity')
    require(value['source_recipe'] == native.SOURCE_RECIPE and value['limits'] == LIMITS
            and all(value[key] is False for key in FLAGS), 'factual observations cannot grant acceptance')
    require(value['status'] in ('failed', 'ordinary_smoke_observed_reviews_required'), 'invalid observation status')
    require(isinstance(value['commands'], list) and len(value['commands']) <= len(SCENES), 'unbounded commands')
    ids = list(SCENES)
    for index, row in enumerate(value['commands']):
        validate_command(row, ids[index])
        if index < len(value['commands']) - 1 or value['status'] != 'failed':
            require(row['outcome'] == 'succeeded' and row['exit_code'] == 0 and row['elapsed_seconds'] <= TIMEOUT,
                    'execution continued after failed command')
    require(set(value['images']) <= {item[2] for item in SCENES.values()}, 'unapproved image name')
    for row in value['images'].values():
        require(set(row) == {'width', 'height', 'sha256'} and capture.hex_string(row['sha256'], 64)
                and type(row['width']) is int and 640 <= row['width'] <= 4096
                and type(row['height']) is int and 360 <= row['height'] <= 4096, 'invalid PNG binding')
    mandatory = {'source', 'policy', 'build_summary', 'archive', 'bundle_manifest', 'native_projection', 'runtime_facts', 'ui_capabilities', 'original_notices'}
    reviews = {'supplemental_assembly', 'dependency_review'}
    allowed = mandatory | reviews
    require((set(value['bindings']) & reviews) in (set(), reviews), 'partial review bindings')
    require(set(value['bindings']) <= allowed and all(capture.valid_record(row) for row in value['bindings'].values()),
            'invalid qualification binding')
    require(value['failure_sidecar'] is None or (value['status'] == 'failed' and bool(value['commands'])
            and capture.valid_record(value['failure_sidecar']) and value['failure_sidecar']['bytes'] <= 2 * device.MAX_PUBLIC + 8192),
            'invalid failure sidecar')
    if value['commands']:
        require(mandatory <= set(value['bindings']), 'runtime lacks original source/build/notice bindings')
    if value['status'] != 'failed':
        require(len(value['commands']) == 2 and mandatory <= set(value['bindings'])
                and set(value['images']) == {item[2] for item in SCENES.values()}, 'incomplete successful observation')
    focused = value['focused_exterior']
    require(isinstance(focused, dict) and set(focused) == {'status', 'command', 'image', 'appearance_accepted'}
            and focused['appearance_accepted'] is False, 'invalid focused observation fields')
    require(focused['status'] in ('not_attempted', 'failed', 'observed_review_required'), 'invalid focused observation status')
    if focused['status'] == 'not_attempted':
        require(focused['command'] is None and focused['image'] is None
                and value['status'] == 'failed', 'successful smoke omitted focused observation')
    else:
        require(len(value['commands']) == 2 and set(value['images']) == {item[2] for item in SCENES.values()},
                'focused observation preceded required smoke cases')
        validate_command(focused['command'], FOCUSED_ID)
        if focused['status'] == 'observed_review_required':
            row = focused['image']
            require(isinstance(row, dict) and set(row) == {'width', 'height', 'sha256'}
                    and capture.hex_string(row['sha256'], 64)
                    and type(row['width']) is int and 640 <= row['width'] <= 4096
                    and type(row['height']) is int and 360 <= row['height'] <= 4096,
                    'invalid focused PNG binding')
        else:
            require(focused['image'] is None, 'failed focused observation claims image')
    require(len(canonical(value)) <= MAX_JSON, 'oversized qualification JSON')


def bindings_paths(private, build_text, assembly=None, review=None):
    result = {'source': private / 'source.json', 'policy': private / 'policy.json',
            'build_summary': build_text / capture.EXPORT_NAME, 'archive': private / ARCHIVE,
            'bundle_manifest': private / 'extracted' / BUNDLE / 'bundle-manifest.json',
            'native_projection': private / 'native-review.json', 'original_notices': private / originals.EXPORT_NAME,
            'runtime_facts': private / 'runtime-facts' / runtime_facts.PROJECTION_NAME,
            'ui_capabilities': private / 'ui-capabilities' / capabilities.EXPORT}
    if assembly is not None:
        result.update(supplemental_assembly=assembly / 'assembly.json', dependency_review=review)
    return result


def public_files(value, private, build_text):
    paths = bindings_paths(private, build_text)
    result = {EXPORT: private / 'result.json', **{name: private / name for name in value['images']}}
    if value['focused_exterior']['image'] is not None:
        result[FOCUSED_SCENE[2]] = private / FOCUSED_SCENE[2]
    for field in ('native_projection', 'runtime_facts', 'ui_capabilities', 'bundle_manifest', 'original_notices'):
        if field in value['bindings']: result[paths[field].name] = paths[field]
    if value['failure_sidecar'] is not None: result['device-failure.json'] = private / 'device-failure.json'
    if 'archive' in value['bindings']: result[ARCHIVE + '.sha256'] = private / (ARCHIVE + '.sha256')
    return result


def validate_export(directory, repo, expected, build_private, build_text, private, *, source_tree, policy_sha, policy_tree,
                    assembly=None, review=None):
    roots(repo, build_private, build_text, private, directory, assembly, review)
    path = directory / EXPORT
    require(record(path)['bytes'] <= MAX_JSON, 'oversized qualification export')
    raw = path.read_bytes(); value = json.loads(raw); validate_shape(value)
    require(raw == canonical(value) == (private / 'result.json').read_bytes(), 'changed noncanonical qualification export')
    require(all(value[key] == expected_value for key, expected_value in
                (('source_sha', expected), ('source_tree', source_tree), ('policy_sha', policy_sha), ('policy_tree', policy_tree))),
            'qualification differs from invocation pins')
    source = capture.source_evidence(repo, expected)
    require(source['source_tree'] == source_tree, 'source tree differs from external pin')
    policy = policy_evidence(source, policy_sha, policy_tree)
    verified = native.validate_build(repo, expected, build_private, build_text)
    require(canonical(source) == (private / 'source.json').read_bytes()
            and canonical(policy) == (private / 'policy.json').read_bytes(), 'source or policy inputs changed')
    allowed = public_files(value, private, build_text)
    require({p.name for p in directory.iterdir()} == set(allowed), 'unexpected public export file')
    for name, original in allowed.items():
        require(record(directory / name) == record(original), 'public file differs from private observation')
    paths = bindings_paths(private, build_text, assembly, review)
    review_keys = {'supplemental_assembly', 'dependency_review'}
    require((set(value['bindings']) & review_keys) == (review_keys if review is not None else set()),
            'review input presence changed')
    require(all(record(paths[key]) == row for key, row in value['bindings'].items()), 'changed qualification input binding')
    bundle = private / 'extracted' / BUNDLE
    if 'archive' in value['bindings']:
        snapshot = capture.read_private_json(private / 'bundle-files.json')
        require(capture.snapshot_tree(private / BUNDLE) == snapshot == capture.snapshot_tree(bundle), 'frozen bundle changed')
        archive_members(private / ARCHIVE, snapshot)
        native.validate_bundle(repo, bundle, verified, original_notices=build_private / 'capture/ordinary/notices')
        if review is not None:
            notices, _ = stage.assembly_api.validate_reviewed_assembly(repo, expected, assembly, review)
            require(record(notices / 'dependency-inventory.json') == verified['builds']['ordinary']['inventory']
                    and record(bundle / stage.release.DEPENDENCY_REVIEW) == record(review),
                    'final bundle review or inventory differs from genuine assembly')
        require((private / (ARCHIVE + '.sha256')).read_bytes() ==
                (value['bindings']['archive']['sha256'] + '  ' + ARCHIVE + '\n').encode('ascii'), 'archive checksum changed')
    if 'runtime_facts' in value['bindings']:
        facts = native.validate_runtime_facts(private / 'runtime-facts', repo, expected, build_private, build_text)
        require(canonical(facts) == paths['runtime_facts'].read_bytes(), 'runtime facts changed')
    if 'ui_capabilities' in value['bindings']:
        caps = capabilities.project(private / 'ui-capabilities', repo, expected)
        require(canonical(caps) == paths['ui_capabilities'].read_bytes(), 'capabilities changed')
    if 'native_projection' in value['bindings']:
        projection = native.project(repo, expected, build_private, build_text, bundle,
                                   runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities')
        require(canonical(projection) == paths['native_projection'].read_bytes(), 'native projection changed')
    if 'original_notices' in value['bindings']:
        originals.verify(paths['original_notices'], repo, expected, build_private, build_text,
                         paths['native_projection'], bundle=bundle,
                         runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities')
    observed = [*value['commands']]
    if value['focused_exterior']['command'] is not None: observed.append(value['focused_exterior']['command'])
    for row in observed:
        base = private / 'commands' / row['id']; journal = capture.read_private_json(base / 'journal.json')
        image_name = ALL_SCENES[row['id']][2]
        require(journal['command'] == command(bundle / 'flightsim-app.exe', private / image_name, row['id'])
                and journal['cwd'] == str(bundle) and journal['timeout_seconds'] == TIMEOUT
                and journal.get('runtime_environment') == WARP, 'runtime invocation changed')
        require(all(row[key] == journal[key] for key in row if key != 'id'), 'runtime journal changed')
        require(all(record(base / stream) == row[stream] for stream in ('stdout', 'stderr')), 'runtime stream changed')
        image = value['focused_exterior']['image'] if row['id'] == FOCUSED_ID else value['images'].get(image_name)
        if image is not None:
            smoke((base / 'stdout').read_text(encoding='utf-8', errors='replace') + '\n'
                  + (base / 'stderr').read_text(encoding='utf-8', errors='replace'), journal, row['id'])
            require(candidate.validate_png(private / image_name) == image, 'runtime PNG changed')
    require(set(value['images']) <= {SCENES[row['id']][2] for row in value['commands']}, 'image lacks actual command')
    if value['failure_sidecar'] is not None:
        projected = sidecar(private, value['commands'][-1], source, policy, record(build_text / capture.EXPORT_NAME))
        require(canonical(projected) == (private / 'device-failure.json').read_bytes()
                and record(private / 'device-failure.json') == value['failure_sidecar'], 'failure sidecar changed')
    return value


def qualify(repo, expected, build_private, build_text, private, evidence, *, source_tree, policy_sha, policy_tree,
            assembly=None, review=None):
    roots(repo, build_private, build_text, private, evidence, assembly, review)
    require(not private.exists() and not evidence.exists(), 'fresh private/export roots required')
    require(sys.platform == 'win32' and platform.machine().lower() in ('amd64', 'x86_64'), 'native Windows x64 required')
    source = capture.source_evidence(repo, expected)
    require(source['source_tree'] == source_tree, 'source tree differs from external pin')
    policy = policy_evidence(source, policy_sha, policy_tree)
    verified = native.validate_build(repo, expected, build_private, build_text)
    private.mkdir(parents=True); (private / 'commands').mkdir()
    write_json(private / 'source.json', source); write_json(private / 'policy.json', policy)
    paths = bindings_paths(private, build_text, assembly, review)
    value = {'schema_version': 1, 'identity': IDENTITY, 'source_sha': expected, 'source_tree': source_tree,
             'policy_sha': policy_sha, 'policy_tree': policy_tree, 'source_recipe': native.SOURCE_RECIPE,
             'status': 'failed', 'limits': LIMITS, 'commands': [], 'images': {}, 'failure_sidecar': None,
             'focused_exterior': {'status': 'not_attempted', 'command': None, 'image': None, 'appearance_accepted': False},
             'bindings': {key: record(paths[key]) for key in ('source', 'policy', 'build_summary')},
             **{key: False for key in FLAGS}}
    if review is not None:
        value['bindings'].update({key: record(paths[key]) for key in ('supplemental_assembly', 'dependency_review')})
    with capture.private_console(private):
        try:
            # Independent facts are prepared before any graphics launch.
            runtime_facts.collect_runtime_facts(private / 'runtime-facts', source_sha=expected,
                recipe_cfg_args=['-D', 'warnings'], linker_trace=build_private / 'capture/ordinary/build.stderr',
                audited_executable=build_private / 'target-ordinary' / native.check.TARGET / 'release/flightsim-app.exe')
            native.validate_runtime_facts(private / 'runtime-facts', repo, expected, build_private, build_text)
            value['bindings']['runtime_facts'] = record(paths['runtime_facts'])
            capabilities.collect(private / 'ui-capabilities', repo, expected)
            value['bindings']['ui_capabilities'] = record(paths['ui_capabilities'])
            stage.stage_captured(repo, expected, build_private, build_text, private / BUNDLE, assembly=assembly, review=review)
            snapshot = freeze(repo, verified, private, build_private); write_json(private / 'bundle-files.json', snapshot)
            for key in ('archive', 'bundle_manifest'): value['bindings'][key] = record(paths[key])
            (private / (ARCHIVE + '.sha256')).write_bytes((value['bindings']['archive']['sha256'] + '  ' + ARCHIVE + '\n').encode('ascii'))
            bundle = private / 'extracted' / BUNDLE
            write_json(paths['native_projection'], native.project(repo, expected, build_private, build_text, bundle,
                runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities'))
            value['bindings']['native_projection'] = record(paths['native_projection'])
            notice_packet = originals.prepare(repo, expected, build_private, build_text, paths['native_projection'],
                bundle=bundle, runtime_facts_private=private / 'runtime-facts', ui_capabilities_private=private / 'ui-capabilities')
            paths['original_notices'].write_bytes(originals.canonical(notice_packet))
            value['bindings']['original_notices'] = record(paths['original_notices'])

            env = {**os.environ, **WARP}
            for name, (_, _, image_name) in SCENES.items():
                base = private / 'commands' / name; base.mkdir()
                actual = capture.execute(command(bundle / 'flightsim-app.exe', private / image_name, name),
                    cwd=bundle, env=env, stdout=base / 'stdout', stderr=base / 'stderr', journal=base / 'journal.json', timeout=TIMEOUT)
                actual['runtime_environment'] = WARP
                write_json(base / 'journal.json', actual)
                row = {'id': name, **{key: actual[key] for key in ('outcome', 'exit_code', 'elapsed_seconds', 'stdout', 'stderr')}}
                value['commands'].append(row)
                require(all(row[key]['bytes'] <= MAX_STREAM for key in ('stdout', 'stderr')), 'runtime stream budget exceeded')
                smoke((base / 'stdout').read_text(encoding='utf-8', errors='replace') + '\n'
                      + (base / 'stderr').read_text(encoding='utf-8', errors='replace'), actual, name)
                value['images'][image_name] = candidate.validate_png(private / image_name)
            # Focused observation of the changed model, separate from the two
            # existing release smoke cases. A scene failure is factual QA output;
            # all source/archive/stream integrity checks remain mandatory.
            focused = value['focused_exterior']
            name, image_name = FOCUSED_ID, FOCUSED_SCENE[2]
            base = private / 'commands' / name; base.mkdir()
            actual = capture.execute(command(bundle / 'flightsim-app.exe', private / image_name, name),
                cwd=bundle, env=env, stdout=base / 'stdout', stderr=base / 'stderr', journal=base / 'journal.json', timeout=TIMEOUT)
            actual['runtime_environment'] = WARP; write_json(base / 'journal.json', actual)
            focused['command'] = {'id': name, **{key: actual[key] for key in ('outcome', 'exit_code', 'elapsed_seconds', 'stdout', 'stderr')}}
            focused['status'] = 'failed'
            require(all(actual[key]['bytes'] <= MAX_STREAM for key in ('stdout', 'stderr')), 'runtime stream budget exceeded')
            try:
                smoke((base / 'stdout').read_text(encoding='utf-8', errors='replace') + '\n'
                      + (base / 'stderr').read_text(encoding='utf-8', errors='replace'), actual, name)
                focused['image'] = candidate.validate_png(private / image_name)
                focused['status'] = 'observed_review_required'
            except (ValueError, OSError):
                pass  # The attempted focused command stays separately bound.
            require(capture.snapshot_tree(private / BUNDLE) == snapshot == capture.snapshot_tree(bundle), 'runtime changed frozen bundle')
            native.validate_bundle(repo, bundle, verified, original_notices=build_private / 'capture/ordinary/notices'); archive_members(private / ARCHIVE, snapshot)
            value['status'] = 'ordinary_smoke_observed_reviews_required'
        except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError, zipfile.BadZipFile, zlib.error) as error:
            write_json(private / 'failure.json', {'type': type(error).__name__, 'message': str(error)})
            if value['commands']:
                try:
                    write_json(private / 'device-failure.json', sidecar(private, value['commands'][-1], source, policy,
                               record(build_text / capture.EXPORT_NAME)))
                    value['failure_sidecar'] = record(private / 'device-failure.json')
                except (ValueError, OSError, KeyError, TypeError): pass  # Unsupported streams stay private.
        write_json(private / 'result.json', value)
    validate_shape(value)
    evidence.mkdir(parents=True)
    for name, path in public_files(value, private, build_text).items(): shutil.copyfile(path, evidence / name)
    validate_export(evidence, repo, expected, build_private, build_text, private,
                    source_tree=source_tree, policy_sha=policy_sha, policy_tree=policy_tree, assembly=assembly, review=review)
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repo', 'build-private', 'build-evidence', 'private', 'evidence'):
        parser.add_argument('--' + name, type=Path, required=True)
    for name in ('source-sha', 'source-tree', 'policy-sha', 'policy-tree'): parser.add_argument('--' + name, required=True)
    parser.add_argument('--assembly', type=Path); parser.add_argument('--review', type=Path)
    parser.add_argument('--validate-only', action='store_true')
    args = parser.parse_args(argv)
    try:
        options = {'source_tree': args.source_tree, 'policy_sha': args.policy_sha, 'policy_tree': args.policy_tree,
                   'assembly': args.assembly, 'review': args.review}
        if args.validate_only:
            value = validate_export(args.evidence, args.repo, args.source_sha, args.build_private, args.build_evidence, args.private, **options)
        else:
            value = qualify(args.repo, args.source_sha, args.build_private, args.build_evidence, args.private, args.evidence, **options)
        print('Full Windows observations: ' + value['status'] + '; release_authorized=false.')
        return 0 if args.validate_only or value['status'] != 'failed' else 1
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError, zipfile.BadZipFile, zlib.error):
        print('Full Windows qualification blocked; raw details remain private.', file=sys.stderr); return 1


if __name__ == '__main__':
    raise SystemExit(main())
