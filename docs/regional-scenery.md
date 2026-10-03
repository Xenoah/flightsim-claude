# Regional OSM surface scenery

## Run the optional regional scene

Build the source checkout normally, then point the app at an independently
prepared regional database and its DEM directory. For the separately supplied
Liechtenstein sample, run from the checkout root (substitute your extraction path):

```sh
cargo run -p flightsim-app --release -- --aircraft swift-sport \
  --tiles /path/to/sample/tiles --scenery /path/to/sample/liechtenstein.fsscenery \
  --start 47.127,9.529 --fly 350 --view chase --cloud-cover 0
```

`--surface-detail on|off` controls the original procedural terrain material
(default on). It does not disable sourced roads/building footprints or change
physical terrain. Omit `--scenery` to run without the optional regional database.
`--render-stats` reports bounded wall-frame interval distributions for diagnosis;
these are whole-app CPU/wall intervals, not GPU timestamps or hardware guarantees.

The built-in global terrain remains approximately 20 km source spacing. The
separate sample uses native GLO-90 elevations normalized to WGS84 ellipsoidal
height and contains 765 runtime tiles (nine complete level-10 subtrees through
level13). Its 65-point runtime grids and a level-13 render mesh do **not** turn
the source into surveyed metre-resolution data. Outside supplied real DEMs, the
global baseline remains available. Supplying scenery with `--global-terrain off`
and no `--tiles` directory is rejected: there is no displayed ground support.

Inside loaded scenery coverage, the app requests level13 within 5.5 km, subject
to the existing maximum-level and tile-count limits. It uses65-point meshes only
for level12+ tiles whose DEM has at least65 samples per axis and whose footprint
intersects immutable scenery bounds. The policy is stable across camera motion
and resets. No pack, distant packs and33-point global fallback retain the original
mesh policy. A dense source grid is a format heuristic, not proof of provenance.

Regional terrain should contain complete primary child subtrees. A sparse pack
can leave a real coarse parent displayed while physics finds a finer child: the
streamer deliberately does not fill the missing siblings with unrelated global
data. More requested LOD cannot manufacture missing source coverage. See the
[measured regional refinement report](qa/regional-render-detail-2026-10-02.md).

## Display, bounds and remaining limits

- Roads, simple footprints and selected land polygons come from the loaded OSM
  database. Widths and heights may be explicit source values or labeled defaults
- Flat roofs, facade colours/windows, road paint and individual conifer-shaped
  trees are original procedural depiction. Trees are placed only within sourced
  forest polygons; they are not a surveyed vegetation inventory. Each tree has
  two closed, downward-facing crown bases so opaque foliage stays visible from
  below (96 vertices / 32 triangles per tree, within the unchanged batch limits)
- Roads and land polygons are clipped/draped onto the actual displayed terrain
  facets and swap atomically with that terrain. Airport pavement/markings and
  supported light fixtures use the same path; compound signs are not included
- Near-vertical support facets above60 degrees are omitted rather than making a
  whole road/land cohort vanish. Extreme precision/work limits can hide or reject
  an overlay. These bounds are reported, not silently raised
- Buildings and trees still use sampled physical DEM heights. Even with matching
  source data, bilinear physics and triangle meshes can differ by metres on a
  saddle. Foundation/contact equality is not guaranteed, and live replacement of
  stationary solid geometry after a late DEM change is not implemented
- The scene selects at most4,096 nearby features within4.5 km. Each update tries
  at most8 mesh batches and uploads at most65,532 total mesh vertices, including
  ground. A complete batch that cannot fit waits for the next update; invalid or
  empty attempts still count. Prepared vertices are capped at600,000, ground vertices at90,000,
  ground batches at24, trees at2,048 and tree-placement attempts at8,192. Dense
  scenes can omit features; diagnostic counts explicitly report truncation
- Tree clearances use a separate2.3 km query around the observer. If that query
  saturates or its bounded exclusion index overflows, vegetation is suppressed
  conservatively. A saturated outer4.5 km display query alone does not disable it
