"""Adversarial local checks. Every successful fixture is synthetic TEST DATA.

Mocked native validators below test consumer plumbing, never native, rights,
visual review or user authorization. There is no positive production receipt.
"""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
import warnings
import zipfile

SPEC = importlib.util.spec_from_file_location('publication', Path(__file__).parents[1] / 'check-analytical-publication.py')
p = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(p)
GATES = ['combined_feature_regressions', 'exact_identity_and_legacy_tests', 'extracted_bundle_isolation',
         'extracted_runtime_acceptance', 'analytical_windows_appearance', 'dependency_and_platform_review', 'publication_receipt']
SHA, TREE = 'a' * 40, 'b' * 40
DATE = '2026-01-01T00:00:00Z'


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(p.encoded(value))


def blob(data=b'SYNTHETIC TEST DATA ONLY'):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def reviews(identity, bindings):
    source = {'schema_version': 1, 'identity': 'analytical-final-source-review-v1', 'status': 'reviewed',
              'source_sha': SHA, 'source_tree': TREE, 'reviewed_by': 'synthetic-source-reviewer', 'reviewed_at': DATE,
              'source_ci': {'repository': p.REPOSITORY, 'workflow_path': '.github/workflows/ci.yml',
                            'workflow': bindings['source_workflow'], 'head_sha': SHA, 'run_id': 1, 'run_attempt': 1,
                            'status': 'completed', 'conclusion': 'success'}, 'release_authorized': False}
    choice = {'schema_version': 1, 'identity': 'analytical-explicit-user-choice-v1', 'release': identity,
              'choice': 'publish_swift_only_reinhard_prerelease', 'publication_requested': True,
              'recorded_by': 'synthetic-choice-recorder', 'recorded_at': DATE,
              'user_message_reference': 'SYNTHETIC TEST DATA ONLY /private/conversation-secret',
              'accepted_limitations': p.LIMITS, 'release_authorized': False}
    decision = {'schema_version': 1, 'identity': 'analytical-independent-publication-review-v1',
                'status': 'reviewed_recommendation_external_authorization_required', 'release': identity,
                'bindings': bindings, 'reviewed_by': 'synthetic-independent-reviewer', 'reviewed_at': DATE,
                'acceptance_conditions': {key: ('external_authorization_required' if key == 'publication_receipt' else 'reviewed') for key in GATES},
                'substantive_reviews': {key: 'accepted_with_declared_limits' for key in p.REVIEWS},
                'accepted_limitations': p.LIMITS, 'release_authorized': False}
    return source, choice, decision


class FileAndArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / 'test-only.zip'
        self.files = {'flightsim-app.exe': blob(), 'third-party/license.txt': blob(b'notice')}

    def zipped(self, rows=None):
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            with zipfile.ZipFile(self.archive, 'w', zipfile.ZIP_DEFLATED) as zipped:
                for name, data in rows or [('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY'),
                                          ('swift-candidate/third-party/license.txt', b'notice')]:
                    zipped.writestr(name, data)

    def test_actual_zip_streams_match_synthetic_extracted_files(self):
        self.zipped()
        self.assertEqual(p.audit_archive(self.archive, self.files), p.record(self.archive))

    def test_changed_member_bytes_fail_even_if_zip_crc_is_valid(self):
        self.zipped([('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA EVIL'),
                     ('swift-candidate/third-party/license.txt', b'notice')])
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)

    def test_unexpected_payload_missing_notice_and_duplicate_names_block(self):
        bad = [[('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY')],
               [('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY'), ('swift-candidate/evil.dll', b'notice')],
               [('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY'), ('swift-candidate/flightsim-app.exe', b'notice')]]
        for rows in bad:
            with self.subTest(rows=rows):
                self.zipped(rows)
                with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)

    def test_archive_links_and_unsupported_compression_rejected(self):
        link = zipfile.ZipInfo('swift-candidate/flightsim-app.exe')
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        self.zipped([(link, b'SYNTHETIC TEST DATA ONLY'), ('swift-candidate/third-party/license.txt', b'notice')])
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)
        with zipfile.ZipFile(self.archive, 'w', zipfile.ZIP_BZIP2) as zipped:
            zipped.writestr('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY')
            zipped.writestr('swift-candidate/third-party/license.txt', b'notice')
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)

    def test_archive_byte_budget_blocks_before_decompression(self):
        self.zipped()
        with mock.patch.object(p, 'MAX_ARCHIVE', 2), self.assertRaises(ValueError):
            p.audit_archive(self.archive, self.files)
        with mock.patch.object(p, 'MAX_UNPACKED', 2), self.assertRaises(ValueError):
            p.audit_archive(self.archive, self.files)

    def test_windows_path_traversal_devices_and_ambiguous_names_block(self):
        for name in ('../secret', '/root', 'a//b', 'a/./b', 'a/../b', 'a\\b', 'C:relative',
                     'NUL', 'aux.txt', 'COM1.log', 'a/trailing.', 'a/trailing ', 'a/\x00b'):
            with self.subTest(name=name), self.assertRaises(ValueError): p.relative(name)

    def test_archive_case_collision_and_comment_block(self):
        self.zipped([('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY'),
                     ('swift-candidate/FLIGHTSIM-APP.EXE', b'notice')])
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)
        self.zipped()
        with zipfile.ZipFile(self.archive, 'a') as zipped: zipped.comment = b'private raw text'
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)

    def test_hidden_prefix_trailer_and_zip_extra_fields_block(self):
        for prefix, suffix in ((b'hidden unapproved payload', b''), (b'', b'hidden unapproved payload')):
            self.zipped(); self.archive.write_bytes(prefix + self.archive.read_bytes() + suffix)
            with self.subTest(prefix=bool(prefix)), self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)
        info = zipfile.ZipInfo('swift-candidate/flightsim-app.exe')
        info.extra = b'\xfe\xca\x04\x00hide'
        self.zipped([(info, b'SYNTHETIC TEST DATA ONLY'), ('swift-candidate/third-party/license.txt', b'notice')])
        with self.assertRaises(ValueError): p.audit_archive(self.archive, self.files)

    def test_hidden_bytes_inside_declared_deflate_payload_block(self):
        self.zipped([('swift-candidate/flightsim-app.exe', b'SYNTHETIC TEST DATA ONLY')])
        with zipfile.ZipFile(self.archive) as zipped:
            central = zipped.start_dir
            compressed = zipped.infolist()[0].compress_size
        hidden = b'HIDDEN-UNAPPROVED-PAYLOAD'
        raw = bytearray(self.archive.read_bytes())
        raw[central:central] = hidden
        struct.pack_into('<I', raw, 18, compressed + len(hidden))
        struct.pack_into('<I', raw, central + len(hidden) + 20, compressed + len(hidden))
        struct.pack_into('<I', raw, len(raw) - 22 + 16, central + len(hidden))
        self.archive.write_bytes(raw)
        # The standard reader ignores the hidden bytes: exercise the stronger contract.
        with zipfile.ZipFile(self.archive) as zipped:
            self.assertEqual(zipped.read('swift-candidate/flightsim-app.exe'), b'SYNTHETIC TEST DATA ONLY')
        with self.assertRaises(ValueError): p.audit_archive(self.archive, {'flightsim-app.exe': blob()})

    def test_large_deflate_output_remains_bounded_and_exact(self):
        for data in (b'synthetic repeated test data' * 100000, b'A' * 1048577, b'A' * 2097153, b'A' * 1048576):
            with self.subTest(size=len(data)):
                self.zipped([('swift-candidate/flightsim-app.exe', data)])
                self.assertEqual(p.audit_archive(self.archive, {'flightsim-app.exe': blob(data)}), p.record(self.archive))

    def test_hard_links_and_symlink_ancestors_block(self):
        original = self.root / 'original'; original.write_bytes(b'test')
        hard = self.root / 'hard'; os.link(original, hard)
        with self.assertRaises(ValueError): p.record(hard)
        hard.unlink()
        linked = self.root / 'linked'
        try: linked.symlink_to(self.root, target_is_directory=True)
        except OSError as error:
            if os.name == 'nt' and getattr(error, 'winerror', None) in (1, 5, 50, 1314):
                self.skipTest('Windows does not grant or support synthetic symlink creation')
            raise
        with self.assertRaises(ValueError): p.record(linked / 'original')

    def test_snapshot_rejects_case_collisions(self):
        directory = self.root / 'tree'; directory.mkdir()
        (directory / 'A').write_bytes(b'one'); (directory / 'a').write_bytes(b'two')
        if len(list(directory.iterdir())) == 1: self.skipTest('case-insensitive filesystem')
        with self.assertRaises(ValueError): p.snapshot(directory)

    def test_json_duplicate_noncanonical_nonfinite_and_oversized_block(self):
        path = self.root / 'value.json'
        for raw in (b'{"x":1,"x":2}', b'{"x": 1}', b'{"x": NaN}', b'{"x": Infinity}', b'[]\n'):
            path.write_bytes(raw)
            with self.subTest(raw=raw), self.assertRaises(ValueError): p.read_json(path)
        path.write_bytes(b' ' * (p.MAX_JSON + 1))
        with self.assertRaises(ValueError): p.read_json(path)

    def test_json_exact_pin_and_hash_mutation_block(self):
        path = self.root / 'value.json'; write(path, {'x': 1})
        old = p.record(path)['sha256']; write(path, {'x': 2})
        with self.assertRaises(ValueError): p.read_json(path, old)
        self.assertEqual(p.read_json(path, p.record(path)['sha256']), {'x': 2})


