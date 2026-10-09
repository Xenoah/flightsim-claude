"""Historical unresolved bytes stay excluded after a current original replacement.

Synthetic source and review fixtures only; no genuine authorization is created.
"""
import hashlib
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    'historical_copy_plan_fixture', Path(__file__).with_name('test_release_authorization.py'))
fixtures = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixtures)
gate = fixtures.gate


class HistoricalReleaseCopyPlanTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.ReleaseAuthorizationTests(methodName='runTest')
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()

    def test_renamed_historical_bytes_cannot_enter_full_copy_plan(self):
        fixture = self.fixture
        payload = 'Synthetic unresolved historical mesh bytes'
        fixture.manifest['historical_excluded_assets'] = [{
            'path': 'assets/aircraft/light_single.glb',
            'review_state': 'unresolved',
            'sha256': hashlib.sha256(payload.encode()).hexdigest(),
        }]
        # README is already in the full plan. A harmless filename must not
        # conceal the old unresolved bytes after the current model is replaced.
        fixture.write('README.md', payload)
        fixture.refresh_evidence()
        result, _ = gate.inspect(fixture.repo)
        self.assertFalse(result['authorized'])
        self.assertTrue(any(b['code'] == 'UNRESOLVED_ASSET_RIGHTS' and 'README.md' in b['message']
                            for b in result['blockers']))

    def test_renamed_historical_dependency_blocks_even_synthetic_authorization(self):
        fixture = self.fixture
        payload = 'Synthetic unresolved historical dependency payload'
        fixture.manifest['historical_excluded_dependency_assets'] = [{
            'id': 'synthetic-historical-lut', 'review_state': 'unresolved',
            'sha256': hashlib.sha256(payload.encode()).hexdigest(),
        }]
        fixture.write('README.md', payload)
        fixture.refresh_evidence()
        # These are explicitly synthetic receipts in a temporary repository.
        # The denial must survive an otherwise matching authorization fixture.
        fixture.authorize_fixture()
        result, _ = gate.inspect(fixture.repo)
        self.assertFalse(result['authorized'])
        self.assertEqual({b['code'] for b in result['blockers']}, {'UNRESOLVED_ASSET_RIGHTS'})
        self.assertTrue(any('README.md' in b['message'] for b in result['blockers']))

    def test_original_current_asset_does_not_inherit_historical_denial(self):
        fixture = self.fixture
        fixture.manifest['historical_excluded_assets'] = [{
            'path': 'assets/aircraft/light_single.glb',
            'review_state': 'unresolved',
            'sha256': hashlib.sha256(b'Distinct synthetic historical bytes').hexdigest(),
        }]
        fixture.refresh_evidence()
        result, _ = gate.inspect(fixture.repo)
        self.assertFalse(result['authorized'])
        self.assertFalse(any(b['code'] == 'UNRESOLVED_ASSET_RIGHTS' for b in result['blockers']))
        self.assertTrue(any(b['code'] == 'AUTHORIZATION_MISSING' for b in result['blockers']))


if __name__ == '__main__':
    unittest.main()
