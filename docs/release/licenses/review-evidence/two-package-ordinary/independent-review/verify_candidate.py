#!/usr/bin/env python3
"""Verify final input packet without editing the candidate or original sources."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
BASE = ROOT.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--candidate', type=Path, default=BASE / 'ordinary-two-package-candidate')
parser.add_argument('--portable', type=Path, default=BASE / 'ordinary-two-package-checkpoint/portable-assembly-inputs')
parser.add_argument('--original-source-root', type=Path, default=BASE / 'flightsim-analytical-license-review')
parser.add_argument('--native-inventory', type=Path, default=BASE / 'flightsim-validation/d918-evidence/windows-candidate/dependency-inventory.json')
parser.add_argument('--output', type=Path, default=ROOT / 'candidate-verification.json')
args = parser.parse_args()
CANDIDATE = args.candidate
PORTABLE = args.portable
ORIGINAL = args.original_source_root
EXPECTED = 'd918943a70d01644b5a6bebb8a903a5bbc89139f'
sha = lambda data: hashlib.sha256(data).hexdigest()
load = lambda p: json.loads(p.read_text())
assessment_path = CANDIDATE / 'docs/release/ordinary-two-package-assessment.json'
assessment = load(assessment_path)
prior = load(ORIGINAL / 'docs/release/two-package-license-assessment.json')
recon = load(CANDIDATE / assessment['package_reconciliation']['path'])
native = CANDIDATE / assessment['native_inventory']['path']
inventory = load(native)
head = subprocess.check_output(['git', '-C', str(CANDIDATE), 'rev-parse', 'HEAD'], text=True).strip()
tracked_diff = subprocess.check_output(['git', '-C', str(CANDIDATE), 'diff', 'HEAD', '--name-only'], text=True)
checks = [
    {'check': 'base_commit', 'passed': head == EXPECTED},
    {'check': 'tracked_files_unchanged', 'passed': not tracked_diff.strip()},
    {'check': 'ordinary_source_binding', 'passed': assessment['source_commit'] == EXPECTED},
    {'check': 'package_scope_only', 'passed': assessment['status'] in ['package_scope_assessed_pending_independent_review','package_scope_reviewed'] and assessment['complete_target_review'] is False and assessment['publication_authorized'] is False},
    {'check': 'native_bytes_unchanged', 'passed': native.read_bytes() == args.native_inventory.read_bytes()},
    {'check': 'portable_inventory_identical', 'passed': native.read_bytes() == (PORTABLE / 'dependency-inventory.json').read_bytes()},
    {'check': 'portable_assessment_identical', 'passed': assessment_path.read_bytes() == (PORTABLE / 'assessment.json').read_bytes()},
    {'check': 'inventory_remains_not_reviewed', 'passed': inventory['review_status'] == 'not_reviewed' and len(inventory['packages']) == 359},
    {'check': 'same_exact_13_copy_plan', 'passed': assessment['evidence_copy_plan'] == prior['evidence_copy_plan'] and len(prior['evidence_copy_plan']) == 13},
    {'check': 'both_original_routes_and_obligations_preserved', 'passed': all(x['proposed_distribution_route'] == y['proposed_distribution_route'] and x['obligations'] == y['obligations'] for x,y in zip(assessment['package_assessments'],prior['package_assessments']))},
]
for refname in ['native_inventory','prior_accepted_assessment','package_reconciliation']:
    ref = assessment[refname]
    data = (CANDIDATE / ref['path']).read_bytes()
    checks.append({'check': refname + '_reference', 'passed': sha(data) == ref['sha256'] and len(data) == ref['bytes'], 'sha256': sha(data)})
for entry in assessment['evidence_copy_plan']:
    candidate = CANDIDATE / entry['source_file']
    original = ORIGINAL / entry['source_file']
    portable = PORTABLE / entry['source_file']
    data = candidate.read_bytes()
    checks.append({'check': 'source_and_portable_evidence:' + entry['path'], 'passed': sha(data) == entry['sha256'] and data == original.read_bytes() == portable.read_bytes(), 'sha256': sha(data)})
resolution_inputs = {x['path']:x['sha256'] for res in assessment['resolutions'] for x in res['evidence']}
checks.append({'check': 'resolutions_reference_every_copied_file', 'passed': resolution_inputs == {x['path']:x['sha256'] for x in assessment['evidence_copy_plan']}})
for package in recon['packages']:
    actual = next(x for x in inventory['packages'] if x['id'] == package['id'])
    checks.append({'check': 'native_reconciliation:' + package['id'], 'passed': actual == package['native_package_record']})
    for entry in package['complete_archive_files']:
        data = (CANDIDATE / entry['path']).read_bytes()
        git_blob = hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()
        checks.append({'check': 'archive_reconciliation:' + entry['path'], 'passed': sha(data) == entry['sha256'] and len(data) == entry['bytes'] and git_blob == entry['git_blob_sha1']})
report = {'schema_version': 1, 'kind': 'independent_package_scope_packet_verification', 'reviewed_at': '2026-10-08', 'assessment_sha256': sha(assessment_path.read_bytes()), 'assessment_status_observed': assessment['status'], 'checks': checks, 'passed': all(x['passed'] for x in checks), 'complete_target_review': False, 'publication_authorized': False}
args.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'passed': report['passed'], 'checks': len(checks), 'assessment_sha256': report['assessment_sha256'], 'failed': [x['check'] for x in checks if not x['passed']]}))
assert report['passed']
