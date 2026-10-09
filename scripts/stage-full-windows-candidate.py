#!/usr/bin/env python3
"""Stage an exact LOCAL two-aircraft Windows candidate. Never authorize upload.

The ordinary release's existing recipe, handshake, asset and notice invariants
are reused. A factual review candidate can remain rights-blocked. A reviewed
candidate additionally consumes the original exact-inventory assembly contract.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


native = load('full_stage_native', 'project-full-native-evidence.py')
capture, require = native.capture, native.require
release = load('full_stage_ordinary', 'check-release-authorization.py')
notices_api, readiness = release.staging, release.readiness
assembly_api = load('full_stage_assembly', 'assemble-analytical-review-notices.py')
IDENTITY = 'local-full-two-aircraft-windows-candidate-v1'


def local_path(path):
    path = Path(path).absolute()
    require('..' not in path.parts, 'parent aliases are forbidden')
    capture.no_links(path)
    return path


def source_files(repo):
    """Use the ordinary explicit payload, including historical denied bytes."""
    manifest = capture.read_private_json(repo / release.ASSET_MANIFEST)
    require(manifest.get('schema_version') == 1, 'unsupported asset manifest')
    assets = {row['path']: row for row in manifest['assets']}
    require(len(assets) == len(manifest['assets']), 'duplicate asset identity')
    allowed = manifest['release_external_assets']
    require(len(allowed) == len(set(allowed)), 'duplicate full asset allowlist')
    require(all(path in assets and assets[path]['review_state'] in release.REVIEWED_STATES
                for path in allowed), 'full allowlist contains an unreviewed asset')
    denied = {row['sha256'] for row in manifest['assets']
              if row['review_state'] not in release.REVIEWED_STATES}
    for key in ('historical_excluded_assets', 'historical_excluded_dependency_assets'):
        denied.update(row['sha256'] for row in manifest.get(key, [])
                      if row.get('review_state') not in release.REVIEWED_STATES)
    names = sorted(set(release.SOURCE_FILES) | set(manifest['required_bundle_files']))
    require(len(names) <= 256 and len({name.casefold() for name in names}) == len(names),
            'unbounded or colliding full source payload')
    rows = []
    for name in names:
        path = notices_api.safe_file(repo, name)
        record = capture.file_record(path)
        require(record['sha256'] not in denied, 'historically unresolved asset bytes cannot be renamed into payload')
        require(not path.name.lower().endswith(release.RAW_SUFFIXES), 'raw data is outside the release payload')
        with path.open('rb') as stream:
            require(stream.read(4) != b'FSSC', 'regional scenery data is outside the release payload')
        if name.startswith('assets/'):
            require(name in allowed and name in assets
                    and assets[name]['review_state'] in release.REVIEWED_STATES
                    and assets[name]['sha256'] == record['sha256'], 'full asset differs from reviewed source')
        rows.append({'path': name, **record})
    return rows


def check_inventory(repo, inventory_path, review_path=None):
    inventory = capture.read_private_json(inventory_path)
    require(inventory.get('schema_version') == 1 and inventory.get('kind') == 'cargo-dependency-notices'
            and inventory.get('target') == release.TARGET and inventory.get('root_package') == 'flightsim-app',
            'wrong full target inventory')
    app = [row for row in inventory.get('packages', []) if row.get('name') == 'flightsim-app']
    version = tomllib.loads((repo / 'Cargo.toml').read_text())['workspace']['package']['version']
    require(len(app) == 1 and app[0]['version'] == version and app[0]['features'] == ['default'],
            'inventory does not describe the ordinary default recipe')
    report = readiness.check(repo, None, inventory_path, review_path)
    blockers = [row for row in report['blockers'] if row['code'] != 'BUNDLE_NOT_CHECKED']
    require(all(row.get('category') == 'review' and row['code'] not in
                ('UNRESOLVED_ASSET_RIGHTS', 'UNAPPROVED_GEODATA') for row in blockers),
            'source or dependency integrity blocks full candidate staging')
    return inventory, blockers


def stage(repo, executable, notices, output, *, review_path=None):
    repo, executable, notices, output = map(local_path, (repo, executable, notices, output))
    require(not output.exists() and output.parent.is_dir(), 'fresh output with an existing parent required')
    capture.disjoint(repo, notices, output)
    require(not output.is_relative_to(executable.parent), 'candidate overlaps its build root')
    sources = source_files(repo)
    inventory, blockers = check_inventory(repo, notices / notices_api.INVENTORY, review_path)
    version = tomllib.loads((repo / 'Cargo.toml').read_text())['workspace']['package']['version']
    distribution = release.verify_distribution_info(executable, version)
    review = capture.read_private_json(review_path) if review_path else None
    notice_names = notices_api.notice_files(notices, inventory, review)
    require(len(notice_names) <= 2048, 'unbounded notice payload')
    original = capture.snapshot_tree(notices)
    binary = capture.file_record(executable)
    with tempfile.TemporaryDirectory(prefix='.full-windows-candidate-', dir=output.parent) as temporary:
        bundle = Path(temporary) / 'bundle'; bundle.mkdir()
        for row in sources:
            notices_api.copy_file(repo / row['path'], bundle / row['path'])
            require(capture.file_record(bundle / row['path']) == {key: row[key] for key in ('bytes', 'sha256')},
                    'source payload changed while copying')
        notices_api.copy_file(executable, bundle / 'flightsim-app.exe')
        for name in notice_names:
            notices_api.copy_file(notices / name, bundle / 'third-party' / name)
            require(capture.file_record(bundle / 'third-party' / name) == original[name],
                    'notice bytes changed while copying')
        if review_path:
            review_record = capture.file_record(review_path)
            notices_api.copy_file(review_path, bundle / release.DEPENDENCY_REVIEW)
            require(capture.file_record(bundle / release.DEPENDENCY_REVIEW) == review_record,
                    'review bytes changed while copying')
        capture.write_json(bundle / 'distribution-info.json', distribution)
        capture.write_json(bundle / 'full-readiness.json', {
            'schema_version': 1, 'status': 'blocked' if blockers else 'source_and_notice_checks_passed',
            'blockers': blockers, 'whole_target_review_present': review_path is not None,
            'release_authorized': False, 'runtime_accepted': False})
        # Reuse the ordinary exact-copy verifier with this explicit engineering
        # payload. This is not its authorization-gated release plan or receipt.
        rows = [{'path': path.relative_to(bundle).as_posix(), **capture.file_record(path)}
                for path in sorted(bundle.rglob('*')) if path.is_file() and path.name != 'flightsim-app.exe']
        release.verify_bundle(bundle, {'files': rows}, executable)
        require(capture.file_record(executable) == binary and capture.snapshot_tree(notices) == original,
                'frozen original build/notices changed during staging')
        require(source_files(repo) == sources, 'source payload changed during staging')
        rows.append({'path': 'flightsim-app.exe', **binary}); rows.sort(key=lambda row: row['path'])
        capture.write_json(bundle / 'bundle-manifest.json', {
            'schema_version': 1, 'kind': IDENTITY, 'release_authorized': False,
            'source_binary_correspondence': 'requires_exact_ordinary_capture_revalidation',
            'distribution': distribution, 'dependency_target': release.TARGET,
            'rights_status': 'blocked' if blockers else 'review_consumed_final_acceptance_required',
            'files': rows, 'inventory_excludes_itself': True})
        require(not output.exists(), 'output appeared while staging')
        bundle.rename(output)
    return bool(blockers)


def stage_captured(repo, expected, build_private, build_text, output, *, assembly=None, review=None):
    verified = native.validate_build(repo, expected, build_private, build_text)
    source = capture.source_evidence(repo, expected)
    require((assembly is None) == (review is None), 'assembly and genuine review must be supplied together')
    notices = build_private / 'capture/ordinary/notices'
    if assembly is not None:
        data = capture.read_private_json(assembly / 'assembly.json')
        require(data.get('candidate_inventory_matches_actual') is True, 'supplement plan binds another native inventory')
        notices, reviewed_source = assembly_api.validate_reviewed_assembly(repo, expected, assembly, review)
        require(reviewed_source == source, 'review source differs from actual full capture')
    require(capture.file_record(notices / notices_api.INVENTORY) == verified['builds']['ordinary']['inventory'],
            'full notice inventory differs from the actual ordinary capture')
    executable = build_private / 'target-ordinary' / release.TARGET / 'release/flightsim-app.exe'
    require(capture.file_record(executable) == verified['builds']['ordinary']['executable'], 'ordinary executable changed')
    blocked = stage(repo, executable, notices, output, review_path=review)
    require(review is None or not blocked, 'genuine review still leaves full readiness blocked')
    require(capture.source_evidence(repo, expected) == source, 'full source changed during staging')
    return {'schema_version': 1, 'kind': IDENTITY, 'source_sha': expected, 'source_tree': source['source_tree'],
            'status': 'staged_review_required' if blocked else 'staged_final_acceptance_required',
            'release_authorized': False, 'runtime_accepted': False,
            'executable': verified['builds']['ordinary']['executable'],
            'inventory': verified['builds']['ordinary']['inventory'],
            'bundle_manifest': capture.file_record(output / 'bundle-manifest.json')}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True); parser.add_argument('--source-sha', required=True)
    parser.add_argument('--build-private', type=Path, required=True); parser.add_argument('--build-evidence', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--assembly', type=Path); parser.add_argument('--review', type=Path)
    args = parser.parse_args(argv)
    try:
        result = stage_captured(local_path(args.repo), args.source_sha, local_path(args.build_private),
                                local_path(args.build_evidence), local_path(args.output),
                                assembly=local_path(args.assembly) if args.assembly else None,
                                review=local_path(args.review) if args.review else None)
        print(json.dumps(result, indent=2)); return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print('Full candidate staging blocked; no approval created.', file=sys.stderr); return 1


if __name__ == '__main__':
    raise SystemExit(main())
