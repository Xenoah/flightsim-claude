#!/usr/bin/env python3
"""Rebuild a fixed, local-only Balzers DEM package from the recovered source ZIP.

This is deliberately not an arbitrary ZIP converter. The source size and whole
archive SHA-256 are pinned before ZIP parsing. No network calls or publication.
"""
import argparse
import copy
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import stat
import struct
import sys
import zipfile

SOURCE_BYTES = 6_551_156
SOURCE_SHA256 = "fc4de5f479d769b0aa6f028ab965a8798f9c11bdbae157bffc57ecd45df66b2a"
SOURCE_MANIFEST_SHA256 = "34e8759340c315c745a9d83746944d082bb94b8139e2806756749572b7d53384"
SOURCE_MANIFEST_BYTES = 175_807
SOURCE_ROOTS = tuple((10, x, y) for x in range(1077, 1080) for y in range(242, 245))
ROOTS = ((10, 1077, 244), (10, 1078, 244))
MAX_LEVEL = 13
TILE_BYTES = 56 + 2 * 65 * 65
SOURCE_DOCS = {
    "docs/notice.txt": (1490, "3c6ea356f260ba6243f275a2064d1f12132951eb64e1b0a8f080b412cfeb82a1"),
    "docs/copernicus-license-bundle.txt": (85124, "2fb23c33ab080faa0dc21c575c327005626b6a07af1d597994fb566129d6ab26"),
    "docs/source-provenance.md": (11167, "67ee3f27c2c5caa18a3b09f8f5d41c20089a60344104addba95ec7bd739cd19c"),
}
PACKAGE_ID = "balzers-glo90-rebuilt"
PACKAGE_VERSION = "1.0.0"
PACKAGE_TITLE = "Balzers vicinity GLO-90 terrain (rebuilt)"
FIXED_DATE = (2026, 10, 7, 0, 0, 0)
PROVENANCE_PATH = "docs/balzers-reconstruction.md"
HISTORICAL_SHA256 = "d9ec9ef06dbc9ad1d878a36dddd395b7627cde7d681093c8cb31835dcce93cc6"
HISTORICAL_BYTES = 1_461_931
CHUNK = 32 * 1024


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def strict_json(data):
    def reject_constant(value):
        raise ValueError(f"nonfinite JSON constant: {value}")
    return json.loads(data.decode("utf-8"), object_pairs_hook=unique_object,
                      parse_constant=reject_constant)


def tile_ids(roots):
    """Complete geographic-quadtree descendants; never trim sibling families."""
    return {(level, x, y)
            for root_level, root_x, root_y in roots
            for level in range(root_level, MAX_LEVEL + 1)
            for x in range(root_x << (level - root_level), (root_x + 1) << (level - root_level))
            for y in range(root_y << (level - root_level), (root_y + 1) << (level - root_level))}


def tile_path(tile):
    return "terrain/{}/{}/{}.fsdem".format(*tile)


def tile_bounds(tile):
    # Offline degree-valued index bounds, matching world::tile (not ECEF math).
    level, x, y = tile
    step = 180.0 / (1 << level)
    return {"west": -180.0 + x * step, "south": 90.0 - (y + 1) * step,
            "east": -180.0 + (x + 1) * step, "north": 90.0 - y * step}


def bounds_for(roots):
    bounds = [tile_bounds(tile) for tile in roots]
    return {key: (min if key in ("west", "south") else max)(b[key] for b in bounds)
            for key in ("west", "south", "east", "north")}


def read_pinned_source(path):
    """Read one bounded regular-file snapshot; authenticate before ZIP parsing."""
    require(stat.S_ISREG(path.lstat().st_mode), "source must be a regular non-symlink file")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    with os.fdopen(os.open(path, flags), "rb") as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode), "source must be a regular file")
        require(info.st_size == SOURCE_BYTES, "source archive size differs from recovered input")
        chunks, count = [], 0
        while True:
            chunk = source.read(min(CHUNK, SOURCE_BYTES + 1 - count))
            if not chunk:
                break
            chunks.append(chunk)
            count += len(chunk)
            require(count <= SOURCE_BYTES, "source grew beyond pinned size")
        data = b"".join(chunks)
    require(len(data) == SOURCE_BYTES, "source truncated during snapshot")
    require(sha256(data) == SOURCE_SHA256, "source archive SHA-256 differs from recovered input")
    return data


