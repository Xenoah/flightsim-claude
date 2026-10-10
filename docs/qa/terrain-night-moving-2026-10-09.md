# Balzers night and moving-terrain qualification — 2026-10-09

## Decision and patch handoff

The bounded terrain patch is ready for publication review. It materially fixes the reproduced night stitch-color spikes and redundant triangular-corner artifacts, with source/geometry regressions and full paired replay evidence. **This report does not close issue #6 or declare all numerical depth/shading behavior resolved.** Retain the documented ordinary-tile color residuals, one pre-existing skyline subsample observation, and the CPU-reference precision limits below. Actual Windows CI/render/release verification remains a separate publication step.

Reviewed local commit: `0bc4a4961c677a5173350630bf2641a2c1b40b94`  
Reviewed tree: `7ac0cedd0963ac4133fa8c5251f57f26037324d7`  
Public base: `42a7ddeae36f3022c2c77e54d5dce33f71b11bca`, tree `625c39f9b182b16a0dada2dc4131434d178a0831`  
Patch SHA-256: `402d822ec8336b16b87b05dd9226051d8369c72a0e8bae173bc490927d62d1a8`

The patch was applied to an isolated index holding the exact public-base tree and reconstructed the reviewed candidate tree exactly. The candidate checkout remains clean. The local commit is not a public Git object; publish the reviewed tree with the actual current public parent and a lease, then verify that remote commit and its CI. No terrain commit, release or issue closure was performed during this task.

Only three runtime files change:
1. TerrainMaterial enables a terrain-only shader define in both stages
2. Its forward world position, normal and color use perspective-centroid interpolation; standard materials, water, optional fields, prepass and deferred interfaces keep their existing contracts
3. A closed, exactly three-source corner emits its complete direct triangle, both windings, in every incident ribbon, only when that encoded triangle has nonzero area. Exactly collapsed direct triangles keep the previous mean fan. Open paths, four-source and polar corners retain their prior construction. Source vertex storage/order, repeated coverage guards and physical terrain remain unchanged

There is no positive-area cutoff, RGB clamp, geometry deletion, MSAA/TAA change or global material override. The causal control and actual candidate library agree on all 250 saved seam outputs. Source review found no blocking issues. World tests: 268 plus one doctest passed, strict Clippy/format/diff checks passed. Earlier unchanged renderer tests: 355 passed, four new shader/interface tests passed with Naga HLSL and SPIR-V centroid-location assertions. This is translation-contract coverage, not a new Windows run. Changed-source admission: 181 passed, one Windows-native junction test skipped, zero failed; all 3,481 tracked files rechecked. Exact logs/receipts are retained separately.

## Captures and source fidelity

Licensed Balzers Copernicus GLO-90/NOAA-geoid fixture: 170 tiles, archive SHA-256 `d7e1265ee8015ad0fb1df23f7110b23124b90d889440f1b14a9cd85119f28528`. Original source, license and no-liability notices are preserved. The authentic Light Single replays use production 120 Hz physics and v3 recording, real regional plus global terrain, calm weather and no teleports. Only the `global-enabled` fixtures are authoritative.

Both combined runs are complete at 1280×720, production MSAA4, 10 Hz serial Main/Extract/Render measurement on Mesa 25.0.7 llvmpipe CPU Vulkan. The exact source-identified diagnostic adds sequencing/readback only; no shader override. Startup alone waits for scene/pipeline stability. Moving captures intentionally include active stitching states.

| Scenario | Full frames | Native samples validated/raster-compared | Consecutive pairs | Motion |
|---|---:|---:|---:|---|
| Night low west | 901 | 3,321,446,400 | 900 | 90 s, about 4.499 km, captured AGL 245.445–264.379 m, sun −47.74° |
| Day high west | 1,201 | 4,427,366,400 | 1,200 | 120 s, about 7.879 km, captured AGL 972.539–3,000 m |
| Total | 2,102 | 7,748,812,800 | 2,100 | All four samples on every recorded frame |

