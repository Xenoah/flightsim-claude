# Isolated polar shading experiment

This is the historical diagnostic record at `dd7dd95` / `1e2d8ed`; its mode
switches and benchmark are preserved on `diagnostic/polar-filtered-shading`.
The production-only candidate is described in [polar visual normals](polar-visual-normals-2026-10-03.md).
No diagnostic mode is shipped by that candidate.

## Newly isolated source contribution

The exact official NOAA elevation/geoid stride-10 DAP2 downloads were reacquired
and matched to the preparation script's pinned full-file SHA-256 values. A direct
bilinear reconstruction of the south canonical row (-89.912109375 degrees) rounds
to all 4096 stored H/N channels. No lake/coast correction applies to this Antarctic
row. The raw sources and per-sample analysis remain private QA inputs.

The canonical row radius is approximately 9816.854 m and longitude-node spacing
30.118 m on the WGS84 polar tangent scale. Stored H has 340 one-metre steps among
2048 edges. Its linear radial pole cap preserves each angular step's physical
east-west derivative as radius contracts. Therefore additional geometry does not
recover smooth relief from the metre-quantized row.

For H+N, pre-packing versus stored angular gradient RMS was 0.006816 versus
0.013529; p95 absolute gradient was 0.013655 versus 0.033203. Packing-added gradient
RMS was 0.011569. These are dimensionless local gradients, not surveyed slopes.
The unquantized row still reached 0.029959 maximum gradient. Neither extra f32
precision nor finer packed height alone is demonstrated to solve all striping.

## Candidate and scope

- Keep atlas bytes, fingerprints, DEMs, source priority, physics, replay settings,
  mesh origin/positions/indices/UVs/elevations and bridge geometry unchanged
- The selector supplies Primary/Fallback from its exact successful read path,
  including same-ID primary replacement. Existing two-argument APIs are wrappers
- The app supplies its already validated immutable global atlas. Only explicitly
  fallback preparations enable the candidate; primary/regional DEMs stay unchanged
- Use four atlas samples at +/-505 m north/east in a core LocalFrame, transform
  through ECEF, and form one filtered visual normal. The unchanged mesh elevation
  anchors the tangent frame; no fifth centre sample or per-tile asset decode
- Fully filter within 75% of the canonical polar cap, smoothly fade the shading
  change to zero at its outer boundary, and leave all exterior vertex attributes unchanged
- Canonical longitude zero at every duplicate exact pole produces one normal
  across tiles and LODs. Boundary extraction follows filtering, so bridges inherit
  the attributes; skirts copy paired top-vertex normals/slopes
- Separate compile-time modes Off / NormalsOnly / SlopeColourOnly / Both support
  a labelled native factorial. Current mode is NormalsOnly. There is no new CLI
  setting or claim this should become a hidden production default

A 505 m half-stencil gives a <=0.001 central-difference gradient-component error
from <=0.505 m atlas packing error in a local height graph. It does not bound total
normal-angle error, source accuracy, mesh error or frame time. Source resolution is
anisotropic near the pole. Filtering removes some retained source variation, and
the outer-cap transition is an explicit visual modeling choice requiring review.
Shared vertices in the transition can retain original per-tile normal differences.
Coarse facets can interpolate a changed pole normal beyond the cap even when
all outside-cap vertex attributes are unchanged.

An independent flat tangent-cap calculation at radii 100/350/1000/3000/5000 m
found the maximum ring p95 packing-induced normal discrepancy fell from 0.02734
rad with 1 m half-stencil to 0.000421 rad with 505 m. But the 505 m filter's p95
change relative to the unfiltered original source reached 0.01367 rad. These
calculations exclude curved-ECEF mesh normals, rendering, shadows and acceptance.

## Verification gates

Pure-Rust standalone tests using source-validated cached core/world rlibs verify
unchanged geometry/indices/elevations/UVs, authorized attribute isolation by mode,
primary/nonpolar identity, same-level and mixed-L9/L10 normals, bridge geometry,
canonical pole duplication at L9/L10/L13, cap limits and render-origin rotation.
The selector regression checks fallback-to-primary replacement with numerically
identical DEM heights so values cannot masquerade as provenance.

Full Cargo tests, strict clippy, architecture/docs and native captures remain
pending at this checkpoint. Benchmark command, when the shared target and CPU
window are available:

```
cargo bench --profile dev -j 2 -p flightsim-render --bench polar_shading_diagnostic
```

The Criterion benchmark uses the actual 4094-tile south-pole 350 m AGL default cut
and holds only one mesh at a time. Compare total CPU preparation for Off,
NormalsOnly and Both. Excludes GPU upload, bridge construction, streaming and
frame timing; no performance acceptance is inferred from bounded sample count.

Native acceptance must compare settled identical cuts/camera/time:
1. Existing baseline, normals only, slope-colour only, both
2. Unlit actual vertex colours, then unlit uniform colours if points remain
3. Lighting with shadows disabled, then unchanged normals with filtered normals
4. Separately identify bridges using a diagnostic colour before hiding/removing
   them; removing bridges can create real holes and is not a valid final fix