def validate_tile(data, tile):
    """Independent ADR-0005 checks; original bytes are never decoded/re-encoded."""
    require(len(data) == TILE_BYTES, "DEM encoded size must match fixed 65 x 65 grid")
    magic, version, level, flags, x, y, width, height, offset, scale, error, checksum = \
        struct.unpack("<4sHBBIIIIdddQ", data[:56])
    require(magic == b"FSDM" and version == 1 and flags == 0, "unsupported FSDM header")
    require((level, x, y) == tile, "DEM tile ID does not match its canonical path")
    require(width == height == 65, "DEM grid differs from fixed 65 x 65 input")
    require(all(math.isfinite(v) for v in (offset, scale, error)), "nonfinite FSDM header")
    require(scale >= 0 and error >= 0, "negative FSDM scale/error")
    payload = data[56:]
    actual = 0xcbf29ce484222325
    for byte in payload:
        actual = ((actual ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    require(actual == checksum, "FSDM payload checksum mismatch")
    values = struct.unpack("<4225H", payload)
    # Runtime stores decoded samples as f32. Match that boundary for range/error.
    low, high = (offset + q * scale for q in (min(values), max(values)))
    require(math.isfinite(low) and math.isfinite(high) and -12000 <= low <= high <= 100000,
            "FSDM elevation outside supported finite range")
    low, high = (struct.unpack("<f", struct.pack("<f", v))[0] for v in (low, high))
    require(error <= high - low + 1e-6, "FSDM geometric error exceeds decoded range")


def validate_source_manifest(manifest):
    require(isinstance(manifest, dict) and set(manifest) == {
        "schema_version", "id", "version", "title", "content_kinds", "terrain", "sources", "files"
    }, "unexpected manifest fields")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1,
            "unexpected source schema")
    require(manifest["id"] == "liechtenstein-glo90" and manifest["version"] == "1.0.0",
            "unexpected source identity")
    require(manifest["content_kinds"] == ["terrain_dem"], "source is not DEM-only")
    require(manifest["terrain"] == {"bounds_degrees": bounds_for(SOURCE_ROOTS),
            "nominal_resolution_m": 90.0, "datum": "EPSG:4979"}, "unexpected source terrain metadata")
    require(isinstance(manifest["sources"], list) and len(manifest["sources"]) == 1,
            "unexpected source records")
    source = manifest["sources"][0]
    require(isinstance(source, dict) and set(source) == {
        "id", "url", "revision", "provenance", "credits", "license"
    } and source["id"] == "regional-dem", "unexpected source record fields")
    require(isinstance(source["license"], dict) and set(source["license"]) == {
        "name", "url", "text_path"
    } and source["license"]["text_path"] == "docs/copernicus-license-bundle.txt",
            "original license reference missing")
    require(isinstance(manifest["files"], list) and len(manifest["files"]) == 768,
            "source must declare all 768 payloads")
    expected_tiles = {tile_path(tile) for tile in tile_ids(SOURCE_ROOTS)}
    records = {}
    for record in manifest["files"]:
        require(isinstance(record, dict) and set(record) == {
            "path", "kind", "size_bytes", "sha256", "source"
        }, "unexpected file record fields")
        path = record["path"]
        require(isinstance(path, str) and path not in records, "duplicate/invalid payload path")
        require(path in expected_tiles or path in SOURCE_DOCS, "unexpected payload path")
        require(record["kind"] == ("terrain_dem" if path in expected_tiles else "documentation"),
                "unexpected payload kind")
        expected_size = TILE_BYTES if path in expected_tiles else SOURCE_DOCS[path][0]
        require(type(record["size_bytes"]) is int and record["size_bytes"] == expected_size,
                "unexpected payload size")
        require(isinstance(record["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", record["sha256"]),
                "invalid payload SHA-256")
        require(record["source"] == "regional-dem", "unexpected payload source")
        if path in SOURCE_DOCS:
            require(record["sha256"] == SOURCE_DOCS[path][1], "original documentation identity changed")
        records[path] = record
    require(set(records) == expected_tiles | set(SOURCE_DOCS), "incomplete source tile families")
    return records


def inspect_source(data):
    # Repeat pins for callers of this function, not only the CLI file reader.
    require(len(data) == SOURCE_BYTES and sha256(data) == SOURCE_SHA256,
            "source archive differs from pinned recovered input")
    require(data[:4] == b"PK\x03\x04" and data[-22:-18] == b"PK\x05\x06",
            "expected uncommented ZIP32 envelope")
    disk, central_disk, disk_entries, entries, size, start, comment = struct.unpack("<4H2IH", data[-18:])
    require(disk == central_disk == comment == 0 and disk_entries == entries == 769
            and 0 < size <= 256 * 1024 and start + size == len(data) - 22,
            "unexpected ZIP32 central directory")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        require(len(infos) == 769 and len(set(names)) == len(names), "duplicate/extra ZIP members")
        # No untrusted extraction. Only allow exact known root-relative names.
        expected_names = {tile_path(t) for t in tile_ids(SOURCE_ROOTS)} | set(SOURCE_DOCS) | {"manifest.json"}
        require(set(names) == expected_names, "archive member set differs from fixed source")
        for info in infos:
            require(info.flag_bits == 0 and info.compress_type == zipfile.ZIP_DEFLATED
                    and info.create_system == 3 and stat.S_ISREG(info.external_attr >> 16)
                    and not (info.extra or info.comment or info.is_dir()), "unsupported ZIP member metadata")
            expected_size = (SOURCE_MANIFEST_BYTES if info.filename == "manifest.json"
                             else SOURCE_DOCS[info.filename][0] if info.filename in SOURCE_DOCS else TILE_BYTES)
            require(info.file_size == expected_size and 0 < info.compress_size <= SOURCE_BYTES
                    and info.file_size <= info.compress_size * 1024, "ZIP size/ratio outside fixed budget")
        def read(info):
            with archive.open(info) as member:
                value = member.read(info.file_size + 1)
            require(len(value) == info.file_size, "ZIP member has unexpected inflated length")
            return value
        manifest_data = read(archive.getinfo("manifest.json"))
        require(sha256(manifest_data) == SOURCE_MANIFEST_SHA256, "source manifest identity changed")
        manifest = strict_json(manifest_data)
        records = validate_source_manifest(manifest)
        payload = {}
        tiles = {tile_path(t): t for t in tile_ids(SOURCE_ROOTS)}
        for info in infos:
            if info.filename == "manifest.json":
                continue
            value = read(info)
            record = records[info.filename]
            require(len(value) == record["size_bytes"] and sha256(value) == record["sha256"],
                    f"source payload integrity mismatch: {info.filename}")
            if info.filename in tiles:
                validate_tile(value, tiles[info.filename])
            else:
                value.decode("utf-8", errors="strict")
            payload[info.filename] = value
    return manifest, payload


def reconstruction_provenance():
    return f"""# Balzers terrain reconstruction, recipe 1

Local reconstruction from Liechtenstein_Terrain_Package_v1.zip ({SOURCE_BYTES} bytes).
Source archive SHA-256: {SOURCE_SHA256}
Original manifest SHA-256: {SOURCE_MANIFEST_SHA256}

Selection: geographic-quadtree L10 roots (1077,244) and (1078,244), with every
child through L13: 2 + 8 + 32 + 128 = 170 tiles. x increases eastward, y southward;
each L10 cell spans 180/1024 = 0.17578125 degrees in both axes. Bounds (W,S,E,N):
9.31640625, 46.93359375, 9.66796875, 47.109375 degrees. Approximately 26.7 km east-west
by 19.5 km north-south near Balzers; these are approximate dimensions, not survey
measurements. The original source's Balzers sample (47.068 N, 9.501 E) is inside.
It is not the rectangle's exact center. Complete source cells constrain placement.

All 765 input DEMs and all 3 original documentation files are verified before
selection. Selected DEM bytes and all original documentation bytes are unchanged.
No DEM rebake, resampling, new datum conversion, height adjustment, downloaded
input, imagery, buildings, airport data or executable content is introduced.
Original manifest source records (including credits/license references) are retained
verbatim as field values. The original documentation describes the larger source:
its 765-tile counts, nine roots and full-area verification are inherited evidence,
not a claim that this subset includes those tiles. Its notice's SOURCE_PROVENANCE.json
reference refers to upstream evidence preserved in docs/source-provenance.md.

The original license bundle and notices remain authoritative source material.
This reconstruction verifies byte integrity and complete selected tile families;
it does not repeat the upstream GeoTIFF/geoid survey, establish legal clearance,
accept terms, authorize redistribution or qualify commercial/Steam distribution.
GLO-90 is a reflective DSM, not bare earth or navigation/airport survey data.
The matching 10-arcminute geoid has unquantified vertical accuracy. A 65 x 65
runtime grid is interpolated source detail, not new measurements. Outside the
subset, existing global fallback remains. Package replay/aircraft restrictions remain.

This is a new artifact, package {PACKAGE_ID}@{PACKAGE_VERSION}. The lost historical
Balzers ZIP was reported as {HISTORICAL_BYTES} bytes with SHA-256 {HISTORICAL_SHA256}.
Those bytes were not recovered; no byte equality or recovered publication approval
is claimed. Stored ZIP entries, sorted payload paths, UTF-8/LF JSON, fixed timestamps
and regular 0644 attributes make this recipe independent of zlib compression versions.
No URL is guessed. Optional catalog construction is offline metadata only and cannot
verify that an explicitly supplied public destination exists or contains these bytes.
""".encode("utf-8")


def create_package(manifest, payload):
    selected = {tile_path(tile) for tile in tile_ids(ROOTS)} | set(SOURCE_DOCS)
    require(selected <= payload.keys(), "missing selected payload")
    output_payload = {path: payload[path] for path in sorted(selected)}
    output_payload[PROVENANCE_PATH] = reconstruction_provenance()
    result = copy.deepcopy(manifest)
    result.update(id=PACKAGE_ID, version=PACKAGE_VERSION, title=PACKAGE_TITLE)
    result["terrain"]["bounds_degrees"] = bounds_for(ROOTS)
    original_records = {record["path"]: record for record in manifest["files"]}
    result["files"] = [copy.deepcopy(original_records[path]) for path in sorted(selected)]
    result["files"].append({"path": PROVENANCE_PATH, "kind": "documentation",
                            "size_bytes": len(output_payload[PROVENANCE_PATH]),
                            "sha256": sha256(output_payload[PROVENANCE_PATH]), "source": "regional-dem"})
    result["files"].sort(key=lambda record: record["path"])
    manifest_data = json_bytes(result)
    target = io.BytesIO()
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
        for path, value in [("manifest.json", manifest_data), *sorted(output_payload.items())]:
            info = zipfile.ZipInfo(path, date_time=FIXED_DATE)
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, value)
    archive_data = target.getvalue()
    report = {
        "schema_version": 1, "recipe": "balzers-terrain-reconstruction-1",
        "archive_size_bytes": len(archive_data), "archive_sha256": sha256(archive_data),
        "manifest_sha256": sha256(manifest_data), "id": PACKAGE_ID, "version": PACKAGE_VERSION,
        "bounds_degrees": bounds_for(ROOTS), "root_tile_ids": ROOTS,
        "counts_by_level": {str(level): sum(t[0] == level for t in tile_ids(ROOTS)) for level in range(10, 14)},
        "verified_source_files": 768, "selected_dem_files": 170,
        "original_documentation_files_retained": 3, "added_documentation_files": 1,
        "source_archive_sha256": SOURCE_SHA256, "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
        "dem_bytes_unchanged": True, "source_records_unchanged": result["sources"] == manifest["sources"],
        "historical_archive_equality_claimed": False, "published": False,
        "verification_scope": "Python input pins, payload hashes/CRC/FSDM checks and complete subtree selection; runtime import/native rendering are separate checks",
    }
    return archive_data, manifest_data, report


def validate_download_url(url):
    # Deliberately matches content::download::DownloadSource's narrow source grammar.
    require(isinstance(url, str) and len(url) <= 2048 and url.isascii(), "invalid download URL envelope")
    match = re.fullmatch(r"https://(github\.com|raw\.githubusercontent\.com)/([A-Za-z0-9._/-]+)", url)
    require(match is not None, "expected canonical HTTPS GitHub source URL without query/credentials/port")
    host, path = match.groups()
    parts = path.split("/")
    require(len(parts) <= 16 and all(0 < len(p) <= 255 and p not in (".", "..") for p in parts),
            "invalid download URL path")
    valid = (len(parts) == 6 and parts[2:4] == ["releases", "download"] and parts[4] != "latest"
             if host == "github.com" else len(parts) >= 4 and re.fullmatch(r"[0-9a-f]{40}", parts[2]))
    require(valid and parts[-1].endswith(".zip"), "expected explicit release asset or commit-pinned raw ZIP")
    return url


def create_catalog(manifest_data, archive_data, url):
    validate_download_url(url)
    manifest = strict_json(manifest_data)
    return json_bytes({"schema_version": 1, "regions": [{
        "id": manifest["id"], "version": manifest["version"], "title": manifest["title"],
        "bounds_degrees": manifest["terrain"]["bounds_degrees"], "url": url,
        "archive_sha256": sha256(archive_data),
        "provenance": f"Rebuilt 170-tile L10-L13 Balzers subset of source ZIP SHA-256 {SOURCE_SHA256}. "
                      "Original Copernicus GLO-90 / NOAA geoid provenance, notices and license retained. "
                      "No new source-resolution, survey-accuracy or distribution-approval claim."
    }]})


def write_new(path, data):
    """No overwrite, including dangling links; clean only a failed new write."""
    with path.open("xb") as output:
        try:
            output.write(data)
            output.flush()
        except BaseException:
            # Exclusive creation established ownership; caller owns the parent dir.
            output.close()
            path.unlink()
            raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="the exact recovered Liechtenstein_Terrain_Package_v1.zip")
    parser.add_argument("output", type=Path, help="new output ZIP; existing files are never replaced")
    parser.add_argument("--manifest", type=Path, help="optional new exact manifest sidecar")
    parser.add_argument("--report", type=Path, help="optional new verification JSON sidecar")
    parser.add_argument("--provenance", type=Path, help="optional new reconstruction provenance sidecar")
    parser.add_argument("--download-url", help="explicit already-verified public destination; never fetched/published")
    parser.add_argument("--catalog", type=Path, help="optional new catalog; requires --download-url")
    args = parser.parse_args(argv)
    if bool(args.catalog) != bool(args.download_url):
        parser.error("--catalog and --download-url must be supplied together")
    paths = [p for p in (args.output, args.manifest, args.report, args.provenance, args.catalog) if p is not None]
    try:
        require(len({p.absolute() for p in paths}) == len(paths), "output paths must be distinct")
        for path in paths:
            require(not os.path.lexists(path), f"refusing to overwrite existing output: {path}")
            require(path.parent.is_dir(), f"output parent does not exist: {path.parent}")
        if args.download_url:
            validate_download_url(args.download_url)
        manifest, payload = inspect_source(read_pinned_source(args.source))
        archive, manifest_data, report = create_package(manifest, payload)
        outputs = [(args.output, archive), (args.manifest, manifest_data), (args.report, json_bytes(report)),
                   (args.provenance, reconstruction_provenance())]
        if args.catalog:
            outputs.append((args.catalog, create_catalog(manifest_data, archive, args.download_url)))
        # Validation is complete before any requested output is created. A later
        # disk/write failure can leave earlier complete sidecars; no overwrite/retry.
        for path, value in outputs:
            if path is not None:
                write_new(path, value)
        print(f"{args.output}: {len(archive)} bytes; SHA-256 {sha256(archive)}")
        print(f"{PACKAGE_ID}@{PACKAGE_VERSION}: 170 unchanged DEMs; original notices retained; local only")
        return 0
    except (OSError, ValueError, zipfile.BadZipFile, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
