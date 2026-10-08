#!/usr/bin/env python3
"""Prepare a separate exact supplemental notice tree; never create a review.

Raw native capture inventory/notices remain immutable. Conditional candidates
may be copied privately, but cannot be staged as reviewed until an independent
genuine source/inventory-bound review covers every addition and all prerequisites.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location('notice_qualification', Path(__file__).with_name('qualify-analytical-swift-windows.py'))
q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(q)
stager = q.load('notice_stager', 'stage-commercial-candidate.py')
require = q.require
MAX_FILES = 1024
MAX_TOTAL = 16 * 1024 * 1024
WITHHELD_ORIGINAL_CODE = {'37a255ed2e3460d7ece5cfa1c145f4fb15f2c91b4d550e1ec7ce0fb97e3a90d1'}


def planned_files(plan_path, supplement_root, assessment_path, assessment_root):
    plan = q.capture.read_private_json(plan_path)
    require(plan.get('schema_version') == 1 and plan.get('kind') == 'prepared_supplement_copy_plan_not_dependency_review_receipt'
            and plan.get('complete_target_review') is False and plan.get('release_authorized') is False,
            'candidate plan must not masquerade as a review or approval')
    require(isinstance(plan.get('files'), list) and 0 < len(plan['files']) <= MAX_FILES, 'invalid candidate file list')
    result = []
    for row in plan['files']:
        source = supplement_root / q.native.relative(row['checkpoint_source'])
        destination = q.native.relative(row['proposed_bundle_path'])
        require(destination.startswith('licenses/review-evidence/'), 'supplement outside review evidence namespace')
        q.capture.no_links(source)
        actual = q.record(source)
        require(actual == {key: row[key] for key in ('sha256', 'bytes')}, 'candidate supplement bytes changed')
        require(actual['sha256'] not in WITHHELD_ORIGINAL_CODE, 'withheld original code cannot be renamed into notice evidence')
        stager.notice_text(source, destination)
        result.append({'path': destination, 'source': source, **actual})
    assessment = q.capture.read_private_json(assessment_path)
    require(isinstance(assessment.get('evidence_copy_plan'), list) and 0 < len(assessment['evidence_copy_plan']) <= 64,
            'accepted two-package evidence plan is missing')
    for row in assessment['evidence_copy_plan']:
        source = assessment_root / q.native.relative(row['source_file'])
        destination = q.native.relative(row['path'])
        require(destination.startswith('licenses/review-evidence/'), 'two-package evidence outside notice namespace')
        q.capture.no_links(source); actual = q.record(source)
        require(actual['sha256'] == row['sha256'], 'two-package source evidence changed')
        require(actual['sha256'] not in WITHHELD_ORIGINAL_CODE, 'withheld original code cannot be renamed into notice evidence')
        stager.notice_text(source, destination)
        result.append({'path': destination, 'source': source, **actual})
    require(len(result) <= MAX_FILES and len({row['path'] for row in result}) == len(result), 'duplicate or unbounded supplement paths')
    require(sum(row['bytes'] for row in result) <= MAX_TOTAL, 'supplement text budget exceeded')
    return result, plan


def assemble(base, plan_path, supplement_root, assessment_path, assessment_root, output):
    for path in (base, plan_path, supplement_root, assessment_path, assessment_root, output): q.capture.no_links(path)
    require(not output.exists(), 'use a fresh supplemental preparation root')
    q.capture.disjoint(base.resolve(), output.absolute())
    inventory_path = base / 'dependency-inventory.json'
    inventory = q.capture.read_private_json(inventory_path)
    require(inventory.get('target') == q.check.TARGET and inventory.get('review_status') == 'not_reviewed', 'not the unmodified native collector inventory')
    base_files = stager.notice_files(base, inventory)
    before = q.capture.snapshot_tree(base)
    additions, plan = planned_files(plan_path, supplement_root, assessment_path, assessment_root)
    require(not set(base_files) & {row['path'] for row in additions}, 'supplements may not overwrite original inventory/notices')
    output.mkdir(parents=True)
    notices = output / 'notices'; notices.mkdir()
    for relative in base_files:
        destination = notices / relative; destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(base / relative, destination)
    for row in additions:
        destination = notices / row['path']; destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(row['source'], destination)
        require(q.record(destination) == {key: row[key] for key in ('sha256', 'bytes')}, 'copy changed supplemental bytes')
    require(q.capture.snapshot_tree(base) == before, 'frozen original notices changed')
    result = {'schema_version': 1, 'kind': 'private_supplement_preparation_not_review', 'status': 'prepared_review_required',
              'release_authorized': False, 'complete_target_review': False, 'runtime_notice_activation_approved': False,
              'original_inventory': q.record(inventory_path), 'assembled_inventory': q.record(notices / 'dependency-inventory.json'),
              'candidate_plan': q.record(plan_path), 'two_package_assessment': q.record(assessment_path),
              'candidate_inventory_sha256': plan['inventory_sha256'],
              'candidate_inventory_matches_actual': plan['inventory_sha256'] == q.check.digest(inventory_path),
              'original_files': before,
              'added_files': [{key: row[key] for key in ('path', 'sha256', 'bytes')} for row in additions],
              'assembled_files': q.capture.snapshot_tree(notices),
              'remaining': ['genuine_whole_target_review', 'modified_source_and_header_rebinding_if_applicable',
                            'native_runtime_and_toolset_binding', 'every_added_file_referenced_by_review',
                            'final_bundle_runtime_and_publication_gates']}
    q.write_json(output / 'assembly.json', result)
    return result


def validate_reviewed_assembly(repo, expected, assembly, review_path):
    """Consume, never fabricate, the separately completed reviewer record."""
    source = q.capture.source_evidence(repo, expected)
    data = q.capture.read_private_json(assembly / 'assembly.json')
    require(data.get('kind') == 'private_supplement_preparation_not_review' and data.get('release_authorized') is False
            and data.get('complete_target_review') is False, 'assembly is not an unapproved preparation')
    notices = assembly / 'notices'; inventory = notices / 'dependency-inventory.json'
    require(q.capture.snapshot_tree(notices) == data['assembled_files'] and q.record(inventory) == data['original_inventory'] == data['assembled_inventory'],
            'assembled original inventory or notice bytes changed')
    review = q.capture.read_private_json(review_path)
    require(review.get('schema_version') == 1 and review.get('status') == 'reviewed' and review.get('source_sha') == expected
            and review.get('source_tree') == source['source_tree'] and review.get('inventory_sha256') == q.check.digest(inventory)
            and review.get('reviewed_by') and review.get('reviewed_at') and review.get('scope'),
            'genuine completed review must bind exact source/tree/native inventory')
    require(review.get('release_authorized', False) is False, 'dependency review cannot grant publication')
    bindings = review.get('supplemental_evidence_binding')
    require(isinstance(bindings, dict) and set(bindings) == {'assembly_sha256', 'source_provenance_complete', 'native_runtime_coverage_complete'}
            and bindings['assembly_sha256'] == q.check.digest(assembly / 'assembly.json')
            and bindings['source_provenance_complete'] is True and bindings['native_runtime_coverage_complete'] is True,
            'genuine reviewer has not completed modified-source/native runtime conditions')
    references = {}
    for resolution in review.get('resolutions', []):
        for evidence in resolution.get('evidence', []):
            path = q.native.relative(evidence['path'])
            require(path not in references or references[path] == evidence['sha256'], 'inconsistent review evidence reference')
            references[path] = evidence['sha256']
    require(all(references.get(row['path']) == row['sha256'] for row in data['added_files']), 'completed review omits supplemental notice evidence')
    stager.notice_files(notices, q.capture.read_private_json(inventory), review)
    readiness = q.check.readiness.check(repo, None, inventory, review_path)
    # No bundle exists at this preflight. Only that exact pending prerequisite
    # is allowed; the unchanged stager must check the actual complete bundle.
    require(readiness['status'] == 'blocked' and len(readiness['blockers']) == 1
            and readiness['blockers'][0]['code'] == 'BUNDLE_NOT_CHECKED',
            'unchanged readiness checker still blocks this reviewer record')
    return notices, source


def stage_reviewed(repo, expected, build_private, build_text, assembly, review_path, output):
    verified = q.capture.validate_export(build_text, repo=repo, expected=expected, private=build_private)
    require(verified['status'] == q.capture.PASS, 'exact native build evidence required')
    notices, source = validate_reviewed_assembly(repo, expected, assembly, review_path)
    require(q.record(notices / 'dependency-inventory.json') == verified['builds']['analytic']['inventory'], 'reviewed inventory differs from exact native build')
    executable = build_private / 'target-analytic' / q.check.TARGET / 'release/flightsim-app.exe'
    require(q.record(executable) == verified['builds']['analytic']['executable'], 'native executable changed')
    require(not output.exists(), 'fresh reviewed candidate destination required')
    # The sole existing stager owns every copy and acceptance rule. This entry
    # creates a private candidate only, with no upload or publication operation.
    blocked = stager.stage(repo, executable, notices, output, review_path)
    require(not blocked, 'unchanged stager/readiness still blocks reviewed candidate')
    require(q.capture.source_evidence(repo, expected) == source, 'source changed while staging')
    return {'status': 'reviewed_private_candidate_staged_runtime_recheck_required', 'release_authorized': False,
            'source_sha': expected, 'source_tree': source['source_tree'], 'inventory': q.record(notices / 'dependency-inventory.json'),
            'review': q.record(review_path), 'bundle_manifest': q.record(output / 'bundle-manifest.json'), 'executable': q.record(executable)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-notices', type=Path); parser.add_argument('--plan', type=Path)
    parser.add_argument('--supplement-root', type=Path); parser.add_argument('--two-package-assessment', type=Path)
    parser.add_argument('--two-package-root', type=Path); parser.add_argument('--output', type=Path)
    parser.add_argument('--stage-reviewed', action='store_true'); parser.add_argument('--assembly', type=Path)
    parser.add_argument('--review', type=Path); parser.add_argument('--source-sha')
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--build-private', type=Path); parser.add_argument('--build-text', type=Path)
    args = parser.parse_args()
    try:
        require(args.output is not None, 'fresh output required')
        if args.stage_reviewed:
            require(all((args.assembly, args.review, args.source_sha, args.build_private, args.build_text)), 'completed-review arguments required')
            result = stage_reviewed(args.repo.resolve(), args.source_sha, args.build_private, args.build_text,
                                    args.assembly, args.review, args.output)
        else:
            require(all((args.base_notices, args.plan, args.supplement_root, args.two_package_assessment, args.two_package_root)), 'preparation inputs required')
            result = assemble(args.base_notices, args.plan, args.supplement_root, args.two_package_assessment, args.two_package_root, args.output)
        print('Supplemental notice preparation: ' + result['status'] + '; release_authorized=false.')
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print('Supplemental notice preparation/staging blocked; no review or approval generated.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
