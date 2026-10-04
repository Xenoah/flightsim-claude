# Global map and seasonal climate

This branch adds an offline, complete-world geographic baseline. It is coarse
terrain for exploration, not a replacement for detailed local DEMs or a real
flight-planning database. See [ADR-0011](adr/0011-offline-global-terrain-climate.md)
for the architecture and [source provenance](data/global-sources.md) for the data.

## Start and navigate

Normal new flights enable the bundled world terrain and monthly climate. No
account, API key or runtime Internet connection is needed.

```sh
cargo run -p flightsim-app --release -- --world-map
# Start directly on the monthly temperature layer:
cargo run -p flightsim-app --release -- --world-map --map-layer climate
# Open the integrated, paginated data-source notices:
cargo run -p flightsim-app --release -- --map-credits
```

- `M`: open or close the world map
- Click the map, choose a region preset, or click LAT/LON and enter exact degrees
- `Ctrl+A`: clear the coordinate entry; `Enter`: apply it
- Terrain and Climate tabs switch the visual layer
- Previous/next month changes the preview only
- PgUp/PgDn or the arrows beneath the map select the next-flight aircraft;
  see [aircraft choices, readiness and cancellation](aircraft-picker.md)
- Start new flight places the aircraft 1,000 m above sampled ground
- The new flight resets the current unsaved recording. Close the map and use `F9`
  first if you want to preserve it
- `Esc` dismisses data credits first, then the map; it does not also toggle flight
  pause on that same press
- An already-paused flight remains paused after browsing and closing the map
- Replay allows map preview but locks the recorded aircraft and disables relocation
- Explicit `--global-terrain off` keeps legacy-aircraft map starts unavailable.
  Restart with `--global-terrain on` to navigate in a legacy aircraft. Supported
  flat-zero jets retain their explicit exception; the map never changes terrain
  underneath airport surfaces that were already placed for the legacy source

Small windows keep the same text and control sizes. Below 900 logical pixels
wide or 600 high, the map and settings stack in a bounded vertical scroll area;
use the mouse wheel to reach aircraft, weather, coordinates and Start. The map
header and recording warning remain outside this area. Credits, Regions and the
wind editor also scroll when needed, with a visible scroll/return hint. Closing
the map resets its scroll position; opening a child panel preserves your place
on the map. PgUp/PgDn retain their existing aircraft/package actions.

The map's MSL height is the coarse orthometric surface elevation `H`. The flight
HUD's ALT is WGS84 ellipsoidal height `h`, not a QNH-adjusted barometric altimeter;
these differ by geoid undulation `N` (`h = H + N`). AGL and `--fly` use height above
the active physical ground sampler, including a regional DEM when available.
The map preview still uses the coarse global source and identifies that limit.

The initial airborne speed is density-adjusted. This is initialization, not an
autopilot: the pilot must still control the aircraft after the jump. Region presets
are geographic destinations, not a claim that a real airport is loaded there.
The synthetic runway remains explicitly synthetic at its original location.

For takeoff, climb, EAS, retained throttle/trim and stall recovery, see the
[keyboard flight guide](keyboard-flight.md).

## Command-line scenarios

```sh
# Alps, January, 1.5 km above ground, local mean solar morning
cargo run -p flightsim-app --release -- \
  --start 46.58,8.00 --fly 1500 --date 2026-01-15 --time 09:30 --view chase

# Southern summer and a wet tropical region
cargo run -p flightsim-app --release -- \
  --start -3.12,-60.02 --fly 1000 --date 2026-01-15 --view chase

# Explicit local DEM takes priority over the global baseline at every level
cargo run -p flightsim-app --release -- \
  --tiles data/tiles --start 35.55,139.78 --max-level 13

# Legacy flat/no-global mode and standard ISA atmosphere
cargo run -p flightsim-app --release -- --global-terrain off --climate off
```

`--fly` accepts 100 through 12,000 metres AGL. The last valid `--fly`, `--approach`
or `--drop` option selects the starting scenario. `--date` accepts a real Gregorian
date in 1900-2100. The default date is fixed at 2026-06-21 for reproducibility.
`--time` is local mean solar time, not civil timezone or a live clock.

