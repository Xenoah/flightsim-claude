#!/usr/bin/env python3
"""Read-only independent verification; writes only this report directory."""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parent
BASE = ROOT.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source-root', type=Path, default=BASE / 'flightsim-analytical-license-review')
parser.add_argument('--assessment', type=Path, help='Original accepted assessment; default: source-root/docs/release/two-package-license-assessment.json')
parser.add_argument('--accepted-assessment', type=Path, default=BASE / 'flightsim-analytical-license-private/portable-whole-review/accepted-two-package-assessment.json')
parser.add_argument('--native-inventory', type=Path, default=BASE / 'flightsim-validation/d918-evidence/windows-candidate/dependency-inventory.json')
parser.add_argument('--registry-root', type=Path, default=BASE / 'flightsim-cargo-recovered/registry')
parser.add_argument('--fresh-primary-records', type=Path, default=ROOT / 'fresh-primary-records.json')
parser.add_argument('--output', type=Path, default=ROOT / 'original-verification.json')
args = parser.parse_args()
SOURCE = args.source_root
EVIDENCE = SOURCE / 'docs/release/licenses/review-evidence'
ASSESSMENT = args.assessment or SOURCE / 'docs/release/two-package-license-assessment.json'
INVENTORY = args.native_inventory
sha = lambda data: hashlib.sha256(data).hexdigest()
assessment = json.loads(ASSESSMENT.read_text())
inventory = json.loads(INVENTORY.read_text())
report = {
    'scope': 'Independent byte/provenance checks of constgebra 0.1.4 and hexf-parse 0.2.1; no whole-target acceptance',
    'assessment_sha256': sha(ASSESSMENT.read_bytes()),
    'accepted_archive_identical': ASSESSMENT.read_bytes() == args.accepted_assessment.read_bytes(),
    'inventory_sha256': sha(INVENTORY.read_bytes()),
    'inventory_expected_hash_matches': sha(INVENTORY.read_bytes()) == '02797865cac5ecee5f05c33ea1a682b2bd772c8f6de8cdfea6ba7d3d91ef11d8',
    'inventory_scope': inventory['scope'],
    'inventory_review_status': inventory['review_status'],
    'inventory_unresolved': inventory['unresolved'],
    'evidence_checks': [], 'archive_checks': [], 'pinned_git_blob_checks': [],
    'complete_target_review': False, 'publication_authorized': False,
}
for entry in assessment['evidence_copy_plan']:
    p = SOURCE / entry['source_file']
    actual = sha(p.read_bytes())
    report['evidence_checks'].append({'path': entry['path'], 'sha256': actual, 'matches': actual == entry['sha256']})
for package in assessment['package_assessments']:
    name = package['id'].replace('@', '-')
    archive = next((args.registry_root / 'cache').rglob(name + '.crate'))
    row = next(x for x in inventory['packages'] if x['id'] == package['id'])
    archive_check = {'id': package['id'], 'archive_sha256': sha(archive.read_bytes()), 'matches_assessment': sha(archive.read_bytes()) == package['archive_sha256'], 'matches_native_inventory': sha(archive.read_bytes()) == row['source_checksum'], 'source_revision': row['source_revision'], 'files': []}
    with tarfile.open(archive) as tf:
        for member in tf.getmembers():
            if not member.isfile():
                continue
            data = tf.extractfile(member).read()
            rel = Path(member.name).relative_to(name)
            local = EVIDENCE / name / 'package' / rel
            source = args.registry_root / 'src/index.crates.io-1949cf8c6b5b557f' / name / rel
            archive_check['files'].append({'path': str(rel), 'bytes': len(data), 'sha256': sha(data), 'evidence_identical': local.read_bytes() == data, 'recovered_registry_identical': source.read_bytes() == data})
    report['archive_checks'].append(archive_check)
fresh = json.loads(args.fresh_primary_records.read_text())
for record in fresh:
    if 'raw.githubusercontent.com/creativecommons/' in record['url']:
        data = record['content'].encode()
        report['cc0_official_fresh_comparison'] = {'source': record['url'], 'sha256': sha(data), 'identical': data == (EVIDENCE / 'standard-license-texts/CC0-1.0.txt').read_bytes()}
    if '/git/trees/' not in record['url']:
        continue
    tree = json.loads(record['content'])
    assert not tree['truncated']
    name = 'constgebra-0.1.4' if 'constgebra' in record['url'] else 'hexf-parse-0.2.1'
    for entry in tree['tree']:
        if entry['type'] != 'blob':
            continue
        path = entry['path']
        if name == 'hexf-parse-0.2.1':
            if not path.startswith('parse/'):
                continue
            path = path[6:]
        path = 'Cargo.toml.orig' if path == 'Cargo.toml' else path
        data = (EVIDENCE / name / 'package' / path).read_bytes()
        git_blob = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        report['pinned_git_blob_checks'].append({'package': name, 'path': path, 'expected_git_blob': entry['sha'], 'actual_git_blob': git_blob, 'matches': git_blob == entry['sha']})
checks = [report['accepted_archive_identical'], report['inventory_expected_hash_matches']]
checks.extend(x['matches'] for x in report['evidence_checks'])
checks.extend(x['matches'] for x in report['pinned_git_blob_checks'])
checks.append(report['cc0_official_fresh_comparison']['identical'])
for entry in report['archive_checks']:
    checks.extend([entry['matches_assessment'], entry['matches_native_inventory']])
    checks.extend(x['evidence_identical'] and x['recovered_registry_identical'] for x in entry['files'])
report['all_byte_checks_passed'] = all(checks)
args.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'all_byte_checks_passed': all(checks), 'cited_inputs': len(report['evidence_checks']), 'archive_files': sum(len(x['files']) for x in report['archive_checks']), 'pinned_git_blobs': len(report['pinned_git_blob_checks']), 'inventory_sha256': report['inventory_sha256']}))
assert all(checks)
