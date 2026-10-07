"""Offline known-answer and hostile-input tests for the fixed Balzers recipe.

The source 6.5 MB recovery artifact need not be committed. Synthetic source
archives explicitly patch the fixed pins in tests only. The CLI has no override.
"""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import struct
import tempfile
import unittest
from unittest import mock
import warnings
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/package-balzers-terrain.py"
SAMPLE = ROOT / "docs/examples/terrain-packages/balzers"
spec = importlib.util.spec_from_file_location("balzers_package", SCRIPT)
recipe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recipe)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fnv(data):
    result = 14695981039346656037
    for byte in data:
        result = ((result ^ byte) * 1099511628211) % (1 << 64)
    return result


SAMPLES = struct.pack("<4225H", *([0, 65535] * 2112 + [0]))
CHECKSUM = fnv(SAMPLES)


def synthetic_tile(tile=(10, 1077, 244), **changes):
    fields = dict(magic=b"FSDM", version=1, level=tile[0], flags=0,
                  x=tile[1], y=tile[2], width=65, height=65,
                  offset=100.0, scale=1.0 / 65535, error=0.1, checksum=CHECKSUM)
    fields.update(changes)
    return struct.pack("<4sHBBIIIIdddQ", *fields.values()) + SAMPLES


def synthetic_source():
    # Only read license/provenance documentation from the checked local sample.
    with zipfile.ZipFile(SAMPLE / "Balzers_Terrain_Package_v1.zip") as archive:
        template = json.loads(archive.read("manifest.json"))
        payload = {path: archive.read(path) for path in recipe.SOURCE_DOCS}
    for tile in recipe.tile_ids(recipe.SOURCE_ROOTS):
        payload[recipe.tile_path(tile)] = synthetic_tile(tile)
    template.update(id="liechtenstein-glo90", title="Synthetic test input")
    template["terrain"]["bounds_degrees"] = recipe.bounds_for(recipe.SOURCE_ROOTS)
    template["files"] = [{"path": path, "kind": "terrain_dem" if path.endswith(".fsdem") else "documentation",
                          "size_bytes": len(data), "sha256": digest(data), "source": "regional-dem"}
                         for path, data in sorted(payload.items())]
    return template, payload


