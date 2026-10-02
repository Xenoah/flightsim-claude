#!/usr/bin/env python3
"""Record immutable, local-only provenance for an existing OSM airport DB.

This is an integrity/lineage companion, not an importer, PBF parser, complete
FSAP validator, proof of a conversion, or permission to distribute data.
"""
from __future__ import annotations

import argparse
from datetime import date
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import sys
import tempfile
from urllib.parse import urlsplit

SCHEMA = "flightsim-osm-airport-provenance"
HEADER = struct.Struct("<4sHHIIQ")
DIRECTORY = struct.Struct("<HHIIIQQ")
MAX_RECORDS = 1_000_000
MAX_PAYLOAD = 96 * 1024 * 1024
MAX_STRINGS = 16 * 1024 * 1024
MAX_MANIFEST = 64 * 1024
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
MASK64 = (1 << 64) - 1
ATTRIBUTION = "Airport data: (c) OpenStreetMap contributors"
COPYRIGHT_URL = "https://www.openstreetmap.org/copyright"
LICENSE_URL = "https://opendatacommons.org/licenses/odbl/1-0/"
NOTICE = (
    f"{ATTRIBUTION}\n"
    f"OpenStreetMap data is available under the Open Database License (ODbL) 1.0.\n"
    f"Source attribution: {COPYRIGHT_URL}\n"
    f"License: {LICENSE_URL}\n"
    "This companion records user-supplied lineage and local file integrity only. "
    "It does not establish ownership, source accuracy, completed share-alike "
    "obligations, a distribution offer, or commercial release clearance."
)
VALIDATION = {
    "fsap_check": "header, framing, directory bounds and FNV-1a checksum only",
    "runtime_geometry_validated": False,
    "pbf_parsed": False,
}
DISTRIBUTION = {"status": "not_reviewed", "machine_readable_offer_url": None}


