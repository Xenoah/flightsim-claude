# Complete-planet terrain baseline

World terrain mode adds a bundled, offline, approximately **19.5 km** equatorial
sample-spacing baseline beneath regional `.fsdem` tiles. It uses real global
relief, independently mapped land/inland water, and static modeled lake surfaces.
It does not provide high-resolution worldwide terrain, photographic imagery,
precise shorelines, current water levels, or surveyed airport elevations.

## Sources and reproducible preparation

- NOAA NCEI ETOPO 2022 v1 **ice-surface** relief, 60 arcseconds, sampled every tenth
  source node. Heights are EGM2008 orthometric metres (EPSG:3855)
- The matching ETOPO 2022 EGM2008 geoid, derived from NGA data
- Natural Earth land and lake polygons, classified independently of height sign
- Copernicus GLO-90 hydro-edited surface samples for reviewed lake/coastal
  corrections, with source URLs/hashes and sampling details in a pinned manifest

The original ETOPO/land combination exposed lake bottoms and occasional coastal
bathymetry as land. The corrected preparation handles every mapped lake node,
Caspian water, and negative coastal/land candidates with independent Copernicus
surface evidence. It does **not** simply clip negative land to zero. The derived
water levels are static coarse models; generalized shorelines and averaged source
overviews can mix shore and water. They are not current lake observations.
See [the source record](data/global-sources.md) and correction manifest for exact
methods, evidence and limitations.

The bundle is a **mixed-source derivative**. NOAA/NGA/Natural Earth data are
public-domain/CC0, but modified Copernicus data require notices and redistribution
of the license text. Include [ATTRIBUTION.md](../ATTRIBUTION.md),
[NOTICE-GLOBAL-TERRAIN.txt](data/NOTICE-GLOBAL-TERRAIN.txt), and
[the Copernicus license](data/copernicus-glo90-license.pdf) with distributions.
No source account, API key, paid service or runtime network connection is needed.

### Reproduce the source preparation and bake

Use a build environment with the Python dependencies documented in the source
record. Scripts never install software. Downloading is explicit:

```bash
python3 scripts/prepare-global-terrain.py \
  --source-dir /tmp/global-terrain-downloads \
  --output-dir /tmp/global-terrain-source --download
python3 scripts/pack-global-terrain.py \
  --prepared /tmp/global-terrain-source \
  --output /tmp/global-terrain.fsgt
```

An offline repeat omits `--download`. The preparation script pins source files and
the correction manifest by SHA-256. To independently audit the original corrected
Copernicus overview samples as well, add
`--verify-copernicus-sources /tmp/copernicus-audit-cache`; `--download` permits
bounded missing range downloads. The source record describes those byte ranges.
No GeoTIFF/GIS/network code enters the runtime simulator.

Prepared source arrays have 1080 rows × 2160 columns, north to south from
89.84166666666667°, eastwards periodically from 0.008333333333325754°, at 1/6°.
That asymmetric original sample origin is explicit, never silently recentered.
Geoid nodes are reordered onto exactly the same positions and remain unchanged
by the lake/coastal correction process.

The packer then resamples onto **2048 × 1024 canonical cell centres**, with origin
longitude −179.912109375°, latitude +89.912109375°, and spacing 0.17578125°
(10.546875 arcminutes, approximately 19.5 km at the equator). This makes source
interpolation breakpoints align with geographic 33-point terrain tile grids at
levels 6 and finer. Source H is converted to a physical surface before resampling:
land/lakes retain corrected H; ocean nodes contribute H=0. H and N remain separate.
Dry-land/inland-water masks are nearest-neighbour resampled, never inferred from
height. Target lake nodes retain the nearest corrected lake level rather than
mixing neighbouring mountains into that water node; target ocean nodes store H=0.

