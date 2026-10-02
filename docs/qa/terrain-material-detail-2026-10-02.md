# Procedural terrain material detail — 2026-10-02

## Scope and attribution

The terrain-only material adds original procedural variation to the existing
linear climate/elevation palette. It does not contain satellite imagery,
land-cover classification, surveyed vegetation, downloaded textures or another
product's artwork. Its Rust/WGSL code follows the repository's MIT OR Apache-2.0
license. No new runtime asset, network request, account or external attribution
is required. Naga is an already-locked Bevy dependency used directly only for
shader-helper validation in development tests.

This work changes albedo and perceptual roughness only. It does not change
mesh vertices/normals, DEM values, geographic source priority, runway contact,
collision, FDM, replay outcomes, sun, climate or water geometry. Existing airport
surfaces retain their StandardMaterial. Terrain surface and bridge functions
accept the same generic material handle, so the app can apply one shared terrain
material to both without a tile-specific shader seam.

## Coordinates and bounded shading

- `TerrainDetailPlugin` embeds the shader in the executable. No loose shader
  file or separately downloaded texture is required by a packaged offline build
- The authoritative location remains f64 ECEF. `RenderFrame` supplies all
  position/axis conversions. No new geodetic sin/cos conversion is introduced
- The CPU divides the f64 origin by each fixed cell size and reduces it modulo
  256 cells **before** narrowing. Each of the 2048 m / 256 m / 32 m / 4 m bands
  has its own phase, so small bands do not inherit the large band's rounding
- The fragment's existing relative render position is rotated back to ECEF
  displacement using those core-derived basis columns. The pattern has no tile
  index, UV mapping, latitude/longitude singularity, random seed or time input
- Four fixed 3D value-noise evaluations use periodic integer hashes and quintic
  interpolation. There are no octave loops. Each band fades from 0.12 to 0.48
  cells per pixel using the derivative matrix's Frobenius norm (an upper bound
  on its largest directional footprint); unresolved bands are
  skipped. This is conservative visual filtering, not an exact analytic filter
- Total linear-albedo modulation is clamped to ±25%. The finest band's maximum
  contribution is 3.5%. Roughness stays within 0.72–1.0 when detail is active
- Blue dominance and high brightness in the authored palette smoothly suppress
  detail over its water/snow cues. This is deliberately a conservative colour
  heuristic, not an independent geographic land mask or observed snow product
- `SurfaceDetailSettings { enabled: false }` leaves the original PBR inputs
  unchanged. The app owns the user-facing on/off option

Changing render LOD can still change the actual triangulated surface position
and existing vertex palette. The material gives equal coordinates at equal
world positions; it cannot make different coarse and fine geometry identical.
All f32 render representations retain their ordinary rounding bounds.

## Verification

Implementation-stage results (integrated GPU acceptance is separate):

- `cargo check -p flightsim-render --tests`: passed in a fresh target directory
- The actual procedural WGSL helper parses and validates with Naga 27
- A separate development probe composed the complete shader with 46 actual
  Bevy 0.18.1 shader imports. Forward and deferred variants both passed Naga
  parsing/validation. This does not establish runtime GPU pipeline success
- Focused unit tests cover finite/bounded phases and basis, independent f64
  ECEF reconstruction, rebasing, equivalent dateline/pole frames, deterministic
  periodic noise reference, palette suppression, derivative filter contracts,
  shader source invariants and late material/settings synchronization

Focused unit-test execution, strict lint and integrated runtime/screenshots are
recorded below when complete. No GPU frame-rate, Steam release approval or
visual perfection is implied by shader validation or source tests.

The rendering-system regression also prepares two real terrain surfaces and
an actual bridge transaction using `TerrainMaterial`, then inspects their ECS
material components. This ensures both paths select the extended terrain
material while the StandardMaterial path remains available to airports.


### Focused execution results

All 13 material tests passed in the fresh `flightsim-ground-material-build`
target. The actual-transformed-mesh test uses existing f32 tile vertices and
Bevy `Transform -> GlobalTransform` matrices, including independently encoded
bridge meshes. Maximum changes in the bounded **albedo multiplier** were:

- Ordinary L3/L7: rebase 0.011946; source/bridge shared knots 0.000341
- Dateline L3/L7: rebase 0.007220; source/bridge 0.000119
- North pole L3/L7: rebase 0.006534; source/bridge 0.000886
- South pole L3/L7: rebase 0.011789; source/bridge 0.000284
- L0/L4 large-parent stress: rebase 0.046917; source/bridge 0.000343

These are CPU-reference numerical deltas, not screenshot pixel measurements.
The stress probe intentionally leaves all bands fully enabled across every
mesh vertex, including distant and far-side root geometry where normal
footprint filtering suppresses small bands. It exposes the remaining ordinary
f32 mesh/transform error instead of claiming exact welding. The test also
requires at least two genuinely shared knots in every fixture (42 ordinary
and dateline, 108 each pole in this run).

Architecture checks, all-target `cargo clippy -p flightsim-render -- -D warnings`,
strict-warning renderer rustdoc, formatting and diff-whitespace checks passed.
The final 13 focused tests were rerun after the conservative Frobenius-footprint
refinement and passed. The full renderer suite subsequently completed: **264 passed, zero failed**,
with two pre-existing ignored tests (manual parent-fallback render probe and
stitch-planning timing probe). This includes all eight stitching and three
streaming-system tests after the generic material API change. Integrated
GPU/native acceptance is owned by the app integration task.
