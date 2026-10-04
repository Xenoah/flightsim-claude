# Bounded draw distance policy

`flightsim_world::draw_distance` supplies three validated visual presets and a
numeric configuration API. App controls choose the preset; physical terrain,
DEM sampling levels, aircraft state, and replay identity never read this policy.

| Preset | Planetary refinement cap | Scenery query | Regional detail request | SSE | Camera far |
|---|---|---|---|---|---|
| Short | 10 km, coarse level 6 outside | 2.25 km | 3.25 km | 24 px | 400 km |
| Standard | Existing SSE-only selection | 4.5 km | 5.5 km | 16 px | 400 km |
| Long | SSE-only selection | 9 km | 11 km | 12 px | 400 km |

Standard retains the previous selector result exactly. It introduces no
planetary radius cap. Short suppresses subdivision outside the horizontal
footprint radius after coarse level 6, while SSE still selects detail inside.
Hard rejection uses a conservative distance to a containing WGS84 ECEF box,
computed in core from the footprint endpoints and coordinate extrema. It is
not based on the tile center or independently clamped latitude/longitude, which
can overestimate the nearest distance near poles. False positives and children
of intersecting tiles can extend detail beyond the requested radius. The legacy
SSE and near-detail distance estimates remain unchanged for exact Standard
compatibility; only the new hard-cap rejection uses this conservative bound.
Every selection still covers both hemispheres, including at the date line and
poles, without overlapping leaves. The existing bridge/overlay transaction
continues to display the old complete terrain cut until its replacement is ready.

Scenery distance selects features whose footprints are near the observer. It is
not a clipping boundary through individual buildings, roads or polygons. The
regional terrain minimum-level request surrounds this radius by at least 1 km,
independent of aircraft height, only when loaded scenery coverage intersects it.
Regional mesh tessellation stays stable per tile/source. Camera far distance is
a separate projection setting; reducing only that plane would not reduce tile
selection or scenery work. All presets retain the same 400 km plane.

Long expands the requested local region under the original hard ceilings: 4096
terrain leaves, the caller's original maximum level (default 13), 512 MiB terrain
cache, and 8 terrain load/mesh attempts per update by default. Scenery still has
at most one background worker, 4096 selected features, 128 batches, 600,000 total
vertices, 90,000 ground vertices, 24 ground batches, 2048 trees and 8192 candidate
attempts. Uploads remain at most 8 batches and 65,532 vertices per frame. Optional
ground admission remains independently capped at 40,000 source triangles.
Dense/polar scenes can therefore report truncation; Long does not promise every
requested object or fine tile will fit. These are work/storage ceilings, not
hardware FPS measurements or a total-process-memory guarantee.

## Numeric validation

`DrawDistancePolicy::new` rejects nonfinite or inconsistent settings without
silently clamping. Exported unit-bearing bounds are:

- Optional planetary refinement radius: 10–200 km
- Scenery query radius: 1–9 km
- Regional detail radius: at least scenery + 1 km and at most 11 km; must fit an
  enabled planetary refinement radius
- Camera far plane: 100–400 km; must contain an enabled refinement radius
- SSE: 12–24 px

No numeric control increases maximum level, cache, terrain leaf, preparation,
scenery residency, tree or upload budgets. Disabling the planetary radius cap
means SSE-only selection under the existing leaf bound, never unlimited work.

## App integration contract

Parse `DrawDistancePreset` at startup and use `preset.policy()`. Replace the live
terrain selector with `policy.apply_to_selector(selector)` at startup and each
change. This returns an updated selector while retaining its viewport, maximum
level, root error estimate and leaf ceiling. It removes any previous radius cap
and regional floor; `SceneryRuntime::terrain_selector` adds the current coverage
request afterward. Keep the live terrain state/cache and pending atomic terrain
transaction intact. Do not change physical `Terrain` or its source configuration.

Construct scenery with `SceneryRuntime::new_with_draw_distance(startup, policy)`.
For runtime changes call `set_draw_distance(policy)` before scenery streaming;
update camera projection separately from `policy.camera_far()`. Equal requests
are no-ops. A changed request immediately invalidates the generation and cancels
the pending worker. The next stream update reclaims prepared, staged, visible and
pending optional ground assets before it can upload an old result. It retains
that one cancelled task until completion before starting the latest request, so
rapid changes cannot create an accumulating queue of workers. This may briefly
hide scenery while replacement work is prepared. Terrain stays visible.

For CLI/UI, label this as local detail/scenery distance and show the actual
scenery kilometers. Preserve the active choice across map relocation and replay
restart. Respect modal input capture; camera far is not the headline range.
The UI/app entry points and screenshot verification are integrated separately.

## Verification scope

Pure tests compare Standard cuts against the original selector, check full
nonoverlapping globe coverage within unchanged leaf limits at the date line and
both poles, verify real Short/Long selection changes and validate numeric
boundaries. Runtime tests exercise distance changes during ready CPU work and
partial upload, and compare physical state, sampled ground and replay bytes
while cycling all presets during recording and replay. Visual/FPS acceptance
requires the final integrated app and is separate from these CPU tests.

## Controls and matched views

Use F2 to cycle Short/Standard/Long and Shift+F2 to restore Standard; startup
accepts `--draw-distance short|standard|long`. These choices persist across map
relocation and replay restart. The [integrated QA](qa/expansion-integration-2026-10-04.md)
records matched regional geometry counts and native cycling. Short visibly
flattens distant mountain relief; lower resource counts are not an FPS claim.
