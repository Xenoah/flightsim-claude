# ADR-0030: Admit batch-capture view draws after bounded scene preparation

- Status: implemented in isolation; native qualification pending
- Date: 2026-10-08

## Evidence and retained contract

The uninstrumented ordinary Windows candidate at
`3b098bc1d20cc7cc99ae85896080d22b688ac6c3` timed out at the unchanged 180-second
watchdog without reaching CPU readiness or requesting Screenshot. The two-frame
backpressure from ADR-0029 was active: the log contains 26 registrations after
complete Render schedules, 31 internal successful submission indices and 1305
finite servicing polls. It does not show a callback deadlock or permanent GPU
freeze. At 82.965 and 132.369 seconds, terrain still displayed two tiles while
41 live tiles awaited an atomic stitched commit; prepared bridges advanced from
16 to 80 across eight complete Render-schedule registrations, retaining the
eight-mesh update budget.

Source: [run 37746387487, job 113208759353](https://github.com/Xenoah/flightsim-claude/actions/runs/37746387487/job/113208759353).
Artifact 11537013914 is 448670 bytes with SHA-256
`7273cbbe0f3cd503b1e4fff2ff689f5419b3c179c738d99317bfe3c93778a742`.
The same Microsoft Basic Render Driver CPU/Dx12 adapter and driver
10.0.26100.33438 were selected; this is not physical-GPU qualification.

Bevy's pipelined rendezvous waits for the render sub-app before extracting again.
Holding that sub-app inside a GPU credit wait therefore also paces Main's terrain
preparation by GPU throughput. Repeating Main without extraction can expire
asset events; repeating extraction while skipping Render can replace unconsumed
extracted assets and leave deferred commands unapplied. Neither is acceptable.

The existing `frames` floor counts real Main updates in Last, not GPU completions
or full-scene draw acknowledgements. Retain those 30 Main updates, the configured
delay, exact ReadyScene/stability checks, and every existing per-update budget.
This change deliberately reduces premature unfinished-scene draws; it does not
claim to preserve 30 full-scene rendered frames. Readiness still does not prove
GPU/shader completion or visual correctness.

## Decision

Only explicit native batch screenshot sessions install admission: Screenshot
must be requested and `exit_after_screenshot` enabled, including headless capture.
Ordinary launches and non-batch screenshots retain their existing renderer path.

Keep all normal Main, ExtractSchedule and Render systems, cameras' active state,
visibility, view/light preparation, pipeline processing, asset uploads, graph
submission, screenshot prepare/collect, time delivery and cleanup. Keep the
original render_system registered once and keep ADR-0029's two-frame credits.
During loading only, save and empty the public SortedCameras vector immediately
before render_system, restoring its exact entities/order/target/HDR fields after
it and before Cleanup. The other inspected consumers, sort_cameras and
prepare_lights, run earlier in ManageViews. Original pipeline processing still
runs before rendering and does not consume SortedCameras.

The original CameraDriver then runs its no-camera window clear for acquired
surfaces. The original initial present remains; subsequent no-view frames can
retain/reuse the acquired surface rather than presenting every clear. Headless
capture retains its original image target. No camera component, render graph
edge, resolution, scene asset, quality, backend or adapter changes.

Admission is copied during each extraction, so the concurrently prepared next
Main state cannot change the prior frame's decision. Once the unchanged complete
eligibility conditions hold, open view execution permanently for that session.
Do not request Screenshot in that same update. Require a generation-tagged prior
admitted Render opportunity for the same ReadyScene. The intended flight camera
must be in SortedCameras with Camera3d, ExtractedCamera, a prepared ViewTarget,
nonzero target/viewport dimensions and a usable target/window. Record the epoch
only after the entire original Render schedule returns. This is a CPU Render
opportunity acknowledgement; queue callbacks remain the separate GPU-credit
completion authority.

Scene identity changes invalidate old acknowledgements. An unready or changed
scene cannot capture, although view execution remains open after its first
admission. Epoch zero never qualifies; overflow closes admission state and fails.
Any extracted Screenshot defensively keeps all views admitted, including a late
request or delivery after cancellation. Capture request, image delivery/save
success or failure and removed request all preserve the one-shot lifecycle;
late/repeated old acknowledgements cannot rearm it.

Restoration depends on the saved vector, never on later session state, including
an intentionally empty saved vector. If a render system unwinds, a cleanup guard
restores saved cameras, drops the candidate opportunity and clears acknowledgement,
then immediately resumes the failure. It does not recover the renderer or grant
capture success. No admitted opportunity or GPU credit is published on that path.

## Costs, tests and remaining acceptance

A batch-capture window initially shows the original no-camera clear while the
complete scene is prepared. It is not a loading screenshot or reduced scene used
for acceptance. The existing real PNG, flush/sync, exit code and watchdog remain
mandatory. A changed/moving scene can still take longer; no 180-second success,
shader readiness, performance gain or visual parity is claimed without a native run.

Deterministic regressions cover the unchanged delay and 29/30 Main-update
boundary, first admission versus request, missing render acknowledgement, changed
scene and stale/repeated acknowledgement, zero/overflow, immutable snapshots,
original ordered camera restoration, preparation/render/cleanup execution,
in-flight screenshot bypass, missing valid flight view, error unwinding, ordinary
and non-batch registration, and real PNG observer success/failure/cancellation.
GPU-free hook tests use the same scheduling helper with a render-body seam; they
do not substitute for execution with a real RenderDevice or Windows swapchain.
All frozen source/capture/release contracts require separate reviewed migration.

Rejected: larger credits/deadlines, more terrain work per update, disabling camera
activity, repeated Main/extract without normal Render, reusing stale scene tokens,
counting loading clears as full-scene draws, and qualifying from counters without
a real screenshot. A bounded GPU-pass timing diagnostic remains appropriate if
native evidence identifies a remaining rendering bottleneck.
