# Surface rendering integration — 2026-10-03

This report accepts the render-only polar-normal policy together with the staged
overlay/shadow milestone and corrected optional timing diagnostics. It does not
claim complete polar cleanup, faster hardware rendering, or commercial-release
clearance. Raw logs, generated replays and regional databases are not distributed
in this source report.

## Source and gates

The preceding upload/shadow source milestone is public commit
`a2837af6c6ef62c87aa649b826943827b7ea623c`, content tree
`91d829dc6e3d302592514d582647aba06181a6ec`. All eight jobs passed in
[CI run 37085660402](https://github.com/Xenoah/flightsim-claude/actions/runs/37085660402).

The combined code was validated at local commit
`ef9717727f793945633c5567b353eea77e2a9ded`; the subsequent acceptance-report edits
change documentation only. Public commit identities may differ from local recovery
commits. Consult the published commit's own CI for its exact remote validation.

All 15 local gates passed with clean, unchanged source before/after: formatting,
diff check, dependency architecture, both Python suites, headless Rust/all-targets,
doctests, native Rust/all-targets, strict clippy, strict docs, native build, and
commercial-profile tests/clippy/docs/build. Totals:

- 1,867 normal Rust tests: 965 headless, 5 doctests, 897 native
- 144 Python tests and 190 commercial-app tests
- 51 untimed benchmark smoke cases, distinct from the measured experiment below
- Zero failures; three normal and one commercial tests explicitly ignored because
  they require external data or a separate manual quiet-window experiment

The normal integration executable used for forest/native checks had SHA-256
`1ecfec60fdb5c2d827de7924ffe5a55fc1b9136ef53473bc7a7002b1c9167110`.
It was preserved before the commercial build replaced the working build output.
The host uses llvmpipe software Vulkan, not a physical GPU performance test.

## Atomic overlay staging in the actual regional scene

The quiet forest run used the same fixed replay, regional DEM/scenery, camera,
clear-weather conditions and 1280×720 output as the accepted reach-only image.
No other FlightSim build, probe or renderer ran concurrently. The final PNG was
byte-identical to that image, SHA-256
`e9f0497e75b6ccbc05cad3bed006a8a5636beda3984af8c87b21990d20df113c`;
all 921,600 decoded pixels also matched.

Its 52 logged nonzero work updates had no count-bound violation. Observed maxima
were two copy attempts, 8,450 copied vertices, eight uploads, and 65,424 uploaded
vertices per update. Final staging took 33 copy updates and ten upload updates,
submitting 26 meshes / 503,466 vertices. The old two-tile/two-visible-bridge cut
was still displayed after three upload updates. Only after completion did the
full 89-surface/204-bridge cut appear, with matching desired/live/displayed IDs,
no pending/planning/queued work and overlay revision 5/5. Thirty subsequent status
snapshots stayed converged. Existing two optional omissions and four distant
precision-hidden required roots were unchanged.

The normal upload target is 65,536 vertices, not a universal hard ceiling: one
indivisible first output can reach 524,288. Forest did not exercise this exception.
Native reset testing did: ten updates uploaded one oversized mesh each, at most
103,215 vertices. There were 203 nonzero native-reset work updates and zero count
violations. Surface/bridge attempts are not exposed in these log rows; complete
shared-budget and generated-asset closure assertions remain unit/public-API test
evidence. See [the exact contract](terrain-overlay-upload-budget-2026-10-03.md).

These are CPU asset-work counts, not GPU fences or elapsed upload measurements.
Retirement and the atomic metadata switch remain synchronous. No reset-latency
or frame-tail improvement is inferred from bounded counts alone.

## Native reset, views and geographic transitions

The normal executable was operated through native keyboard/mouse controls at an
observed 1180×812 viewport, with the regional sample loaded.

- Repeated F8 rewinds returned to 119 surfaces / 271 visible bridges. A specifically
  observed in-progress revision-11 snapshot job was cancelled before any of its
  uploads; the next required cohort and optional revision 13 completed normally
- Final revision 13/13, 15 roots (11 optional), 119 surfaces and 271 bridges stayed
  stable across 44 terrain/overlay status snapshots, without later submissions,
  retired terrain assets, queued bridges or pending work. No visible duplicate
  terrain/scene ghosts were observed. Full generated-asset reclamation is the
  independent lifecycle-test result, not something these counters can prove alone
- Chase, free, tower and cockpit views rendered; chase was restored afterward
- Replay map preview kept relocation disabled, including an attempted Enter
- Normal-flight world-map navigation went from the loaded regional scene to Tokyo:
  the regional scenery became zero features / zero optional roots, while the
  global cut converged to 32 surfaces / 79 bridges with four required roots,
  remaining stable across 55 snapshot pairs
- Exact latitude/longitude entry returned to the regional sample. After initial
  movement and pause, the new 1,000 m AGL flight converged to 116 surfaces /
  265 bridges, 20 roots (16 optional), revision 15/15 and no pending work. The
  changed altitude/position does not require the original 119/271 cut. Thirty-one
  returned snapshot pairs stayed converged; one optional output was explicitly
  omitted by the existing output-limit guard, a bounded visual fallback
- Regional features returned, no old regional buildings persisted in Tokyo, and
  opening/closing the map preserved the paused flight. The map scenario closed
  normally with exit 0. Its 178 work updates had no count violations, including
  eight valid single-mesh first-output exceptions up to 103,215 vertices. The
  earlier replay scenario was deliberately interrupted
  after its checks (exit 130), not an application crash or a clean-exit test

Mixed map/camera histories include faster UI-only frames and cannot represent
steady 3D rendering performance. These checks do not replace the preceding
flight-dynamics/takeoff/stall-recovery evidence; no physics or control policy was
changed here.

## Polar appearance, causal controls and cost

The [normal policy and source derivation](polar-visual-normals-2026-10-03.md)
use explicit fallback provenance and a fixed 505 m tangent stencil. Source data,
geometry, indices, slopes, palette, physical sampling and replay fingerprints
remain unchanged; primary regional DEMs bypass the filter.

Fresh South and North comparisons all finished on the full 4,094-surface /
8,185-visible-bridge cut, using identical zero-duration fixtures and actual
surface-detail-on settings. The broad South radial stripes disappear. The North
image has no broad band/seam/hue/horizon/aircraft regression, but its existing
sparse bright pixels remain.

The four South captures compare original/filtered normals and 150/2,000 m
shadow reach. Rare dark flecks occur under extended reach both with and without
filtering: 69 versus 76 pixels cross the declared 5/255 channel threshold, with
17 locations in common. This supports a residual shadow/normal interaction,
not a specific acne, bridge or geometry diagnosis. The combined image also
contains accepted overlay-lifecycle changes; equal final cuts do not establish
identical GPU submission ordering or uniquely isolate the artifact's cause. It is explicitly accepted as
a remaining limitation alongside the broad stripe improvement. No blanket bias
or terrain workaround is included. Forest contact shadows also remain soft or
separated at distance; restored coverage does not imply corrected contact.

A North control changing only terrain UNLIT, retaining its real palette, fog,
geometry and normals, removes all nine recorded bright specks into their local
colour. That narrows the defect to the lit PBR path/input interaction; it is not
a production fix or proof of a specific lighting term. Further private material
controls remain separate from this accepted normal policy.

A separate quiet production Criterion experiment used ten samples/iterations per
path and one mesh resident at a time for the actual 4,094-tile South stress cut.
Median CPU preparation was 0.852354 s unfiltered versus 1.468071 s filtered:
+0.615716 s / +72.2% for the whole cut. This includes atlas DEM generation and
geometry, but excludes palette/bridge work, uploads, GPU rendering, streaming
and frame cadence. It is a measured extra preparation cost, not a speedup or FPS
claim. The documented dev profile and existing desktop apps are not a fully
isolated hardware benchmark.

## Timing diagnostic correction and remaining limits

The old `steady` label could include unresolved selection/preparation before
stitching began. The corrected classifier consumes existing readiness/work flags,
counts both sides of loading transitions, includes actual overlay attempts, and
keeps intentionally disabled empty-terrain scenes steady. It adds no full-cut scan
per frame. Periodic primary discovery retries can still conservatively count as
loading after visible convergence; historical summaries are printed without
implying that new samples entered both buckets. The 2,048-sample rings remain
bounded. Fourteen focused diagnostic tests pass.

The regional sample is not global OSM coverage; most building heights remain
inferred. Global terrain is coarse, and visual normal filtering cannot recover
missing measured relief. Hardware GPU, driver diversity and commercial readiness
remain unverified here.

Binary publication remains blocked by the seven existing rights,
dependency-inventory and authorization conditions. The preceding Release
workflow's authorization job correctly skipped build/tag/publish and produced
no artifacts; latest executable release remains `v0.6.0-alpha.20`. No gate was
weakened, and the existing pending public screenshot approval is unchanged.