The flight generator's higher-cadence telemetry has slightly different AGL extrema; it is not substituted for captured-frame extrema. Night render exits 0; its interrupted storage sidecar was independently recovered and all 901 raw depth frames verified. The missing original storage.exit remains absent, with a distinct recovery receipt. Fresh high render/storage both exit 0. The older interrupted 911-frame high run remains partial and was not spliced into the complete run.

All shared camera/physics/time/light/LOD values and geometry-instance ordering match baseline for both full runs. Mesh positions match; only the intended bridge indices change. Original baseline did not record normal/color arrays, so full baseline attribute parity is not claimed. Exact same-frame attribute parity is available for the preserved centroid-only windows (396 night/397 high frames). A cross-state unused-anchor normal difference is explicitly retained as an inapplicable comparison, not a rendered defect.

## Night and moving-coverage findings

Across both scenarios: zero invalid native samples, off-edge farther discrepancies, reversed-face regions, tile-topology/coverage metadata failures, whole-pixel enclosed holes or motion-reprojection clear gaps. These are bounded measurements with an unchanged independent CPU reference, not universal proof.

Night has zero strict-core missing samples and zero smooth-depth color-spike candidates. Its original all-depth color-candidate count falls 320→1. The remaining pair at frame235, (1082,242)→(1083,242), has a step of8 rather than13 and a 159.897% expected-depth jump; all eight native samples remain covered and the full native depth frame is unchanged. Retain its small dark pixel without asserting its exact shading cause. It is not evidence of a smooth-surface stitch seam.

Night captures 29 stitching-in-progress frames before eight atomic displayed-cut changes at 5.4,17.2,20.7,24.8,29.7,33.7,37.4,49.4 s. High captures seven such frames around changes at73.7 s (refine),82.4 s (coarsen),103.2 s (moving footprint). All these frames have no waiting pipelines and live desired-cut agreement; their old displayed geometry remains active and was audited. Render-origin rebases at night80.0 s/high60.7 s show no reprojection-clear gap.

The known interior partial-sample gaps at night423 and high630 are filled with no new clear samples in their full-frame native comparisons. High retains exactly one strict-core clear subsample: frame1105, sample3, pixel(739,166), at an ordinary tile's front/back skyline shared edge. Its entire native frame and target RGB are unchanged from baseline; the other three pixel samples are covered. This is neither a whole-pixel hole nor a newly introduced gap, and it is not silently waived.

Raw reference exceptions remain visible:

| Raw measure | Night | High |
|---|---:|---:|
| Strict-core comparisons | 2,695,143,955 | 3,428,744,041 |
| Within slope-aware tolerance | 2,689,977,400 | 3,423,533,873 |
| Farther measurements within 1/256 px edge ambiguity | 8,651 | 8,397 |
| Nearer-unresolved samples (not terrain matches) | 5,157,904 | 5,201,771 |
| Expected-terrain/sky silhouette samples | 1,608 | 2,263 |
| Strict-core missing subsamples | 0 | 1 |

The reference excludes only its external one-pixel silhouette from strict comparison. Raw exceptions are not erased. Nearer fragments can include the aircraft, whose geometry is not exported into this terrain oracle; their individual identity is not inferred from proximity alone.

## Actual color improvements and retained residuals

Literal source-hashed crops and native-depth neighborhoods demonstrate:
- Night frame54 original blue/yellow spikes now match local terrain colors, with unchanged full-frame depth
- Night257 target RGB (159,15,10) → centroid-only (11,1,0) → combined (4,5,6)
- High200 target RGB (255,179,158) → centroid-only (119,65,96) → combined (97,106,113)
- Additional known high51/289/317/368 spikes decrease to local one-value-or-less neighbor steps

