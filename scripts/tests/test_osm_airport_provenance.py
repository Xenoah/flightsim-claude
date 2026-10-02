"""Offline tests: existing independently generated FSAP fixture, no OSM downloads."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("osm_provenance", ROOT / "scripts/record-osm-airport-provenance.py")
provenance = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(provenance)
FIXTURE = ROOT / "crates/flightsim-tilegen/tests/fixtures/airport-valid.fsairports"


def fnv(payload):
    value = 0xCBF29CE484222325
    for byte in payload:
        value = ((value ^ byte) * 0x100000001B3) & ((1 << 64) - 1)
    return value


def rechecksum(data):
    data = bytearray(data)
    struct.pack_into("<Q", data, 16, fnv(data[24:]))
    return bytes(data)


def frame(version, payload, count, size):
    return struct.pack("<4sHHIIQ", b"FSAP", version, 0, count, size, fnv(payload)) + payload


class ProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.db = self.root / "region.fsairports"
        shutil.copyfile(FIXTURE, self.db)
        self.source = self.root / "region.osm.pbf"
        # PBF bytes are opaque to the helper; the independent Rust fixture is
        # exercised for real FSAP framing. Never claim this string is a PBF.
        self.source.write_bytes(b"opaque local input; provenance tool must not parse PBF")
        self.output = self.root / "region.fsairports.provenance.json"

    def tearDown(self):
        self.directory.cleanup()

    def record(self, **overrides):
        arguments = dict(database=self.db, input_pbf=self.source, output=self.output,
                         snapshot="2026-09-30", converter_id="flightsim-airportgen@2d2295b",
                         url="https://example.org/extracts/region.osm.pbf")
        arguments.update(overrides)
        provenance.record(**arguments)

    def test_real_synthetic_v3_fixture_passes_header_directory_and_checksum(self):
        info = provenance.inspect_fsap(self.db)
        self.assertEqual(info["bytes"], 400)
        self.assertEqual(info["fsap_version"], 3)
        self.assertEqual(info["sha256"], "199260d950dcab5d0cd24e845806e4c9dee6e65f4c222ead02d1b35d231ee135")

    def test_legacy_v1_and_v2_supported(self):
        runway = struct.pack("<qddddd", 10, 35.0, 139.0, 35.01, 139.0, 30.0)
        v2 = struct.pack("<B7xqII", 0, 10, 0, 0) + runway[8:]
        for version, payload, size in ((1, runway, 48), (2, v2, 64)):
            with self.subTest(version=version):
                self.db.write_bytes(frame(version, payload, 1, size))
                self.assertEqual(provenance.inspect_fsap(self.db)["fsap_version"], version)

    def test_hash_and_checksum_cross_the_streaming_chunk_boundary(self):
        runway = struct.pack("<qddddd", 10, 35.0, 139.0, 35.01, 139.0, 30.0)
        payload = runway * 22_000
        self.db.write_bytes(frame(1, payload, 22_000, 48))
        self.assertEqual(provenance.inspect_fsap(self.db)["bytes"], 1_056_024)

    def test_unknown_optional_v3_section_is_allowed_like_the_rust_reader(self):
        payload = (struct.pack("<HHIIIQQ", 1, 1, 0, 64, 0, 64, 0)
                   + struct.pack("<HHIIIQQ", 42, 9, 1, 1, 2, 64, 2) + b"ok")
        self.db.write_bytes(frame(3, payload, 2, 32))
        self.assertEqual(provenance.inspect_fsap(self.db)["fsap_version"], 3)

    def test_missing_core_v3_section_is_rejected(self):
        payload = struct.pack("<HHIIIQQ", 42, 9, 1, 1, 2, 32, 2) + b"ok"
        self.db.write_bytes(frame(3, payload, 1, 32))
        with self.assertRaisesRegex(provenance.ProvenanceError, "core section is missing"):
            provenance.inspect_fsap(self.db)

    def test_record_and_verify_never_assert_release_readiness(self):
        original = self.db.read_bytes(), self.source.read_bytes()
        self.record()
        value = json.loads(self.output.read_text())
        self.assertEqual(value["distribution"]["status"], "not_reviewed")
        self.assertIsNone(value["distribution"]["machine_readable_offer_url"])
        self.assertFalse(value["validation"]["runtime_geometry_validated"])
        self.assertFalse(value["validation"]["pbf_parsed"])
        self.assertIn("user-attested", value["conversion"]["relationship"])
        self.assertIn("OpenStreetMap contributors", value["notice"])
        self.assertEqual(value["source"]["snapshot_date"], "2026-09-30")
        self.assertEqual(value["source"]["filename"], self.source.name)
        self.assertNotIn(str(self.root), self.output.read_text())
        provenance.verify(self.output, self.db, self.source)
        self.assertEqual(original, (self.db.read_bytes(), self.source.read_bytes()))

    def test_omitted_source_url_is_null_and_never_invented(self):
        self.record(url=None)
        self.assertIsNone(json.loads(self.output.read_text())["source"]["url"])
        provenance.verify(self.output, self.db, None)

    def test_relocated_and_renamed_files_can_be_verified(self):
        self.record()
        other = self.root / "renamed.bin"
        shutil.copyfile(self.db, other)
        provenance.verify(self.output, other, self.source)

    def test_no_overwrite_of_existing_companion(self):
        self.output.write_bytes(b"existing important data")
        with self.assertRaisesRegex(provenance.ProvenanceError, "refusing to overwrite"):
            self.record()
        self.assertEqual(self.output.read_bytes(), b"existing important data")

    def test_no_overwrite_even_when_target_is_created_during_publication(self):
        real_link = os.link

        def raced_link(source, destination, **kwargs):
            Path(destination).write_bytes(b"other publisher")
            return real_link(source, destination, **kwargs)

        with mock.patch.object(provenance.os, "link", side_effect=raced_link):
            with self.assertRaises(FileExistsError):
                self.record()
        self.assertEqual(self.output.read_bytes(), b"other publisher")
        self.assertEqual(sorted(p.name for p in self.root.iterdir()),
                         sorted([self.db.name, self.source.name, self.output.name]))

    def test_publication_failure_leaves_inputs_untouched_and_no_partial_manifest(self):
        before = self.db.read_bytes(), self.source.read_bytes()
        with mock.patch.object(provenance.os, "link", side_effect=OSError("hard links unavailable")):
            with self.assertRaisesRegex(OSError, "hard links unavailable"):
                self.record()
        self.assertFalse(self.output.exists())
        self.assertEqual(before, (self.db.read_bytes(), self.source.read_bytes()))
        self.assertEqual(len(list(self.root.iterdir())), 2)

    def test_flush_failure_does_not_publish_a_partial_companion(self):
        with mock.patch.object(provenance.os, "fsync", side_effect=OSError("disk full")):
            with self.assertRaisesRegex(OSError, "disk full"):
                self.record()
        self.assertFalse(self.output.exists())
        self.assertEqual(len(list(self.root.iterdir())), 2)

    def test_identical_resolved_and_hardlinked_inputs_are_rejected(self):
        for output in (self.db, self.source, self.root / ".." / self.root.name / self.db.name):
            with self.subTest(path=output), self.assertRaises(provenance.ProvenanceError):
                self.record(output=output)
        alias = self.root / "alias.osm.pbf"
        os.link(self.db, alias)
        with self.assertRaisesRegex(provenance.ProvenanceError, "hard-link"):
            self.record(input_pbf=alias)

    def test_existing_symlink_output_is_preserved(self):
        self.output.symlink_to(self.source)
        before = self.source.read_bytes()
        with self.assertRaises(provenance.ProvenanceError):
            self.record()
        self.assertTrue(self.output.is_symlink())
        self.assertEqual(self.source.read_bytes(), before)

    def test_dangling_symlink_and_symlink_loop_outputs_are_rejected(self):
        self.output.symlink_to(self.root / "missing")
        with self.assertRaises(provenance.ProvenanceError):
            self.record()
        self.assertTrue(self.output.is_symlink())
        self.output.unlink()
        self.output.symlink_to(self.output)
        with self.assertRaises((OSError, provenance.ProvenanceError)):
            self.record()
        self.assertTrue(self.output.is_symlink())

    def test_empty_missing_directory_and_wrong_suffix_inputs_fail(self):
        empty = self.root / "empty.osm.pbf"
        empty.touch()
        for source in (empty, self.root / "missing.osm.pbf", self.root, self.db):
            with self.subTest(source=source), self.assertRaises((OSError, provenance.ProvenanceError)):
                self.record(input_pbf=source)
        self.assertFalse(self.output.exists())

    def test_non_regular_input_is_rejected_before_open(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("POSIX FIFO test")
        fifo = self.root / "fifo.osm.pbf"
        os.mkfifo(fifo)
        with self.assertRaisesRegex(provenance.ProvenanceError, "regular file"):
            self.record(input_pbf=fifo)

    def test_dates_validate_calendar_and_exact_format(self):
        self.assertEqual(provenance.snapshot_date("2024-02-29"), "2024-02-29")
        for value in ("", "2026-02-29", "2026-2-01", "2026-13-01", " 2026-01-01", "2026-01-01T00:00:00", None):
            with self.subTest(value=value), self.assertRaises(provenance.ProvenanceError):
                provenance.snapshot_date(value)

    def test_source_url_validation_rejects_credentials_and_private_query_strings(self):
        for value in ("", "not-a-url", "http://example.org/a", "file:///data/local.pbf",
                      "https://", "https://name:secret@example.org/a", "https://example.org/a?token=secret",
                      "https://example.org/a#anchor", "https://example.org/a b", "https://example.org\\a",
                      "https://example.org:invalid/a", "https://example.org:0/a", "https://[broken"):
            with self.subTest(value=value), self.assertRaises(provenance.ProvenanceError):
                provenance.source_url(value)
        self.assertIsNone(provenance.source_url(None))

    def test_converter_identity_must_be_explicit_clean_text(self):
        for value in ("", " ", " tool", "tool\nversion", "x" * 201, None):
            with self.subTest(value=value), self.assertRaises(provenance.ProvenanceError):
                self.record(converter_id=value)
        self.assertFalse(self.output.exists())

    def test_header_and_framing_corruptions_rejected(self):
        valid = self.db.read_bytes()
        corruptions = [b"", valid[:23], valid[:-1], valid + b"\0", b"NOPE" + valid[4:]]
        for offset, code, value in ((4, "H", 4), (6, "H", 1), (8, "I", 17), (12, "I", 64), (16, "Q", 0)):
            changed = bytearray(valid)
            struct.pack_into("<" + code, changed, offset, value)
            corruptions.append(bytes(changed))
        for number, bad in enumerate(corruptions):
            with self.subTest(number=number):
                self.db.write_bytes(bad)
                with self.assertRaises(provenance.ProvenanceError):
                    provenance.inspect_fsap(self.db)

    def test_v3_directory_corruptions_rejected_with_valid_checksums(self):
        valid = self.db.read_bytes()
        fields = [(24, "H", 0), (26, "H", 2), (28, "I", 1), (32, "I", 48),
                  (36, "I", 1_000_001), (40, "Q", 225), (48, "Q", 127),
                  (56, "H", 1), (60, "I", 3), (72, "Q", 0)]
        for offset, code, value in fields:
            with self.subTest(offset=offset):
                changed = bytearray(valid)
                struct.pack_into("<" + code, changed, offset, value)
                self.db.write_bytes(rechecksum(changed))
                with self.assertRaises(provenance.ProvenanceError):
                    provenance.inspect_fsap(self.db)

    def test_oversized_database_rejected_before_hashing(self):
        with self.db.open("wb") as stream:
            stream.truncate(provenance.MAX_PAYLOAD + 25)
        with self.assertRaisesRegex(provenance.ProvenanceError, "96 MiB"):
            provenance.inspect_fsap(self.db)

    def test_mutated_input_is_detected_during_fingerprinting(self):
        identity = provenance.file_identity(self.source)
        changed = identity[:-1] + (identity[-1] + 1,)
        with mock.patch.object(provenance, "file_identity", side_effect=[identity, changed]):
            with self.assertRaisesRegex(provenance.ProvenanceError, "changed while"):
                provenance.fingerprint(self.source)

    def test_verification_detects_source_and_database_changes(self):
        self.record()
        self.source.write_bytes(b"changed source")
        with self.assertRaisesRegex(provenance.ProvenanceError, "source PBF does not match"):
            provenance.verify(self.output, self.db, self.source)
        # A checksummed file change still must fail the SHA-256 comparison.
        changed = bytearray(self.db.read_bytes())
        changed[-1] ^= 1
        self.db.write_bytes(rechecksum(changed))
        with self.assertRaisesRegex(provenance.ProvenanceError, "database does not match"):
            provenance.verify(self.output, self.db, None)

    def test_invalid_or_upgraded_review_status_in_manifest_fails(self):
        self.record()
        original = self.output.read_text()
        for content in ("{}", "[]", "{broken", original.replace('"not_reviewed"', '"cleared"'),
                        original.replace('"schema_version": 1', '"schema_version": 999'),
                        original.replace('"schema_version": 1', '"schema_version": true'),
                        original.replace('"runtime_geometry_validated": false', '"runtime_geometry_validated": 0'),
                        original.replace('"snapshot_date": "2026-09-30"', '"snapshot_date": "2026-09-31"'),
                        original.replace('"source": {', '"source": {"unexpected": true,'),
                        original.replace('"schema_version": 1', '"schema_version": 1, "schema_version": 1')):
            with self.subTest(content=content[:60]):
                self.output.write_text(content)
                with self.assertRaises(provenance.ProvenanceError):
                    provenance.verify(self.output, self.db, None)

    def test_recorded_digest_tampering_is_detected(self):
        self.record()
        value = json.loads(self.output.read_text())
        value["database"]["sha256"] = "0" * 64
        self.output.write_text(json.dumps(value))
        with self.assertRaisesRegex(provenance.ProvenanceError, "database does not match"):
            provenance.verify(self.output, self.db, None)

    def test_oversized_manifest_is_rejected(self):
        self.output.write_bytes(b" " * (provenance.MAX_MANIFEST + 1))
        with self.assertRaisesRegex(provenance.ProvenanceError, "64 KiB"):
            provenance.verify(self.output, self.db, None)

    def test_parser_integer_limit_and_excessive_nesting_return_cli_errors(self):
        documents = (b'{"schema_version":' + b"1" * 4301 + b"}",
                     b"[" * 10_000 + b"0" + b"]" * 10_000)
        for content in documents:
            with self.subTest(bytes=len(content)):
                self.assertLess(len(content), provenance.MAX_MANIFEST)
                self.output.write_bytes(content)
                before = self.db.read_bytes(), self.source.read_bytes(), self.output.read_bytes()
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    result = provenance.main(["verify", "--manifest", str(self.output),
                                              "--database", str(self.db)])
                self.assertEqual(result, 1)
                self.assertTrue(stderr.getvalue().startswith("error:"))
                self.assertNotIn("Traceback", stderr.getvalue())
                self.assertEqual(before, (self.db.read_bytes(), self.source.read_bytes(),
                                          self.output.read_bytes()))

    def test_parser_value_error_handler_does_not_hide_unrelated_implementation_errors(self):
        self.record()
        with mock.patch.object(provenance, "make_manifest", side_effect=ValueError("implementation bug")):
            with self.assertRaisesRegex(ValueError, "implementation bug"):
                provenance.verify(self.output, self.db, None)

    def test_cli_reports_partial_verification_and_nonzero_failures(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(provenance.main(["record", "--database", str(self.db), "--input-pbf", str(self.source),
                                             "--snapshot-date", "2026-09-30", "--converter-id", "test"]), 0)
            self.assertEqual(provenance.main(["verify", "--manifest", str(self.output), "--database", str(self.db)]), 0)
        self.assertIn("NOT rechecked", output.getvalue())
        self.assertIn("not reviewed", output.getvalue())
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(provenance.main(["verify", "--manifest", str(self.root / "missing"), "--database", str(self.db)]), 1)


if __name__ == "__main__":
    unittest.main()
