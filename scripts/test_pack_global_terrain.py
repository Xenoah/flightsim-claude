#!/usr/bin/env python3
"""Offline regression tests for global terrain packing (numpy required)."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import numpy as np

PACKER_PATH = Path(__file__).with_name('pack-global-terrain.py')
packer = types.ModuleType('global_terrain_packer_under_test')
exec(compile(PACKER_PATH.read_text(), str(PACKER_PATH), 'exec'), packer.__dict__)


class TerrainPackerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.prepared = Path(cls.directory.name) / 'prepared'
        cls.prepared.mkdir()
        shape = (1080, 2160)
        arrays = {
            'etopo_orthometric_north_to_south_10min.npy': np.full(shape, 455, dtype='<f4'),
            'etopo_geoid_north_to_south_10min.npy': np.full(shape, 30, dtype='<f4'),
            'ne_land_north_to_south_10min.npy': np.zeros(shape, dtype=np.bool_),
            'inland_water.npy': np.ones(shape, dtype=np.bool_),
        }
        outputs = {}
        for name, array in arrays.items():
            path = cls.prepared / name
            np.save(path, array, allow_pickle=False)
            data = path.read_bytes()
            outputs[name] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        manifest = {
            'schema': 'flightsim-global-terrain-inputs-v2',
            'shape': list(shape), 'outputs': outputs,
            'row_order': 'north-to-south', 'column_order': 'eastward, periodic',
            'longitude_origin_deg': 1 / 120,
            'latitude_origin_deg': 90 - 19 / 120,
            'longitude_step_deg': 1 / 6, 'latitude_step_deg': -1 / 6,
            'elevation': 'float32 little-endian EGM2008 orthometric height H in metres',
            'geoid': 'float32 little-endian EGM2008 undulation N in metres; h = H + N',
            'source': 'SYNTHETIC TEST ONLY, never a distributed terrain asset',
        }
        (cls.prepared / 'terrain-preparation.json').write_text(json.dumps(manifest))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def run_packer(self, output, metadata=None):
        arguments = ['pack-global-terrain.py', '--prepared', str(self.prepared), '--output', str(output)]
        if metadata is not None:
            arguments += ['--rust-metadata', str(metadata)]
        with patch.object(sys, 'argv', arguments), contextlib.redirect_stdout(io.StringIO()):
            packer.main()

    def test_lake_dry_land_and_ocean_are_distinct(self):
        height = np.full((2, 4), 455, dtype='<f4')
        geoid = np.full((2, 4), 30, dtype='<f4')
        land = np.zeros((2, 4), dtype=bool)
        water = np.ones((2, 4), dtype=bool)
        h, n, dry, wet, _, _, _ = packer.canonical_resample(height, geoid, land, water, -135, 45)
        self.assertTrue(np.all(h == 455) and np.all(n == 30) and not dry.any() and wet.all())
        height[:] = -420
        land[:] = True
        water[:] = False
        h, _, dry, wet, _, _, _ = packer.canonical_resample(height, geoid, land, water, -135, 45)
        self.assertTrue(np.all(h == -420) and dry.all() and not wet.any())
        height[0, 0] = -8000
        land[0, 0] = False
        height[land] = 1000
        h, _, _, _, _, _, _ = packer.canonical_resample(height, geoid, land, water, -135, 45)
        self.assertGreaterEqual(h.min(), 0)
        self.assertLessEqual(h.max(), 1000)

    def test_complete_v2_layout_retains_nonzero_lake_height(self):
        output = Path(self.directory.name) / 'complete.fsgt'
        metadata = Path(self.directory.name) / 'global_metadata.rs'
        self.run_packer(output, metadata)
        data = output.read_bytes()
        self.assertEqual(data[:4], b'FSGT')
        self.assertEqual(struct.unpack_from('<H', data, 4)[0], 2)
        width, height = struct.unpack_from('<II', data, 8)
        self.assertEqual((width, height), (2048, 1024))
        count = width * height
        values = np.frombuffer(data, dtype='<i2', count=count * 2, offset=64).reshape(count, 2)
        self.assertTrue(np.all(values[:, 0] == 455) and np.all(values[:, 1] == 3000))
        mask_offset = 64 + count * 4
        self.assertEqual(data[mask_offset:mask_offset + count // 8], b'\x00' * (count // 8))
        self.assertEqual(data[mask_offset + count // 8:], b'\xff' * (count // 8))
        self.assertEqual(struct.unpack_from('<Q', data, 24)[0], packer.fingerprint(data))
        self.assertIn(f'pub const GLOBAL_TERRAIN_FINGERPRINT: u64 = 0x{packer.fingerprint(data):016x};', metadata.read_text())
        self.assertIn(f'pub const GLOBAL_TERRAIN_DATASET_ID: &str = "{packer.DATASET_ID}";', metadata.read_text())
        self.assertEqual(json.loads(output.with_suffix('.provenance.json').read_text())['status'], 'COMPLETE')

    def test_failed_final_marker_cannot_leave_stale_complete_attestation(self):
        output = Path(self.directory.name) / 'failure.fsgt'
        marker = output.with_suffix('.provenance.json')
        marker.write_text('{"status":"COMPLETE","previous":true}')
        original = packer.atomic_write
        def fail_complete(path, data):
            if path == marker and json.loads(data)['status'] == 'COMPLETE':
                raise OSError('injected final sidecar write failure')
            original(path, data)
        with patch.object(packer, 'atomic_write', fail_complete):
            with self.assertRaisesRegex(OSError, 'injected final sidecar'):
                self.run_packer(output)
        self.assertTrue(output.exists())
        self.assertEqual(json.loads(marker.read_text())['status'], 'INCOMPLETE')

    def test_wrong_source_registration_is_rejected_before_any_output(self):
        path = self.prepared / 'terrain-preparation.json'
        original = path.read_text()
        output = Path(self.directory.name) / 'wrong-origin.fsgt'
        try:
            for change in [{'longitude_origin_deg': -179.99166666666667},
                           {'latitude_origin_deg': 89.99166666666667},
                           {'row_order': 'south-to-north'},
                           {'column_order': 'westward'}]:
                manifest = json.loads(original)
                manifest.update(change)
                path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, 'registration/orientation mismatch'):
                    self.run_packer(output)
                self.assertFalse(output.exists())
                self.assertFalse(output.with_suffix('.provenance.json').exists())
        finally:
            path.write_text(original)

    def test_output_cannot_replace_a_prepared_input(self):
        source = self.prepared / 'inland_water.npy'
        original = hashlib.sha256(source.read_bytes()).hexdigest()
        with self.assertRaisesRegex(ValueError, 'must not overwrite an input'):
            self.run_packer(source)
        self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), original)


if __name__ == '__main__':
    unittest.main()