A final feature-preserving pass assigns every corrected inland-water source point
and negative dry-land source point to its nearest canonical node cell. Inland
water wins a cell conflict; the closest lake source point supplies its level,
with source row-major order breaking exact-distance ties. Otherwise the genuine
negative dry-land minimum is retained. This prevents narrow lakes/depressions
from vanishing between target sample centres. The source-point-to-assigned-node
coordinate change is at most 0.087890625° per axis; **this is not a bound on
shoreline displacement or the interpolated feature footprint**, which can extend
farther. Narrow features can shift/widen, and neighbouring high terrain can still
raise the interpolated height between preserved negative nodes.

In this bake the pass changes 1468 negative-ground nodes and264 inland-water nodes;
the largest node-height change versus unpooled resampling is837.582m. That is an
explicit source-feature preservation tradeoff, not a claimed improvement in
pointwise accuracy. Detailed local DEMs are the accuracy path. Named-place probes
and remaining biases are recorded in [data QA](qa/global-terrain-data-2026-10-02.md).

The Rust offline encoder reproduces the same FSGT bytes from raw arrays:

```bash
cargo run -p flightsim-tilegen --bin flightsim-globalgen -- \
  --elevation /tmp/global-terrain-source/elevation.f32le \
  --geoid /tmp/global-terrain-source/geoid.f32le \
  --land-mask /tmp/global-terrain-source/land.u8 \
  --inland-water-mask /tmp/global-terrain-source/inland_water.u8 \
  --width 2160 --height 1080 \
  --origin-lon 0.008333333333325754 --origin-lat 89.84166666666667 \
  --source-vertical-datum egm2008 --canonical \
  --source-manifest /tmp/global-terrain-source/terrain-preparation.json \
  --output /tmp/global-terrain-rust.fsgt
cmp /tmp/global-terrain.fsgt /tmp/global-terrain-rust.fsgt
```

The raw Rust CLI treats its text manifest as caller-provided provenance, not source
authentication. It labels arbitrary output externally prepared; only exact byte
identity with the bundled asset earns the reviewed bundled dataset ID. The pinned
preparation script is the source-reproduction path.

For an intentional distributed-data update, the Python packer can also generate
`--rust-metadata crates/flightsim-world/src/global_metadata.rs`. This keeps the
compiled dataset identity/fingerprint paired with the exact reviewed bytes.
Never regenerate the distributed asset from unreviewed source changes.

Both packers atomically mark provenance INCOMPLETE **before** replacing an atlas,
and COMPLETE only after all requested writes succeed. Each individual output uses
a same-directory temporary file. This is not a multi-file transaction and does not
promise power-loss durability. A failed final marker write cannot leave an old
COMPLETE attestation beside new bytes. The exact output SHA-256 and fingerprint
are recorded in `crates/flightsim-world/data/global-terrain.provenance.json`.

## Height, water, coastline and pole contracts

Runtime surface height is WGS84 ellipsoidal `h = H + N`. H is quantized to metres,
N to centimetres, halfway away from zero. Per-node height conversion quantization
is at most 0.505 m; that is not an accuracy claim for source data or resampling.

- Dry land uses corrected orthometric H, including genuine negative depressions
- Inland water uses corrected lake H and is classified separately from dry land
- Ocean nodes use H=0, giving ellipsoidal sea-surface height N, not globally zero
- Coast/shore cells bilinearly blend these surfaces and land fractions. Very near
  shore, a water-classified point can therefore have a blended terrain height
- The polar caps converge continuously to the longitude-independent mean of the
  nearest atlas row. The exact poles are single points, not longitude seams
- Longitude is periodic, including ±180° and finite wrapped longitudes

Small islands and peaks can disappear at this resolution. No invented fine-scale
relief is added when the mesh subdivides. The dry-land and inland-water masks are
mutually exclusive at stored nodes; interpolated samples prefer dry land on exact
half-way ties. `is_inland_water` allows lake labels/colour without moving lakes to
sea level. The masks and heights remain a coarse representation of shoreline areas.

## Runtime priority, geometry and bounds

`GlobalTerrain` validates bundled bytes once and shares immutable storage through
`Arc`. Sampling is constant-work and returns ellipsoidal surface height, H, N,
dry-land/inland-water classification, and interpolated land fraction. Invalid or
nonfinite latitude/longitude returns None instead of propagating NaN.

