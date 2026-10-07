# Balzers terrain package: local reconstruction

**Development/validation artifact; source-tree publication authorized on 2026-10-07.**
The recovered sample retains the source licence, attribution, no-liability notices
and reconstruction record. Source publication does not qualify a Windows binary
or a commercial/Steam release. Existing release allowlists and gates are unchanged.
No download catalog or binary-release authorization is introduced by this sample.

## Distribution terms

The terrain is governed by the [Copernicus GLO-90 free general-public licence](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf)
(PDF pages 19–21), separately from this repository's MIT/Apache software licences.
The complete original licence text is inside the ZIP at
`docs/copernicus-license-bundle.txt`; the source/adaptation credit, no-liability
notice and downstream obligations are retained at `docs/notice.txt`.
Redistribution and adaptation remain subject to those terms, including preserving
the required notices and avoiding implied endorsement. The NOAA geoid source is
public domain and its attribution/provenance is retained. This engineering check
of the included notices is not legal advice or commercial-release clearance.

This sample is a reproducible subset of the recovered Liechtenstein terrain ZIP,
not a recovery of the lost later Balzers ZIP. It requires no network, GIS library,
credential or paid service. The generator uses only Python's standard library.

## What is included

- Package identity: `balzers-glo90-rebuilt@1.0.0`
- 170 original, byte-unchanged DEM tiles: 2 at L10, 8 at L11, 32 at L12 and 128 at L13
- L10 roots `(10,1077,244)` and `(10,1078,244)` with all 42 complete four-child groups
- Bounds, west/south/east/north: `9.31640625, 46.93359375, 9.66796875, 47.109375` degrees
- Approximate extent: 26.7 km east-west × 19.5 km north-south near Balzers
- All three original UTF-8 license, notice and source-provenance files, byte-exact
- All original manifest source records, including full attribution and license references
- One new document explaining this subset, reconstruction and inherited-evidence limits

The geographic quadtree has `2^(L+1)` longitude cells and `2^L` latitude cells, with
y increasing southward. An L10 cell is `180/1024 = 0.17578125` degrees across.
The two adjoining cells contain the source's Balzers reference point (47.068 N,
9.501 E). That point is not the rectangle's exact center; whole-cell selection
preserves every sibling family instead of trimming fine tiles around a point.

Source resolution remains nominal GLO-90 (native 3-arcsecond posts). The 65 × 65
runtime grids add no measured detail. Heights retain the source's EPSG:4979
ellipsoidal-metre conversion and its documented 10-arcminute EGM2008 geoid limit.
This is a reflective DSM, not bare-earth, airport-survey or navigation data.
No imagery, buildings, roads or airport data are included. The original larger
region's 765-tile verification records remain clearly identified as inherited
upstream evidence, not verification performed again from raw GIS inputs here.

## Exact identities

| Item | Bytes | SHA-256 |
|---|---:|---|
| Recovered `Liechtenstein_Terrain_Package_v1.zip` input | 6,551,156 | `fc4de5f479d769b0aa6f028ab965a8798f9c11bdbae157bffc57ecd45df66b2a` |
| Original input `manifest.json` | 175,807 | `34e8759340c315c745a9d83746944d082bb94b8139e2806756749572b7d53384` |
| Rebuilt `Balzers_Terrain_Package_v1.zip` | 1,610,134 | `d7e1265ee8015ad0fb1df23f7110b23124b90d889440f1b14a9cd85119f28528` |

`manifest.json`, `reconstruction-provenance.md` and `verification.json` beside the
ZIP are exact recipe outputs. `SHA256SUMS` covers the ZIP and those sidecars.
The ZIP contains 175 entries: one manifest and 174 declared payloads.
The local `.gitattributes` pins those exact sidecars and `SHA256SUMS` to LF so a
Windows checkout with `core.autocrlf=true` preserves their recorded identities.

The historical lost Balzers artifact was reported as 1,461,931 bytes and SHA-256
`d9ec9ef06dbc9ad1d878a36dddd395b7627cde7d681093c8cb31835dcce93cc6`.
Those bytes were not recovered. This recipe deliberately uses a distinct package
ID and new artifact hash. It does not claim equality, old release status or old
publication permission.

## Rebuild offline

From the repository root, choose a new output directory and provide the recovered
input ZIP explicitly. The input is intentionally not fetched or inferred.

```sh
mkdir -p /tmp/balzers-rebuild
python scripts/package-balzers-terrain.py \
  /absolute/path/to/Liechtenstein_Terrain_Package_v1.zip \
  /tmp/balzers-rebuild/Balzers_Terrain_Package_v1.zip \
  --manifest /tmp/balzers-rebuild/manifest.json \
  --provenance /tmp/balzers-rebuild/reconstruction-provenance.md \
  --report /tmp/balzers-rebuild/verification.json
```