- `R`, replay rewind and map relocation cancel stale work and retire owned scene
  assets. Scenery affects neither collision/contact nor replay/FDM state

The sample remains optional data with its own ODbL and Copernicus notices. It is
not included in the commercial binary allowlist. Source availability, a working
scene and permission to distribute a commercial executable are separate gates.

## Prepare your own bounded input

The bounded offline importer prepares roads, simple building footprints and
selected landcover from a user-supplied small regional OSM PBF:

```sh
cargo run -p flightsim-tilegen --bin flightsim-scenerygen -- \
  --input region.osm.pbf --output region.fsscenery \
  --source-name "Region and source snapshot date" \
  --source-url "https://official-provider.example/region.osm.pbf"
```

Use the source's real provenance URL. The importer does not download data or
accept a made-up hash; it computes its exact-byte input fingerprint. Keep the
provider URL, download date, snapshot timestamp, SHA-256 and applicable licence
with any redistributed database. FSSC itself retains source identity, required
OSM attribution and the ODbL URI. Keep OSM-derived data separate from code licensing.

Read the command's full report. It distinguishes genuine source geometry from
estimated widths/heights and counts unsupported/invalid features. A malformed or
oversized regional source fails without replacing an existing output. Individual
ways exceeding 512 points, unsupported relations, bridges/tunnels, raised buildings,
missing references and invalid polygons are omitted and reported. Multipolygon
members are suppressed even when the relation's outer way has its own tags.

The input limit is 64 MiB. This importer is intentionally not a whole-country or
planet pipeline. Prepare smaller extracts externally when a source exceeds its
published aggregate bounds; do not raise limits blindly. No contributor personal
metadata, addresses, names, raw PBF or GIS parser is needed at runtime.

## Coordinate and visual meaning

- Coordinates are f64 WGS84. FSSC stores radians exactly; altitude is always zero
- The renderer must sample its selected DEM at the actual position before display
- DEM altitudes remain WGS84 ellipsoidal metres. OSM `ele` is ignored
- A building's footprint is sourced; a 9 m default height is only a display estimate
- `building:levels` uses an explicitly marked 3 m-per-level estimate
- Widths without valid tags use explicit class-based display defaults
- Water/forest/landuse polygons are selected OSM outlines, not a complete land mask
- Road routes/roundabouts retain node order. No traffic, names or navigation claims
- Scenery cannot alter physical terrain, aircraft collision or replay physics

## Runtime API

`SceneryDatabase::read_path` is the single validated disk boundary. `roads()`,
`buildings()` and `landcover()` return immutable borrowed records.
`query_near(center, radius, limit)` ignores altitude, uses conservative ECEF bounds,
uses exact local-tangent-plane line/polygon distance after the broad phase, and
caps results at 4,096. A result may include geometry
partly outside the radius; consumers still need bounded clipping/draping/meshing.

FSSC v1 and its limits are specified in
[ADR-0012](adr/0012-bounded-regional-surface-scenery.md). Rendering integration and
actual-scene acceptance are independent of a successful bake. Neither a parser
test nor a non-empty output proves a visually correct or performant application.

## Tests

```sh
cargo test -j2 -p flightsim-world --test scenery_database
cargo test -j2 -p flightsim-tilegen --test scenery_pipeline
cargo clippy -j2 -p flightsim-world -p flightsim-tilegen --all-targets -- -D warnings
```

All checked-in PBF fixtures used by these tests are deliberately synthetic. The
separately prepared Liechtenstein fixture comes from the public Geofabrik extract;
it is not checked into source or implicitly approved for a commercial bundle.

Actual data benchmarks can be repeated without bundling data into the source tree:

```sh
FLIGHTSIM_SCENERY_BENCH_FILE=/path/to/region.fsscenery \
  cargo bench -j2 -p flightsim-world --bench scenery -- --quick
```

Without the environment variable, that benchmark uses explicitly synthetic roads.