class ProvenanceError(ValueError):
    """A bounded validation error safe to show at the command line."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ProvenanceError(message)


def clean_text(value: object, label: str, maximum: int = 200) -> str:
    require(isinstance(value, str), f"{label} must be text")
    require(bool(value) and value == value.strip() and len(value) <= maximum,
            f"{label} must be nonempty, trimmed and at most {maximum} characters")
    require(not any(ord(c) < 32 or ord(c) == 127 for c in value),
            f"{label} cannot contain control characters")
    return value


def snapshot_date(value: object) -> str:
    value = clean_text(value, "snapshot date", 10)
    require(re.fullmatch(r"\d{4}-\d{2}-\d{2}", value) is not None,
            "snapshot date must be YYYY-MM-DD")
    try:
        date.fromisoformat(value)
    except ValueError as error:
        raise ProvenanceError("snapshot date is not a calendar date") from error
    return value


def source_url(value: object) -> str | None:
    if value is None:
        return None
    value = clean_text(value, "source URL", 2048)
    require(not any(c.isspace() for c in value) and "\\" not in value,
            "source URL cannot contain whitespace or backslashes")
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError as error:
        raise ProvenanceError("source URL is malformed") from error
    require(parsed.scheme == "https" and bool(parsed.hostname),
            "source URL must be an absolute HTTPS URL, or omitted when unknown")
    require(parsed.username is None and parsed.password is None,
            "source URL cannot contain credentials")
    require(not parsed.query and not parsed.fragment,
            "source URL must not contain a query or fragment; use a public source page")
    require(port is None or 0 < port <= 65535, "source URL has an invalid port")
    return value


def safe_filename(value: object) -> str:
    value = clean_text(value, "filename", 255)
    require(value not in (".", "..") and not any(c in value for c in "/\\:"),
            "recorded filenames must be basenames, not paths")
    return value


def file_identity(path: Path) -> tuple[int, int, int, int]:
    info = path.stat()
    require(stat.S_ISREG(info.st_mode), f"input is not a regular file: {path}")
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def fingerprint(path: Path, *, fsap: bool = False) -> tuple[dict, bytes, int]:
    before = file_identity(path)
    require(before[2] > 0, f"input is empty: {path}")
    if fsap:
        require(HEADER.size <= before[2] <= HEADER.size + MAX_PAYLOAD,
                "FSAP file is truncated or exceeds the 96 MiB payload limit")
    sha = hashlib.sha256()
    prefix = bytearray()
    count = 0
    fnv = FNV_OFFSET
    with path.open("rb") as stream:
        opened = os.fstat(stream.fileno())
        require((opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns) == before,
                f"input changed before reading: {path}")
        while block := stream.read(1024 * 1024):
            sha.update(block)
            if fsap:
                prefix.extend(block[:max(0, HEADER.size + 16 * DIRECTORY.size - len(prefix))])
                for value in block[max(0, HEADER.size - count):]:
                    fnv = ((fnv ^ value) * FNV_PRIME) & MASK64
            count += len(block)
            if fsap:
                require(count <= HEADER.size + MAX_PAYLOAD, "FSAP grew beyond its size limit")
        after = os.fstat(stream.fileno())
    require((after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) == before
            and file_identity(path) == before and count == before[2],
            f"input changed while being read: {path}")
    return {"filename": safe_filename(path.name), "bytes": count, "sha256": sha.hexdigest()}, bytes(prefix), fnv


def inspect_fsap(path: Path) -> dict:
    facts, prefix, checksum = fingerprint(path, fsap=True)
    magic, version, flags, count, size, expected_checksum = HEADER.unpack(prefix[:HEADER.size])
    require(magic == b"FSAP", "not an FSAP database")
    require(version in (1, 2, 3), "unsupported FSAP version")
    require(flags == 0, "unsupported FSAP header flags")
    if version in (1, 2):
        require(size == (48 if version == 1 else 64), "incorrect FSAP record size")
        require(count <= MAX_RECORDS, "FSAP record count exceeds its limit")
        require(facts["bytes"] == HEADER.size + count * size, "FSAP length does not match its header")
    else:
        require(1 <= count <= 16 and size == DIRECTORY.size, "invalid FSAP v3 directory header")
        require(len(prefix) >= HEADER.size + count * size, "truncated FSAP v3 directory")
        offset, previous, records, has_core = count * size, -1, 0, False
        sizes = {1: 64, 2: 64, 3: 64, 4: 40, 5: 24, 6: 8, 7: 1}
        for index in range(count):
            start = HEADER.size + index * size
            kind, schema, section_flags, record_size, entries, start_offset, byte_len = DIRECTORY.unpack(prefix[start:start + size])
            require(kind > previous, "FSAP v3 sections are not strictly ordered")
            previous = kind
            require(section_flags in (0, 1), "FSAP v3 section has reserved flags")
            if kind in sizes:
                require(schema == 1 and record_size == sizes[kind]
                        and section_flags == (0 if kind == 1 else 1), "invalid known FSAP v3 section")
            else:
                require(section_flags == 1, "unknown required FSAP v3 section")
            require(record_size > 0 and byte_len == entries * record_size,
                    "FSAP v3 section length does not match its records")
            require(start_offset == offset, "FSAP v3 sections overlap or have gaps")
            offset += byte_len
            require(offset <= MAX_PAYLOAD, "FSAP v3 payload exceeds its limit")
            if kind == 7:
                require(byte_len <= MAX_STRINGS, "FSAP v3 string data exceeds its limit")
            else:
                records += entries
                require(records <= MAX_RECORDS, "FSAP v3 record count exceeds its limit")
            has_core |= kind == 1
        require(has_core, "FSAP v3 core section is missing")
        require(facts["bytes"] == HEADER.size + offset, "FSAP v3 length does not match its directory")
    require(checksum == expected_checksum, "FSAP payload checksum mismatch")
    facts["fsap_version"] = version
    return facts


def distinct_paths(*paths: Path) -> None:
    try:
        resolved = [path.resolve() for path in paths]
    except RuntimeError as error:
        raise ProvenanceError("input/output path contains a symlink loop") from error
    require(len(set(resolved)) == len(paths), "input and output paths must be distinct")
    for index, path in enumerate(paths):
        for other in paths[:index]:
            if path.exists() and other.exists():
                require(not os.path.samefile(path, other), "input and output files must not be hard-link aliases")


def make_manifest(source: dict, database: dict, converter_id: str) -> dict:
    return {
        "schema": SCHEMA,
        "schema_version": 1,
        "source": source,
        "database": database,
        "conversion": {
            "converter_id": clean_text(converter_id, "converter ID"),
            "relationship": "user-attested; file hashes do not prove this conversion",
            "snapshot_date_basis": "user-supplied; not checked against PBF contents",
        },
        "validation": VALIDATION,
        "licensing": {"attribution": ATTRIBUTION, "copyright_url": COPYRIGHT_URL,
                      "data_license": "ODbL-1.0", "license_url": LICENSE_URL},
        "distribution": DISTRIBUTION,
        "notice": NOTICE,
    }


def publish_new_json(path: Path, value: dict) -> None:
    """Publish one complete file atomically, without replacing any existing path."""
    require(not os.path.lexists(path), f"companion already exists; refusing to overwrite: {path}")
    payload = (json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n").encode()
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", dir=path.parent,
                                         prefix=f".{path.name}.", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        # Same-directory hard-link publication is atomic and fails if the target
        # already exists (including a symlink). No overwrite fallback is allowed.
        os.link(temporary, path, follow_symlinks=False)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def record(database: Path, input_pbf: Path, output: Path, snapshot: str,
           converter_id: str, url: str | None) -> None:
    distinct_paths(database, input_pbf, output)
    require(not os.path.lexists(output), f"companion already exists; refusing to overwrite: {output}")
    require(database.name.endswith(".fsairports"), "database must have the .fsairports extension")
    require(input_pbf.name.endswith(".osm.pbf"), "source must have the .osm.pbf extension")
    snapshot, url = snapshot_date(snapshot), source_url(url)
    converter_id = clean_text(converter_id, "converter ID")
    source, _, _ = fingerprint(input_pbf)
    source.update({"snapshot_date": snapshot, "url": url})
    database_facts = inspect_fsap(database)
    publish_new_json(output, make_manifest(source, database_facts, converter_id))


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate manifest field: {key}")
        result[key] = value
    return result


def check_facts(value: object, *, database: bool = False) -> dict:
    require(isinstance(value, dict), "manifest file record must be an object")
    keys = {"filename", "bytes", "sha256", "fsap_version"} if database else {"filename", "bytes", "sha256", "snapshot_date", "url"}
    require(set(value) == keys, "manifest file record has missing or unknown fields")
    safe_filename(value["filename"])
    require(type(value["bytes"]) is int and value["bytes"] > 0, "manifest byte count is invalid")
    require(isinstance(value["sha256"], str) and re.fullmatch("[0-9a-f]{64}", value["sha256"]) is not None,
            "manifest SHA-256 is invalid")
    if database:
        require(type(value["fsap_version"]) is int and value["fsap_version"] in (1, 2, 3), "manifest FSAP version is invalid")
    else:
        snapshot_date(value["snapshot_date"])
        source_url(value["url"])
    return value


def verify(manifest_path: Path, database: Path, input_pbf: Path | None) -> None:
    distinct_paths(manifest_path, database, *([input_pbf] if input_pbf else []))
    require(file_identity(manifest_path)[2] <= MAX_MANIFEST, "manifest exceeds 64 KiB limit")
    with manifest_path.open("rb") as stream:
        raw = stream.read(MAX_MANIFEST + 1)
    require(len(raw) <= MAX_MANIFEST, "manifest exceeds 64 KiB limit")
    try:
        manifest = json.loads(raw, object_pairs_hook=unique_object)
    except ProvenanceError:
        # Preserve deliberate duplicate-field rejection from the object hook.
        raise
    except (ValueError, RecursionError) as error:
        # Only the parser boundary: JSON syntax/encoding, Python's integer digit
        # limit, and excessive nesting are malformed input, not CLI tracebacks.
        raise ProvenanceError(f"invalid provenance JSON: {error}") from error
    try:
        require(isinstance(manifest, dict), "manifest must be an object")
        source = check_facts(manifest["source"])
        stored_db = check_facts(manifest["database"], database=True)
        expected = make_manifest(source, stored_db, manifest["conversion"]["converter_id"])
        require(json.dumps(manifest, sort_keys=True) == json.dumps(expected, sort_keys=True),
                "manifest schema, notices or review status have changed")
    except (KeyError, TypeError) as error:
        raise ProvenanceError(f"invalid provenance manifest: {error}") from error
    actual_db = inspect_fsap(database)
    actual_db["filename"] = stored_db["filename"]  # Relocation/renaming is permitted.
    require(actual_db == stored_db, "database does not match the recorded digest, size or FSAP version")
    if input_pbf:
        actual_source, _, _ = fingerprint(input_pbf)
        require(all(actual_source[key] == source[key] for key in ("bytes", "sha256")),
                "source PBF does not match the recorded digest or size")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("record", help="create a new immutable provenance companion")
    create.add_argument("--database", type=Path, required=True)
    create.add_argument("--input-pbf", type=Path, required=True)
    create.add_argument("--snapshot-date", required=True, help="known source snapshot date, YYYY-MM-DD; not today's date by default")
    create.add_argument("--converter-id", required=True, help="actual converter version/revision used, not the current checkout by assumption")
    create.add_argument("--source-url", help="public HTTPS source/extract page; omit if unknown; never fetched")
    create.add_argument("--output", type=Path, help="new companion path; defaults to <database>.provenance.json")
    check = commands.add_parser("verify", help="verify recorded local file integrity without changing files")
    check.add_argument("--manifest", type=Path, required=True)
    check.add_argument("--database", type=Path, required=True)
    check.add_argument("--input-pbf", type=Path, help="optional original PBF; otherwise its digest is not rechecked")
    args = parser.parse_args(argv)
    try:
        if args.command == "record":
            output = args.output or Path(f"{args.database}.provenance.json")
            record(args.database, args.input_pbf, output, args.snapshot_date, args.converter_id, args.source_url)
            print(f"Recorded local provenance: {output}")
            print("Source lineage is user-attested. Distribution status: not reviewed.")
        else:
            verify(args.manifest, args.database, args.input_pbf)
            print("Database integrity verified; FSAP geometry and source lineage are not certified.")
            print("Source PBF digest verified." if args.input_pbf else "Source PBF digest NOT rechecked (no --input-pbf supplied).")
            print("Distribution status: not reviewed.")
    except (OSError, ProvenanceError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
