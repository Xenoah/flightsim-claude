#!/usr/bin/env python3
"""Export exact original ordinary notice bytes in a closed, text-only packet.

The collector inventory is never rebuilt or sanitized and called original.
Every notice destination/content pair is pinned to public package source
provenance or admitted application source. Independent source/build/native validation is still
required. This exports facts, never grants, reviews, terms, or runtime acceptance.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import stat
import subprocess
import sys


SPEC = importlib.util.spec_from_file_location(
    'original_notices_native', Path(__file__).with_name('project-full-native-evidence.py'))
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)
capture, require = native.capture, native.require
staging = native.release.staging
EXPORT_NAME = 'original-notices.json'
POLICY_NAME = 'full-original-notice-content.json'
POLICY_SHA256 = 'b89444af3df2d35223408eb88cb6a242c77dfb42c079e54470e24f8a67238855'
IDENTITY = 'full-original-ordinary-notice-packet-v1'
MAX_PACKET_BYTES = 16 * 1024 * 1024
MAX_TOTAL_BYTES = 10 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
MAX_FILES = 2048
FLAGS = ('release_authorized', 'dependency_review_approved', 'whole_target_review_complete',
         'runtime_accepted', 'appearance_accepted', 'distribution_qualified', 'terms_added')
BINDINGS = {'source', 'build_summary', 'native_projection', 'inventory', 'metadata', 'lock', 'content_policy'}
SCOPE = ('Conservative normal/build dependency closure of target-filtered Cargo metadata; excludes dev-only '
         'edges, not proof of final linkage or exhaustive nested-source licensing')
README = (
    'Exact upstream notice bytes and declared license expressions, collected for review.\n'
    'This is not a license grant, legal approval, or a complete linked-binary SBOM.\n'
    'AND obligations, OR alternatives, embedded assets, source headers and platform runtimes\n'
    'need review. Missing notices remain unresolved in dependency-inventory.json.\n').encode('utf-8')
PRIVATE_PATH = re.compile(
    r'(?i)(?:file:/|(?<![A-Za-z0-9])[A-Z]:[\\/]|\\\\[A-Za-z0-9]|'
    r'(?<![A-Za-z0-9:])/(?:Users|home|workspace|tmp|private|root|mnt)/)')


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + '\n').encode('ascii')


def bytes_record(data):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def strict_json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate JSON key')
            result[key] = value
        return result
    def invalid_constant(_):
        raise ValueError('nonfinite JSON value')
    return json.loads(data.decode('utf-8'), object_pairs_hook=pairs, parse_constant=invalid_constant)


def local_path(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'canonical absolute path required')
    capture.no_links(path)
    require(path == path.resolve(), 'aliased path forbidden')
    return path


def relative(name):
    native.relative(name)
    for part in name.split('/'):
        require(not part.endswith(('.', ' ')) and part.split('.')[0].upper() not in
                {'CON', 'PRN', 'AUX', 'NUL', *(f'COM{i}' for i in range(1, 10)),
                 *(f'LPT{i}' for i in range(1, 10))}, 'nonportable notice path')
    return name


def read_bytes(path, maximum):
    path = local_path(path)
    details = path.stat()
    require(stat.S_ISREG(details.st_mode) and details.st_nlink == 1
            and 0 < details.st_size <= maximum, 'independent bounded regular file required')
    with path.open('rb') as stream:
        data = stream.read(maximum + 1)
    require(len(data) == details.st_size and len(data) <= maximum, 'file changed or exceeds budget')
    return data


def text_bytes(data):
    require(0 < len(data) <= MAX_TEXT_BYTES, 'notice text exceeds budget')
    value = data.decode('utf-8')
    require(not any(ord(char) < 32 and char not in '\t\r\n' or 127 <= ord(char) <= 159
                    for char in value), 'binary/control bytes are forbidden')
    require(not PRIVATE_PATH.search(value), 'private absolute path is forbidden')
    return value


def content_policy(repo):
    raw = read_bytes(Path(__file__).with_name(POLICY_NAME).absolute(), MAX_TEXT_BYTES)
    require(bytes_record(raw)['sha256'] == POLICY_SHA256, 'notice content policy changed')
    value = strict_json(raw)
    require(raw == canonical(value), 'noncanonical content policy')
    for name, row in value['source_files'].items():
        require(bytes_record(read_bytes(repo / relative(name), MAX_TEXT_BYTES)) == row,
                'source differs from pinned notice content policy')
    return value


def expected_inventory(policy, projection, build):
    """Close every collector field; no arbitrary repository or error strings."""
    allowed = {row['inventory_fields']['id']: row for row in policy['packages']}
    observed = {row['id']: row for row in projection['packages']}
    require(len(observed) == len(projection['packages']) and set(observed) == set(allowed),
            'native package set is outside the closed content policy')
    packages, unresolved = [], []
    for item in policy['packages']:
        row = dict(item['inventory_fields'])
        actual = observed[row['id']]
        values = actual['conservative_features']
        require(isinstance(values, list) and all(isinstance(name, str) for name in values)
                and len(values) == len(set(values)) and set(values) <= set(item['allowed_features']),
                'native feature outside known public package features')
        row['features'] = sorted(values)
        require(actual['name'] == row['name'] and actual['version'] == row['version']
                and actual['declared_license'] == row['license_expression'], 'native package identity changed')
        expected_notices = [{key: n[key] for key in ('path', 'upstream_path', 'sha256', 'bytes')}
                            for n in row['notices']]
        require(actual['notices'] == expected_notices, 'native notice set differs from pinned original content')
        packages.append(row)
        unresolved.extend({'id': row['id'], 'reason': reason} for reason in row['unresolved'])
    app = [row for row in packages if row['name'] == 'flightsim-app']
    require(len(app) == 1 and app[0]['features'] == ['default'], 'ordinary default application required')
    assets = policy['embedded_assets']
    observed_assets = {row['id']: row for row in projection['embedded_assets']}
    require(len(observed_assets) == len(projection['embedded_assets'])
            and set(observed_assets) == {row['id'] for row in assets}, 'native embedded asset set changed')
    for asset in assets:
        actual = observed_assets[asset['id']]
        require(all(actual[key] == asset[key] for key in
                    ('id', 'package', 'version', 'feature', 'source_path', 'sha256', 'notices')),
                'native embedded notice binding differs from source')
        if asset['observed_sha256'] != asset['sha256']:
            unresolved.append({'id': asset['id'], 'reason': 'Embedded dependency asset differs from the reviewed hash'})
        if asset['review_state'] == 'unresolved':
            unresolved.append({'id': asset['id'], 'reason': asset['reason']})
    return {'schema_version': 1, 'kind': 'cargo-dependency-notices', 'target': native.check.TARGET,
            'root_package': 'flightsim-app', 'cargo_lock_sha256': build['bindings']['lock']['sha256'],
            'metadata_sha256': build['builds']['ordinary']['metadata']['sha256'],
            'asset_manifest_sha256': policy['source_files']['docs/release/asset-rights-manifest.json']['sha256'],
            'supplement_manifest_sha256': policy['source_files']['docs/release/dependency-notice-supplements.json']['sha256'],
            'scope': SCOPE, 'review_status': 'not_reviewed', 'packages': packages,
            'embedded_assets': assets, 'unresolved': unresolved}


def original_files(root, inventory, policy):
    references = {n['path']: {key: n[key] for key in ('sha256', 'bytes')}
                  for row in inventory['packages'] + inventory['embedded_assets'] for n in row['notices']}
    require(set(references) == set(policy['content_sources']), 'original notice provenance set differs')
    expected = {'dependency-inventory.json', 'README.txt', *references}
    require(len(expected) <= MAX_FILES, 'notice file budget exceeded')
    directories = {parent.as_posix() for name in expected for parent in Path(name).parents
                   if parent.as_posix() != '.'}
    observed, folded = set(), set()
    entries = 0
    for path in root.rglob('*'):
        entries += 1
        require(entries <= MAX_FILES * 3, 'notice tree entry budget exceeded')
        capture.no_links(path)
        name = relative(path.relative_to(root).as_posix())
        require(name.casefold() not in folded, 'colliding notice path')
        folded.add(name.casefold())
        if path.is_dir():
            require(name in directories, 'unreferenced notice directory')
        else:
            require(name in expected, 'unreferenced original notice file')
            observed.add(name)
    require(observed == expected, 'missing original notice file')
    result, total = {}, 0
    for name in sorted(expected):
        raw = read_bytes(root / name, MAX_TEXT_BYTES)
        text_bytes(raw)
        total += len(raw)
        require(total <= MAX_TOTAL_BYTES, 'decoded original notice budget exceeded')
        if name in references:
            require(bytes_record(raw) == references[name], 'notice bytes are outside pinned public corpus')
        elif name == 'README.txt':
            require(raw in (README, README.replace(b'\n', b'\r\n')), 'collector README changed')
        result[name] = raw
    # Reuse the ordinary stager's independently maintained reference/closure gate.
    require(set(staging.notice_files(root, inventory)) == expected, 'ordinary notice_files set differs')
    return result


def validate_shape(value):
    fields = {'schema_version', 'identity', 'source_sha', 'source_tree', 'source_recipe', 'target',
              'encoding', 'inventory_path', 'bindings', 'file_manifest', 'file_count', 'decoded_bytes', 'files', *FLAGS}
    require(isinstance(value, dict) and set(value) == fields and type(value['schema_version']) is int
            and value['schema_version'] == 1 and value['identity'] == IDENTITY, 'wrong original packet shape')
    require(all(value[key] is False for key in FLAGS), 'original packet cannot grant acceptance')
    require(all(capture.hex_string(value[key], 40) for key in ('source_sha', 'source_tree'))
            and value['source_recipe'] == native.SOURCE_RECIPE and value['target'] == native.check.TARGET
            and value['encoding'] == 'base64-exact-original-utf8'
            and value['inventory_path'] == 'dependency-inventory.json', 'wrong original packet identity')
    require(isinstance(value['bindings'], dict) and set(value['bindings']) == BINDINGS
            and all(capture.valid_record(row) for row in value['bindings'].values()), 'wrong original packet bindings')
    require(isinstance(value['files'], dict) and 2 < len(value['files']) <= MAX_FILES,
            'invalid original packet file set')
    records, decoded, folded = {}, {}, set()
    for name, row in value['files'].items():
        relative(name)
        require(name.casefold() not in folded and (name in ('dependency-inventory.json', 'README.txt')
                or name.startswith('licenses/')), 'invalid or colliding packet path')
        folded.add(name.casefold())
        require(isinstance(row, dict) and set(row) == {'sha256', 'bytes', 'content_base64'}
                and capture.valid_record({key: row[key] for key in ('sha256', 'bytes')})
                and 0 < row['bytes'] <= MAX_TEXT_BYTES and isinstance(row['content_base64'], str)
                and len(row['content_base64']) <= ((MAX_TEXT_BYTES + 2) // 3) * 4, 'invalid original file record')
        raw = base64.b64decode(row['content_base64'], validate=True)
        require(base64.b64encode(raw).decode('ascii') == row['content_base64'], 'noncanonical base64')
        text_bytes(raw)
        records[name] = bytes_record(raw)
        require(records[name] == {key: row[key] for key in ('sha256', 'bytes')}, 'packet content hash changed')
        decoded[name] = raw
    require({'dependency-inventory.json', 'README.txt'} <= set(records), 'missing packet originals')
    require(type(value['file_count']) is int and value['file_count'] == len(records)
            and type(value['decoded_bytes']) is int
            and value['decoded_bytes'] == sum(row['bytes'] for row in records.values()) <= MAX_TOTAL_BYTES
            and value['file_manifest'] == bytes_record(canonical(records)), 'original packet manifest differs')
    require(value['bindings']['inventory'] == records['dependency-inventory.json'], 'original inventory binding differs')
    return decoded


def prepare(repo, expected, build_private, build_text, native_path, *, bundle,
            runtime_facts_private=None, ui_capabilities_private=None):
    """Return a deterministic packet after revalidating S, build and native facts.

    No scene success, supplemental assembly, review approval, or write is needed.
    The caller writes canonical(result) to the single EXPORT_NAME file.
    """
    repo, build_private, build_text, native_path, bundle = map(
        local_path, (repo, build_private, build_text, native_path, bundle))
    capture.disjoint(repo, build_private, build_text)
    capture.disjoint(build_private, native_path, bundle)
    source = capture.source_evidence(repo, expected)
    policy = content_policy(repo)
    require(source['source_tree'] == policy['source_tree'], 'source tree outside original content policy')
    build = native.validate_build(repo, expected, build_private, build_text)
    native_raw = read_bytes(native_path, native.MAX_BYTES)
    projection = native.project(repo, expected, build_private, build_text, bundle,
                                runtime_facts_private=runtime_facts_private,
                                ui_capabilities_private=ui_capabilities_private)
    require(native_raw == canonical(projection), 'native projection differs from independent validation')
    require(projection['source_sha'] == expected and projection['source_tree'] == source['source_tree']
            and projection['bindings']['inventory'] == build['builds']['ordinary']['inventory']
            and projection['bindings']['metadata'] == build['builds']['ordinary']['metadata']
            and projection['bindings']['build_summary'] == bytes_record(read_bytes(
                build_text / capture.EXPORT_NAME, capture.MAX_EXPORT_BYTES)), 'native original bindings differ')
    root = build_private / 'capture/ordinary/notices'
    inventory_raw = read_bytes(root / 'dependency-inventory.json', MAX_TEXT_BYTES)
    require(bytes_record(inventory_raw) == build['builds']['ordinary']['inventory'], 'original inventory bytes changed')
    inventory = strict_json(inventory_raw)
    require(canonical(inventory) == canonical(expected_inventory(policy, projection, build)),
            'original inventory has unknown or changed fields')
    collector_encoding = (json.dumps(inventory, indent=2, ensure_ascii=False, allow_nan=False) + '\n').encode('utf-8')
    require(inventory_raw in (collector_encoding, collector_encoding.replace(b'\n', b'\r\n')),
            'inventory is not exact collector JSON encoding')
    files = original_files(root, inventory, policy)
    require(files['dependency-inventory.json'] == inventory_raw, 'inventory changed during export preparation')
    records = {name: bytes_record(raw) for name, raw in files.items()}
    result = {'schema_version': 1, 'identity': IDENTITY, 'source_sha': expected,
              'source_tree': source['source_tree'], 'source_recipe': native.SOURCE_RECIPE,
              'target': native.check.TARGET, 'encoding': 'base64-exact-original-utf8',
              'inventory_path': 'dependency-inventory.json', **{key: False for key in FLAGS},
              'bindings': {'source': build['bindings']['source'], 'build_summary': projection['bindings']['build_summary'],
                           'native_projection': bytes_record(native_raw), 'inventory': records['dependency-inventory.json'],
                           'metadata': build['builds']['ordinary']['metadata'], 'lock': build['bindings']['lock'],
                           'content_policy': bytes_record(canonical(policy))},
              'file_manifest': bytes_record(canonical(records)), 'file_count': len(files),
              'decoded_bytes': sum(len(raw) for raw in files.values()),
              'files': {name: {**records[name], 'content_base64': base64.b64encode(raw).decode('ascii')}
                        for name, raw in files.items()}}
    validate_shape(result)
    require(len(canonical(result)) <= MAX_PACKET_BYTES, 'original packet exceeds export budget')
    require(original_files(root, inventory, policy) == files
            and read_bytes(native_path, native.MAX_BYTES) == native_raw
            and capture.source_evidence(repo, expected) == source
            and native.validate_build(repo, expected, build_private, build_text) == build,
            'frozen originals changed during export preparation')
    return result


def verify(packet_path, repo, expected, build_private, build_text, native_path, *, bundle,
           runtime_facts_private=None, ui_capabilities_private=None):
    """Verify one packet against independently revalidated private originals."""
    raw = read_bytes(packet_path, MAX_PACKET_BYTES)
    value = strict_json(raw)
    require(raw == canonical(value), 'noncanonical original packet')
    validate_shape(value)
    actual = prepare(repo, expected, build_private, build_text, native_path, bundle=bundle,
                     runtime_facts_private=runtime_facts_private, ui_capabilities_private=ui_capabilities_private)
    require(raw == canonical(actual), 'packet differs from independently validated original bytes')
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repo', 'build-private', 'build-evidence', 'native-projection', 'bundle', 'packet'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--runtime-facts-private', type=Path)
    parser.add_argument('--ui-capabilities-private', type=Path)
    args = parser.parse_args(argv)
    try:
        verify(args.packet, args.repo, args.source_sha, args.build_private, args.build_evidence,
               args.native_projection, bundle=args.bundle, runtime_facts_private=args.runtime_facts_private,
               ui_capabilities_private=args.ui_capabilities_private)
        print('Original notice packet matches independently validated private originals.'); return 0
    except (ValueError, OSError, KeyError, TypeError, UnicodeError, RecursionError, subprocess.SubprocessError):
        print('Original notice packet blocked; private details were not exported.', file=sys.stderr); return 1


if __name__ == '__main__':
    raise SystemExit(main())
