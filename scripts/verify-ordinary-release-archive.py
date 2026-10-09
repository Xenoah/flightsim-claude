#!/usr/bin/env python3
"""Strict ordinary archive and two-scene private software-D3D12 observations.

The caller owns the unchanged ordinary source gate and authorized copy plan.
These helpers only bind bytes and observations; they confer no runtime, review
or publication approval. Validation never extracts, launches, or rewrites files.
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
import stat
import struct
import sys
import zipfile
import zlib


SPEC = importlib.util.spec_from_file_location(
    'ordinary_archive_payload', Path(__file__).with_name('project-ordinary-release-payload.py'))
adapter = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(adapter)
capture, require = adapter.capture, adapter.require
candidate = capture.check.candidate
MAX_ARCHIVE = 2 * 1024 ** 3
MAX_UNPACKED = MAX_ARCHIVE
MAX_MEMBERS = 2048
MAX_STREAM = 64 * 1024 ** 2
TIMEOUT = 180
WARP = {'WGPU_BACKEND': 'dx12', 'WGPU_FORCE_FALLBACK_ADAPTER': '1'}
ASSET_ENVIRONMENT_POLICY = {'unset': ['BEVY_ASSET_ROOT', 'CARGO_MANIFEST_DIR'],
                            'name_matching': 'case_insensitive'}
SCENES = {'light-single-cockpit': ('light-single', 'cockpit', 'flightsim-windows-smoke.png'),
          'swift-sport-chase': ('swift-sport', 'chase', 'flightsim-windows-smoke-swift.png')}
EXPORT = 'smoke.json'
STATUS = 'two_smokes_observed_software_d3d12_only'
DIAGNOSTIC_MARKER = ('component-terms: internal-release-smoke diagnostic-only; no assent read or written; '
                     'interactive input disabled; max_main_updates=10800; max_seconds=180')
VERSION = r'[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?'
relative = adapter.member_name


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + '\n').encode('ascii')


def record(path, maximum=None):
    adapter.independent_file(path)
    if maximum is not None:
        require(path.stat().st_size <= maximum, 'private file exceeds byte budget')
    return capture.file_record(path)


def archive_top_level(version):
    require(isinstance(version, str) and re.fullmatch(VERSION, version), 'invalid authorized version')
    return validate_top_level('flightsim-claude-v' + version + '-windows-x86_64')


def validate_top_level(value):
    relative(value)
    require(re.fullmatch('flightsim-claude-v' + VERSION + '-windows-x86_64', value),
            'unexpected ordinary archive top-level directory')
    return value


def validate_files(files):
    require(isinstance(files, dict) and 0 < len(files) <= MAX_MEMBERS, 'invalid frozen member set')
    names, directories = set(), set()
    for name, binding in files.items():
        relative(name)
        require(capture.valid_record(binding) and binding['bytes'] <= MAX_UNPACKED,
                'invalid frozen file binding')
        require(name.casefold() not in names, 'case-colliding archive member')
        names.add(name.casefold())
        directories.update(parent.as_posix().casefold() for parent in Path(name).parents if parent != Path('.'))
    require(not names.intersection(directories), 'archive file/directory collision')
    # Directory spellings must also be unique on the Windows target.
    spellings = {}
    for name in files:
        for parent in Path(name).parents:
            if parent == Path('.'): continue
            key = parent.as_posix().casefold()
            require(key not in spellings or spellings[key] == parent.as_posix(),
                    'case-colliding archive directory')
            spellings[key] = parent.as_posix()
    require(sum(item['bytes'] for item in files.values()) <= MAX_UNPACKED, 'frozen bundle exceeds byte budget')


def audit_archive(archive, files, top_level):
    """Compare each compressed member's actual bytes to the final extracted copy.

    Never extract, execute or rewrite. Names, duplicates, links and bombs fail
    before decompression; streaming caps remain enforced during decompression.
    """
    archive = adapter.absolute(archive)
    validate_top_level(top_level)
    validate_files(files)
    before = record(archive, MAX_ARCHIVE)
    require(before['bytes'] <= MAX_ARCHIVE, 'archive exceeds budget')
    expected = {top_level + '/' + relative(name): item for name, item in files.items()}
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
                    and row.volume == 0 and not row.internal_attr
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
                    'archived bytes differ from frozen ordinary bundle')
        require(cursor == zipped.start_dir, 'hidden bytes before archive directory')
        central_size = sum(46 + len(row.filename.encode('utf-8' if row.flag_bits & 0x800 else 'cp437')) for row in rows)
        require(cursor + central_size + 22 == before['bytes'], 'archive has trailing or unaccounted bytes')
        zipped.fp.seek(cursor + central_size)
        end = zipped.fp.read(22)
        require(len(end) == 22 and struct.unpack('<4s4H2IH', end) ==
                (b'PK\x05\x06', 0, 0, len(rows), len(rows), central_size, cursor, 0), 'unexpected archive end record')
    require(record(archive) == before, 'archive changed during verification')
    return before


def create_archive(bundle, archive, top_level):
    """Freeze exact staged files into a new, strictly accounted ordinary ZIP."""
    bundle, archive = map(adapter.absolute, (bundle, archive))
    validate_top_level(top_level)
    capture.disjoint(bundle, archive)
    require(archive.name == top_level + '.zip', 'ordinary archive filename differs from top-level')
    require(archive.parent.is_dir() and not archive.exists(), 'fresh archive path with existing parent required')
    files = adapter.snapshot_payload(bundle)
    validate_files(files)
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED, allowZip64=False) as zipped:
        for name in sorted(files):
            zipped.write(bundle / name, top_level + '/' + name)
    require(adapter.snapshot_payload(bundle) == files, 'staged bytes changed during archive creation')
    audit_archive(archive, files, top_level)
    return files


def extract_verified_archive(archive, destination, files, top_level):
    """Audit first, extract once to a new root, then recheck every exact byte."""
    archive, destination = map(adapter.absolute, (archive, destination))
    capture.disjoint(archive, destination)
    binding = audit_archive(archive, files, top_level)
    require(not destination.exists() and destination.parent.is_dir(), 'fresh extraction root required')
    destination.mkdir()
    bundle = destination / top_level
    with zipfile.ZipFile(archive) as zipped:
        # Extract only already admitted names, retaining byte budgets even if
        # an external writer changes the ZIP between audit and this read.
        for name, wanted in sorted(files.items()):
            output = bundle / name
            output.parent.mkdir(parents=True, exist_ok=True)
            count, digest = 0, hashlib.sha256()
            with zipped.open(top_level + '/' + name) as source, output.open('xb') as target:
                while True:
                    chunk = source.read(min(1024 * 1024, wanted['bytes'] - count + 1))
                    if not chunk: break
                    count += len(chunk)
                    require(count <= wanted['bytes'], 'extraction exceeded frozen member length')
                    digest.update(chunk); target.write(chunk)
            require(count == wanted['bytes'] and digest.hexdigest() == wanted['sha256'],
                    'extracted member differs from frozen payload')
    require(adapter.snapshot_payload(destination) == {top_level + '/' + name: row for name, row in files.items()},
            'extraction differs from exact staged member set')
    require(adapter.snapshot_payload(bundle) == files, 'extracted bytes differ from staged payload')
    require(audit_archive(archive, files, top_level) == binding, 'archive changed during extraction')
    return bundle


def command(bundle, private, name):
    require(name in SCENES, 'unknown ordinary smoke scene')
    # A closed diagnostic entry, not consent or a general interactive bypass.
    # The app owns the exact scene settings, input exclusion and bounded exit.
    return [str(bundle / 'flightsim-app.exe'), '--internal-release-smoke', name,
            str(private / SCENES[name][2])]


def smoke(log, actual, name):
    require(actual['outcome'] == 'succeeded' and type(actual['exit_code']) is int
            and actual['exit_code'] == 0, 'screenshot did not exit zero')
    elapsed = actual['elapsed_seconds']
    require(type(elapsed) in (int, float) and math.isfinite(elapsed) and 0 <= elapsed <= TIMEOUT,
            'screenshot exceeded 180 seconds')
    plain = candidate.ANSI.sub('', log)
    for proof in (DIAGNOSTIC_MARKER, 'aircraft model fitted:', '(' + SCENES[name][0] + ')', 'Screenshot saved to',
                  'Batch capture complete: status 0'):
        require(proof in plain, 'missing ordinary release smoke proof')
    require(not re.search(r'(?m)(^|\s)ERROR(\s|:|$)|(?im:thread .+ panicked at|panic(?:ked)? at|Failed to load asset|unregistered type)', plain),
            'ordinary release smoke logged a fatal error')


def observe_scene(bundle, private, name):
    base = private / 'commands' / name
    actual, journal_binding = adapter.read_object(base / 'journal.json')
    require(set(actual) == {'command', 'cwd', 'exit_code', 'outcome', 'timeout_seconds',
                           'elapsed_seconds', 'stdout', 'stderr', 'runtime_environment', 'asset_environment_policy'},
            'unexpected runtime journal contract')
    require(actual['command'] == command(bundle, private, name) and actual['cwd'] == str(bundle)
            and type(actual['timeout_seconds']) is int and actual['timeout_seconds'] == TIMEOUT
            and actual['runtime_environment'] == WARP
            and actual['asset_environment_policy'] == ASSET_ENVIRONMENT_POLICY, 'runtime invocation changed')
    for stream in ('stdout', 'stderr'):
        require(capture.valid_record(actual[stream]) and record(base / stream, MAX_STREAM) == actual[stream],
                'runtime stream changed')
    smoke((base / 'stdout').read_text(encoding='utf-8', errors='replace') + '\n'
          + (base / 'stderr').read_text(encoding='utf-8', errors='replace'), actual, name)
    image = private / SCENES[name][2]
    image_binding = record(image, candidate.MAX_EVIDENCE_BYTES)
    decoded = candidate.validate_png(image)
    require(decoded['sha256'] == image_binding['sha256'] and record(image) == image_binding,
            'screenshot changed during decoding')
    require(record(base / 'journal.json') == journal_binding
            and all(record(base / stream, MAX_STREAM) == actual[stream] for stream in ('stdout', 'stderr')),
            'runtime originals changed during verification')
    return {'execution': actual, 'journal': journal_binding, 'screenshot': {**image_binding, **decoded}}


def smoke_document(bundle, private, files, scenes):
    return {'schema_version': 1, 'kind': 'ordinary_release_smoke_observations', 'status': STATUS,
            'bundle': str(bundle), 'private': str(private), 'bundle_files': files,
            'environment': WARP, 'asset_environment_policy': ASSET_ENVIRONMENT_POLICY, 'scenes': scenes,
            'recipient_assent_collected': False, 'component_terms_dialog_tested': False}


def validate_smoke(bundle, private):
    """Revalidate existing observations against the exact original private files.

    This is intentionally read-only, cross-platform and has no capture fallback.
    The caller separately rechecks source authorization and the source archive.
    """
    bundle, private = map(adapter.absolute, (bundle, private))
    capture.disjoint(bundle, private)
    before = adapter.snapshot_payload(private)
    expected_names = {EXPORT, *(row[2] for row in SCENES.values()),
                      *('commands/' + name + '/' + filename for name in SCENES
                        for filename in ('stdout', 'stderr', 'journal.json'))}
    require(set(before) == expected_names, 'unexpected or missing private smoke member')
    value, binding = adapter.read_object(private / EXPORT)
    files = adapter.snapshot_payload(bundle)
    validate_files(files)
    require('flightsim-app.exe' in files, 'ordinary executable missing')
    scenes = {name: observe_scene(bundle, private, name) for name in SCENES}
    expected = smoke_document(bundle, private, files, scenes)
    require(canonical(value) == canonical(expected) and (private / EXPORT).read_bytes() == canonical(expected),
            'private smoke observation binding changed')
    require(record(private / EXPORT) == binding and adapter.snapshot_payload(private) == before
            and adapter.snapshot_payload(bundle) == files, 'smoke originals changed during validation')
    return value


def smoke_environment(environ):
    # Ordinary assets_directory() considers these roots before current_exe().
    # Never let an inherited checkout/model root replace the extracted assets.
    # Windows environment names are case-insensitive; normalize WARP keys too.
    controlled = {*ASSET_ENVIRONMENT_POLICY['unset'], *WARP}
    env = {name: value for name, value in environ.items() if name.upper() not in controlled}
    env.update(WARP)
    return env


def run_smoke(bundle, private):
    """Run only the two existing ordinary selections with the 180-second watchdog."""
    bundle, private = map(adapter.absolute, (bundle, private))
    capture.disjoint(bundle, private)
    require(sys.platform == 'win32' and platform.machine().lower() in ('amd64', 'x86_64'),
            'native Windows x64 required')
    require(not private.exists() and private.parent.is_dir(), 'fresh private smoke root required')
    files = adapter.snapshot_payload(bundle)
    validate_files(files)
    require('flightsim-app.exe' in files, 'ordinary executable missing')
    private.mkdir(); (private / 'commands').mkdir()
    env = smoke_environment(os.environ)
    scenes = {}
    for name in SCENES:
        base = private / 'commands' / name; base.mkdir()
        actual = capture.execute(command(bundle, private, name), cwd=bundle, env=env,
                                 stdout=base / 'stdout', stderr=base / 'stderr',
                                 journal=base / 'journal.json', timeout=TIMEOUT)
        actual['runtime_environment'] = dict(WARP)
        actual['asset_environment_policy'] = dict(ASSET_ENVIRONMENT_POLICY)
        capture.write_json(base / 'journal.json', actual)
        # Failures retain the exact original journal and streams, never a success document.
        scenes[name] = observe_scene(bundle, private, name)
        require(adapter.snapshot_payload(bundle) == files, 'runtime changed frozen ordinary bundle')
    value = smoke_document(bundle, private, files, scenes)
    (private / EXPORT).write_bytes(canonical(value))
    return validate_smoke(bundle, private)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', required=True, type=Path)
    parser.add_argument('--private', required=True, type=Path)
    # Standalone CLI deliberately offers no execute/build/publication switch.
    parser.add_argument('--validate-only', action='store_true', required=True)
    args = parser.parse_args(argv)
    try:
        validate_smoke(args.bundle, args.private)
    except (ValueError, OSError, KeyError, TypeError, UnicodeError, zipfile.BadZipFile, zlib.error):
        print('ordinary smoke original-file validation failed', file=sys.stderr)
        return 2
    print('ordinary smoke original-file bindings checked; software D3D12 observations only')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