Explicit `--cloud-cover`, `--cloud-base`, `--cloud-top` or `--cloud-visibility`
keeps the manual cloud layer instead of the monthly regional cloud amount.
`--wind` and `--turbulence` remain explicit simulator settings. They are not fetched
from current weather, and monthly mean precipitation is not a storm forecast.

## What the layers mean

Terrain colours are procedural, climate-derived visual cues:

- Blue: water surface
- Sand/brown: dry and warm regional conditions
- Green: wetter regional conditions
- White: a temperature-derived snow/ice cue

Climate colours show the interpolated monthly near-surface temperature, adjusted
from the reanalysis model elevation to the rendered terrain: blue is cold, cream
is temperate, red is hot. The sidebar shows height and climatological temperature,
precipitation and cloud fraction. These are not airport observations.

Neither layer is satellite imagery, measured vegetation cover, a snow-depth map,
or today's weather. Cloud-layer geometry and snow/ice appearance are approximate. The automatic
layer uses a nominal 1,500-2,700 m AGL envelope; manual cloud settings override it.
Water surfaces do not include waves or ditching hydrodynamics.
The climate source itself is NOAA NCEP/NCAR Reanalysis 1 monthly 1991-2020 means.

The bundled terrain is roughly 20 km sampling. Finer generated mesh tiles preserve
geometry and earth curvature; they do not create missing 30 m source detail.
Small islands, local ridge lines, detailed coastlines and airport grading remain
unresolved. Use the existing regional `.fsdem` pipeline where that detail matters.
Lake surfaces are static estimates. The source ledger flags160 of942 represented
lakes as low confidence where a flat water plateau could not be isolated from
the overview; narrow or steep-shore lakes can retain substantial height error.

## Physical integration and replay

`flightsim-sim` samples a fixed climatological date at the moving aircraft position.
A regional temperature offset changes the ISA atmosphere's temperature, density
and speed of sound, so aerodynamic forces respond. Pressure remains ISA; there is
no live QNH, atmospheric sounding or real storm physics.

The chosen climate date is fixed for one flight. Advancing the visual solar clock
does not change the physical climate behind the recorder's back. Choosing a new
month and starting a new flight records the new date explicitly.

- Legacy v1 recordings keep legacy terrain/ISA behavior and byte compatibility
- V2 recordings include enabled terrain/climate fingerprints and climate phase
- A different bundled data identity is rejected instead of silently replaying a
  different trajectory
- Local tile directories remain caller-provided and must match the original
  recording; a global fingerprint does not attest arbitrary external DEM files

## Loading, fallback and limitations

The renderer counts missing reads, failed reads and generated fallback tiles
against the same per-frame budget. Hidden meshes are prepared separately under
an equal budget, and visible coverage switches as one non-overlapping cut. A
primary DEM found later replaces global fallback at that ID. Initial coarse roots
can cover the world while finer primary discovery continues.

When the LOD leaf budget is exhausted, coarse leaves remain rather than dropping
unvisited parts of the globe. Local ground height is used when choosing detail,
so a high-altitude runway is not mistaken for an aircraft kilometres above ground.
The result is still a bounded local-height estimate, not a surveyed surface.

Exact source registration, vertical datum conversion, shoreline/lake corrections,
quantization, polar policy and remaining error sources are described in
[global terrain](global-terrain.md) and [global climate](data/global-climate.md).
Use the map's Data credits panel and distributed ATTRIBUTION.md for full notices.

Diagnostic terrain lines distinguish displayed tiles, availability-aware live
leaves and the last sampled desired leaves. `displayed_match true` requires exact
ID equality and completed seam commitment, rather than just equal counts. A
truncated desired cut still has coarser detail than the requested screen-space
error; this readiness flag does not make the source higher-resolution.


## Verification status

Source inspection and an atlas bake are not equivalent to a passing application.
The current development checkpoint and QA report distinguish successful tests,
failed tests, unrun stages, real screenshots, and hardware limitations. Until the
final QA record is written, commands above describe the implemented interface and
must not be interpreted as a completed Windows release or hardware validation.
