# Global runtime integration: independent static review

Review time: 2026-10-02 02:16 UTC. This reviews the working tree over base
`2cde734a996b2af8c55a70c6b9ad9efa935e50d7`, not a published build.

## Result and evidence limits

The four substantive defects identified in the first pass are corrected in the
reviewed source. The related inland-water, practice-traffic and runway-scoring
integration gaps are also addressed. No additional proven functional blocker was
found in the reviewed root-seeding, fallback-ancestor, provenance or budget logic.
The low-priority reporting defect found during this pass was also corrected and
re-inspected before this report was finalized. No proven issue from this review
remains open in the inspected source; runtime acceptance is still outstanding.

This is **static verification, not a runtime acceptance or compilation pass**.
The Rust toolchain is absent and installation approval remains pending. The added
Rust tests, Bevy schedule, actual renderer, replay integration and aggregate gates
have **not been run** by this review. The architecture script was attempted during
the first pass and stopped at its dependency-tree prerequisite with
`cargo: command not found`; that is neither an architecture pass nor evidence of
an actual dependency violation. No software was installed, no production source
was edited, and no commit or publication was performed by this reviewer.

`git diff --check` passed again during the second pass. Two independent Python
standard-library arithmetic checks were run, as described below; these do not
execute or validate the Rust implementation.

## Corrected findings

### 1. Polar terrain-color callback panic — former P1

- Source: `crates/flightsim-render/src/terrain.rs:154-165`
- `terrain_vertex_colors` now clamps reconstructed latitude to the closed
  `[-PI/2, PI/2]` domain before calling the geographic color sampler. This removes
  the path from valid southern tile-edge roundoff to the `GlobalTerrain::sample`
  rejection and `WorldRuntime::appearance` panic.
- The extracted helper is used by `spawn_tile_impl`, so the fix applies to the
  actual colored-mesh path rather than only a test helper.
- New regression: `colored_mesh_queries_never_cross_a_closed_polar_endpoint`
  checks both polar rows at levels 0, 6, 10 and 13, all callback coordinates, and
  color/vertex count agreement. Present but **unrun**.
- Independent IEEE-f64 arithmetic modeling checked 1,650 coordinates across
  levels 0 through 24, both polar rows and all 33 mesh rows. The original formula
  overshot at levels 6, 10, 13, 15, 20 and 24; the clamped formula kept every
  coordinate in range. This is arithmetic evidence, not rendered-scene evidence.

### 2. Airborne starts confused ground speed with airspeed — former P2

- Source: `crates/flightsim-app/src/world_runtime.rs:233-242`
- Initial ground NED velocity now adds `startup.wind.to_ned().0` to the target
  density-adjusted air-relative velocity. The preserved steady wind therefore
  cancels correctly when the FDM computes relative air velocity.
- New regression: `airborne_start_preserves_target_airspeed_in_head_tail_and_crosswinds`
  verifies both magnitude and direction after subtracting wind. Present but
  **unrun**. Turbulence remains a separate deterministic environmental effect.
- Independent vector arithmetic checked 16 heading/wind-bearing combinations;
  adding and then subtracting steady wind preserved the chosen relative speed.
  This does not substitute for FDM execution or aircraft-envelope validation.

### 3. Relocated tower could be buried on sloping terrain — former P2

- Source: `crates/flightsim-app/src/world_runtime.rs:245-252,278` and
  `crates/flightsim-app/src/main.rs:2979`
- Both startup and map relocation use `tower_anchor`. It obtains the offset
  coordinate through core's local ECEF frame, samples terrain at that coordinate,
  then adds the explicit 25 m tower clearance. Departure elevation no longer
  substitutes for the tower's actual supporting terrain.
- New regression: `tower_uses_its_own_ground_on_slopes_and_stays_valid_at_both_poles`
  checks a synthetic slope and finite valid coordinates at both poles. Present
  but **unrun**. The polar portion uses a missing-regional-data fallback rather
  than proving actual bundled polar terrain appearance.

### 4. Tower LOD incorrectly followed the aircraft's ground elevation — former P2

- Source: `crates/flightsim-app/src/main.rs:3331-3344`
- Tower mode derives its fixed ground reference from tower altitude minus the
  same `TOWER_CLEARANCE` used to construct it. Other views retain the aircraft
  ground reference. This matches the existing tower observation point and no
  longer changes tower-local tessellation as the aircraft crosses other relief.
- An actual tower-mode scene, aircraft movement over different elevations and
  camera-mode switches remain runtime validation items.

### Related new-flight consistency corrections