class ReviewContractTests(unittest.TestCase):
    def setUp(self):
        self.identity = p.release_identity('0.6.0-alpha.21', 1, SHA, TREE)
        self.bindings = {'source_workflow': blob(), 'archive': blob(), 'executable': blob()}
        self.source, self.choice, self.decision = reviews(self.identity, self.bindings)

    def validate(self):
        p.source_review(self.source, self.identity, self.bindings['source_workflow'])
        p.user_choice(self.choice, self.identity)
        p.publication_decision(self.decision, self.identity, self.bindings, self.source, self.choice, GATES)

    def test_synthetic_consistency_is_never_authorization(self):
        self.validate()
        report = p.result_report('consistent_external_authorization_required')
        self.assertIs(report['release_authorized'], False)
        self.assertIs(report['publication_blocked'], True)
        self.assertEqual(self.decision['acceptance_conditions']['publication_receipt'], 'external_authorization_required')

    def test_missing_choice_or_old_fuller_release_does_not_qualify(self):
        for choice in (None, '', 'publish_fuller_release', 'silence', 'publish_swift_only_reinhard_prerelease_later'):
            self.choice['choice'] = choice
            with self.subTest(choice=choice), self.assertRaises(ValueError): self.validate()

    def test_self_authorizing_booleans_or_numeric_true_block(self):
        for obj, key, values in ((self.choice, 'publication_requested', [1, False, None]),
                                 (self.choice, 'release_authorized', [0, True, None]),
                                 (self.decision, 'release_authorized', [0, True, None]),
                                 (self.source, 'schema_version', [True, '1'])):
            original = obj[key]
            for value in values:
                obj[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError): self.validate()
            obj[key] = original

    def test_changed_source_tree_and_ci_fail(self):
        for field, value in [('source_sha', 'c' * 40), ('source_tree', 'd' * 40)]:
            original = self.source[field]; self.source[field] = value
            with self.assertRaises(ValueError): self.validate()
            self.source[field] = original
        for key, value in [('head_sha', 'f' * 40), ('conclusion', 'failure'), ('status', 'in_progress'),
                           ('run_attempt', True), ('workflow_path', 'release.yml'), ('workflow', blob(b'changed'))]:
            old = self.source['source_ci'][key]; self.source['source_ci'][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError): self.validate()
            self.source['source_ci'][key] = old

    def test_wrong_variant_tag_assets_and_promotion_fail(self):
        for key, value in [('variant', 'two-aircraft'), ('release_tag', 'v0.6.0'), ('prerelease', False),
                           ('prerelease', 1), ('title', 'Full release'), ('assets', ['unexpected.exe'])]:
            self.choice['release'] = {**self.identity, key: value}
            with self.subTest(key=key, value=value), self.assertRaises(ValueError): self.validate()

    def test_every_frozen_binding_and_condition_is_required(self):
        for name in self.bindings:
            self.decision['bindings'] = {**self.bindings, name: blob(b'changed')}
            with self.subTest(binding=name), self.assertRaises(ValueError): self.validate()
        self.decision['bindings'] = self.bindings
        for name in GATES:
            old = self.decision['acceptance_conditions'].pop(name)
            with self.subTest(gate=name), self.assertRaises(ValueError): self.validate()
            self.decision['acceptance_conditions'][name] = old
        self.decision['acceptance_conditions']['publication_receipt'] = 'reviewed'
        with self.assertRaises(ValueError): self.validate()

    def test_all_visual_platform_and_source_reviews_required(self):
        for key in p.REVIEWS:
            old = self.decision['substantive_reviews'].pop(key)
            with self.subTest(key=key), self.assertRaises(ValueError): self.validate()
            self.decision['substantive_reviews'][key] = old

    def test_limitations_cannot_disappear(self):
        self.choice['accepted_limitations'] = p.LIMITS[:-1]
        with self.assertRaises(ValueError): self.validate()
        self.choice['accepted_limitations'] = p.LIMITS
        self.decision['accepted_limitations'] = []
        with self.assertRaises(ValueError): self.validate()

    def test_independent_reviewer_and_decision_order_required(self):
        for reviewer in (self.source['reviewed_by'], self.choice['recorded_by'], self.source['reviewed_by'].upper()):
            self.decision['reviewed_by'] = reviewer
            with self.assertRaises(ValueError): self.validate()
        self.decision['reviewed_by'] = 'synthetic-independent-reviewer'
        for date in ('2025-01-01T00:00:00Z', '2999-01-01T00:00:00Z', DATE.replace('Z', '+01:00')):
            self.decision['reviewed_at'] = date
            with self.assertRaises(ValueError): self.validate()

    def test_no_unknown_fields_or_private_text_in_public_manifest(self):
        self.decision['secret'] = '/private/conversation-secret'
        with self.assertRaises(ValueError): self.validate()
        value = p.manifest(self.identity, self.bindings, blob())
        raw = p.encoded(value)
        for forbidden in (b'conversation-secret', b'synthetic-source-reviewer', b'user_message_reference', b'raw_log'):
            self.assertNotIn(forbidden, raw)
        self.assertFalse(value['release_authorized']); self.assertTrue(value['publication_blocked'])

    def test_invalid_version_revision_and_identity_rejected(self):
        for version in ('0.6.0+build', '../secret', 'v0.6.0', '01.2.3', '1.2.3\n'):
            with self.subTest(version=version), self.assertRaises(ValueError): p.release_identity(version, 1, SHA, TREE)
        for revision in (0, -1, True, '1', 10000):
            with self.subTest(revision=revision), self.assertRaises(ValueError): p.release_identity('1.2.3', revision, SHA, TREE)
        with self.assertRaises(ValueError): p.release_identity('1.2.3', 1, 'HEAD', TREE)


class LocalPreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / 'repo'; self.repo.mkdir()
        self.paths = {}
        for key in p.INPUTS:
            path = self.root / key
            if key not in ('source_review', 'user_choice', 'publication_decision'):
                path.mkdir(); (path / 'SYNTHETIC-TEST-DATA').write_bytes(b'not native evidence')
            self.paths[key] = path
        self.input = self.root / 'inputs.json'; write(self.input, {key: str(value) for key, value in self.paths.items()})
        self.bundle = self.root / 'bundle'; self.bundle.mkdir()
        (self.bundle / 'flightsim-app.exe').write_bytes(b'NOT AN EXECUTABLE: SYNTHETIC TEST DATA')
        self.notices = self.root / 'notices'; self.notices.mkdir(); (self.notices / 'notice.txt').write_bytes(b'synthetic notice')
        workflow = self.root / 'workflow'; workflow.write_bytes(b'synthetic workflow')
        archive = self.root / 'archive'; archive.write_bytes(b'NOT A ZIP: mocked collection only')
        self.bound = {'source_workflow': workflow, 'archive': archive, 'source_review': self.paths['source_review'], 'user_choice': self.paths['user_choice']}
        self.identity = p.release_identity('0.6.0-alpha.21', 1, SHA, TREE)
        bindings = {'source_workflow': p.record(workflow)}
        source, choice, _ = reviews(self.identity, bindings)
        write(self.paths['source_review'], source); write(self.paths['user_choice'], choice)
        self.bindings = {name: p.record(path) for name, path in self.bound.items()}
        for key in ('runtime_export', 'live_export', 'legacy_export'):
            self.bindings[key + '_tree'] = p.tree_record(p.snapshot(self.paths[key]))
        self.bindings['bundle_tree'] = p.tree_record(p.snapshot(self.bundle))
        self.bindings['notices_tree'] = p.tree_record(p.snapshot(self.notices))
        _, _, decision = reviews(self.identity, self.bindings)
        write(self.paths['publication_decision'], decision)
        self.pins = {name: p.record(self.paths[name])['sha256'] for name in ('source_review', 'user_choice', 'publication_decision')}
        source_identity = {'source_sha': SHA, 'source_tree': TREE}
        q = types.SimpleNamespace(capture=types.SimpleNamespace(source_evidence=lambda *args: source_identity))
        self.state = {'q': q, 'paths': self.bound, 'source': source_identity, 'bundle': self.bundle, 'notices': self.notices,
                      'files': p.snapshot(self.bundle), 'notice_files': p.snapshot(self.notices), 'gates': GATES}
        self.output = self.root / 'public-text'

    def check(self, output=True):
        return p.check_local(self.repo, SHA, TREE, self.input, 1, self.pins, self.output if output else None)

    def collector(self):
        return mock.patch.object(p, 'collect', return_value=(self.identity, self.bindings, self.state))

    def test_mocked_complete_consistency_writes_only_two_bounded_text_files(self):
        with self.collector(): result = self.check()
        self.assertIs(result['release_authorized'], False); self.assertTrue(result['publication_blocked'])
        self.assertEqual({path.name for path in self.output.iterdir()}, {'release-variant.json', 'SHA256SUMS.txt'})
        raw = (self.output / 'release-variant.json').read_bytes()
        self.assertNotIn(b'conversation-secret', raw)
        self.assertIn(hashlib.sha256(raw).hexdigest(), (self.output / 'SHA256SUMS.txt').read_text())
        self.assertFalse(json.loads(raw)['release_authorized'])
        self.assertLess(len(raw), 32768)

    def test_missing_choice_blocks_before_native_collection_and_output(self):
        self.paths['user_choice'].unlink()
        with mock.patch.object(p, 'collect') as collect, self.assertRaises(OSError): self.check()
        collect.assert_not_called(); self.assertFalse(self.output.exists())

    def test_forged_pinned_choice_is_rejected_before_collection(self):
        choice = p.read_json(self.paths['user_choice']); choice['recorded_by'] = 'forged-user'
        write(self.paths['user_choice'], choice)
        with mock.patch.object(p, 'collect') as collect, self.assertRaises(ValueError): self.check()
        collect.assert_not_called(); self.assertFalse(self.output.exists())

    def test_missing_native_validator_result_blocks_metadata(self):
        with mock.patch.object(p, 'collect', side_effect=ValueError('native evidence absent')), self.assertRaises(ValueError): self.check()
        self.assertFalse(self.output.exists())

    def test_frozen_bytes_mutation_after_review_is_blocked(self):
        for path in (self.bundle / 'flightsim-app.exe', self.notices / 'notice.txt', self.bound['archive'],
                     self.paths['live_export'] / 'SYNTHETIC-TEST-DATA'):
            previous = path.read_bytes(); path.write_bytes(previous + b'changed')
            with self.subTest(path=path.name), self.collector(), self.assertRaises(ValueError): self.check()
            path.write_bytes(previous)
        self.assertFalse(self.output.exists())

    def test_choice_swapped_during_collection_is_blocked(self):
        def changed(*args):
            value = p.read_json(self.paths['user_choice']); value['recorded_by'] = 'forged-late-recorder'
            write(self.paths['user_choice'], value)
            return self.identity, self.bindings, self.state
        with mock.patch.object(p, 'collect', side_effect=changed), self.assertRaises(ValueError): self.check()
        self.assertFalse(self.output.exists())

    def test_moved_source_at_final_boundary_blocks(self):
        self.state['q'].capture.source_evidence = lambda *args: {'source_sha': 'c' * 40, 'source_tree': TREE}
        with self.collector(), self.assertRaises(ValueError): self.check()
        self.assertFalse(self.output.exists())

    def test_output_collision_and_private_overlap_do_not_overwrite(self):
        self.output.mkdir(); protected = self.output / 'keep'; protected.write_bytes(b'keep')
        with self.collector(), self.assertRaises(ValueError): self.check()
        self.assertEqual(protected.read_bytes(), b'keep')
        self.output = self.repo / 'output'
        with self.collector(), self.assertRaises(ValueError): self.check()
        self.output = self.paths['live_private'] / 'output'
        with self.collector(), self.assertRaises(ValueError): self.check()

    def test_destination_race_cannot_replace_even_an_empty_directory(self):
        original_mkdir = Path.mkdir
        raced = False
        def mkdir(path, *args, **kwargs):
            nonlocal raced
            if path == self.output and not raced:
                raced = True
                original_mkdir(path)
            return original_mkdir(path, *args, **kwargs)
        with self.collector(), mock.patch.object(Path, 'mkdir', mkdir), self.assertRaises(FileExistsError): self.check()
        self.assertTrue(raced)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_parent_segment_alias_cannot_write_inside_source_or_evidence(self):
        (self.root / 'alias').mkdir()
        for destination in ('repo', 'live_private'):
            self.output = self.root / 'alias' / '..' / destination / 'unapproved-inside-source'
            with self.subTest(destination=destination), self.collector(), self.assertRaises(ValueError): self.check()
            self.assertFalse(self.output.exists())

    def test_cli_never_returns_success_even_on_mocked_consistency(self):
        args = ['checker', '--source-sha', SHA, '--source-tree', TREE, '--inputs', str(self.input), '--variant-revision', '1',
                '--source-review-sha256', self.pins['source_review'], '--user-choice-sha256', self.pins['user_choice'],
                '--publication-decision-sha256', self.pins['publication_decision']]
        with mock.patch.object(sys, 'argv', args), mock.patch.object(p, 'check_local', return_value=p.result_report('consistent_external_authorization_required')):
            with mock.patch('builtins.print'): self.assertEqual(p.main(), 2)

    def test_argparse_failures_are_sanitized_blocked_exit_one(self):
        for args in ([], ['--variant-revision', 'PRIVATE-INVALID-ARGUMENT'], ['--unknown-private-argument']):
            result = subprocess.run([sys.executable, str(Path(p.__file__)), *args], capture_output=True, text=True)
            with self.subTest(args=args):
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stderr, '')
                self.assertNotIn('PRIVATE', result.stdout)
                self.assertNotIn('unknown-private', result.stdout)
                self.assertFalse(json.loads(result.stdout)['release_authorized'])

    def test_real_cli_missing_inputs_is_nonzero_and_sanitized(self):
        result = subprocess.run([sys.executable, str(Path(p.__file__)), '--source-sha', SHA, '--source-tree', TREE,
                                 '--inputs', str(self.root / 'private-secret-missing'), '--variant-revision', '1',
                                 '--source-review-sha256', 'c' * 64, '--user-choice-sha256', 'd' * 64,
                                 '--publication-decision-sha256', 'e' * 64], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertNotIn('private-secret', result.stdout + result.stderr)
        self.assertFalse(json.loads(result.stdout)['release_authorized'])
        self.assertFalse(self.output.exists())


class NativeConsumerTests(unittest.TestCase):
    """Mock producer failure states only; never replace them with native passes."""
    def setUp(self):
        self.source = {'source_sha': SHA, 'source_tree': TREE}
        self.q = mock.MagicMock()
        self.q.capture.source_evidence.return_value = self.source
        self.q.validate_export.return_value = {'phase': 'regressions', 'status': 'engineering_evidence_complete_reviews_required'}
        self.final = mock.MagicMock()
        self.final.verify_export.return_value = {'status': 'final_bundle_runtime_observed_review_required'}
        self.ui = mock.MagicMock()
        self.ui.verify_export.side_effect = [{'scenario': 'live', 'status': 'observable_sequence_recorded_review_required'},
                                            {'scenario': 'legacy', 'status': 'blocked'}]
        self.q.load.side_effect = [self.final, self.ui]
        self.paths = {name: Path('/synthetic-test-only') / name for name in p.INPUTS}

    def collect(self):
        with mock.patch.object(p, 'qualification_module', return_value=self.q):
            return p.collect(Path('/synthetic-test-only/repo'), SHA, TREE, self.paths, 1)

    def test_changed_final_source_tree_blocks_before_regressions(self):
        self.source['source_tree'] = 'c' * 40
        with self.assertRaises(ValueError): self.collect()
        self.q.validate_export.assert_not_called()

    def test_failed_regression_cannot_be_replaced_by_runtime_phase(self):
        for value in ({'phase': 'runtime', 'status': 'engineering_evidence_complete_reviews_required'},
                      {'phase': 'regressions', 'status': 'failed'}):
            self.q.validate_export.return_value = value
            with self.subTest(value=value), self.assertRaises(ValueError): self.collect()
        self.q.load.assert_not_called()

    def test_failed_final_bundle_blocks_before_ui(self):
        self.final.verify_export.return_value = {'status': 'failed'}
        with self.assertRaises(ValueError): self.collect()
        self.ui.verify_export.assert_not_called()

    def test_incomplete_legacy_ui_blocks_and_both_use_exact_final_bundle(self):
        with self.assertRaises(ValueError): self.collect()
        self.assertEqual(self.ui.verify_export.call_count, 2)
        for scenario, call in zip(('live', 'legacy'), self.ui.verify_export.call_args_list):
            self.assertEqual(call.args, (self.paths[scenario + '_export'], self.paths[scenario + '_private'],
                                        Path('/synthetic-test-only/repo'), SHA,
                                        self.paths['runtime_private'], self.paths['runtime_export']))

    def test_wrong_live_scenario_blocks_before_legacy(self):
        self.ui.verify_export.side_effect = [{'scenario': 'legacy', 'status': 'observable_sequence_recorded_review_required'}]
        with self.assertRaises(ValueError): self.collect()
        self.assertEqual(self.ui.verify_export.call_count, 1)

    def test_genuine_producer_validator_exception_propagates(self):
        self.final.verify_export.side_effect = ValueError('synthetic changed native archive')
        with self.assertRaises(ValueError): self.collect()
        self.ui.verify_export.assert_not_called()

    def test_platform_projection_revalidates_exact_final_build_and_private_roots(self):
        private = Path('/synthetic/final-private')
        build = {'build_private': Path('/synthetic/original-build'), 'build_text': Path('/synthetic/original-text')}
        bundle = private / 'extracted/swift-candidate'
        repo = Path('/synthetic/repo')
        p.platform_projection(self.q, repo, SHA, private, build, bundle)
        self.q.validate_runtime_facts.assert_called_once_with(private, repo, SHA, build['build_private'], build['build_text'])
        self.q.native.project.assert_called_once_with(repo, SHA, build['build_private'], build['build_text'], bundle,
                                                     runtime_facts_private=private / 'runtime-facts',
                                                     ui_capabilities_private=private / 'ui-capabilities')

    def test_missing_or_changed_native_runtime_facts_block_projection(self):
        self.q.validate_runtime_facts.side_effect = ValueError('synthetic changed origin or audited executable')
        with self.assertRaises(ValueError):
            p.platform_projection(self.q, Path('/synthetic/repo'), SHA, Path('/synthetic/private'),
                                  {'build_private': Path('/synthetic/build'), 'build_text': Path('/synthetic/text')},
                                  Path('/synthetic/bundle'))
        self.q.native.project.assert_not_called()

    def test_all_seven_original_gate_identifiers_are_retained(self):
        q = p.qualification_module()
        self.assertEqual(list(q.check.GATES), GATES)


if __name__ == '__main__': unittest.main()
