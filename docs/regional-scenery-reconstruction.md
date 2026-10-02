# Liechtenstein OSM scenery reconstruction method

This document and the accompanying machine-readable
[reconstruction manifest](regional-scenery-reconstruction.json) specify how to
reconstruct the exact OSM-derived `liechtenstein.fsscenery` database used by the
regional surface screenshots. The complete alteration method consists of these
files **and the referenced importer, core geometry, codec, vendored parser and
locked dependency source** in the same repository revision. They are provided together in this public repository for download without charge.

Map data © OpenStreetMap contributors. The resulting derivative database is
available under [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/).
[OpenStreetMap copyright and attribution](https://www.openstreetmap.org/copyright).
Computer-program source retains its repository and dependency licences; this
method does not relicense game code or waive database obligations.

## Purpose and publication condition

[ODbL section 4.6(b)](https://opendatacommons.org/licenses/odbl/1-0/) permits an
access offer containing the complete alteration method, including additional
contents, as an alternative to offering the entire derivative database.
Internet distribution must be free of charge. The
[OSMF Produced Work guideline](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Produced_Work_-_Guideline)
identifies raster images as usual Produced Works and connects public use to the
underlying-database or alteration access requirement.

This is the engineering reconstruction component of that offer. Public screenshot
reports link a revision-specific copy of this document and manifest, with all
referenced source in the same revision. A private data download alone does not
provide this public access offer. This documentation is not a legal opinion,
a commercial binary approval, or a change to the project's release/data exclusions.
Other datasets shown in an image retain their own notices and obligations.

## Exact source and output identity

The only geographic input is the public Geofabrik Liechtenstein OSM PBF extract.
There is no additional clipping, private correction, second OSM extract or hidden
geographic input. The entire extract is processed, including any source-provider
border coverage; the importer does not impose a new country boundary.

- Official listing: [Geofabrik Liechtenstein](https://download.geofabrik.de/europe/liechtenstein.html)
- Exact dated input: [liechtenstein-260930.osm.pbf](https://download.geofabrik.de/europe/liechtenstein-260930.osm.pbf)
- OSM snapshot in the PBF header: `2026-09-30T20:22:42Z`; replication sequence `4924`
- Input size: `3458908` bytes
- Input SHA-256: `c6f495bd86bdc52ba48557733f765508be0098eb95ad32dbb60cfb683b8b4ae7`
- Input FNV-1a-64: hexadecimal `4f34bdb072691af3`, decimal `5707395193316645619`
- Output size: `8381178` bytes
- Output SHA-256: `ef6c802053df442a1db8a871e4591476bb5f845bb4ec394a1bf6df9cf81a867e`
- Output counts: `13932` roads, `17906` buildings, `4067` landcover polygons,
  `336453` point entries and `506193` triangle-index entries

The dated input URL above was exposed by the official listing, then independently
read and compared byte-for-byte with the original input on 2026-10-02. It was not
inferred from a filename pattern. Its listing timestamp was 2026-10-01 01:22 UTC.
Upstream retention is outside this project's control; this verification is not a
promise that the dated object will remain available indefinitely. Do not substitute
`latest` or a different dated file when checking this historical output hash.

## Reproduction commands

From the repository root, obtain the exact dated source and verify it before
running the importer. A Rust 1.93.0 x86_64 Linux build was used for the independent
byte-exact check. Use the accompanying `Cargo.lock`, `earcutr = 0.5.0` and the
repository's patched `vendor/osmpbf` 0.3.7; an unpatched registry parser is not a
replacement for the supplied source. No Bevy build, graphics driver, account or
API key is needed by this offline command.

```sh
mkdir -p reconstruction
curl --fail --location \
  'https://download.geofabrik.de/europe/liechtenstein-260930.osm.pbf' \
  --output reconstruction/liechtenstein-260930.osm.pbf
printf '%s  %s\n' \
  'c6f495bd86bdc52ba48557733f765508be0098eb95ad32dbb60cfb683b8b4ae7' \
  'reconstruction/liechtenstein-260930.osm.pbf' | sha256sum --check -

cargo +1.93.0 build --locked --release -j 2 \
  -p flightsim-tilegen --bin flightsim-scenerygen

target/release/flightsim-scenerygen \
  --input reconstruction/liechtenstein-260930.osm.pbf \
  --output reconstruction/liechtenstein.fsscenery \
  --source-name 'Geofabrik Liechtenstein OSM, snapshot2026-09-30T20:22:42Z' \
  --source-url 'https://download.geofabrik.de/europe/liechtenstein-latest.osm.pbf'

printf '%s  %s\n' \
  'ef6c802053df442a1db8a871e4591476bb5f845bb4ec394a1bf6df9cf81a867e' \
  'reconstruction/liechtenstein.fsscenery' | sha256sum --check -
```

`--source-name` and `--source-url` are exact serialized metadata. In particular,
there is no space between `snapshot` and `2026`, and the embedded URL deliberately
remains the original `latest` acquisition URL even though the reconstruction
fetches the verified dated object. Changing either string changes the output hash.
A different local input/output filename does not enter the database. If
`CARGO_TARGET_DIR` is set, use the binary in that target directory instead.

Independent verification on 2026-10-02 invoked the already built release importer
without Cargo, regenerated the output, and confirmed byte-for-byte equality with
the original. This establishes reproduction for that tested build and source;
it does not assert untested cross-platform floating-point byte identity. No
wall-clock time, random seed, environment variable or network lookup contributes
to the importer output.

## Complete alterations and additional contents

The exact implementation is
[`crates/flightsim-tilegen/src/scenery.rs`](../crates/flightsim-tilegen/src/scenery.rs),
with its CLI in
[`crates/flightsim-tilegen/src/bin/flightsim-scenerygen.rs`](../crates/flightsim-tilegen/src/bin/flightsim-scenerygen.rs).
The following rules describe its transformations, including all inferred display
values. The machine-readable manifest pins the corresponding file hashes.

### Source traversal and retained fields

1. Read the input bytes once and use the safe sequential `BlobReader` for two
   passes. The first pass selects ways and records multipolygon members. The
   second resolves only referenced normal/dense node coordinates.
2. Read only these tags for classification or display values: `highway`,
   `building`, `landuse`, `natural`, `leisure`, `width`, `height`,
   `building:levels`, `min_height`, `area`, `bridge`, `tunnel`, `layer`, `type`.
   If a tag key repeats, the first value is used. Classification values are
   case-sensitive exact strings. Lengths, levels and layer have the specific
   whitespace handling described below.
3. Retain original positive OSM way IDs and node order. Sort records by increasing
   source ID separately within roads, buildings and landcover. Duplicate selected
   way IDs or duplicate referenced node IDs fail the entire bake.
4. No contributor IDs/usernames, changesets, timestamps, names, addresses, arbitrary
   tags or node IDs are exported. There is no independent named-place database.
   The input fingerprint covers all input bytes, not just the selected records.
5. Classification precedence is building, then supported highway, then the ordered
   landcover matches below. Thus a qualifying building tag wins over highway or
   landcover; a supported highway wins over landcover.

### Building selection and heights

A way is a building when `building` exists and is neither `no`, `construction`
nor `ruins`. This is an exact rule, not a whitelist of known building subtypes.

- A successfully parsed positive `min_height` suppresses the building.
- Prefer `height` when it parses to a finite value in the inclusive range 1–500 m;
  store `height_source = OsmHeight` (numeric value 1).
- Otherwise parse `building:levels` as a trimmed finite floating-point number in
  the inclusive range 1–166. Multiply by exactly 3 m; store `OsmLevels` (2).
  Fractional levels in that range are accepted.
- Otherwise use exactly 9 m and store `Default` (3).

The fixture has 15 direct heights, 861 levels-derived estimates and 17,030 default
heights. These are lengths above rendered terrain, not surveyed elevations.

### Roads and widths

Supported `highway` values map as follows. A record stores the simplified enum,
not the original tag text.

| FSSC class and numeric value | Source values | Default width in metres |
| --- | --- | ---: |
| Motorway 1 | motorway, motorway_link, trunk, trunk_link | 12 |
| Primary 2 | primary, primary_link | 9 |
| Secondary 3 | secondary, secondary_link, tertiary, tertiary_link | 7 |
| Residential 4 | residential, living_street, unclassified | 5.5 |
| Service 5 | service | 4 |
| Track 6 | track | 3 |
| Path 7 | path, footway, cycleway, pedestrian, steps, bridleway | 1.5 |

Roads are omitted if `bridge` or `tunnel` is present with any value other than
exactly `no`, if trimmed `layer` is present and not exactly `0`, or if `area` is
exactly `yes`. Unsupported highways are not roads, although a separately matching
landcover tag can still select the way through the classification precedence.
A valid `width` in the inclusive range 0.5–100 m overrides the table and sets
`width_inferred = false`; otherwise the table value is used with `true`.
The fixture has 13,678 inferred road widths. Road centerlines are neither widened
nor extended in FSSC; the width is a separate display attribute.

### Shared length parser

Trim whitespace, then accept a floating-point number optionally ending in the
case-sensitive suffix `m`, `ft` or a single apostrophe. Trim the remaining number.
Bare values and `m` are metres; `ft` and apostrophe values are multiplied by the
core conversion constant exactly `0.3048`. Only positive finite results parse.
Other units, compound feet-and-inches strings and lists are not interpreted.
The height/width ranges above are applied after parsing. `min_height` only needs
to parse as positive; it has no extra accepted upper range.

### Landcover precedence

After building/highway selection, use the first matching row below. Each `or`
is an alternative condition; unrelated tags do not prevent a match.

| FSSC class and numeric value | Source conditions |
| --- | --- |
| Forest 1 | landuse=forest or natural=wood |
| Grass 2 | landuse in grass, meadow, recreation_ground, village_green; or natural in grassland, heath, scrub; or leisure in park, garden, pitch, golf_course |
| Farmland 3 | landuse in farmland, farmyard, orchard, vineyard, allotments |
| Residential 4 | landuse=residential |
| Industrial 5 | landuse in industrial, commercial, retail |
| Water 6 | landuse in reservoir, basin or natural=water |
| Bare 7 | landuse in quarry, brownfield, construction or natural in sand, beach, scree, shingle |
| Rock 8 | natural=bare_rock |

Unmatched ways are not retained. These outlines are incomplete OSM coverage,
not a comprehensive land mask. There is no raster landcover classification input.

### Relations and geometry

- Every relation with exact `type=multipolygon` is inspected. Its way members are
  suppressed from building/landcover candidates regardless of member role or
  whether the relation itself has classification tags. Road candidates are not
  suppressed by multipolygon membership. No relation geometry or holes are baked.
- Polygons require at least four source references, with equal first/last node
  references. Remove that closing reference in the output; retain every other
  original coordinate and order. Roads retain their source reference sequence.
- Omit candidates with missing nodes, non-finite/out-of-range coordinates, fewer
  than two retained points, or adjacent points separated by less than 0.001 m in
  core-computed zero-altitude ECEF. There is no simplification, coordinate snapping,
  resampling, boundary crop or manual correction.
- Convert source degree coordinates to f64 WGS84 radians with
  [`flightsim-core`](../crates/flightsim-core/src/geodetic.rs). All scenery altitudes
  are exactly zero; `ele` is ignored. WGS84 uses semi-major axis 6,378,137 m and
  inverse flattening 298.257223563. These core transforms are the only geodetic
  transforms used by the importer.
- Project polygons to the east/north plane of a core
  [`LocalFrame`](../crates/flightsim-core/src/frames.rs) anchored at their first
  vertex. Reject self-intersections between nonadjacent ring edges. Intersection
  collinearity/coordinate tolerance is `1e-8` in the implementation's planar tests.
  Require finite polygon area at least `0.01 m²`.
- Triangulate with locked `earcutr 0.5.0`, two dimensions and an empty hole list.
  Require exactly `n−2` triangles, valid indices, each triangle area at least
  `1e-6 m²`, and summed absolute triangle area within `area×1e-6 + 0.001 m²`
  of ring area. Failure omits that polygon. No new geographic vertices are added.
- The immutable
  [`SceneryDatabase`](../crates/flightsim-world/src/scenery.rs) then validates the
  full result: simple rings, unique nondegenerate triangles, consistent winding,
  exact ring-edge coverage, opposite paired interior edges, no crossing exterior
  diagonals and interior diagonal midpoints. Its geometry epsilon is `1e-8`;
  its area tolerance is again `area×1e-6 + 0.001`. Failure rejects the complete bake.

### Bounded processing and failure rules

These bounds are part of this exact method. They are not raised for the fixture.
Aggregate exhaustion is an error rather than silent truncation.

- Input bytes: 1 through 64 MiB
- PBF blocks: at most 512; at most 5,000,000 elements per pass
- Decompressed PBF bytes: at most 256 MiB per pass, charged before decoding
- Individual decoder block bound: less than 32 MiB, with validated raw/raw_size
  and exact zlib input/output/stream termination checks
- Selected candidate features: at most 50,000
- Candidate node references, total multipolygon references, distinct relation-way
  IDs and retained referenced nodes: each bounded by 1,000,000
- Per-feature point limit: 512; a closed polygon may have 513 input references
  before its closing reference is removed; larger selected ways are omitted
- Output point entries: at most 1,000,000; triangle-index entries: at most 3,000,000
- Polygon validation work: sum `3n² + 10n` at most 50,000,000
- Regional extent: all coordinates within 200,000 m ECEF distance of the first
  coordinate in canonical class/source-ID order
- FSSC payload: at most 32 MiB; source name/URL: each nonempty, at most 2,048 UTF-8
  bytes, without control characters; OSM URL must start with `https://`

The parser requires an OSM header before OSM data, validates referenced array
indices and deltas, and skips unknown blob types. It does not use the indexed
reader. See all patched source in [`vendor/osmpbf`](../vendor/osmpbf/), including
[`FLIGHTSIM-PATCHES.md`](../vendor/osmpbf/FLIGHTSIM-PATCHES.md). Invalid input or a
failed complete-database validation does not replace an existing output: the
codec writes a same-directory temporary file and persists it only after success.

## FSSC encoding and exact additional metadata

[`crates/flightsim-world/src/scenery/io.rs`](../crates/flightsim-world/src/scenery/io.rs)
is the complete machine-readable codec. FSSC v1 has a 48-byte little-endian header,
magic `FSSC`, version 1, zero flags/reserved fields, exact aggregate counts and a
payload FNV-1a-64 checksum. Record order is roads, buildings, landcover; each class
is sorted by source ID. Coordinates are little-endian f64 latitude/longitude
radians; triangle indices are little-endian u32. There is no compression,
quantization, stored elevation or serialized runtime spatial index.

Metadata stores source kind `OpenStreetMap` (1), the FNV-1a-64 of the full original
PBF, the exact name/URL command arguments, and this exact UTF-8 notice:

```text
Map data © OpenStreetMap contributors; https://www.openstreetmap.org/copyright; derived database licensed under ODbL 1.0: https://opendatacommons.org/licenses/odbl/1-0/
```

FNV-1a-64 starts at `0xcbf29ce484222325`; for every byte, XOR then multiply by
`0x100000001b3`, wrapping modulo 2⁶⁴. Source text lengths are little-endian u16.
The source/format enums, inferred-width flag, height-source flag, width/height
constants, triangle indices, metadata and checksums described here are the complete
additional FSSC contents beyond the selected OSM coordinates/source IDs/classes.

## Expected omissions and independent check

The exact source yields 404,761 inspected elements, 37,190 selected ways and
283,947 resolved referenced nodes. Omitted candidates: 7 oversized ways,
475 non-ground roads, 13 area roads, 780 multipolygon-member polygons and
10 invalid geometries. The scan observes 238 multipolygon relations. Raised
buildings, open polygons, missing-node candidates and invalid-coordinate candidates
are all zero in this fixture. These reason counts and the final hash are useful
checks against accidental changes to input or transformation rules.

All 336,453 output point entries were independently matched against the original
PBF references with exact radian agreement in this fixture. The reproduced FSSC
is unchanged across optional DEM package revisions. The reconstruction manifest
pins the reviewed candidate commit, 30 relevant source/build/parser files and
critical locked dependencies. The whole repository revision and dependency lock
remain authoritative, rather than this prose replacing source code.

## Inputs that do not enter this database

No non-public additional geography enters FSSC. Copernicus DEM, NOAA geoid,
regional terrain tiles, global terrain/climate, airport databases, imagery,
hand-authored buildings, flight telemetry, credentials and personal metadata are
not importer inputs. At rendering time, terrain draping, building facades and
procedural individual trees are separate visual operations; they do not write
back into or alter FSSC. Consequently they are not hidden additions needed to
reconstruct this OSM-derived database. Their own source, licensing and image
attribution requirements remain separate.

Keep the optional database outside the commercial release allowlist unless it
passes the project's separate review. Source publication and this engineering
offer do not certify a Steam build, navigation accuracy or legal compliance.
