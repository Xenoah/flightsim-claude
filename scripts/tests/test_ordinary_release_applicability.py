"""Synthetic review-condition tests. No real review, native run, or authority."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HERE = Path(__file__).resolve().parent
q = load('ordinary_applicability_tests', HERE.parent / 'check-ordinary-release-applicability.py')
payload_tests = load('ordinary_applicability_payload_fixtures', HERE / 'test_ordinary_release_payload.py')
SYNTHETIC = 'SYNTHETIC UNIT TEST ONLY; NOT A REAL REVIEW OR APPROVAL'


class ApplicabilityFixture:
    """Reuse the existing real-gate/copy-plan fixture without inheriting its tests."""
    def __init__(self, owner):
        self.payload = payload_tests.OrdinaryPayloadTests(methodName='runTest')
        self.payload.setUp()
        owner.addCleanup(self.payload.doCleanups)
        self.fixture = self.payload.fixture
        self.repo = self.payload.repo
        sections = {name: {'synthetic': SYNTHETIC, 'section': name} for name in q.SECTIONS}
        self.native = {'content_view': {'schema_version': 2,
            'kind': 'ordinary_native_content_view_not_approval', 'sections': sections},
            'payload_projection': {'inventory_comparison': q.payload.compare_inventories(
                self.payload.original, self.payload.captured)}}
        self.review = {
            'schema_version': 1, 'kind': q.KIND, 'status': 'reviewed',
            'reviewed_by': SYNTHETIC, 'reviewed_at': '2000-01-01T00:00:00Z', 'scope': SYNTHETIC,
            'basis': {'source_sha': self.payload.expected, 'source_tree': self.payload.source['source_tree'],
                      'native_projection': q.payload.record(q.payload.canonical(self.native)),
                      'original_inventory': q.capture.file_record(self.payload.original),
                      'metadata_sha256': self.fixture.inventory['metadata_sha256']},
            'dependency_review': q.capture.file_record(self.repo / q.release.DEPENDENCY_REVIEW),
            'source_content_files': q.source_content(self.repo),
            'native_content_sha256': {key: q.digest(value) for key, value in sections.items()},
            'inventory_policy': {'allow_metadata_digest_change': False, 'allow_encoding_change': False,
                                 'rationale': SYNTHETIC, 'evidence': [SYNTHETIC]},
            'coverage': {'source_provenance_complete': True, 'native_runtime_coverage_complete': True,
                         'unresolved_conditions': [], 'reason': SYNTHETIC, 'evidence': [SYNTHETIC]},
        }
        self.save()

    def save(self, *, authorize=True):
        self.fixture.write_json(q.PATH, self.review)
        self.fixture.commit()
        if authorize:
            self.fixture.authorize_fixture()
        self.refresh_source()

    def refresh_source(self):
        p = self.payload
        p.expected = self.fixture.git('rev-parse', 'HEAD').stdout.decode().strip()
        p.source = {'source_sha': p.expected,
                    'source_tree': self.fixture.git('rev-parse', 'HEAD^{tree}').stdout.decode().strip()}
        p.refresh_verified()
        p.rebuild_bundle()

    def comparison(self):
        result = q.payload.compare_inventories(self.payload.original, self.payload.captured)
        self.native['payload_projection']['inventory_comparison'] = result
        return result


class OrdinaryApplicabilityTests(unittest.TestCase):
    def setUp(self):
        self.f = ApplicabilityFixture(self)
        self.repo, self.review, self.native = self.f.repo, self.f.review, self.f.native

    def validate(self):
        return q.validate(self.repo, self.native)

    def test_complete_synthetic_review_matches_without_creating_authority(self):
        before = {p: (self.repo / p).read_bytes() for p in
                  (q.PATH, q.release.DEPENDENCY_REVIEW, q.release.DEPENDENCY_INVENTORY, q.release.AUTHORIZATION)}
        value = self.validate()
        self.assertIs(value['conditions_matched'], True)
        self.assertEqual(value['applicability_record'], q.capture.file_record(self.repo / q.PATH))
        for key in ('release_authorized', 'dependency_review_approved', 'runtime_accepted', 'appearance_accepted'):
            self.assertIs(value[key], False)
        self.assertEqual(before, {p: (self.repo / p).read_bytes() for p in before})
        self.assertEqual(self.f.fixture.git('status', '--porcelain').stdout, b'')

    def test_missing_and_untracked_applicability_never_satisfy_the_gate(self):
        original = (self.repo / q.PATH).read_bytes()
        self.f.fixture.git('rm', q.PATH); self.f.fixture.commit(); self.f.fixture.authorize_fixture()
        with self.assertRaisesRegex(ValueError, 'not committed'):
            self.validate()
        (self.repo / q.PATH).write_bytes(original)
        with self.assertRaisesRegex(ValueError, 'not committed'):
            self.validate()

    def test_applicability_edit_cannot_authorize_itself_with_stale_receipt(self):
        self.review['scope'] += ' amended'
        self.f.save(authorize=False)
        with self.assertRaisesRegex(ValueError, 'authorization gate is blocked'):
            self.validate()

    def test_missing_or_stale_dependency_review_is_not_waived(self):
        self.f.fixture.review['inventory_sha256'] = 'f' * 64
        self.f.fixture.write_json(q.release.DEPENDENCY_REVIEW, self.f.fixture.review)
        self.f.fixture.commit(); self.f.fixture.authorize_fixture()
        with self.assertRaisesRegex(ValueError, 'authorization gate is blocked'):
            self.validate()
        self.f.fixture.git('rm', q.release.DEPENDENCY_REVIEW)
        self.f.fixture.commit(); self.f.fixture.authorize_fixture()
        with self.assertRaisesRegex(ValueError, 'authorization gate is blocked'):
            self.validate()

    def test_fresh_dependency_review_still_requires_exact_review_binding(self):
        self.f.fixture.review['scope'] += ' another completed synthetic review'
        self.f.fixture.write_json(q.release.DEPENDENCY_REVIEW, self.f.fixture.review)
        self.f.fixture.commit(); self.f.fixture.authorize_fixture()
        with self.assertRaisesRegex(ValueError, 'another dependency review'):
            self.validate()

    def test_source_bytes_change_fails_even_after_new_ordinary_authorization(self):
        for name in ('crates/flightsim-app/src/main.rs', '.github/workflows/release.yml', 'README.md'):
            with self.subTest(path=name):
                path = self.repo / name; before = path.read_bytes()
                path.write_bytes(before + b'\nSYNTHETIC CONTENT CHANGE')
                self.f.fixture.commit(); self.f.fixture.authorize_fixture()
                with self.assertRaisesRegex(ValueError, 'reviewed source content changed'):
                    self.validate()
                path.write_bytes(before); self.f.fixture.commit(); self.f.fixture.authorize_fixture()

    def test_every_native_content_section_is_exact_and_mandatory(self):
        for section in sorted(q.SECTIONS):
            with self.subTest(section=section):
                previous = self.native['content_view']['sections'][section]
                self.native['content_view']['sections'][section] = {'synthetic': SYNTHETIC, 'changed': section}
                with self.assertRaisesRegex(ValueError, 'content differs'):
                    self.validate()
                self.native['content_view']['sections'][section] = previous
                expected = self.review['native_content_sha256'].pop(section)
                self.f.save()
                with self.assertRaisesRegex(ValueError, 'every complete native-content section'):
                    self.validate()
                self.review['native_content_sha256'][section] = expected
                self.f.save()

    def test_source_content_binding_rejects_json_number_type_changes(self):
        # Python considers integer and equal floating-point values equal; an
        # exact reviewed JSON contract must not silently normalize their types.
        self.review['source_content_files'][0]['bytes'] = float(self.review['source_content_files'][0]['bytes'])
        self.f.save()
        with self.assertRaisesRegex(ValueError, 'reviewed source content changed'):
            self.validate()

    def test_duplicate_keys_and_unexpected_review_fields_fail(self):
        path = self.repo / q.PATH
        original = path.read_text(encoding='utf-8')
        path.write_text(original[:-1] + ', "status": "reviewed"}', encoding='utf-8')
        self.f.fixture.commit(); self.f.fixture.authorize_fixture()
        with self.assertRaisesRegex(ValueError, 'duplicate JSON object key'):
            self.validate()
        self.review['self_issued_approval'] = True
        self.f.save()
        with self.assertRaisesRegex(ValueError, 'completed named content review'):
            self.validate()

    def test_content_view_unknown_sections_and_boolean_schema_fail(self):
        content = self.native['content_view']
        for key, value in (('schema_version', True), ('schema_version', 1), ('kind', 'cached_approval'),
                           ('sections', {**content['sections'], 'unknown': {}})):
            with self.subTest(key=key):
                original = content[key]; content[key] = value
                with self.assertRaisesRegex(ValueError, 'unknown native content view'):
                    self.validate()
                content[key] = original

    def test_metadata_change_requires_explicit_policy_and_preserves_cause_unknown(self):
        captured = json.loads(self.f.payload.captured.read_bytes())
        captured['metadata_sha256'] = 'a' * 64
        self.f.payload.captured_value(captured)
        self.f.comparison()
        with self.assertRaisesRegex(ValueError, 'raw metadata digest change'):
            self.validate()
        self.review['inventory_policy']['allow_metadata_digest_change'] = True
        self.f.save()
        with self.assertRaisesRegex(ValueError, 'raw inventory encoding change'):
            self.validate()
        self.review['inventory_policy']['allow_encoding_change'] = True
        self.f.save()
        value = self.validate()
        self.assertEqual(value['metadata_difference_cause'], 'not_established')
        self.assertNotEqual(value['inventory_comparison']['original'], value['inventory_comparison']['captured'])

    def test_encoding_change_requires_its_own_explicit_policy(self):
        self.f.payload.captured.write_bytes(b'\n' + self.f.payload.captured.read_bytes() + b'\n')
        self.f.comparison()
        self.review['inventory_policy']['allow_metadata_digest_change'] = True
        self.f.save()
        with self.assertRaisesRegex(ValueError, 'raw inventory encoding change'):
            self.validate()
        self.review['inventory_policy']['allow_encoding_change'] = True
        self.f.save()
        value = self.validate()
        self.assertFalse(value['inventory_comparison']['raw_bytes_equal'])
        self.assertEqual(value['inventory_comparison']['different_fields'], [])

    def test_inventory_basis_and_non_metadata_obligations_cannot_change(self):
        comparison = self.native['payload_projection']['inventory_comparison']
        for field, value in (('original', {'sha256': 'f' * 64, 'bytes': 123}),
                             ('original_metadata_sha256', 'f' * 64),
                             ('all_other_fields_equal', False), ('different_fields', ['packages'])):
            with self.subTest(field=field):
                old = comparison[field]; comparison[field] = value
                with self.assertRaises(ValueError):
                    self.validate()
                comparison[field] = old

    def test_incomplete_or_unnamed_review_and_ambiguous_policy_fail(self):
        mutations = [(['status'], 'pending'), (['reviewed_by'], ' '), (['reviewed_at'], '2000-01-01'),
            (['schema_version'], True), (['scope'], ''),
            (['inventory_policy', 'allow_encoding_change'], 1),
            (['inventory_policy', 'rationale'], ''), (['inventory_policy', 'evidence'], []),
            (['coverage', 'source_provenance_complete'], False),
            (['coverage', 'native_runtime_coverage_complete'], 1),
            (['coverage', 'unresolved_conditions'], ['SYNTHETIC unresolved condition']),
            (['coverage', 'reason'], ''), (['coverage', 'evidence'], []),
            (['basis', 'source_sha'], 'invalid')]
        pristine = copy.deepcopy(self.review)
        for keys, value in mutations:
            with self.subTest(keys=keys):
                self.review.clear(); self.review.update(copy.deepcopy(pristine))
                parent = self.review
                for key in keys[:-1]: parent = parent[key]
                parent[keys[-1]] = value
                self.f.save()
                with self.assertRaises(ValueError):
                    self.validate()
        self.review.clear(); self.review.update(pristine); self.f.save()

    def test_working_tree_changes_and_review_changes_during_comparison_fail(self):
        path = self.repo / q.PATH
        path.write_bytes(path.read_bytes() + b'\n')
        with self.assertRaises(subprocess.CalledProcessError):
            self.validate()
        self.f.fixture.git('checkout', '--', q.PATH)
        original = q.source_content
        calls = 0
        def change_review(repo):
            nonlocal calls
            calls += 1
            result = original(repo)
            if calls == 1:
                path.write_bytes(path.read_bytes() + b'\n')
            return result
        with patch.object(q, 'source_content', side_effect=change_review), self.assertRaises(ValueError):
            self.validate()


if __name__ == '__main__':
    unittest.main()
