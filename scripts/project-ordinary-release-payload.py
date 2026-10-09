#!/usr/bin/env python3
"""Compare an ordinary copy-plan payload with its same-build native capture.

This read-only factual adapter does not evaluate review applicability, runtime
acceptance, archive/smoke correspondence, or permission to publish. It does not
run a compiler or executable. A successful return is NOT a publication gate.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import stat
import subprocess
import sys


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release = load('check-release-authorization')
capture = load('capture-analytical-swift-msvc')
require = capture.require
MAX_JSON_BYTES = 8 * 1024 * 1024
MAX_MEMBERS = 16384
MAX_PAYLOAD_BYTES = 1024 ** 3
INVENTORY_FIELDS = {
    'schema_version', 'kind', 'target', 'root_package', 'cargo_lock_sha256',
    'metadata_sha256', 'asset_manifest_sha256', 'supplement_manifest_sha256',
    'scope', 'review_status', 'packages', 'embedded_assets', 'unresolved',
}


def canonical(value):
    # JSON equality must distinguish booleans, integers and floating-point data.
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False).encode('ascii')


def record(raw):
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)}


def absolute(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'canonical absolute input paths required')
    capture.no_links(path)
    require(path.resolve() == path, 'canonical absolute input paths required')
    return path


def member_name(value):
    require(isinstance(value, str) and 0 < len(value) <= 512, 'invalid payload member name')
    for component in value.split('/'):
        require(component not in ('', '.', '..') and not component.endswith(('.', ' '))
                and not re.search(r'[\\<>:"|?*\x00-\x1f\x7f]', component)
                and not re.fullmatch(r'(?i:con|prn|aux|nul|com[1-9\u00b9\u00b2\u00b3]|lpt[1-9\u00b9\u00b2\u00b3])(?:\..*)?', component),
                'unsafe or aliased Windows payload member')
    return value


def independent_file(path):
    capture.no_links(path)
    details = path.lstat()
    require(stat.S_ISREG(details.st_mode) and details.st_nlink == 1,
            'input must be an independent regular file')
    return details


def read_object(path):
    path = absolute(path)
    details = independent_file(path)
    require(0 < details.st_size <= MAX_JSON_BYTES, 'JSON input is outside the byte budget')
    raw = path.read_bytes()
    require(len(raw) == details.st_size and b'\0' not in raw, 'JSON input changed or contains NUL bytes')

    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate JSON object key')
            result[key] = value
        return result

    def invalid_constant(_):
        raise ValueError('non-finite JSON value')

    try:
        value = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs, parse_constant=invalid_constant)
        require(isinstance(value, dict), 'JSON input must be an object')
        canonical(value)
    except (UnicodeError, RecursionError) as error:
        raise ValueError('invalid or excessively nested JSON input') from error
    return value, record(raw)


def read_inventory(path):
    value, binding = read_object(path)
    require(set(value) == INVENTORY_FIELDS and type(value['schema_version']) is int
            and value['schema_version'] == 1 and value['kind'] == 'cargo-dependency-notices'
            and value['target'] == release.TARGET and value['root_package'] == 'flightsim-app'
            and value['review_status'] == 'not_reviewed', 'unexpected original collector inventory contract')
    for name in ('cargo_lock_sha256', 'metadata_sha256', 'asset_manifest_sha256'):
        require(capture.hex_string(value[name], 64), 'invalid inventory digest')
    require(value['supplement_manifest_sha256'] is None
            or capture.hex_string(value['supplement_manifest_sha256'], 64), 'invalid supplement digest')
    require(isinstance(value['scope'], str) and value['scope'].strip(), 'missing inventory scope')
    for name, minimum, maximum in (('packages', 1, 1024), ('embedded_assets', 0, 64), ('unresolved', 0, 8192)):
        rows = value[name]
        require(isinstance(rows, list) and minimum <= len(rows) <= maximum
                and all(isinstance(row, dict) for row in rows), 'invalid inventory member set')
    for name in ('packages', 'embedded_assets'):
        identities = [row.get('id') for row in value[name]]
        require(all(isinstance(identity, str) and identity for identity in identities)
                and len(identities) == len(set(identities)), 'duplicate or missing inventory identity')
    return value, binding


def compare_inventories(original, captured):
    """Report exact raw identities; reject every other semantic field change.

    No normalized inventory or content-count shortcut is produced. A metadata
    difference is only an observation; its cause and acceptability are unknown.
    """
    old, old_binding = read_inventory(original)
    new, new_binding = read_inventory(captured)
    different = [key for key in sorted(INVENTORY_FIELDS) if canonical(old[key]) != canonical(new[key])]
    require(not set(different) - {'metadata_sha256'}, 'inventory content differs outside metadata_sha256')
    return {
        'original': old_binding, 'captured': new_binding,
        'original_metadata_sha256': old['metadata_sha256'],
        'captured_metadata_sha256': new['metadata_sha256'],
        'raw_bytes_equal': old_binding == new_binding,
        'different_fields': different,
        'all_other_fields_equal': True,
        'status': ('metadata_digest_changed_requires_applicability_review' if different else
                   'raw_bytes_equal' if old_binding == new_binding else 'encoding_changed_requires_applicability_review'),
        'metadata_difference_cause': 'not_established' if different else 'no_digest_difference',
        'review_applicability_approved': False,
    }


def snapshot_payload(root):
    root = absolute(root)
    require(root.is_dir(), 'payload must be a directory')
    files, directories, folded = {}, set(), set()
    total = 0
    for index, path in enumerate(root.rglob('*')):
        require(index < 2 * MAX_MEMBERS, 'payload entry budget exceeded')
        capture.no_links(path)
        relative = member_name(path.relative_to(root).as_posix())
        require(relative.casefold() not in folded, 'case-colliding Windows payload member')
        folded.add(relative.casefold())
        if path.is_dir():
            directories.add(relative)
            continue
        details = independent_file(path)
        total += details.st_size
        require(len(files) < MAX_MEMBERS and total <= MAX_PAYLOAD_BYTES, 'payload byte or member budget exceeded')
        files[relative] = capture.file_record(path)
    wanted_directories = {parent.as_posix() for name in files for parent in Path(name).parents
                          if parent != Path('.')}
    require(files and directories == wanted_directories, 'payload has empty or unexplained directories')
    return files


def compare_notices(original_root, captured_root, comparison, review):
    """Preserve the closed original notice tree and compare each captured byte."""
    old, old_binding = read_inventory(original_root / release.staging.INVENTORY)
    new, new_binding = read_inventory(captured_root / release.staging.INVENTORY)
    require(old_binding == comparison['original'] and new_binding == comparison['captured'],
            'inventory changed during comparison')
    original_names = set(release.staging.notice_files(original_root, old, review))
    captured_names = set(release.staging.notice_files(captured_root, new))
    require(captured_names <= original_names, 'captured notice set differs from shipped notices')
    old_files, new_files = snapshot_payload(original_root), snapshot_payload(captured_root)
    require(set(old_files) == original_names and set(new_files) == captured_names, 'unexplained notice members')
    rows = []
    for name in sorted(captured_names - {release.staging.INVENTORY}):
        require(old_files[name] == new_files[name], 'captured original notice bytes differ')
        rows.append({'path': name, **new_files[name]})
    return rows


def project_payload(repo, expected, build_private, build_text, bundle):
    """Use only current source, the real capture API and ordinary gate authority."""
    repo, build_private, build_text, bundle = map(absolute, (repo, build_private, build_text, bundle))
    capture.disjoint(repo, build_private, build_text, bundle)
    require(capture.hex_string(expected, 40), 'exact source SHA required')
    read_inventory(repo / release.DEPENDENCY_INVENTORY)
    gate, plan = release.inspect(repo)
    require(gate['authorized'] is True and gate['status'] == 'authorized' and not gate['blockers'],
            'ordinary source and authorization gate is blocked')
    require(plan['files'] and release.hash_value(plan) == gate['release_inventory_sha256'], 'invalid ordinary copy plan')
    summary_path = build_text / capture.EXPORT_NAME
    summary_details = independent_file(summary_path)
    require(0 < summary_details.st_size <= capture.MAX_EXPORT_BYTES, 'native summary exceeds byte budget')
    summary_binding = capture.file_record(summary_path)
    verified = capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    validated_summary = (json.dumps(verified, indent=2, sort_keys=True, allow_nan=False) + '\n').encode('ascii')
    require(record(validated_summary) == summary_binding and capture.file_record(summary_path) == summary_binding,
            'native summary changed across validation')
    require(verified['status'] == capture.PASS and verified['source_sha'] == expected
            and verified.get('source_recipe') == capture.check.SOURCE_RECIPE,
            'completed current same-build native audit required')
    ordinary = verified['builds']['ordinary']
    require(ordinary['lut_payloads_found'] == 2, 'ordinary build must contain the two admitted LUT payloads')
    source = capture.source_evidence(repo, expected)
    require(source['source_tree'] == verified['source_tree'], 'native source tree differs')
    executable = build_private / 'target-ordinary' / release.TARGET / 'release/flightsim-app.exe'
    captured_root = build_private / 'capture/ordinary/notices'
    original_root = (repo / release.DEPENDENCY_INVENTORY).parent
    paths = {'executable': executable, 'inventory': captured_root / release.staging.INVENTORY,
             'metadata': build_private / 'capture/ordinary/metadata.json'}
    for key, path in paths.items():
        independent_file(path)
        require(capture.file_record(path) == ordinary[key], 'same-build native input differs: ' + key)
    comparison = compare_inventories(original_root / release.staging.INVENTORY, paths['inventory'])
    require(comparison['captured_metadata_sha256'] == ordinary['metadata']['sha256'],
            'captured inventory metadata digest differs from audited metadata')
    review, review_binding = read_object(repo / release.DEPENDENCY_REVIEW)
    notices = compare_notices(original_root, captured_root, comparison, review)
    wanted = {row['path']: {key: row[key] for key in ('sha256', 'bytes')} for row in plan['files']}
    wanted['flightsim-app.exe'] = ordinary['executable']
    release.verify_bundle(bundle, plan, executable)
    shipped = snapshot_payload(bundle)
    require(shipped == wanted, 'payload snapshot differs from authorized plan and audited executable')
    require(all(wanted.get('third-party/' + row['path']) == {key: row[key] for key in ('sha256', 'bytes')}
                for row in notices), 'captured notice comparison differs from authorized plan')
    require(shipped['third-party/dependency-inventory.json'] == comparison['original']
            and shipped[release.DEPENDENCY_REVIEW] == review_binding, 'shipped inventory/review binding differs')
    result = {
        'schema_version': 1, 'kind': 'ordinary_release_payload_projection_not_acceptance',
        'status': 'facts_projected', 'source_sha': expected, 'source_tree': source['source_tree'],
        'source_recipe': verified['source_recipe'], 'version': gate['version'], 'build': plan['build'],
        'source_inventory_sha256': gate['source_inventory_sha256'],
        'release_inventory_sha256': gate['release_inventory_sha256'],
        'authorization_receipt_sha256': gate['authorization_sha256'],
        'bindings': {'native_build_summary': summary_binding, 'native_executable': ordinary['executable'],
                     'captured_metadata': ordinary['metadata'], 'dependency_review': review_binding},
        'inventory_comparison': comparison, 'matched_original_notices': notices,
        'shipped_files': [{'path': name, **binding} for name, binding in sorted(shipped.items())],
        'release_authorized': False, 'dependency_review_approved': False,
        'review_applicability_approved': False, 'runtime_accepted': False,
        'archive_and_extracted_smoke_binding': 'not_evaluated',
        'whole_target_native_review_applicability': 'not_evaluated',
    }
    # Observe source/payload/input stability over this read. The existing gate
    # still owns the copy plan; no cached caller-supplied plan is accepted here.
    require(capture.source_evidence(repo, expected) == source, 'source changed during projection')
    final_gate, final_plan = release.inspect(repo)
    require(final_gate == gate and final_plan == plan and snapshot_payload(bundle) == wanted,
            'ordinary source, authorization or payload changed during projection')
    require(capture.file_record(summary_path) == summary_binding
            and all(capture.file_record(path) == ordinary[key] for key, path in paths.items()),
            'native inputs changed during projection')
    require(compare_notices(original_root, captured_root, comparison, review) == notices,
            'captured notices changed during projection')
    require(len(canonical(result)) <= MAX_JSON_BYTES, 'projection exceeds byte budget')
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('repo', 'build-private', 'build-text', 'bundle'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--expected-sha', required=True)
    args = parser.parse_args(argv)
    try:
        result = project_payload(args.repo, args.expected_sha, args.build_private, args.build_text, args.bundle)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RecursionError, subprocess.SubprocessError):
        print('Ordinary payload projection failed: input validation did not complete.', file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True, allow_nan=False))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
