# ADR-0028: Bounded source-aware discovery of regional terrain roots

- Status: implemented; acceptance tracked separately
- Date: 2026-10-07

## Problem

At Balzers (47.068 N, 9.501 E), the default 16 px selector at +3000 m AGL
requests no tile finer than L9. The prepared real DEM begins at L10. Ancestor
fallback cannot discover descendants, so all 35 requested tiles were global
fallback even though the physical sampler found the real regional terrain.
Coarse global relief is not a substitute for high-altitude real-DEM acceptance.

## Decision

Add an optional immutable `TileSource::primary_coverage` hint, forwarded by
references, boxes and global wrappers. `PrimaryCoverage` contains only strict
ancestor paths to the coarsest supplied primary IDs. An available ancestor stops
a floor from demanding all its finer descendants. Sparse roots are allowed;
missing siblings are still supplied by the existing availability/fallback logic.

The selector follows these paths within the horizontal local-detail footprint
(Short 3.25 km, Standard/default 5.5 km, Long 11 km), independent of AGL, and then
returns to ordinary SSE. The footprint uses core’s conservative WGS84 distance
bound, retaining dateline/polar intersections. Its conservative box and complete
child splits can include nearby false positives. Existing maximum LOD, hard
refinement radius and 4096-leaf ceiling remain authoritative; an exhausted leaf
budget remains explicitly truncated and always covers the whole globe.

Package sources construct hints from their already validated manifest paths.
Raw directory sources opt in only at render-source preparation: canonical
`level/x/y.fsdem` regular-file names, at most three directory levels, 32768
accepted entries and 16384 tile records (one extra record detects overflow). Symlinks and unrelated names supply no
hint. Directory I/O or limits discard the whole snapshot; app logs the error and
keeps ordinary SSE discovery. Physical, airport and weather sources do not scan.
A later raw-directory addition requires source recreation to refresh its hint;
normal payload reads/retries remain dynamic and are never disabled by a hint.

Per-frame selection performs only bounded in-memory lookups. Index storage is
at most 24 ancestors per accepted tile, and construction counts duplicate input
records against its limit. Hints are advisory, not successful reads: the normal
reader still verifies every DEM, and missing/corrupt reads consume the existing
read budget. Mesh, cache, residency, bridge and overlay budgets and atomic cut
commit are unchanged. The current source is consulted on each selection, so
switching sources cannot retain the previous source’s floor.

## Rejected alternatives

- Raising global minimum LOD: requests nonexistent fine data around the planet
- Fabricating coarse real ancestors from a small crop: a tile’s complete footprint
  is its contract; extrapolation would invent geography or mix source identity
- Using physics cache/resident meshes as coverage: cold startup and eviction
  would change discovery and conceal the original failure
- Unbounded directory scans or per-frame filesystem probes: violate work bounds

## Unchanged contracts and limits

No DEM bytes, source notices, measured resolution, physical sampling, FDM or
replay schema/identity changes. Package-backed replay is still refused. A hint
is not a measured geometric-error hierarchy and does not guarantee every ridge
or remote part of a region is requested. Maximum LOD below a regional root still
prevents that root being loaded. No global source receives a synthetic hint.

Regression tests cover real package/raw parity across the old altitude boundary,
empty/distant hints, sparse/missing sources, climb/move/return transitions,
max-level/radius/leaf bounds, poles/dateline, malformed paths and invalid payloads.
Actual-scene, moving/depth/night, physical GPU and final exact-main CI/release
acceptance remain separate. Existing nine binary-distribution gates are unchanged.
