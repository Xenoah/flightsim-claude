#!/usr/bin/env python3
"""Local, fail-closed publication preparation. Never authorizes or publishes.

A consistent result always exits 2 and retains an external-authorization blocker.
Review/choice digests are caller-supplied consistency anchors, NOT credentials or
proof that a user/reviewer actually approved. See the companion specification.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tempfile
import tomllib
import zipfile
import zlib

REPOSITORY = 'Xenoah/flightsim-claude'
VARIANT = 'swift-reinhard-windows-prerelease-v1'
TITLE = 'Swift-only / Reinhard Windows prerelease'
IDENTITY = 'analytical-publication-preparation-v1'
MAX_JSON = 256 * 1024
MAX_ARCHIVE = 2 * 1024**3
MAX_UNPACKED = 4 * 1024**3
MAX_MEMBERS = 10000
INPUTS = {'regressions_private', 'regressions_export', 'runtime_private', 'runtime_export',
          'live_private', 'live_export', 'legacy_private', 'legacy_export',
          'source_review', 'user_choice', 'publication_decision'}
LIMITS = ['swift_only_light_single_and_other_aircraft_excluded', 'darker_reinhard_highlights',
          'coarse_global_terrain', 'monthly_climate', 'offline_region_downloads_disabled',
          'physical_gpu_controller_audio_not_qualified', 'legacy_partial_identity_only',
          'cancellation_during_admitted_preparation_not_proven',
          'not_steam_or_certified_flight_qualification']
REVIEWS = ['day_low_sun_night', 'fog_clouds_water', 'cockpit_hud', 'camera_map_lifecycle',
           'narrow_viewport_legacy_disclosure', 'normal_exit', 'reinhard_tradeoff',
           'whole_dependency_platform_runtime', 'modified_source_and_notice_provenance']
BLOCKER = 'EXTERNAL_USER_AUTHORIZATION_AND_INDEPENDENT_PUBLICATION_PROCESS_REQUIRED'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True, allow_nan=False) + '\n').encode('ascii')


def hex_string(value, length=64):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(length) + '}', value) is not None


def no_links(path):
    require(path.is_absolute() and '..' not in path.parts, 'absolute path without parent segments required')
    for part in (path, *path.parents):
        if part.exists() or part.is_symlink():
            details = part.lstat()
            require(not stat.S_ISLNK(details.st_mode) and not getattr(details, 'st_file_attributes', 0) & 0x400,
                    'linked or reparse input forbidden')


def record(path, maximum=None):
    no_links(path)
    before = path.stat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1, 'single-link regular file required')
    require(maximum is None or before.st_size <= maximum, 'file exceeds budget')
    stable = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns, value.st_nlink, value.st_mode)
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        require(stable(os.fstat(stream.fileno())) == stable(before), 'file changed before reading')
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
        require(stable(os.fstat(stream.fileno())) == stable(before), 'file changed while reading')
    require(stable(path.stat()) == stable(before), 'file replaced while reading')
    return {'sha256': digest.hexdigest(), 'bytes': before.st_size}


def unique_object(rows):
    result = {}
    for key, value in rows:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def read_json(path, expected_sha256=None):
    before = record(path, MAX_JSON)
    if expected_sha256 is not None:
        require(hex_string(expected_sha256) and before['sha256'] == expected_sha256, 'independently pinned record changed')
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == before['sha256'], 'JSON changed while reading')
    value = json.loads(raw.decode('ascii'), object_pairs_hook=unique_object,
                       parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite JSON')))
    require(isinstance(value, dict) and raw == encoded(value), 'canonical JSON object required')
    return value


def fields(value, expected):
    require(isinstance(value, dict) and set(value) == set(expected), 'unexpected record fields')


def relative(value):
    require(isinstance(value, str) and 0 < len(value) <= 512 and '\\' not in value and ':' not in value,
            'unsafe member path')
    parts = value.split('/')
    require(all(part and part not in ('.', '..') and part == part.rstrip(' .')
                and not any(ord(c) < 32 or ord(c) == 127 for c in part)
                and not re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', part)
                for part in parts), 'unsafe Windows member path')
    return value


def snapshot(root):
    no_links(root)
    require(root.is_dir(), 'missing frozen tree')
    result = {}
    for path in sorted(root.rglob('*')):
        no_links(path)
        require(path.is_dir() or path.is_file(), 'nonregular frozen member')
        if path.is_file():
            name = relative(path.relative_to(root).as_posix())
            result[name] = record(path)
            require(len(result) <= MAX_MEMBERS, 'too many frozen members')
    require(result and len({name.casefold() for name in result}) == len(result), 'empty or case-colliding frozen tree')
    require(sum(item['bytes'] for item in result.values()) <= MAX_UNPACKED, 'frozen byte budget exceeded')
    return result


def tree_record(files):
    return {'sha256': hashlib.sha256(encoded(files)).hexdigest(), 'files': len(files),
            'bytes': sum(item['bytes'] for item in files.values())}


def audit_archive(archive, files):
    """Compare each compressed member's actual bytes to the final extracted copy.

    Never extract, execute or rewrite. Names, duplicates, links and bombs fail
    before decompression; streaming caps remain enforced during decompression.
    """
    before = record(archive, MAX_ARCHIVE)
    expected = {'swift-candidate/' + relative(name): item for name, item in files.items()}
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
    require(record(archive, MAX_ARCHIVE) == before, 'archive changed during verification')
    return before


def timestamp(value):
    require(isinstance(value, str) and re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ', value), 'UTC decision date required')
    date = datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    require(date <= datetime.now(timezone.utc), 'future decision date')
    return date


def person(value):
    require(isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.@+-]{1,127}', value), 'bounded reviewer identity required')
    return value


def release_identity(version, revision, source_sha, source_tree):
    require(isinstance(version, str) and len(version) <= 80
            and re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?', version),
            'unsupported package version (build metadata requires separate review)')
    require(type(revision) is int and 1 <= revision <= 9999, 'bounded positive variant revision required')
    require(hex_string(source_sha, 40) and hex_string(source_tree, 40), 'full source identity required')
    tag = 'v' + version + '-swift-reinhard.' + str(revision)
    return {'repository': REPOSITORY, 'variant': VARIANT, 'source_sha': source_sha, 'source_tree': source_tree,
            'package_version': version, 'variant_revision': revision, 'release_tag': tag, 'title': TITLE,
            'prerelease': True, 'assets': ['flightsim-' + tag + '-windows-msvc.zip', 'SHA256SUMS.txt', 'release-variant.json']}


def source_review(value, identity, workflow):
    fields(value, {'schema_version', 'identity', 'status', 'source_sha', 'source_tree', 'reviewed_by', 'reviewed_at',
                   'source_ci', 'release_authorized'})
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['identity'] == 'analytical-final-source-review-v1' and value['status'] == 'reviewed'
            and value['release_authorized'] is False, 'not a separate source review')
    require(all(value[key] == identity[key] for key in ('source_sha', 'source_tree')), 'source review identity moved')
    person(value['reviewed_by']); timestamp(value['reviewed_at'])
    ci = value['source_ci']
    fields(ci, {'repository', 'workflow_path', 'workflow', 'head_sha', 'run_id', 'run_attempt', 'status', 'conclusion'})
    require(ci['repository'] == REPOSITORY and ci['workflow_path'] == '.github/workflows/ci.yml'
            and ci['workflow'] == workflow and ci['head_sha'] == identity['source_sha']
            and ci['status'] == 'completed' and ci['conclusion'] == 'success'
            and all(type(ci[key]) is int and ci[key] > 0 for key in ('run_id', 'run_attempt')), 'wrong or incomplete exact-source CI')


def user_choice(value, identity):
    fields(value, {'schema_version', 'identity', 'release', 'choice', 'publication_requested', 'recorded_by',
                   'recorded_at', 'user_message_reference', 'accepted_limitations', 'release_authorized'})
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['identity'] == 'analytical-explicit-user-choice-v1'
            and encoded(value['release']) == encoded(identity)
            and value['choice'] == 'publish_swift_only_reinhard_prerelease'
            and value['publication_requested'] is True and value['release_authorized'] is False,
            'explicit exact reduced-variant publication choice required')
    require(value['accepted_limitations'] == LIMITS, 'choice omits or changes a variant limitation')
    person(value['recorded_by']); timestamp(value['recorded_at'])
    ref = value['user_message_reference']
    require(isinstance(ref, str) and 1 <= len(ref) <= 256 and ref.strip() == ref
            and not any(ord(c) < 32 for c in ref), 'actual private user-message reference required')


def publication_decision(value, identity, bindings, source, choice, gates):
    fields(value, {'schema_version', 'identity', 'status', 'release', 'bindings', 'reviewed_by', 'reviewed_at',
                   'acceptance_conditions', 'substantive_reviews', 'accepted_limitations', 'release_authorized'})
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['identity'] == 'analytical-independent-publication-review-v1'
            and value['status'] == 'reviewed_recommendation_external_authorization_required'
            and value['release_authorized'] is False, 'a self-authorizing receipt is forbidden')
    require(encoded(value['release']) == encoded(identity) and encoded(value['bindings']) == encoded(bindings),
            'publication review does not bind every exact final input')
    reviewer = person(value['reviewed_by'])
    require(reviewer.casefold() not in (source['reviewed_by'].casefold(), choice['recorded_by'].casefold()),
            'publication review must be independent')
    date = timestamp(value['reviewed_at'])
    require(date >= timestamp(source['reviewed_at']) and date >= timestamp(choice['recorded_at']), 'decision predates source review or choice')
    require(value['acceptance_conditions'] == {key: ('external_authorization_required' if key == 'publication_receipt' else 'reviewed') for key in gates}
            and value['substantive_reviews'] == {key: 'accepted_with_declared_limits' for key in REVIEWS}
            and value['accepted_limitations'] == LIMITS, 'required substantive condition or limitation missing')


def qualification_module():
    spec = importlib.util.spec_from_file_location('publication_qualification', Path(__file__).with_name('qualify-analytical-swift-windows.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def platform_projection(q, repo, expected, private, build_paths, bundle):
    # The factual collector keeps coverage/approval false and unknowns explicit.
    # Revalidation binds original private origins and the exact audited build.
    q.validate_runtime_facts(private, repo, expected, build_paths['build_private'], build_paths['build_text'])
    return q.native.project(repo, expected, build_paths['build_private'], build_paths['build_text'], bundle,
                            runtime_facts_private=private / 'runtime-facts',
                            ui_capabilities_private=private / 'ui-capabilities')


def collect(repo, expected, expected_tree, paths, revision):
    """Consume the unchanged native validators, never passing their status through as authorization."""
    q = qualification_module()
    source = q.capture.source_evidence(repo, expected)
    require(source['source_tree'] == expected_tree, 'final source tree moved')
    regression = q.validate_export(paths['regressions_export'], repo, expected, paths['regressions_private'])
    require(regression['phase'] == 'regressions' and regression['status'] == 'engineering_evidence_complete_reviews_required',
            'complete native regression phase required')
    final = q.load('publication_reviewed_bundle', 'recheck-reviewed-analytical-bundle.py')
    runtime = final.verify_export(paths['runtime_export'], repo, expected, paths['runtime_private'])
    require(runtime['status'] == 'final_bundle_runtime_observed_review_required', 'complete final bundle runtime required')
    ui = q.load('publication_ui', 'observe-analytical-windows-ui.py')
    for scenario in ('live', 'legacy'):
        value = ui.verify_export(paths[scenario + '_export'], paths[scenario + '_private'], repo, expected,
                                 paths['runtime_private'], paths['runtime_export'])
        require(value['scenario'] == scenario and value['status'] == 'observable_sequence_recorded_review_required',
                'complete same-bundle live and legacy observations required')
    runtime_paths, notices, _ = final.private_inputs(repo, expected, paths['runtime_private'])
    bundle = paths['runtime_private'] / 'extracted/swift-candidate'
    files, notice_files = snapshot(bundle), snapshot(notices)
    archive = paths['runtime_private'] / 'swift-candidate.zip'
    archive_record = audit_archive(archive, files)
    distribution = q.capture.read_private_json(bundle / 'distribution-info.json')
    version = tomllib.loads((repo / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
    require(distribution['package_version'] == version, 'binary handshake differs from package version')
    identity = release_identity(version, revision, expected, expected_tree)
    projected = platform_projection(q, repo, expected, paths['runtime_private'], runtime_paths, bundle)
    require((paths['runtime_private'] / 'native-review.json').read_bytes() == encoded(projected), 'native projection changed')
    bound_paths = {'cargo_lock': repo / 'Cargo.lock', 'source_workflow': repo / '.github/workflows/ci.yml',
                   'regressions': paths['regressions_export'] / q.EXPORT,
                   'original_build': runtime_paths['build_text'] / q.capture.EXPORT_NAME,
                   'assembly': runtime_paths['assembly'] / 'assembly.json',
                   'dependency_review': runtime_paths['review'],
                   'final_runtime': paths['runtime_export'] / final.EXPORT,
                   'live_ui': paths['live_export'] / 'ui-observation.json',
                   'legacy_ui': paths['legacy_export'] / 'ui-observation.json',
                   'native_projection': paths['runtime_private'] / 'native-review.json',
                   'runtime_facts': paths['runtime_private'] / 'runtime-facts' / q.runtime_facts.PROJECTION_NAME,
                   'ui_capabilities': paths['runtime_private'] / 'ui-capabilities' / q.ui_capabilities.EXPORT,
                   'archive': archive, 'executable': bundle / 'flightsim-app.exe',
                   'inventory': bundle / 'third-party/dependency-inventory.json',
                   'bundle_manifest': bundle / 'bundle-manifest.json',
                   'source_review': paths['source_review'], 'user_choice': paths['user_choice']}
    bindings = {name: record(path) for name, path in bound_paths.items()}
    require(bindings['archive'] == archive_record, 'archive changed after comparison')
    bindings.update(bundle_tree=tree_record(files), notices_tree=tree_record(notice_files))
    # Raw image/OCR/log content is private; bind it all as hashes to prevent a
    # visually reviewed PNG being replaced independently of a JSON summary.
    for name in ('runtime_export', 'live_export', 'legacy_export'):
        bindings[name + '_tree'] = tree_record(snapshot(paths[name]))
    return identity, bindings, {'q': q, 'paths': bound_paths, 'source': source, 'bundle': bundle, 'notices': notices,
                                'files': files, 'notice_files': notice_files, 'gates': list(q.check.GATES)}


def manifest(identity, bindings, decision_record):
    # Closed construction: no reviewer names, message references, private paths,
    # freeform decisions, raw logs, OCR or arbitrary input text can escape.
    result = {'schema_version': 1, 'identity': IDENTITY, 'kind': 'prepared_variant_metadata_not_authorization',
              'release': identity, 'release_authorized': False, 'publication_blocked': True,
              'remaining_blocker': BLOCKER,
              'build': {'target': 'x86_64-pc-windows-msvc', 'toolchain': '1.93.0', 'profile': 'release', 'jobs': 2,
                        'locked': True, 'offline': True, 'default_features': False,
                        'features': ['analytic-tonemapping', 'commercial-staging'], 'region_downloads': False},
              'limitations': LIMITS, 'frozen_inputs': bindings, 'publication_review': decision_record,
              'staging_markers': 'Unchanged commercial-staging and LOCAL-CANDIDATE markers describe the build route; they grant no publication permission.'}
    require(len(encoded(result)) <= 32 * 1024, 'public metadata budget exceeded')
    return result


def result_report(status, prepared=False):
    return {'schema_version': 1, 'identity': IDENTITY, 'status': status, 'release_authorized': False,
            'publication_blocked': True, 'public_text_prepared': prepared, 'remaining_blocker': BLOCKER}


def prepare_text(output, value):
    no_links(output)
    require(not output.exists(), 'fresh public-text destination required')
    require(output.parent.is_dir(), 'public-text parent must already exist')
    raw = encoded(value)
    checksums = (value['frozen_inputs']['archive']['sha256'] + '  ' + value['release']['assets'][0] + '\n'
                 + hashlib.sha256(raw).hexdigest() + '  release-variant.json\n').encode('ascii')
    # Stage text privately, then exclusively reserve the destination. Directory
    # rename can replace a concurrently created empty directory on POSIX, so
    # use mkdir and exclusive file creation instead. An I/O failure is blocked;
    # any partial text is never a successful result or authorization.
    with tempfile.TemporaryDirectory(prefix='.analytical-public-text-', dir=output.parent) as temporary:
        temporary = Path(temporary)
        (temporary / 'release-variant.json').write_bytes(raw)
        (temporary / 'SHA256SUMS.txt').write_bytes(checksums)
        require({path.name for path in temporary.iterdir()} == {'release-variant.json', 'SHA256SUMS.txt'}, 'unexpected public output')
        output.mkdir()  # Exclusive even if another process raced the precheck.
        for name in ('release-variant.json', 'SHA256SUMS.txt'):
            with (output / name).open('xb') as stream:
                stream.write((temporary / name).read_bytes())
        require((output / 'release-variant.json').read_bytes() == raw
                and (output / 'SHA256SUMS.txt').read_bytes() == checksums, 'public text changed while writing')


def check_local(repo, expected, expected_tree, inputs, revision, pins, output=None):
    no_links(repo); no_links(inputs)
    require(hex_string(expected, 40) and hex_string(expected_tree, 40), 'full immutable source identity required')
    paths = read_json(inputs); fields(paths, INPUTS)
    paths = {name: Path(path) for name, path in paths.items()}
    for path in paths.values(): no_links(path)
    fields(pins, {'source_review', 'user_choice', 'publication_decision'})
    # Missing choice/pinned reviews fail before expensive native validation or output.
    records = {name: read_json(paths[name], pins[name]) for name in pins}
    identity, bindings, state = collect(repo, expected, expected_tree, paths, revision)
    require(all(record(paths[name])['sha256'] == digest for name, digest in pins.items()), 'pinned reviewer or choice record changed during collection')
    source_review(records['source_review'], identity, bindings['source_workflow'])
    user_choice(records['user_choice'], identity)
    publication_decision(records['publication_decision'], identity, bindings,
                         records['source_review'], records['user_choice'], state['gates'])
    decision_record = record(paths['publication_decision'])
    value = manifest(identity, bindings, decision_record)
    # Recheck frozen identities immediately before writing bounded text. The
    # future publisher must independently repeat at its own action boundary.
    require(all(record(path) == bindings[name] for name, path in state['paths'].items()), 'bound bytes changed during review')
    require(record(paths['publication_decision']) == decision_record, 'publication recommendation moved')
    require(snapshot(state['bundle']) == state['files'] and snapshot(state['notices']) == state['notice_files'], 'frozen payload changed')
    for name in ('runtime_export', 'live_export', 'legacy_export'):
        require(tree_record(snapshot(paths[name])) == bindings[name + '_tree'], 'reviewed image export changed')
    require(state['q'].capture.source_evidence(repo, expected) == state['source'], 'final source moved during check')
    if output is not None:
        no_links(output)
        require(not output.is_relative_to(repo) and not repo.is_relative_to(output), 'output must be separate from source')
        for path in (inputs, *paths.values()):
            require(not output.is_relative_to(path) and not path.is_relative_to(output), 'output overlaps private evidence')
        prepare_text(output, value)
    return result_report('consistent_external_authorization_required', output is not None)


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError('invalid command-line arguments')


def main():
    parser = PrivateArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source-sha', required=True); parser.add_argument('--source-tree', required=True)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--variant-revision', type=int, required=True)
    for name in ('source-review', 'user-choice', 'publication-decision'):
        parser.add_argument('--' + name + '-sha256', required=True)
    parser.add_argument('--prepare-public-text', type=Path)
    try:
        args = parser.parse_args()
        no_links(args.repo)
        require(args.repo.resolve() == Path(__file__).resolve().parents[1], 'run the checker from the exact reviewed source checkout')
        pins = {name: getattr(args, name + '_sha256') for name in ('source_review', 'user_choice', 'publication_decision')}
        report = check_local(args.repo, args.source_sha, args.source_tree, args.inputs, args.variant_revision, pins, args.prepare_public_text)
        print(encoded(report).decode('ascii'), end='')
        return 2  # Deliberately never a publication-success exit code.
    except (ValueError, OSError, KeyError, TypeError, AttributeError, subprocess.SubprocessError, zipfile.BadZipFile, zlib.error, RuntimeError):
        # Never echo private paths, user messages or raw evidence in a public log.
        print(encoded(result_report('blocked_missing_invalid_or_changed_evidence')).decode('ascii'), end='')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
