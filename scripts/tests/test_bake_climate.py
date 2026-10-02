"""Offline path/transaction regressions for the climate bake's output family.

Run with Python and NumPy; HDF5/h5py are not needed for these tests:
  python -m unittest discover -s scripts/tests -p test_bake_climate.py
No source-data download is required for these boundary tests.
"""

import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "bake_climate.py"
SPEC = importlib.util.spec_from_file_location("bake_climate", SCRIPT)
BAKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BAKE)


class OutputFamilyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.args = SimpleNamespace(
            input_dir=self.root / "sources", geoid=self.root / "geoid.npy",
            geoid_metadata=self.root / "terrain-preparation.json",
            output=self.root / "alternate.fsclim", metadata_output=None,
        )

    def tearDown(self):
        self.directory.cleanup()

    def test_alternate_output_never_implicitly_targets_checkout_metadata(self):
        binary, metadata, provenance = BAKE.output_paths(self.args)
        self.assertEqual(binary, self.root / "alternate.fsclim")
        self.assertEqual(metadata, self.root / "alternate.metadata.rs")
        self.assertEqual(provenance, self.root / "alternate.json")
        self.assertNotEqual(metadata.resolve(), BAKE.DEFAULT_METADATA_OUTPUT.resolve())

    def test_json_binary_alias_is_rejected_before_any_files_are_created(self):
        self.args.output = self.root / "collision.json"
        with self.assertRaises(ValueError):
            BAKE.bake(self.args)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_explicit_metadata_alias_to_binary_or_provenance_is_rejected(self):
        for metadata in [self.args.output, self.args.output.with_suffix(".json")]:
            self.args.metadata_output = metadata
            with self.assertRaises(ValueError):
                BAKE.output_paths(self.args)

    def test_symlink_alias_is_rejected(self):
        target = self.root / "shared-output"
        target.write_bytes(b"must remain unchanged")
        self.args.output.symlink_to(target)
        alias = self.root / "metadata.rs"
        alias.symlink_to(target)
        self.args.metadata_output = alias
        with self.assertRaises(ValueError):
            BAKE.output_paths(self.args)
        self.assertEqual(target.read_bytes(), b"must remain unchanged")

    def test_outputs_cannot_overwrite_any_source_or_manifest(self):
        inputs = [self.args.geoid, self.args.geoid_metadata]
        inputs += [self.args.input_dir / spec[0] for spec in BAKE.FILES.values()]
        for source in inputs:
            self.args.metadata_output = source
            with self.assertRaises(ValueError):
                BAKE.output_paths(self.args)

    def test_atomic_replacement_leaves_no_temporary_files(self):
        target = self.root / "atlas"
        target.write_bytes(b"old")
        BAKE.atomic_write(target, b"new")
        self.assertEqual(target.read_bytes(), b"new")
        self.assertEqual(list(self.root.iterdir()), [target])


class GeoidRegistrationTests(unittest.TestCase):
    def setUp(self):
        self.axes = {
            "schema": "flightsim-global-terrain-inputs-v1",
            "geoid": "float32 little-endian EGM2008 undulation N in metres; h = H + N",
            "shape": [1080, 2160], "row_order": "north-to-south",
            "column_order": "eastward, periodic",
            "latitude_origin_deg": 89.84166666666667,
            "longitude_origin_deg": 0.008333333333325754,
            "latitude_step_deg": -1/6, "longitude_step_deg": 1/6,
        }

    def test_documented_source_registration_is_valid(self):
        BAKE.validate_geoid_axes(self.axes)

    def test_lake_aware_v2_preparation_preserves_the_same_pinned_geoid_registration(self):
        BAKE.validate_geoid_axes({**self.axes, "schema": "flightsim-global-terrain-inputs-v2"})
        for unsupported in ["flightsim-global-terrain-inputs-v0", "flightsim-global-terrain-inputs-v3"]:
            with self.assertRaises(ValueError):
                BAKE.validate_geoid_axes({**self.axes, "schema": unsupported})

    def test_changed_axes_rejected_even_when_data_checksum_matches(self):
        for field in ["latitude_origin_deg", "longitude_origin_deg", "latitude_step_deg", "longitude_step_deg"]:
            for value in [-self.axes[field], self.axes[field] + .01, float("nan"), float("inf")]:
                changed = {**self.axes, field: value}
                with self.assertRaises(ValueError):
                    BAKE.validate_geoid_axes(changed)
        for field, value in [("shape", [2160, 1080]), ("row_order", "south-to-north"), ("column_order", "westward")]:
            with self.assertRaises(ValueError):
                BAKE.validate_geoid_axes({**self.axes, field: value})


if __name__ == "__main__":
    unittest.main()
