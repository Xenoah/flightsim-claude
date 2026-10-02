# Airport surface versus rendered terrain: CPU diagnostic

Date: 2026-10-02. This is a numerical diagnosis and a future implementation
contract, **not a production fix, simulator screenshot, or visual acceptance**.

## Result

A terrain triangle can lie above the bilinear ground sampler even when both use
the same source data and vertex elevations. Current 10 m runway tessellation does
not remove that difference. A callback returning exact mesh heights at airport
vertices would still be insufficient: airport triangles can bridge a terrain
crease unless split at the terrain triangle edges.

The bundled global atlas, sampled at possible uniform LODs, produces these
**observed** differences over 25,000 deterministic area-distributed world probes:

| LOD | Minimum terrain minus physical ground (m) | Maximum (m) | Probes above pavement's 0.08 m lift |
| --- | ---: | ---: | ---: |
| 6 | -47.5563 | 47.0974 | 2.420% |
| 8 | -4.6928 | 4.3750 | 1.616% |
| 10 | -0.1776 | 0.2047 | 0.060% |
| 11 | -0.0869 | 0.0629 | 0% |
| 12 | -0.0143 | 0.0142 | 0% |
| 13 | -0.0039 | 0.0036 | 0% |

Positive values mean terrain is **above** the physical sampler. Negative values
can leave airport geometry floating above the terrain instead. These are sample
statistics, not global worst-case bounds, live selected-LOD evidence, or
probabilities of rendered occlusion. Coarse parent fallback/cut transitions must
be checked separately even where the desired LOD is fine.

A 3 km × 55 m coordinate-centered east/west strip near Kathmandu reaches +9.2608 m
at L6 and +0.9499 m at L8, versus +0.0010 m at L13. Additional strips cover Haneda,
Lukla, Quito, Alps, and the dateline. Each contains 7,212 probes. These generic
strips are deliberately **not represented as surveyed runway layouts**.

### Existing synthetic Haneda runway

The diagnostic also reconstructs the current `Runway::synthetic` footprint:
threshold 35.548°N, 139.775°E, heading 050°, length 2,500 m, width 45 m. This is the
simulator's fictional runway. It uses the marking-aware pavement grid from
`runway.rs`, its 0.08 m pavement lift, and all 106 fixture positions from
`runway_lights.rs`, with top surfaces at ground +0.47 m. Airport-relative f32
vertices and each triangle's geographic centroid are included.

| LOD | Minimum pavement minus terrain (m) | Minimum light top minus terrain (m) |
| --- | ---: | ---: |
| 6 | 4.3157 | 4.7030 |
| 8 | 0.1557 | 0.5499 |
| 10 | 0.0821 | 0.4719 |
| 11 | 0.0800 | 0.4703 |
| 12 | 0.07997 | 0.47014 |
| 13 | 0.07998 | 0.47007 |

None of the 16,100 pavement triangle centroids or 212 light-top triangle
centroids at any tested LOD was covered by the bundled global terrain. This
**does not reproduce the earlier regional-Copernicus overlap**. It also does not
establish whole-triangle clearance or a clear line of sight from an observer.
At L6 the several-metre positive gap illustrates terrain curvature/coarsening,
not a desired physical airport placement.

No `.fsdem` fixture exists under `/workspace` or `/tmp` in this environment.
Available source TIFF overviews are not the previous normalized runtime fixture
and are not silently substituted for it. Regional fine-DEM precedence is already
implemented, but its rendered-mesh mismatch remains unmeasured here.

## Reproduce

Existing Python and NumPy only; no downloads, installs, GPU, or Rust needed:

```sh
python -m unittest discover -s scripts/tests -p test_airport_clearance.py -v
python scripts/diagnose_airport_clearance.py --output clearance.json
```

The full deterministic result is
[airport-mesh-clearance-2026-10-02.json](airport-mesh-clearance-2026-10-02.json).
The global atlas is 2048 × 1024, SHA-256:
`57bc03c12caa772ecbd52f1d24cd12704ccc4ace5123e9d8c26b6e454907f13e`.

The diagnostic mirrors these specific source contracts:

- `global.rs`: land/water-masked orthometric height plus EGM2008 geoid, periodic
  bilinear sampling, and f32 33 × 33 generated DEM values
- `mesh.rs`: tile-center origin, geographic grid, tile-relative f32 positions,
  and NW–SW–NE / NE–SW–SE surface indices; skirts are excluded
- Terrain heights come from actual triangle-plane ray intersections along the
  probe's WGS84 ellipsoid normal, including point-in-triangle checks
- Neighbor cells are searched because ECEF edges do not project to perfectly
  straight latitude/longitude lines; all 507,504 terrain probe/LOD queries hit
- WGS84 equations are independently implemented in this offline QA script, not
  introduced into production crates or used as a new FDM sampler

This is an independent Python reconstruction, not an execution of the Rust mesh
builder. It does not include GPU Transform/rotation/depth-buffer rounding,
material rendering, mixed visible LODs, shadows, depth bias, or observer sightlines.
Source constants changing requires updating this fixture and repeating checks.

## Known-answer tests and regression fixture

Seven Python tests pass. They check known WGS84 axes; a flat plane and missed
triangle; the saddle answer below; why vertex-only projection misses a crease;
curvature on a flat ellipsoid; current synthetic airport counts; and periodic/
polar atlas behavior. The full data diagnostic completed with no missing rays.
`git diff --check` passed for this patch.