North sparse pixels are a separate defect candidate. The checked capture has
notable outliers at (617,114), (652,108), (660,113), (630,220), (638,476),
(637,655), (642,655). Example (630,220) is RGB[100,98,89] versus nearby
approximately [43,61,68]. This is far greater than the north row's <=0.000332
angular gradient. Neither source-gradient filtering nor shadow assumptions alone
identify those pixels. All four airport overlays were logged precision-hidden;
synthetic traffic defaults off. Attribute/unlit/bridge/shadow controls must identify
the actual primitive/path before assigning a cause.

## Complete-cut palette identity

An optimized standalone probe using source-validated core/world rlibs and the
current biome function checked the actual 4094-surface / 8185-bridge cuts at both
poles, 350 m AGL, June 21 north / December 21 south climate. It reproduced the
current surface and bridge geographic colour-coordinate paths. For each scene:

- 4,982,398 surface/skirt vertex colours compared bit-for-bit: zero differences
- 659,068 bridge vertex colours compared bit-for-bit: zero differences
- 616,610 surface/skirt slopes and 80,774 bridge slopes changed
- Positions, normals, indices, UVs and elevations remain identical in slope-only
- Both complete cuts include distant opposite-pole geometry as well as the
  immediate scene; this comparison did not discard offscreen tiles

Old/new FNV-1a64 colour-array checksums also agree (zero per-channel bit differences
is the primary evidence; hash equality alone is not used as proof):

| Scene | Surfaces/skirt RGBA | Bridge RGBA |
| --- | --- | --- |
| South / December | ac0f64b3ef0c969f | 1a0d3db63de9f547 |
| North / June | 0148f9e8c171df20 | 4d3fce7c4c2290a0 |

Thus slope-colour-only uploads identical GPU data in these exact scenes, and Both
has the same palette as NormalsOnly. A redundant slope-only native capture is not
required for this pair; this is not a guarantee for other geography, date or source.
The normal candidate still requires native comparison. Full Cargo verification is
pending. The attempted combined standalone legacy-selector suite was explicitly
interrupted to avoid duplicating the integrated heavy suite; it is not a passing
full-suite result. Separate new-only runs passed six shading tests and one
provenance-supersession test.

## Fresh Cargo and quiet Criterion results

Before Cargo, all 228 workspace Rust source/manifests/lock files were content-hashed,
mtime-refreshed, and rehashed unchanged. This prevents shared-target artifacts from
another checkout being mistaken for current source. Focused Cargo tests passed
(six shading checks and one provenance check); strict render/app all-target clippy,
strict render/app docs, architecture checks, formatting and native app build passed.
No full renderer or app suite is claimed here; the final integrated gate owns it.

A parent-confirmed quiet CPU window ran the above cargo bench command at source
`dd7dd95ff79edc6bdf3ccc6a5cc4c6928c3bebfe`, using the dev profile (workspace
opt-level1, dependency overrides opt-level3, debug0, incremental0, warnings denied).
Every mode measured ten full-cut iterations in ten samples. Criterion extended
its requested 2-second measurement to finish those ten samples.

| Mode | Median whole-cut CPU seconds | Median bootstrap 95% interval |
| --- | ---: | ---: |
| Off | 0.837528 | 0.833569–0.855211 |
| NormalsOnly, 505 m | 1.446674 | 1.424362–1.472368 |
| Both, 505 m | 1.443105 | 1.433223–1.460613 |

NormalsOnly added approximately 0.609146 s / 72.7% to this whole-cut CPU preparation
measurement. It is not free. This excludes palette evaluation, bridge construction,
streaming, GPU work/upload and frame cadence, and does not prove acceptable worst-
frame cost. The two filtered modes' intervals overlap; do not infer that adding
slope work is faster. The benchmark keeps only one mesh at a time plus atlas/cut
inputs. Criterion binary SHA-256:
`e5a3fa265d7af79b290d7b80fddbbaef3f4fcb39290151eecf4f236d93f2a074`.

The separately copied NormalsOnly native binary is built from dd7dd95, SHA-256
`da5cec99fcfbbacfddf66164dbda5f0803fa3fa521dfce9a280a4531ae3782f3`.
The unlit/red-bridge control is separately identified by branch
`diagnostic/polar-unlit-bridges`, source `67c7d8b`, binary SHA-256
`1c6c928d98bdc100a72d56df90d9f42d4a25a9c997220f17e22b8eb65e41d575`.
That control preserves shadow configuration but intentionally bypasses terrain
shadow reception through unlit shading. Raw framebuffer constants or exact bridge
coverage at antialiased/postprocessed boundaries must not be inferred.

Canonical source was restored afterward with all 228 original content hashes
matching. Native comparison and production acceptance remain pending.