The standalone causal probe reproduces abnormal pre-lighting inputs on the redundant submillimetre source-corner subdivision. Guarded direct triangulation preserves all native coverage in the two controlled full-screen states; each target's one sample instead sees ordinary backing geometry. Actual production captures confirm the color improvements. The probe omits the aircraft and substitutes diagnostic MRTs for full PBR; native depth correspondence does not prove complete shader-binary/color equivalence.

High's full unchanged metric reports smooth-depth candidate pairs 5,452→4,770 and all-depth pairs317,327→317,250. These include ordinary material/terrain edges and are **not defect counts**. Twelve retained comparison pairs change by at least8 RGB values in the presentation union: frames13,577,730,773,907,910,913,938,961(two pairs),970,994. This is a bounded inspection list, not an exhaustive new acceptance threshold or a claim each is a bug.

High13, pixel(336,189), is the independently inspected representative residual: RGB(119,118,134)→(151,135,135), adjacent step10→21, with unchanged native depth/coverage. Its diagnostic winner is an ordinary near-edge-on tile face, not a junction fan. Diagnostic centroid RGB/normal/world coordinates exceed source bounds. The broader terrain band is real context, but does not explain that sample by itself. Full production-PBR causation remains unproven; no driver-conformance claim, homogeneous-normalization fix or new broad renderer change is proposed. The probe does not request sample-frequency shading. Interpolated world.w and color.a were not recorded and must not be invented.

## Depth precision: measured limits, not a blanket pass

The unchanged comparison tolerance is max(64 float32 ULPs,2e-5×expected depth) plus0.02 pixel times the absolute projected depth slopes. “Matching” therefore means agreement inside this slope-aware allowance. It does not imply uniformly small world/view-depth error.

Worst per-frame primary p99 absolute CPU/native view-depth differences:
- Night:1.43 mm at100–1,000 m;6.87 mm at1–10 km;13.47 mm at10–100 km
- High:11.20 mm at100–1,000 m;8.56 mm at1–10 km;0.3493 m at10–100 km;24.8249 m beyond100 km

These are maxima of per-frame p99 values within the admitted primary-sample class, not pooled quantiles. Isolated admitted maxima are574.02 m night and9,925.16 m high. All-native admitted relative-depth maxima are9.9991% and21.1327%. Float32 one-ULP representation spacing is separately recorded and is not total error.

One bounded CPU-only witness resolves the interpretation of the night maximum: frame205/sample0/pixel(851,217), CPU view depth9,043.02685 m versus native9,617.04740 m. Its reference sample lies0.0000233744 px from a triangle edge; slope allowance is39.3422% and admits the5.9688% discrepancy. The original/candidate full native frame is byte-identical, with target RGB(4,4,5) in both. This is strong evidence of the reference's subpixel sensitivity at that state, not a demonstrated new candidate regression or a certified real-world position error. High's maxima provenance is retained from existing rows without a new investigation. No threshold was relaxed to produce these results.

## Acceptance scope and remaining work

Issue #6's licensed-real-DEM input, automated authentic rehearsal, detailed night evidence and moving LOD/horizon/depth measurements now have durable, reproducible evidence. The bounded patch addresses the confirmed terrain defects. The evidence supports sampled-night continuity and the stated moving-coverage findings, while retaining the high ordinary-tile color residuals and numerical-reference limitations above. Do not close #6 as universally artifact-free or give an unqualified depth-accuracy pass from this report. If a stronger residual correction is required, the smallest next investigation is exact production-fragment instrumentation of an already saved affected high state; that is a proposal, not work performed or a proven fix.

The recipe and helpers are delivered as a diagnostic bundle, not newly installed shipping functionality. `REPRODUCE.md` describes source/fixture identity, build, capture, recovery, audit and limitations. The source candidate includes reviewed ADRs/tests; a final public QA-document copy can accompany publication without pretending the local commit is public. No physical-GPU, human-handling or all-pixel-perfection requirement was invented. No further QA or runtime changes are included in this handoff.