- `world_runtime.rs:353-357` distinguishes `Inland water` using
  `is_inland_water`, with `Land` and `Ocean` as the other cases.
- `traffic_runtime.rs:86-89` replaces only an already-enabled synthetic traffic
  source. `world_runtime.rs:293-295` calls it after the new simulation has been
  validated. LAN session state is untouched. Existing synthetic entity IDs are
  updated by the normal traffic update, ordered after the new-flight transaction.
  The new reanchoring regression is present but **unrun**; it does not test a live
  LAN connection.
- `main.rs:2370-2393` returns absent runway metrics outside the known active
  airport's 15 km ECEF radius. `report_landings` consumes those optional values.
  This avoids grading a remote world destination against the departure runway
  without inventing a destination runway. The new local/remote metric regression
  is present but **unrun**.

## Cold-start and supported-fallback-ancestor review

Sources: `crates/flightsim-render/src/terrain_selection.rs` and
`crates/flightsim-world/src/lod.rs`.

- Complete-globe best-first selection starts with both roots and replaces one
  leaf with all four children only when the extra three leaves fit the budget.
  Exhaustion therefore retains a complete, non-overlapping cut rather than
  discarding an unvisited hemisphere.
- Root seeds are considered only for root domains completely covered by the
  requested leaves and not already fully covered by the live cut
  (`terrain_selection.rs:289-305,422-427`). Partial regional requests do not
  automatically seed unrelated hemisphere geometry.
- A root primary read precedes a root fallback generation. Both are counted by
  the same `load_attempts < frame_budget` loop, including misses and failures
  (`454-504`). Mesh preparation uses its separate bounded counter, including
  cached preparation (`543-573`). Seed requests do not bypass either budget.
- Primary and generated provenance remain separate for cache and resident
  lifetimes. Root seeds remain fallback data; they do not terminate subsequent
  primary searches. Real same-ID replacement is prepared hidden and explicitly
  revealed by the committing cut.
- After primary ancestors have been searched, unsupported fallback leaf levels
  record a fallback attempt and permit traversal to their ancestors
  (`111-148`). An existing resident/cached fallback ancestor terminates further
  redundant generation. A real available ancestor prevents fallback generation
  from taking priority over it.
- The new `global_cold_start_seeds_complete_globe_before_fine_discovery` and
  `global_fallback_uses_supported_ancestor_above_its_tessellation_cap` regressions
  exercise these intended transitions but are **unrun**. Cold-start time,
  moving-camera convergence, visual refinement transitions and cache-pressure
  behavior still need actual Rust/runtime evidence. No performance claim is made.

## Follow-up diagnostic correction

### Selection-budget truncation disclosure — former P3

- The intermediate snapshot discarded `LodSelection.truncated` in both streaming
  wrappers, preventing application disclosure of unmet requested screen-space
  accuracy. This was a pre-existing zero-height-wrapper gap carried into the new
  surface-aware wrapper, not a coverage hole.
- Corrected source: `crates/flightsim-render/src/terrain_selection.rs:95-98,367-390`
  stores the flag in selection state for both entry points.
  `crates/flightsim-app/src/main.rs:3423-3427` reads it in the existing rate-limited
  report and warns when a sampled reporting interval enters the limited state.
- `streaming_exposes_when_lod_detail_was_limited` covers setting the flag with a
  two-leaf budget, then clearing it through the other wrapper. It is present but
  **unrun**. These corrective changes were inspected at 02:16 UTC.

## Reviewed file fingerprints

SHA-256 values captured during this pass; later working-tree changes require
their own verification:

```text
5b4281cfbe3480ebfd47a214d7f90a883709187930f6579c8c5e5e10d5bd05f2  crates/flightsim-app/src/world_runtime.rs
cb356372f96e7010265f29bb279271a84a75f0f1d86ad9937a90cc46ce26af9e  crates/flightsim-app/src/main.rs
d259e6e4fd3cb9e1d70768f13510d73d7f7787ee55e9158bf8eccd8e365f4f8d  crates/flightsim-app/src/traffic_runtime.rs
7a11c8b62a42c4482bce057564e46b605664db36cc099015f177213a78834720  crates/flightsim-render/src/terrain.rs
4aa44e9f0727bca416ccc46030a41c48bb46d5d0e300a67066a2993a4ee9a98b  crates/flightsim-render/src/terrain_selection.rs
69e45c6a040e2e944c2ae0fcbc0cd5fec928943ce6dccb03d1498f2fc8b0a27c  crates/flightsim-world/src/lod.rs
```
