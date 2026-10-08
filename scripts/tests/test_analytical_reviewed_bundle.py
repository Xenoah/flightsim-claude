"""Synthetic notice/provenance/package fixtures; no actual positive review."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('reviewed_bundle', ROOT / 'scripts/recheck-reviewed-analytical-bundle.py')
r = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(r)
q, a = r.q, r.assembler


def make_file(root, relative, data):
    path = root / relative; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(data)
    return path


class SupplementTests(unittest.TestCase):
    def fixture(self, root):
        base = root / 'base'; base.mkdir()
        notice = make_file(base, 'licenses/example/LICENSE-MIT', b'original notice\n')
        inventory = {'target': q.check.TARGET, 'review_status': 'not_reviewed',
                     'packages': [{'notices': [{'path': 'licenses/example/LICENSE-MIT', **q.record(notice)}]}], 'embedded_assets': []}
        q.write_json(base / 'dependency-inventory.json', inventory)
        supplements = root / 'supplements'; supplements.mkdir()
        header = make_file(supplements, 'source/header.txt', b'exact candidate notice\n')
        plan = {'schema_version': 1, 'kind': 'prepared_supplement_copy_plan_not_dependency_review_receipt',
                'complete_target_review': False, 'release_authorized': False, 'inventory_sha256': 'f' * 64,
                'files': [{'checkpoint_source': 'source/header.txt', 'proposed_bundle_path': 'licenses/review-evidence/header.txt', **q.record(header)}]}
        plan_path = root / 'plan.json'; q.write_json(plan_path, plan)
        accepted = root / 'accepted'; accepted.mkdir(); grant = make_file(accepted, 'sources/grant.txt', b'grant evidence\n')
        assessment_path = root / 'assessment.json'
        q.write_json(assessment_path, {'evidence_copy_plan': [{'path': 'licenses/review-evidence/grant.txt',
                      'source_file': 'sources/grant.txt', 'sha256': q.check.digest(grant)}]})
        return base, plan_path, supplements, assessment_path, accepted

    def test_preparation_keeps_original_inventory_and_does_not_adopt_old_inventory_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); inputs = self.fixture(root)
            before = q.capture.snapshot_tree(inputs[0])
            result = a.assemble(*inputs, root / 'assembly')
            self.assertEqual(q.capture.snapshot_tree(inputs[0]), before)
            self.assertEqual(result['original_inventory'], result['assembled_inventory'])
            self.assertFalse(result['candidate_inventory_matches_actual'])
            self.assertFalse(result['complete_target_review']); self.assertFalse(result['runtime_notice_activation_approved'])
            self.assertEqual(len(result['added_files']), 2)

    def test_changed_source_and_destination_collision_fail_before_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); inputs = self.fixture(root)
            (inputs[2] / 'source/header.txt').write_bytes(b'changed')
            with self.assertRaises(ValueError): a.assemble(*inputs, root / 'assembly')
            self.assertFalse((root / 'assembly').exists())

    def test_completed_review_preflight_allows_only_unchecked_bundle_prerequisite(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); inputs = self.fixture(root); assembly = root / 'assembly'
            data = a.assemble(*inputs, assembly)
            source = {'source_tree': 'b' * 40}
            review = {'schema_version': 1, 'status': 'reviewed', 'source_sha': 'a' * 40, 'source_tree': 'b' * 40,
                      'inventory_sha256': data['original_inventory']['sha256'], 'reviewed_by': 'synthetic unit fixture only',
                      'reviewed_at': '2026-10-08', 'scope': 'not a real review', 'release_authorized': False,
                      'supplemental_evidence_binding': {'assembly_sha256': q.check.digest(assembly / 'assembly.json'),
                                                       'source_provenance_complete': True, 'native_runtime_coverage_complete': True},
                      'resolutions': [{'id': 'synthetic', 'reason': 'test fixture',
                                       'evidence': [{'path': row['path'], 'sha256': row['sha256'], 'source': 'synthetic'} for row in data['added_files']]}]}
            review_path = root / 'review.json'; q.write_json(review_path, review)
            with mock.patch.object(a.q.capture, 'source_evidence', return_value=source), \
                    mock.patch.object(a.q.check.readiness, 'check', return_value={'status': 'blocked', 'blockers': [{'code': 'BUNDLE_NOT_CHECKED'}]}) as check:
                a.validate_reviewed_assembly(root, 'a' * 40, assembly, review_path)
                check.return_value['blockers'].append({'code': 'DEPENDENCY_REVIEW_REQUIRED'})
                with self.assertRaises(ValueError): a.validate_reviewed_assembly(root, 'a' * 40, assembly, review_path)

    def test_review_copy_is_part_of_the_exact_bundle_allowlist(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); repo = root / 'repo'; repo.mkdir(); bundle = root / 'bundle'; bundle.mkdir()
            notices = root / 'notices'; notices.mkdir()
            for relative in a.stager.SOURCE_FILES: make_file(repo, relative, ('source ' + relative).encode())
            make_file(repo, 'assets/aircraft/light_single.glb', b'excluded original model')
            executable = make_file(root, 'built.exe', b'synthetic trusted application')
            notice = make_file(notices, 'licenses/example/LICENSE-MIT', b'notice')
            supplement = make_file(notices, 'licenses/review-evidence/extra.txt', b'exact supplement')
            inventory = {'packages': [{'notices': [{'path': 'licenses/example/LICENSE-MIT', **q.record(notice)}]}], 'embedded_assets': []}
            q.write_json(notices / 'dependency-inventory.json', inventory)
            review = {'resolutions': [{'evidence': [{'path': 'licenses/review-evidence/extra.txt', **q.record(supplement)}]}]}
            review_path = root / 'review.json'; q.write_json(review_path, review)
            for relative in a.stager.SOURCE_FILES: make_file(bundle, relative, (repo / relative).read_bytes())
            for relative in a.stager.notice_files(notices, inventory, review): make_file(bundle, 'third-party/' + relative, (notices / relative).read_bytes())
            make_file(bundle, 'flightsim-app.exe', executable.read_bytes())
            make_file(bundle, 'docs/release/dependency-review.json', review_path.read_bytes())
            for relative in ('distribution-info.json', 'commercial-readiness.json', 'LOCAL-CANDIDATE.txt'): make_file(bundle, relative, b'generated')
            files = [{'path': path.relative_to(bundle).as_posix(), **q.record(path)} for path in sorted(bundle.rglob('*')) if path.is_file()]
            q.write_json(bundle / 'bundle-manifest.json', {'release_authorized': False, 'files': files})
            manifest = q.check.digest(bundle / 'bundle-manifest.json')
            q.strict_bundle(bundle, executable, manifest, repo, notices, review_path)
            with self.assertRaises(ValueError): q.strict_bundle(bundle, executable, manifest, repo, notices)
            (bundle / 'docs/release/dependency-review.json').write_bytes(b'changed review')
            with self.assertRaises(ValueError): q.strict_bundle(bundle, executable, manifest, repo, notices, review_path)


class ModifiedSourceTests(unittest.TestCase):
    def test_path_patch_requires_complete_current_tree_and_reviewed_original_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary); vendor = repo / 'vendor/zune-jpeg'; vendor.mkdir(parents=True)
            make_file(vendor, 'Cargo.toml', b'[package]\nname = "zune-jpeg"\nversion = "0.5.15"\n')
            make_file(vendor, 'src/lib.rs', b'original replacement implementation')
            make_file(vendor, 'LICENSE', b'retained notice')
            patch = make_file(repo, 'docs/release/replacement.patch', b'reviewed patch bytes')
            identity = 'zune-jpeg@0.5.15'; origin = q.native.REVIEWED_PATCH_ORIGINS[identity]
            row = {'id': identity, 'source_root': 'vendor/zune-jpeg',
                   'upstream': {'registry_checksum': origin[0], 'revision': origin[1]},
                   'patch': {'path': 'docs/release/replacement.patch', **q.record(patch)},
                   'files': [{'path': name, **record} for name, record in q.capture.snapshot_tree(vendor).items()],
                   'modified_paths': ['src/lib.rs'], 'retained_notices': ['LICENSE']}
            manifest = repo / q.native.MODIFIED_MANIFEST; manifest.parent.mkdir(parents=True, exist_ok=True)
            q.write_json(manifest, {'schema_version': 1, 'packages': [row]})
            source = {'files': [{'path': path.relative_to(repo).as_posix()} for path in repo.rglob('*') if path.is_file()]}
            package = {'id': 'path+' + vendor.as_uri() + '#0.5.15', 'name': 'zune-jpeg', 'version': '0.5.15',
                       'source': None, 'manifest_path': str(vendor / 'Cargo.toml')}
            metadata = {'workspace_members': []}
            value = q.native.modified_source(repo, package, metadata, source)
            self.assertEqual(value['identity'], 'modified_vendored_source')
            self.assertFalse(value['upstream_registry_bytes_are_current_source'])
            original = dict(package)
            for changes in ({'manifest_path': str(vendor / 'Cargo.alternate.toml')},
                            {'id': 'path+file:///different/vendor/zune-jpeg#0.5.15'},
                            {'source': 'registry+https://github.com/rust-lang/crates.io-index'},
                            {'manifest_path': str(repo / 'other/Cargo.toml')}):
                with self.subTest(changes=changes), self.assertRaises(ValueError):
                    q.native.modified_source(repo, {**original, **changes}, metadata, source)
            with self.assertRaises(ValueError):
                q.native.modified_source(repo, package, {'workspace_members': [package['id']]}, source)
            row['upstream']['registry_checksum'] = 'a' * 64; q.write_json(manifest, {'schema_version': 1, 'packages': [row]})
            with self.assertRaises(ValueError): q.native.modified_source(repo, package, metadata, source)
            row['upstream']['registry_checksum'] = origin[0]; q.write_json(manifest, {'schema_version': 1, 'packages': [row]})
            make_file(vendor, 'unlisted.rs', b'unlisted')
            with self.assertRaises(ValueError): q.native.modified_source(repo, package, metadata, source)

    def test_final_recheck_commands_use_completed_review_and_original_build_tree(self):
        repo, private = Path('/repo'), Path('/final-private')
        specs = q.command_specifications(repo, private, 'runtime', 'a' * 40, build_private=Path('/frozen-build'),
                                         build_text=Path('/frozen-text'), notices=Path('/assembly/notices'), review_path=Path('/review.json'))
        self.assertIn('/frozen-build/x86_64-pc-windows-msvc/release/flightsim-app.exe'.replace('/frozen-build/', '/frozen-build/target-analytic/'), specs['stage'])
        self.assertEqual(specs['stage'][-2:], ['--dependency-review', '/review.json'])
        self.assertEqual(specs['extracted-readiness'][-2:], ['--dependency-review', '/final-private/extracted/swift-candidate/docs/release/dependency-review.json'])
        self.assertNotIn('build-capture', r.COMMANDS)

    def test_failed_public_export_rejects_arbitrary_last_command_payload(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private = root / 'private'; private.mkdir(); export = root / 'export'; export.mkdir()
            build = root / 'build'; build.mkdir(); text = root / 'build-text'; text.mkdir()
            (text / q.capture.EXPORT_NAME).write_bytes(b'synthetic build evidence')
            assembly = root / 'assembly'; assembly.mkdir(); (assembly / 'assembly.json').write_bytes(b'synthetic assembly')
            notices = assembly / 'notices'; notices.mkdir(); review = root / 'review.json'; review.write_bytes(b'{}')
            paths = {'build_private': build, 'build_text': text, 'assembly': assembly, 'review': review}
            stage = private / 'commands/runtime-facts'; stage.mkdir(parents=True)
            for name in ('stdout', 'stderr'): (stage / name).write_bytes(b'')
            command = {'id': 'runtime-facts', 'outcome': 'failed', 'exit_code': 7,
                       'stdout': q.record(stage / 'stdout'), 'stderr': q.record(stage / 'stderr')}
            specs = q.command_specifications(root, private, 'runtime', 'a' * 40, build_private=build,
                                             build_text=text, notices=notices, review_path=review)
            value = {'schema_version': 1, 'identity': r.IDENTITY, 'source_sha': 'a' * 40, 'source_tree': 'b' * 40,
                     'status': 'failed', 'release_authorized': False, 'appearance_accepted': False, 'distribution_qualified': False,
                     'commands': [command], 'images': {}, 'runtime_facts': None, 'ui_capabilities': None, 'bindings': {'original_build': q.record(text / q.capture.EXPORT_NAME),
                     'assembly': q.record(assembly / 'assembly.json'), 'review': q.record(review)}}
            def write():
                journal = {key: command[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}
                journal.update(command=specs['runtime-facts'], cwd=str(root))
                q.write_json(stage / 'journal.json', journal)
                q.write_json(private / r.EXPORT, value); q.write_json(export / r.EXPORT, value)
            write()
            with mock.patch.object(r, 'private_inputs', return_value=(paths, notices, {'source_tree': 'b' * 40})):
                r.verify_export(export, root, 'a' * 40, private)
                for outcome, code in (({'arbitrary_private_payload': 'secret'}, ['unchecked']),
                                      ('failed', ['private']), ('succeeded', False), ('failed', 2**40), ('succeeded', 7)):
                    command.update(outcome=outcome, exit_code=code); write()
                    with self.subTest(outcome=outcome, code=code), self.assertRaises(ValueError):
                        r.verify_export(export, root, 'a' * 40, private)


if __name__ == '__main__': unittest.main()