Existing outputs, including symlinks, are never overwritten. Use new paths for a
second run. Compare its ZIP bytes or SHA-256 with the identity above. Sorted paths,
fixed 2026-10-07 ZIP timestamps, regular 0644 attributes, UTF-8/LF JSON and stored
(uncompressed) entries avoid platform-dependent filesystem metadata and zlib
compression-version differences. The fixed timestamp is recipe metadata, not a
claim about the source acquisition date.

The script authenticates a bounded 6,551,156-byte regular-file snapshot **before
ZIP parsing**, with no command-line bypass for the source hash. It then checks the
pinned manifest; fixed 769-entry ZIP32 envelope, member allowlist and file types;
per-entry compressed/inflated sizes and CRCs; all 768 file sizes/SHA-256 hashes;
UTF-8 documentation; and all 765 FSDM headers, grid sizes, IDs, payload FNV checksums,
finite height ranges and geometric-error limits. It never extracts arbitrary
archive paths or executes archive content. Unselected input DEMs are also checked.
These fixed-input checks are deliberately narrower than a general-purpose importer.

Output parents must already exist and be caller-owned/trusted. This is not a
sandbox against another local process changing the caller's directories. All
input validation precedes output creation; a disk failure while writing several
requested sidecars can leave earlier complete outputs. No automatic overwrite or
repair occurs. Inspect those outputs and select new paths for a retry.

## Validation

```sh
python -m unittest discover -s scripts/tests -p test_balzers_terrain_package.py -v
python -m py_compile scripts/package-balzers-terrain.py
```

On 2026-10-07, all 20 focused Python tests passed. Coverage includes deterministic
output, known geographic bounds, full descendants, all inherited file identities,
input pin failures before ZIP parsing, altered unselected tiles, duplicate/invalid
JSON, unsupported datum, malformed FSDM values, ZIP duplicates/special-file metadata,
nonregular input, output overwrite protection and strict optional catalog URLs.
Tests use a synthetic source with explicit in-test pin replacements; the CLI has
no such override. The checked local sample is independently hash/CRC/FSDM-checked.

Independent read-only review verified all 173 inherited payload/record equalities,
retained source records, every child family and whole-tile bounds. The retained
sample height range is 476.2840576171875–2837.994873046875 ellipsoidal metres.
The package was generated twice from the actual pinned recovered source and the
ZIP, manifest, provenance and report matched byte-for-byte.

Separate production-Rust validation on this exact sample passed all three
`flightsim-content/tests/balzers_package.rs` integration tests: complete tile loads
and payload/original-notice hashes; pinned identity, ZIP and manifest; duplicate
installation and corruption refusal; all import cancellation phases, dropped staging
and the existing replay refusal. The `validate_region_package` CLI's `validate`,
`cancel`, `install` and `inspect` modes each exited successfully. Content clippy with
`downloads` and all targets also passed. These are content-lifecycle checks; they do
not establish native rendering, GPU behavior, flight performance, regional replay
support, scientific accuracy or rights clearance.
For application import and explicit new-flight activation, follow the existing
[terrain package guide](../../../content-packages.md). Package-backed replay and
unsupported aircraft/source combinations retain their existing restrictions.

The rebuilt Linux application's `--import-region` and `--list-regions` paths
also passed with this ZIP; a second import correctly exited 1 instead of replacing
the installed version. A source build can use the following separate operations
(replace `LOCAL_REGION_STORE` with the same trusted local directory each time):

```sh
cargo run --locked -p flightsim-app -- \
  --import-region docs/examples/terrain-packages/balzers/Balzers_Terrain_Package_v1.zip \
  --region-store LOCAL_REGION_STORE
cargo run --locked -p flightsim-app -- \
  --list-regions --region-store LOCAL_REGION_STORE
cargo run --locked -p flightsim-app -- \
  --region balzers-glo90-rebuilt@1.0.0 --region-store LOCAL_REGION_STORE \
  --aircraft swift-sport --start 47.068,9.501 --max-level 13
```

The third command is the existing pending-selection workflow, not a recorded
native-flight acceptance: close Regions, review the map/departure and explicitly
Start new flight. The import/list commands exit before renderer startup. The older
published alpha.20 binary does not establish support for these later source features.

## Optional catalog metadata

No download URL or catalog is included. No cache receipts are fabricated. The
current download workflow cannot use a `file:` URL or preseed a verified cache
merely by copying this ZIP into a directory.

Only after an authorized public destination has independently been verified to
contain these exact bytes, the generator can produce a local catalog with
`--download-url "$VERIFIED_PUBLIC_ZIP_URL" --catalog /new/path/catalog.json`.
Both options are required together. The supplied URL must follow the existing
explicit GitHub release-asset or full-commit-pinned raw ZIP contract; mutable raw
branches, normalization aliases, credentials, query/fragment strings, explicit
ports, non-HTTPS hosts and `latest` releases are rejected.

Catalog identity, title, bounds and archive hash come from the newly built artifact.
URL handling is inert and offline: it does not fetch, prove availability, upload,
publish, accept terms or grant redistribution rights. The tests use clearly
synthetic owner names and make no network requests. See the
[download contract](../../../content-downloads.md).