def source_archive(manifest, payload, mutate_info=None, duplicate=None, manifest_bytes=None):
    manifest_bytes = manifest_bytes or recipe.json_bytes(manifest)
    target = io.BytesIO()
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, allowZip64=False) as archive:
        items = [("manifest.json", manifest_bytes), *sorted(payload.items())]
        if duplicate:
            items.append((duplicate, payload[duplicate]))
        for path, data in items:
            info = zipfile.ZipInfo(path, date_time=(2020, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            if mutate_info:
                mutate_info(info)
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr(info, data)
    return target.getvalue(), manifest_bytes


def pins(data, manifest_bytes):
    return mock.patch.multiple(recipe, SOURCE_BYTES=len(data), SOURCE_SHA256=digest(data),
                               SOURCE_MANIFEST_BYTES=len(manifest_bytes),
                               SOURCE_MANIFEST_SHA256=digest(manifest_bytes))


class BalzersPackageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest, cls.payload = synthetic_source()
        cls.source, cls.manifest_bytes = source_archive(cls.manifest, cls.payload)

    def test_geographic_bounds_known_answer_and_balzers_point(self):
        self.assertEqual(recipe.ROOTS, ((10, 1077, 244), (10, 1078, 244)))
        self.assertEqual(recipe.bounds_for(recipe.ROOTS), {
            "west": 9.31640625, "south": 46.93359375,
            "east": 9.66796875, "north": 47.109375})
        self.assertEqual(recipe.tile_bounds((10, 1078, 244)), {
            "west": 9.4921875, "south": 46.93359375,
            "east": 9.66796875, "north": 47.109375})
        bounds = recipe.bounds_for(recipe.ROOTS)
        self.assertTrue(bounds["west"] < 9.501 < bounds["east"])
        self.assertTrue(bounds["south"] < 47.068 < bounds["north"])

    def test_complete_family_counts_and_all_42_child_groups(self):
        ids = recipe.tile_ids(recipe.ROOTS)
        self.assertEqual(len(ids), 170)
        self.assertEqual({level: sum(t[0] == level for t in ids) for level in range(10, 14)},
                         {10: 2, 11: 8, 12: 32, 13: 128})
        self.assertEqual(sum(t[0] < 13 for t in ids), 42)
        for level, x, y in ids:
            if level < 13:
                self.assertTrue({(level + 1, x * 2 + dx, y * 2 + dy)
                                 for dx in (0, 1) for dy in (0, 1)} <= ids)
        self.assertEqual(len(recipe.tile_ids(recipe.SOURCE_ROOTS)), 765)

    def test_every_selected_tile_bounds_inside_declared_box(self):
        outer = recipe.bounds_for(recipe.ROOTS)
        for tile in recipe.tile_ids(recipe.ROOTS):
            inner = recipe.tile_bounds(tile)
            self.assertTrue(outer["west"] <= inner["west"] < inner["east"] <= outer["east"])
            self.assertTrue(outer["south"] <= inner["south"] < inner["north"] <= outer["north"])

    def test_synthetic_source_full_positive_validation(self):
        with pins(self.source, self.manifest_bytes):
            manifest, payload = recipe.inspect_source(self.source)
        self.assertEqual(manifest, self.manifest)
        self.assertEqual(payload, self.payload)

    def test_pins_reject_before_any_zip_parser(self):
        with mock.patch.object(recipe.zipfile, "ZipFile", side_effect=AssertionError("must not parse")):
            for value in (b"", b"not a ZIP", b"x" * recipe.SOURCE_BYTES):
                with self.subTest(length=len(value)), self.assertRaisesRegex(ValueError, "pinned"):
                    recipe.inspect_source(value)

    def test_regular_snapshot_rejects_size_hash_and_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.zip"
            path.write_bytes(self.source)
            with pins(self.source, self.manifest_bytes):
                self.assertEqual(recipe.read_pinned_source(path), self.source)
                path.write_bytes(self.source[:-1])
                with self.assertRaisesRegex(ValueError, "size"):
                    recipe.read_pinned_source(path)
                path.write_bytes(bytes([self.source[0] ^ 1]) + self.source[1:])
                with self.assertRaisesRegex(ValueError, "SHA-256"):
                    recipe.read_pinned_source(path)
                with self.assertRaisesRegex(ValueError, "regular"):
                    recipe.read_pinned_source(Path(directory))
                link = Path(directory) / "link"
                try:
                    link.symlink_to(path)
                except OSError:
                    pass  # Windows may not grant symlink creation to this test user.
                else:
                    with self.assertRaisesRegex(ValueError, "non-symlink"):
                        recipe.read_pinned_source(link)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO not available on this platform")
    def test_fifo_is_rejected_without_opening(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fifo"
            os.mkfifo(path)
            with self.assertRaisesRegex(ValueError, "regular"):
                recipe.read_pinned_source(path)

    def test_duplicate_and_nonfinite_json_fail_closed(self):
        for value in (b'{"id":"a","id":"b"}', b'{"x":NaN}', b'{"x":Infinity}'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                recipe.strict_json(value)
        with self.assertRaises(UnicodeDecodeError):
            recipe.strict_json(b'"\xff"')

    def test_manifest_unknown_fields_missing_family_and_bad_paths(self):
        cases = []
        changed = copy.deepcopy(self.manifest)
        changed["future"] = 1
        cases.append(changed)
        changed = copy.deepcopy(self.manifest)
        changed["files"] = changed["files"][:-1]
        cases.append(changed)
        for path in ("../escape", "terrain/10/01077/244.fsdem", "TERRAIN/10/1077/244.fsdem"):
            changed = copy.deepcopy(self.manifest)
            changed["files"][0]["path"] = path
            cases.append(changed)
        changed = copy.deepcopy(self.manifest)
        changed["files"][1] = changed["files"][0]
        cases.append(changed)
        changed = copy.deepcopy(self.manifest)
        changed["terrain"]["datum"] = "EPSG:3855"
        cases.append(changed)
        changed = copy.deepcopy(self.manifest)
        changed["sources"][0]["license"]["text_path"] = "missing.txt"
        cases.append(changed)
        changed = copy.deepcopy(self.manifest)
        changed["schema_version"] = True
        cases.append(changed)
        for value in cases:
            with self.subTest(value=str(value)[:100]), self.assertRaises(ValueError):
                recipe.validate_source_manifest(value)

    def test_manifest_payload_hash_failure_in_unselected_tile(self):
        # Validating only the 170 selected members would miss this corrupt input.
        payload = self.payload.copy()
        path = "terrain/10/1079/242.fsdem"
        payload[path] = payload[path][:-1] + bytes([payload[path][-1] ^ 1])
        data, manifest_bytes = source_archive(self.manifest, payload)
        with pins(data, manifest_bytes), self.assertRaisesRegex(ValueError, "payload integrity"):
            recipe.inspect_source(data)

    def test_source_manifest_hash_and_zip_envelope_failures(self):
        with pins(self.source, self.manifest_bytes):
            with mock.patch.object(recipe, "SOURCE_MANIFEST_SHA256", "0" * 64):
                with self.assertRaisesRegex(ValueError, "manifest identity"):
                    recipe.inspect_source(self.source)
        for value in (b"prepend" + self.source, self.source + b"append", self.source[:-1]):
            with pins(value, self.manifest_bytes), self.assertRaisesRegex(ValueError, "ZIP32"):
                recipe.inspect_source(value)

    def test_zip_duplicate_and_special_file_rejected(self):
        data, manifest_bytes = source_archive(self.manifest, self.payload,
                                              duplicate="docs/notice.txt")
        with pins(data, manifest_bytes), self.assertRaisesRegex(ValueError, "central directory"):
            recipe.inspect_source(data)
        def symlink(info):
            if info.filename == "docs/notice.txt":
                info.external_attr = (stat.S_IFLNK | 0o777) << 16
        data, manifest_bytes = source_archive(self.manifest, self.payload, mutate_info=symlink)
        with pins(data, manifest_bytes), self.assertRaisesRegex(ValueError, "metadata"):
            recipe.inspect_source(data)

    def test_dem_header_checksum_finite_range_and_error_limits(self):
        good = synthetic_tile()
        recipe.validate_tile(good, (10, 1077, 244))
        changes = ({"magic": b"BADD"}, {"version": 2}, {"flags": 1}, {"x": 1078},
                   {"width": 4096}, {"height": 0}, {"offset": float("nan")},
                   {"scale": float("inf")}, {"scale": -1.0}, {"error": -1.0},
                   {"checksum": 0}, {"offset": -12001.0}, {"offset": 100001.0},
                   {"error": 1.1}, {"scale": 1e308})
        for changed in changes:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                recipe.validate_tile(synthetic_tile(**changed), (10, 1077, 244))
        for changed in (good[:-1], good + b"x"):
            with self.assertRaisesRegex(ValueError, "encoded size"):
                recipe.validate_tile(changed, (10, 1077, 244))

    def test_generation_is_deterministic_and_retains_original_bytes(self):
        first = recipe.create_package(self.manifest, self.payload)
        second = recipe.create_package(self.manifest, dict(reversed(list(self.payload.items()))))
        self.assertEqual(first, second)
        archive_data, manifest_bytes, report = first
        manifest = json.loads(manifest_bytes)
        self.assertEqual(manifest["sources"], self.manifest["sources"])
        self.assertEqual(report["selected_dem_files"], 170)
        self.assertFalse(report["historical_archive_equality_claimed"])
        with zipfile.ZipFile(io.BytesIO(archive_data)) as archive:
            self.assertEqual(len(archive.infolist()), 175)
            self.assertIsNone(archive.testzip())
            self.assertEqual(archive.read("manifest.json"), manifest_bytes)
            for record in manifest["files"]:
                value = archive.read(record["path"])
                self.assertEqual(len(value), record["size_bytes"])
                self.assertEqual(digest(value), record["sha256"])
                if record["path"] != recipe.PROVENANCE_PATH:
                    self.assertEqual(value, self.payload[record["path"]])
                info = archive.getinfo(record["path"])
                self.assertEqual(info.compress_type, zipfile.ZIP_STORED)
                self.assertEqual(info.date_time, (2026, 10, 7, 0, 0, 0))
                self.assertEqual(info.external_attr >> 16, 0o100644)
            self.assertIn(b"not the rectangle's exact center", archive.read(recipe.PROVENANCE_PATH))

    def test_download_catalog_uses_exact_built_identity_and_hash(self):
        archive, manifest_bytes, _ = recipe.create_package(self.manifest, self.payload)
        # Synthetic .invalid owner name documents no real publication is asserted.
        urls = ["https://github.com/test.invalid/terrain/releases/download/test/balzers.zip",
                "https://raw.githubusercontent.com/test.invalid/terrain/" + "a" * 40 + "/balzers.zip"]
        manifest = json.loads(manifest_bytes)
        for url in urls:
            catalog = json.loads(recipe.create_catalog(manifest_bytes, archive, url))
            row = catalog["regions"][0]
            self.assertEqual(row["url"], url)
            self.assertEqual(row["archive_sha256"], digest(archive))
            self.assertEqual(row["bounds_degrees"], manifest["terrain"]["bounds_degrees"])
            for key in ("id", "version", "title"):
                self.assertEqual(row[key], manifest[key])

    def test_catalog_rejects_mutable_raw_aliases_and_unsafe_urls(self):
        base = "https://github.com/test.invalid/terrain/releases/download/test/balzers.zip"
        invalid = [base.replace("https:", "http:"), base + "?token=x", base + "#fragment",
                   base + "?", base.replace("github.com", "github.com:443"),
                   base.replace("github.com", "user@github.com"), base.replace("test/balzers", "latest/balzers"),
                   base.replace("test/balzers", "../balzers"), base.replace("test/balzers", "%2e%2e/balzers"),
                   base.replace("test/balzers", "test//balzers"), base.replace("github.com", "github.com.evil"),
                   base.replace("https", "HTTPS"), base.replace("balzers.zip", "balzers.ZIP"),
                   base.replace("test/balzers", "test/" + "x" * 256), base + "\n", base + " ",
                   "https://raw.githubusercontent.com/test.invalid/terrain/main/balzers.zip",
                   "https://raw.githubusercontent.com/test.invalid/terrain/" + "A" * 40 + "/balzers.zip",
                   "https://raw.githubusercontent.com/test.invalid/terrain/" + "a" * 40 + "/" + "a/" * 16 + "x.zip",
                   "file:///tmp/balzers.zip"]
        for url in invalid:
            with self.subTest(url=url), self.assertRaises(ValueError):
                recipe.validate_download_url(url)

    def test_cli_refuses_overwrite_and_invalid_url_before_input_read(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "out.zip"
            output.write_bytes(b"existing")
            with mock.patch.object(recipe, "read_pinned_source", side_effect=AssertionError("must not read")):
                with mock.patch("sys.stderr", new=io.StringIO()):
                    self.assertEqual(recipe.main(["missing.zip", str(output)]), 1)
                    self.assertEqual(recipe.main(["missing.zip", str(root / "new.zip"),
                                                 "--catalog", str(root / "catalog.json"),
                                                 "--download-url", "file:///x.zip"]), 1)
            self.assertEqual(output.read_bytes(), b"existing")
            self.assertFalse((root / "new.zip").exists())
            self.assertFalse((root / "catalog.json").exists())

    def test_cli_has_no_source_hash_bypass_and_requires_catalog_pair(self):
        for options in (["--source-sha256", "a" * 64], ["--download-url", "https://github.com/x/y"],
                        ["--catalog", "out.json"]):
            with mock.patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit):
                recipe.main(["in.zip", "out.zip", *options])

    def test_exclusive_write_preserves_existing_and_dangling_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "output"
            recipe.write_new(path, b"original")
            with self.assertRaises(FileExistsError):
                recipe.write_new(path, b"replacement")
            self.assertEqual(path.read_bytes(), b"original")
            link = Path(directory) / "dangling"
            try:
                link.symlink_to(Path(directory) / "absent")
            except OSError:
                return
            with self.assertRaises(FileExistsError):
                recipe.write_new(link, b"replacement")
            self.assertFalse((Path(directory) / "absent").exists())

    def test_shipped_checksum_ledger_matches_exact_sidecar_bytes(self):
        expected_names = {"Balzers_Terrain_Package_v1.zip", "manifest.json",
                          "reconstruction-provenance.md", "verification.json"}
        lines = (SAMPLE / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), len(expected_names))
        seen = set()
        for line in lines:
            checksum, name = line.split("  ", 1)
            self.assertIn(name, expected_names)
            self.assertNotIn(name, seen)
            seen.add(name)
            self.assertEqual(digest((SAMPLE / name).read_bytes()), checksum, name)
        self.assertEqual(seen, expected_names)

    def test_shipped_sample_matches_sidecars_and_known_answer(self):
        data = (SAMPLE / "Balzers_Terrain_Package_v1.zip").read_bytes()
        report = json.loads((SAMPLE / "verification.json").read_bytes())
        self.assertEqual(len(data), 1_610_134)
        self.assertEqual(digest(data), "d7e1265ee8015ad0fb1df23f7110b23124b90d889440f1b14a9cd85119f28528")
        self.assertEqual(digest(data), report["archive_sha256"])
        self.assertEqual(len(data), report["archive_size_bytes"])
        self.assertNotEqual(digest(data), recipe.HISTORICAL_SHA256)
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            manifest_bytes = archive.read("manifest.json")
            self.assertEqual(manifest_bytes, (SAMPLE / "manifest.json").read_bytes())
            self.assertEqual(digest(manifest_bytes), report["manifest_sha256"])
            self.assertEqual(archive.read(recipe.PROVENANCE_PATH), (SAMPLE / "reconstruction-provenance.md").read_bytes())
            manifest = json.loads(manifest_bytes)
            self.assertEqual(len(manifest["files"]), 174)
            actual_tiles = set()
            for record in manifest["files"]:
                value = archive.read(record["path"])
                self.assertEqual(digest(value), record["sha256"])
                self.assertEqual(len(value), record["size_bytes"])
                if record["kind"] == "terrain_dem":
                    parts = record["path"].split("/")
                    tile = (int(parts[1]), int(parts[2]), int(parts[3].removesuffix(".fsdem")))
                    actual_tiles.add(tile)
                    recipe.validate_tile(value, tile)
                else:
                    value.decode("utf-8")
            self.assertEqual(actual_tiles, recipe.tile_ids(recipe.ROOTS))
            for path, (size, expected) in recipe.SOURCE_DOCS.items():
                self.assertEqual(len(archive.read(path)), size)
                self.assertEqual(digest(archive.read(path)), expected)


if __name__ == "__main__":
    unittest.main()
