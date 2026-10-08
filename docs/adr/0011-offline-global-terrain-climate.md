# ADR-0011: Offline global terrain and monthly climate beneath regional DEMs

- **Status**: Implemented in the global-map development branch; validation tracked separately
- **Date**: 2026-10-02
- **Context**: Requested complete-world terrain and climate navigation

## Problem

The geographic quadtree already covers the planet, but an absent local `.fsdem`
previously meant ellipsoid height zero and no rendered terrain. That is not a
geographic world map. A full 30 m global bake would require hundreds of GB and
millions of files. Live weather services would add credentials, availability,
licensing and nondeterminism unrelated to a first reliable offline world.

## Decision

1. Bundle a compact, versioned NOAA ETOPO 2022 elevation + matching EGM2008 geoid
   + independently sourced Natural Earth geographic land mask. Offline baking
   produces a bounded immutable atlas, not millions of baked runtime tile files.
   The canonical grid and shoreline/inland-water policy are specified in
   [global terrain](../global-terrain.md) and [source provenance](../data/global-sources.md).
2. Keep the physical contract `h = H + N` in WGS84 ellipsoidal metres. Open ocean
   is its water surface, not seafloor bathymetry. An independent mask preserves
   real below-sea-level land. Coarse source disagreements must be documented and
   resolved explicitly at bake time, never silently called surveyed geography.
3. Regional `.fsdem` files retain priority at every ancestor level. `TileSource`
   exposes primary reads separately from global fallback generation and direct
   fallback height sampling. A fine generated fallback cannot mask a real,
   coarser local tile. No runtime network, GeoTIFF, NetCDF, GIS or credential use.
   Only an explicitly immutable `EmptyTileSource` opts out of primary discovery
   through `primary_reads_possible() == false`; the default is conservative true.
   An absent configured directory or currently empty memory source is not evidence
   that future primary reads are impossible. Cached/resident primary data retain
   precedence even when a caller reuses them with an explicitly empty source.
4. Existing bounded DEM LRU and renderer read/mesh budgets remain authoritative.
   Missing/failed reads and fallback generation all consume the read budget.
   The renderer tracks fallback provenance and retries primary data. A real
   replacement at the same tile ID atomically replaces the visible mesh.
   Curved-Earth/source gaps use actual-edge bridge ribbons, shared corner caps
   and polar fans. Surface and bridge builds share that existing mesh budget;
   selection pauses while the previous visible cut is retained, then the new
   cut and its bridges commit together. Source generations invalidate bridges.
   The old same-ID entity remains alive until commit. Explicit relocation also
   cancels and destroys all pending/retired terrain assets. Geometry residency
   has explicit old/new transition bounds and observable logical byte counts
   ([mixed-LOD QA](../qa/terrain-mixed-lod-stitching-2026-10-02.md)).
5. LOD selection accepts a local terrain-height reference. Distance to the
   ellipsoid alone incorrectly coarsens the mesh at high-altitude airports.
   This is still an estimated local reference, not a terrain bounding-volume
   hierarchy. Render-only LOD never writes back into the physical sampler.
6. Bundle NOAA NCEP/NCAR Reanalysis 1 monthly 1991-2020 temperature, precipitation,
   and cloud-cover climatology. Retain native Gaussian latitude registration and
   periodic longitudes. Interpolate smoothly between months and over the seam,
   with one value at each pole. No wall clock, random generator or live API.
7. `flightsim-sim` alone connects sampled climate to flight dynamics. A local
   ISA temperature offset changes temperature, density, sound speed and hence
   aerodynamic forces. Pressure remains ISA: this is not live QNH or a sounding.
   Lapse correction, climate labels, surface biome colours, cloud-layer geometry
   and snow/ice appearance remain identified modeling approximations.
8. A modal `M` world map presents the real-data raster, exact-coordinate entry,
   geographic presets and monthly previews. Preview edits do not alter a flight.
   An explicit new-flight start resets recording and places the aircraft above
   sampled ground. The map independently gates physics, input, solar time and
   sound while preserving a pre-existing pause.
9. Replay v1 is retained byte-for-byte for legacy settings and restored as legacy
   world/ISA behavior. Replay v2 records global terrain identity, climate identity
   and fixed annual phase. Unknown/invalid settings and incompatible data
   fingerprints are rejected. The selected climate date is fixed for a flight;
   accelerated visual solar time does not silently change physical conditions.

## Alternatives rejected

- Full global GLO-30 bake: incompatible with this bounded download/package budget
- On-demand commercial tiles/live weather: adds accounts, credentials, recurring
  cost and network-dependent reproducibility before an offline foundation exists
- Procedural continents or latitude-only climate: would not reflect real geography
- WorldClim bundling: current terms require permission for redistribution/commercial
  use; public availability is not a redistribution grant
- Reusing global fallback as a primary fine tile: silently masks local DEM ancestors
- Increasing a fixed skirt constant or assuming one-level adjacent LOD: actual
  default cuts can differ by four levels, and source availability can make that
  larger; height error does not bound curved-Earth chords. Actual-edge bridges
  avoid arbitrary depth assumptions while preserving original DEM surfaces.
- Making contact depend on the currently visible LOD: flight/replay outcomes would
  change with camera, frame cadence and cache state
- Calling monthly means weather: misleading. Reanalysis climate is not today's
  observed conditions, an airport METAR or a forecast

## Costs and limits

- Global relief is coarse, roughly 20 km sampling; small islands, local ridges,
  detailed shoreline, airport grading and buildings are unresolved. Use regional
  DEMs for meaningful airport/contact detail. This is not a navigational database.
- Regridding, quantization, mask disagreement and mesh triangulation are separate
  error sources. Source resolution and rendered geometric tessellation are not
  the same. Numerical regression and actual-scene checks must report both.
- Monthly mean wind is not being used as today's wind. Explicit wind/turbulence
  and cloud overrides remain available; no live weather claim is made.
- Coarse local-reference LOD does not solve every steep remote terrain case.
  Runway/light occlusion needs measured shared-surface validation, not assumption.
- New default world/climate settings intentionally change new-flight physics;
  old recordings must explicitly keep legacy settings to remain reproducible.
- Models, GUI, controls, source readers and render-time state require actual app
  and aggregate checks; a successful atlas bake alone is not application success.

## Verification

See the current global-map QA record. Required gates include independent source
and byte checks, corruption tests, regional/seasonal contrasts, dateline/poles,
primary-ancestor precedence, bounded streaming under cache pressure, repeated map
start/close/preview flows, v1/v2 replay, actual native/offscreen screenshots,
aggregate split tests, clippy, strict documentation and architecture checks.

## 2026-10-07 extension: regional roots above the SSE cut

[ADR-0028](0028-bounded-primary-terrain-coverage.md) adds optional bounded,
read-free source coverage hints. Within the current local-detail footprint,
selection can reach a region's coarsest declared tiles even when high-altitude
SSE would stop above them. The usual availability checks, primary provenance,
streaming/mesh budgets and atomic seam commit remain authoritative. This does
not change physical sampling, the global atlas or package replay restrictions.