The planar saddle has NW = SE = 0 m and NE = SW = 100 m. Its bilinear center is
50 m. The render mesh's NE–SW diagonal is at 100 m: **a 50 m overlap**. Sampling
that exact mesh at the corners of a 10 m × 10 m airport face that straddles the
crease still leaves its interior 50 m below terrain in the adversarial fixture.
Adding the terrain-edge intersection makes each resulting face share a plane.

`crates/flightsim-world/tests/airport_mesh_clearance.rs` applies the saddle to
one cell of an actual 33 × 33 `build_mesh`, retaining f32-relative vertices and
core-owned geodetic conversions. It characterizes the mismatch while confirming
that the physical bilinear result stays unchanged. It is **not compiled or run**:
`cargo` and `rustc` are absent, and toolchain installation is awaiting approval.
The required next command, once the environment is authorized and ready, is:

```sh
cargo test -p flightsim-world --test airport_mesh_clearance
```

The test records a known current limitation; it does not assert that rendering
has been repaired. Rust formatting, clippy, aggregate tests, actual native/
offscreen captures, and frame-budget measurements remain unrun.

## Proposed narrow fix: shared visible surface, clipped airport overlays

This proposal is **not implemented**. Adding only a scalar mesh sampler would
create a false clearance guarantee. A complete render-only solution needs all
of the following, without changing `Terrain::elevation_at`, FDM ground, physical
runway/evaluation geometry, terrain source priority, or replay physics:

1. **Bind to one displayed terrain generation.** Retain the surface triangle
   data actually used by the visible terrain cut, including its f32-relative
   vertex conversion and origin. Key airport geometry by tile IDs, source/mesh
   identity, mesh options, and visible-cut generation. A freshly resampled DEM,
   invisible requested child, or evicted/reloaded tile is not equivalent evidence.
   Exclude skirts. World owns mesh data; rendering never adds terrain disk IO.
2. **Clip geometry to the shared triangles.** Intersect the active airport's
   pavement/apron/taxiway polygons and marking boundaries with the projected
   surface triangle edges. Construct every output surface fragment in the
   corresponding terrain plane. Reuse common edge intersection vertices and
   topology for pavement/paint, including tile boundaries and mixed LODs.
   Independent 10 m resampling cannot substitute for edge intersections.
3. **Preserve small, ordered offsets.** Apply the existing centimetre-scale layer
   ordering to the shared fragments, using a consistent local-up offset or an
   equivalently bounded plane offset with seam handling. Check the final stored
   vertices/triangles after f32 conversion. Quantization allowances need measured
   bounds; they must not become arbitrary multi-metre lifts. The same surface
   definition must position fixture bases, signs, and holding markings. Fixture
   bases/top fragments straddling creases also need clipping or a documented
   bounded support construction, not just four independently sampled corners.
4. **Prepare and swap atomically.** Prepare all affected airport fragments and
   lights for a candidate terrain cut while hidden. Change terrain and matching
   overlays in the same complete visibility transaction. If preparation is
   unfinished or fails, retain the previous mutually consistent local cut and
   overlays. On first load, only publish an airport overlay when its covering
   surface is ready. Obsolete generations must release resources. Airport scene
   changes/replay resets must invalidate old geometry and pending work.
5. **Keep bounded work and memory.** Spatially index only the active-airport
   footprint and needed terrain triangles, with explicit per-frame clipping/
   mesh budgets and fragment/vertex/resident-byte caps. Do not re-read DEMs for
   every polygon. A limit hit must have an observable diagnostic and consistent
   fallback rather than a silent hole, unbounded allocation, or permanent terrain
   transition deadlock. Integrate the local transaction with existing bounded
   terrain selection; retaining an old local cut must not starve unrelated tiles.
6. **Do not equate visibility with physical fidelity.** Draping onto coarse
   rendered terrain can shift the visible airport metres from the physical
   aircraft/ground. Measure and report that separation. Near a runway, require a
   suitably refined terrain patch/cut for the intended physical tolerance before
   presenting the placement as correct. Where a regional DEM remains coarse or
   cannot meet the tolerance, source-aware local terrain refinement/consistent
   patch triangulation may be needed. Do not hide the gap by changing FDM heights.

An alternative is a terrain patch constrained by the airport boundaries, with
shared triangles and matching source sampling for both airport and nearby terrain.
It can reduce the visible/physical discrepancy, but introduces terrain cutout,
patch-boundary, normal, and streaming responsibilities. It should be selected
explicitly rather than mixed into a callback-only patch.

### Acceptance before calling overlap fixed

- Reproduce the adversarial saddle and crease-crossing face in Rust, then prove
  every clipped fragment clears its corresponding final terrain triangle
- Test primary regional DEM at finer and coarser resolutions than mesh cells,
  missing-child parent fallback, steep terrain, negative elevations, dateline,
  tile boundaries, mixed LODs, and small/degenerate polygons
- Check pavement, paint, apron, taxiway, holding markings, signs, and lights;
  verify whole intersecting fragments, not only vertices or representative probes
- Under tiny frame/cache budgets, exercise refine/coarsen, source/cache eviction,
  map new-flight/restart, and interrupted preparation; assert no mismatched epoch,
  stale light mesh, overlap cut, resource leak, or transition starvation
- Confirm deterministic physical ground queries/replay are unchanged
- Run actual cockpit, shallow-approach and overhead captures by day/night over
  the original regional fixture and global mountain/coastal cases, including the
  transition itself; separately assess long-range light visibility
- Run split aggregate tests, rustfmt/clippy, architecture checks, and measured
  frame-time/resource budgets before any performance or completion claim
