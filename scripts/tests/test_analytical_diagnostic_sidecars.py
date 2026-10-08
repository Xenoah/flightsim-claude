"""S/P isolation and canonical post-step exports; no real native observation."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('sidecars', Path(__file__).parents[1] / 'observe-analytical-diagnostic-sidecars.py')
s = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(s)


class SidecarBoundaryTests(unittest.TestCase):
    def test_native_os_and_image_observations_are_closed_and_case_independent(self):
        version = SimpleNamespace(major=10, minor=0, build=20348, platform=2, service_pack_major=0,
                                  service_pack_minor=0, product_type=3, platform_version=(10, 0, 20348))
        with mock.patch.object(s.sys, 'platform', 'win32'), mock.patch.object(s.sys, 'getwindowsversion', return_value=version, create=True), \
             mock.patch.dict(s.os.environ, {'IMAGEOS': 'win22', 'IMAGEVERSION': '20261005.2.0'}, clear=True):
            value = s.windows_observation()
        self.assertEqual(value['platform_version'], [10, 0, 20348]); self.assertEqual(value['runner_image_os'], 'win22')
        with mock.patch.object(s.sys, 'platform', 'win32'), mock.patch.object(s.sys, 'getwindowsversion', return_value=version, create=True), \
             mock.patch.dict(s.os.environ, {'IMAGEOS': 'C:\\private', 'IMAGEVERSION': 'SECRET'}, clear=True):
            value = s.windows_observation()
        self.assertIsNone(value['runner_image_os']); self.assertIsNone(value['runner_image_version'])
        self.assertNotIn('SECRET', json.dumps(value))

    def test_wrong_source_and_policy_identity_reject_before_importing_source_code(self):
        with mock.patch.object(s, 'load', side_effect=AssertionError('no source execution')):
            with self.assertRaises(ValueError): s.inputs(Path('/source'), 'a' * 40, Path('/private'), Path('/export'), 'b' * 40)
            with self.assertRaises(ValueError): s.inputs(Path('/source'), s.SOURCE_SHA, Path('/private'), Path('/export'), 'bad')

    def test_sidecar_file_set_and_exact_reprojection_reject_resealed_payloads(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); export = root / 'export'; export.mkdir(); private = root / 'private'; private.mkdir()
            # Upstream original-build/source validation is separate. This test
            # attacks the real last-mile artifact membership/canonical binding.
            origin = {'kind': 'synthetic-origin', 'release_authorized': False}
            facts = {'kind': 'synthetic-facts', 'release_authorized': False}
            failure = {'kind': 'synthetic-failure', 'runtime_accepted': False}
            def write():
                for name, value in ((s.ORIGIN, origin), (s.facts.PROJECTION_NAME, facts), (s.failure.EXPORT, failure)):
                    (export / name).write_bytes(s.failure.canonical(value))
            write()
            args = (export, root / 'source', s.SOURCE_SHA, root / 'source-private', root / 'source-export', 'b' * 40, private)
            with mock.patch.object(s, 'projected_origin', return_value=(origin, facts, failure)):
                s.validate_export(*args)
                bad = copy.deepcopy(failure); bad['arbitrary_private_payload'] = 'SECRET'
                (export / s.failure.EXPORT).write_bytes(s.failure.canonical(bad))
                with self.assertRaises(ValueError): s.validate_export(*args)
                write(); (export / 'raw-stderr.log').write_bytes(b'PRIVATE')
                with self.assertRaises(ValueError): s.validate_export(*args)

    def test_linux_cannot_run_native_collector_or_scene(self):
        with mock.patch.object(s.sys, 'platform', 'linux'), mock.patch.object(s.facts, 'collect_runtime_facts', side_effect=AssertionError('no native queries')):
            with self.assertRaises(ValueError):
                s.observe(Path('/source'), s.SOURCE_SHA, Path('/source-private'), Path('/source-export'), 'b' * 40, Path('/private'), Path('/export'))


if __name__ == '__main__': unittest.main()