`GlobalTileSource<S>` preserves regional primary tiles at **every** level:

1. `TileSource::load` tries only the primary source at the supplied ID
2. Physics searches all configured primary levels, including real ancestors,
   before calling the direct global height fallback
3. Rendering exhausts its budgeted primary ancestor search before explicitly
   calling `load_fallback`. Generated fine tiles must not hide real ancestors

A generated fallback DEM always has 33×33 samples and a maximum level of 13. The
source performs no disk IO and holds no generated-tile cache. Generation is charged
against existing per-frame renderer tile/mesh budgets and resident-cache limits.
`Terrain` retains at most 4096 primary-load errors; later errors are counted by
`dropped_load_failures()` instead of growing an unbounded diagnostics vector.

For the canonical atlas, direct atlas sampling and bilinear DEM sampling align
from level 6 upward apart from f32 height rounding. Tests include source-node
breakpoints and polar caps. This does **not** imply the rendered triangular mesh
exactly equals the bilinear physics surface: triangle saddle interpolation and
curved-Earth chord error remain. Lower levels also simplify relief. Near-ground
LOD and regional high-resolution `.fsdem` data remain important for landing.

## FSGT version 2 format

All multibyte fields are little-endian. The header is 64 bytes. Version 1 was an
unpublished development format and is deliberately rejected by this reader.

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | `FSGT` |
| 4 | 2 | version 2 |
| 6 | 2 | flags, zero |
| 8 | 4 | width |
| 12 | 4 | height; width must equal twice height |
| 16 | 2 | horizontal EPSG 4326 |
| 18 | 2 | orthometric source EPSG 3855 |
| 20 | 2 | ellipsoidal target EPSG 4979 |
| 22 | 2 | registration 1: explicit periodic sample origin |
| 24 | 8 | FNV-1a checksum of all bytes except this field |
| 32 | 8 | first-sample longitude, f64 degrees |
| 40 | 8 | northernmost-sample latitude, f64 degrees |
| 48 | 16 | reserved, zero |
| 64 | 4 × count | interleaved i16 H metres, i16 N centimetres |
| following | ceil(count/8) | dry-land bits, low bit first |
| following | ceil(count/8) | inland-water bits, low bit first |

Mask overlap, unused mask bits, unknown fields, sample ranges, lengths, checksum
and trailing data are strictly checked. Height is 2..=2048, width exactly twice
height, and streams over40 MiB are rejected before interpreting payload data.
Raw H range is ±12 km, N is ±200 m. Non-ocean physical surface H below−500 m is
rejected as unexplained submerged terrain, never silently clipped. This conservative
surface invariant does not replace lake/coastal source validation; positive lake
bottoms would still need the independent correction process.

The canonical v2 atlas is 8,912,960 bytes. Dataset identity is
`ETOPO2022-COP90-NE-2048x1024-surface-v2`; the exact fingerprint and source/output
hashes are generated with the bundle and recorded in its provenance JSON.
Current fingerprint: `500e8db32bde7019`; SHA-256:
`57bc03c12caa772ecbd52f1d24cd12704ccc4ace5123e9d8c26b6e454907f13e`.

## Verification commands

```bash
python3 scripts/test_pack_global_terrain.py
cargo test -p flightsim-world global
cargo test -p flightsim-world long_global_travel_keeps_failure_diagnostics_bounded
cargo test -p flightsim-tilegen global
```

Tests cover datum sign, negative dry land, ocean-vs-seabed distinction, nonzero
lake levels, disjoint masks, explicit origins, longitude wraps, continuous poles,
shared tile edges, source-breakpoint alignment, fixed generation bounds, primary
ancestor priority, bounded diagnostics, malformed streams, exact encoder round
trips, protected input files and failed-final-provenance-write behavior. Actual
execution evidence belongs in the task's QA record; commands listed here do not
by themselves claim a test run passed.
