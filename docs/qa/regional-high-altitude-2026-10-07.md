# Regional high-altitude coverage regression, 2026-10-07

## Scope

This addresses the specific high-altitude regional-discovery gap recorded in
[backlog reconciliation](backlog-reconciliation-2026-10-07.md). It is not closure
of every issue #6 criterion, approval of package-backed replay, hardware GPU
qualification, a Windows release or clearance of the nine distribution gates.
The [coverage decision](../adr/0028-bounded-primary-terrain-coverage.md) preserves
existing physical data, replay identity, runtime budgets and atomic transitions.

## Matched actual-scene result

Using the unchanged Balzers sample (`balzers-glo90-rebuilt@1.0.0`, archive SHA-256
`d7e1265ee8015ad0fb1df23f7110b23124b90d889440f1b14a9cd85119f28528`) at
47.068 N, 9.501 E, the ground sample is 522.6750371563289 ellipsoidal metres.
The +3000 m AGL fixed pose previously requested 35 L9-or-coarser tiles and drew
only the coarse global fallback. After the fix, the settled native scene draws
41 tiles: two real L10 regional meshes and 39 global fallback meshes. The exact
same pose shows the valley and nearby mountain relief where the prior image was
flat/coarse. The +100 m AGL scene retains 53 tiles: 14 primary and 39 fallback.

Both new captures use Mesa 25.0.7/LLVM 19.1.7 llvmpipe software Vulkan, Swift Sport,
Chase view, Light graphics/water, Standard detail, clouds Off and 09:00 local
solar time. Both report identical displayed/live/desired cuts, settled source
availability, no pending stitching or overlay revision, no failed reads and no
capacity-limited updates; both PNG saves and process exits succeed with status 0.
The distant synthetic airport's four overlays remain correctly precision-hidden;
this DEM-only sample contains no local runway or approach-light evidence.

These fixed poses use the existing authentic zero-duration v3 QA files through
raw `--tiles`. They do not demonstrate motion. Package and raw-source selection
parity is checked separately through the production package integration test;
`InstalledPackage::require_replay_support` still rejects package-backed replay.

The captured Linux binary SHA-256 is
`942015d68677eddc0fde1fac7ab3547895aa9e2af9bad75598e6cce6324bd3fe`.
Matched before/after PNGs, raw logs, commands, binary/code hashes and external
fixture helpers are retained with the local validation evidence; screenshots and
test binaries are not automatically added to a public release.

## Moving renderer evidence

An external fixture helper used the original Swift physical-profile JSON tokens,
production `Simulation::advance_with_controls`, the fixed 120 Hz step and the
production v3 recorder/decoder. It made no teleports and did not relax package
replay admission. A neutral-control, initially westbound 45 m/s flight ran 120 s
(14400 recorded steps), travelled 7096.053 m and ended at 2119.309 m AGL. Its
one-second telemetry samples span 1172.466–3000 m AGL. Every stored keyframe and
final state, elapsed time, log and ground sample reproduce exactly; no crash or
divergence was reported.

The actual app played that fixture to `REPLAY COMPLETE 2:00 / 2:00`. It retained
regional terrain through the old altitude dropout, rebased the render origin at
replay 68.158 s, and finished with a settled 38-tile cut (one primary, 37 fallback).
The other regional root leaves the bounded local-detail footprint as the camera
moves west. A separate 1:25 capture still has two primary tiles. These are actual
moving-flight captures, unlike the zero-duration matched poses above.

A second authentic 120 s fixture adds only elevator -0.03. It travels 8093.452 m,
has exact numerical replay by the same checks, and one-second samples reach
1051.668 m AGL. The native app was observed through 1:16, not its full duration.
Its log shows a genuine L10-to-L11 transaction: 41 displayed / 44 prepared live
with stitching pending, followed by 44 displayed/live/desired (five primary,
39 fallback) with stitching complete. The inspected 1:16 image shows continuous
regional mountain/valley terrain; its render-origin rebase occurs at 59.258 s.
Both moving capture paths save a PNG and exit 0, with no failed reads or
capacity-limited updates at capture readiness.

The 22:00 high-altitude capture also settles at two primary plus 39 fallback and
exits 0. Its terrain is very dark, so it is not strong fine-seam/night acceptance.
Sampled logs and stills do not certify every intervening frame, all depth/reversed
face cases, every LOD/source boundary, or approach-light visibility. The existing
software-Vulkan warning and absence of an audio device remain explicitly logged.
No hardware FPS conclusion is drawn from concurrent cloud-machine runs.

## Local checks and source-admission boundary

- Pure Rust/content aggregate: 1348 tests passed, including the real Balzers
  package/raw parity regression and existing physical/replay/data tests
- Renderer terrain-selection slice: all 46 tests passed, including cold high
  startup, descent/climb/move/return, absent/corrupt hinted tiles, one-read and
  one-mesh budgets, and the existing large global/polar convergence tests
- World/content Clippy with warnings denied passed; formatting, architecture and
  local documentation links passed
- Final full default and region-downloads app/render aggregates, expanded Clippy,
  strict documentation and the new Criterion comparison are tracked in the
  subsequent binding-review record; the earlier focused passes are not a claim
  that those pending aggregate checks already passed

The first complete Python suite intentionally rejects this new reviewed source:
316 tests run, five failures and 19 errors, with one Windows-only skip. Five
whole-file replay-candidate pins and analytical main.rs binding still name the
previous implementation. No pins or gates are silently refreshed here. A separate
reviewed binding migration must preserve independent/legacy anchors, bind the
new coverage helpers and mutation witnesses, and update only affected source
and transitive contract identities through the documented maintenance procedure.
Until that migration and final exact-main checks pass, this commit is not a
passing distribution candidate. All nine release blockers remain in force.
