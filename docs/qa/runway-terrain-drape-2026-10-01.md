# Runway terrain drape and shallow-angle light visibility

## Reproduced defect

The October 1 real, EGM2008-normalized Copernicus run showed only a few runway
lights from the cockpit. The renderer used the threshold altitude for every
runway and light vertex. Pavement was one quad over the entire runway. A changing
DEM therefore covered the runway and lights even though the threshold was placed
correctly. The horizontal light quads also approached zero projected area at
shallow viewing angles.

## Rendering contract

The backward-compatible `runway_mesh` and `runway_light_meshes` helpers still
accept a threshold, heading, length, and width and produce constant-ellipsoid-height
geometry. The new `*_with_elevation` helpers accept one additional
`FnMut(Geodetic) -> Meters` argument, and retain the same tuple return values.
The app supplies terrain elevations using its existing source and fallback policy.
These renderer modules do not fetch terrain, import simulation types, change the
physical runway, or modify FDM ground planes.

- All geodetic/ECEF transformations remain in `flightsim-core`
- Absolute coordinates remain f64 ECEF until subtracting the mesh origin
- Pavement grid spacing is at most 10 m, with extra splits at every paint edge
- Each grid point is sampled once and shared by pavement and paint cells
- Paint and pavement share corners and triangulation, preserving the existing
  0.08 m pavement and 0.13 m paint lifts over every corresponding triangle
- Dimensions are bounded to 10 km length and 200 m width; an additional 65,536-cell
  cap bounds output to at most 524,288 vertices including paint
- Invalid geometry produces empty meshes and a finite origin; a non-finite or
  out-of-range elevation sample falls back to threshold altitude

Lights retain the original 1.6 m proxy width and the existing color, 6000 emissive
strength, and daylight dimming curve. Their four corners receive local terrain
heights. A top and four outward-facing sidewalls replace the single horizontal
quad. The base is ground +0.12 m and the top is ground +0.47 m. This is a 0.35 m
visual fixture proxy, not a photometric or certified airport-light model. Lights
remain grouped into three materials/meshes, with no per-light point lights.

## Regression checks

Tests cover interior terrain height variation, original surface/paint lifts,
shared paint/pavement cell geometry, triangle winding, bounded tessellation,
non-finite inputs/samples, flat-helper equivalence, per-corner light elevation,
outward light-face winding, and positive projected emitting area at elevations
0°, 0.5°, 3°, 10°, and 90° across bearings in 15° increments. The minimum
horizontal silhouette is 1.6 m × 0.35 m; the radiance test fixes the existing
6000 emissive strength and verifies daylight shutoff.

Verification on October 1: `cargo test -j 2 -p flightsim-render` passed 172
unit and 23 integration tests; `cargo clippy -j 2 -p flightsim-render
--all-targets -- -D warnings` passed. A newly added outward-face winding test
caught and corrected a sidewall orientation error before delivery. Dateline,
high-latitude, and invalid/polar-coordinate coverage is included.

Run the focused tests with:

```sh
cargo test -j 2 -p flightsim-render runway
```

## Limits and visual acceptance

A bounded 10 m grid does not guarantee complete clearance over arbitrary terrain.
In particular, `GroundSampler` samples the DEM while the visible terrain may be a
coarser, differently triangulated LOD mesh. A rendered terrain triangle can still
sit above the DEM sample. Increasing the runway lift arbitrarily would mask that
separate mismatch rather than fix it. Check real-data cockpit, shallow approach,
and overhead captures at day and night after app wiring, alongside the synthetic
flat case. CPU mesh tests establish geometry and radiance invariants, not final
on-screen visibility or frame rate.
