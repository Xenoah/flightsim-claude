# Bounded Windows readback diagnostic

## Completed Windows v2 observation and opt-in boundary

The one authorized Windows v2 observation is complete. Exact source
`f3850a2e23948e778e1122ff3881708a331b07ba` ran in candidate
[37197960445, job 111423624879](https://github.com/Xenoah/flightsim-claude/actions/runs/37197960445/job/111423624879).
The exported evidence artifact was `11302661595`, 1,051,396 bytes, with SHA-256
`01da663894619f27032b99307cbb24840a55e3b68e52f56dcf0dbe6191f9a0b7`.
Its exported acceptance/log/JSON evidence was validated read-only. The adapter
was CPU Microsoft Basic Render Driver 10.0.26100.33438, D3D12; executable SHA-256
was `d3a5a1130d47f98a102cb9169dff5604f613bcf5049b7cc3b60d952a70b93b78`.

The pre-scene clear/copy successfully mapped all sixteen valid green pixels and
finished cleanup at 3 ms. Its one wait returned `queue_empty`, with 1 ms measured
wall time. Ordinary-scene callbacks for frames 1 and 2 arrived at 23,167 ms and
frame 4 at 45,019 ms. The scene summary froze at 60,565 ms with six registrations
and three credited completions. Callbacks for frames 8 and 16 arrived at 65,868 ms
and 105,196 ms with `cancelled=true phase=owner_released`, and receive no completion
credit beyond the 60-second observation window. No frame-30 completion was
observed.

The late marker's requested 250 ms wait returned timeout after 284 ms wall time.
Its separate async task started, suspended, received its signal and resumed.
At its 15,015 ms summary there were no successful map, validated pixels or queue
callback; the only map callback was a cancelled cleanup error and cannot count
as success. Both the primary screenshot and the secondary diagnostic timed out
at their unchanged 180-second watchdogs. No PNG was exported, acceptance remained
failed with no passed checks, and `qualifies_acceptance` remained `false`.

This establishes working basic pre-scene readback on that exact software-adapter
run. The full-scene screenshot/progress problem remains unresolved. Callback
delivery times do not distinguish CPU scene preparation from GPU/backend cost,
and the result is not a production rendering fix or Windows qualification.

The checker now requires explicit `--diagnose-readback` to request the secondary
attempt after primary failure. The default and unchanged automatic candidate
workflow perform only the ordinary primary attempt. The opt-in allows at most
one probe using the same source, extracted executable and adapter settings; it
retains the original failure even if that probe succeeds. A successful primary
capture never launches a probe. The original v1/v2 evidence rules remain intact,
and primary-only failures retain their validated command, timeout, launch and
log-hash evidence. This completed observation authorizes no further diagnostic
runs. The [candidate recipe](../release/swift-windows-candidate.md#bounded-screenshot-diagnostics)
documents the explicit opt-in separately from normal acceptance.

## Integrated Linux v2 observation

The combined jet/profile/replay and diagnostic source `b8ff801ea3a36a1934d01d47b24da6851959e1ed`
was built with default development features and exercised on the cloud desktop's
llvmpipe Vulkan driver. Binary SHA-256 was
`f0a22a06ce8e4f2b4c1f168be20170a42ccc6cc64f946e698260d28cbdfa46d6`.
This run used the ordinary Swift model, Light graphics/water and the existing
flat-reference native scene; it does not substitute for Windows qualification.

The actual log passed the strict v2 parser. The pre-scene copy verified all sixteen
green pixels and completed cleanup at 23 ms; its finite wait returned `queue_empty`.
All six ordinary-scene markers (1, 2, 4, 8, 16 and 30) completed. The ordinary
screenshot saved, and the late probe observed GPU/queue/map/pixel and async-resume
success, with its final cleanup at 15,018 ms. Sparse observation then froze at
49,986 ms after startup, with six registered and six completed markers. These
timestamps describe diagnostic observations and include startup/scene work;
they are not a frame-rate benchmark or physical-GPU evidence.

No panic or error was logged. After the complete observation, the operator stopped
the deliberately non-auto-exiting process (exit 130 / operator interrupt); this
is not a clean-exit assertion. The saved scene PNG SHA-256 is
`2f5774abf4e654571189d6a825a6b720676b498f86f74e9b3303bfd9c31d82a5`,
and the complete log SHA-256 is
`c1e1bfbd66b77fadea3d4c5f28f3b64d128e1cf0e5fac7b70ac1189e6e2678d3`.
The separately authorized exact Windows v2 observation was subsequently completed
with the failed result recorded above.

## V2 pre-scene and sparse scene observation (2026-10-04)

The preceding exact Windows candidate (`dfd7754`) timed out in both its ordinary
capture and its one flagged diagnostic capture at the unchanged 180-second
watchdog. The adapter was CPU Microsoft Basic Render Driver, D3D12, driver
10.0.26100.33438. The late tiny marker was submission 37, behind scene submission
36; its submission-specific 250 ms wait returned timeout, and there was no map
callback, queue callback or verified pixel by the 15-second summary. The separate
async task did start, suspend, receive its explicit signal and resume. This is
observed evidence; it does not isolate the driver or prove that queue polling was
missing. The lock already has `wgpu-hal` 27.0.4, including the known D3D12 shared
wait-event correction. No dependency is changed for this extension.

V2 adds one **shader-free 4×4 green clear/copy before scene extraction**, through
public Bevy 0.18.1 `RenderStartup`. The schedule executes before the first
`ExtractSchedule`/scene render. Other RenderStartup initialization can occur, so
this does not claim to be the device's very first submission. The marker uses the
already-created `RenderDevice`/`RenderQueue` with exactly the same adapter,
features and limits. It creates no camera, surface, shader, second device or
alternate backend. It retains the actual `Queue::submit` return value, registers
`map_async` directly, and makes exactly one `PollType::Wait` for that index with
`timeout=Some(5 seconds)`. It does not retry or wait for an unspecified/latest
submission. The existing late marker still makes its separate 250 ms wait.

After the startup wait returns, a successful map is checked against all sixteen
opaque-green RGBA pixels at the existing 256-byte row pitch and exact 1,024-byte
mapped size. The view is dropped before unmap. Before releasing resources, map
evidence is frozen with its mutex unlocked, then public `Buffer::destroy` and
`Texture::destroy` release/cancel in pending, mapped-but-unchecked, checked/unmapped
and failed states. The shared marker owner makes destruction idempotent, also on
ordinary Rust scope/app teardown. The pre-scene callback owns only a `Weak` signal
reference and an `Instant`; callbacks during/after destruction are labelled
`cancelled=true phase=cleanup` and cannot revise frozen observations. Ordinary
app teardown also closes the late probe and sparse callback state before GPU
resource destruction. A process kill cannot promise destructors or summaries.

After each ordinary public `render_system`, the diagnostic registers
`on_submitted_work_done` only at render frames **1, 2, 4, 8, 16 and 30**, before
submitting any late marker in that frame. These registrations add no rendering,
copy, submit or device poll. At most six closures retain only weak references to
one fixed-size state. Each registration and completion logs its frame and elapsed
milliseconds since the pre-scene observation returned. Missing callbacks remain
missing. Sparse evidence freezes at the existing late-probe cleanup or 60 seconds after
startup returns, whichever occurs first. Each callback independently checks this
60-second cutoff, so late delivery cannot gain completion credit even if a render
frame is stalled. State release and summary occur at the first render-system
opportunity to observe the boundary. `reason=late_probe|deadline` records which boundary was
observed. Frames not reached before cleanup remain absent. Later callbacks are
cancelled/late evidence only. This window allows frame 30 on the previously
observed roughly one-frame-per-second software path without adding work or
extending the 180-second process watchdog.

The 5-second wait limits GPU waiting only. Arbitrary callbacks, driver/API calls,
resource teardown or a stalled render frame can exceed this wall time or prevent
cleanup. A `prescene_poll_enter` without `prescene_poll_return` is partial evidence,
not a returned timeout. The unchanged outer 180-second watchdog remains required.
A returned timeout is an observation bound, **not a performance failure or proof
of an isolated driver defect**. Successful startup pixels prove only the tiny
pre-scene resource; sparse callback delivery proves only that callback's observed
queue progress. Neither qualifies Bevy's independent ordinary scene screenshot.
Normal screenshot success retains its immediate batch exit, so partial v2 logs
and absent late/scene summaries are valid inspectable evidence.

### Exact structured protocol

All lines retain the `FS_READBACK_PROBE` prefix. V2 starts with exactly
`event=enabled version=2`. The existing late-probe events and their fields remain
unchanged. New pre-scene events are:

- `prescene_begin` (no fields)
- `prescene_submitted`, `prescene_map_register_enter`, `prescene_map_registered`
  (`elapsed_ms`)
- `prescene_poll_enter` (`gpu_timeout_ms=5000`, `elapsed_ms`)
- `prescene_poll_return` (`status`, `wall_ms`, `elapsed_ms`)
- `prescene_map_callback` (`result=ok|error`, `cancelled=true|false`,
  `phase=startup|cleanup`, `elapsed_ms`)
- `prescene_pixels` (`valid=true|false`, `count=16`, `elapsed_ms`)
- `prescene_summary` (`submitted=true`, `map_callback=missing|ok|error`,
  `pixels=missing|valid|invalid`, `poll`, `cleanup=true`, `elapsed_ms`)

Pre-scene elapsed times start at `prescene_begin`. `status`/`poll` use only
`queue_empty|wait_succeeded|timeout|wrong_submission|unexpected_poll`. One direct
map callback can occur synchronously after register-entry and before its return.
Pixels require a successful noncancelled callback and the wait's return. Summary
fields must agree with all preceding active observations. Cancellation never
counts as map success, even if its callback result is `ok`.

New scene events are `scene_register` (`frame`, `elapsed_ms`), `scene_callback`
(`frame`, `cancelled`, `phase`, `elapsed_ms`), and `scene_summary` (`registered`, `completed`,
`cleanup=true`, `reason=late_probe|deadline`, `elapsed_ms`). Frame values are only
1, 2, 4, 8, 16, 30; registration order must follow that prefix, each callback must
follow its registration, counts are at most six, and summaries must agree with
noncancelled callback evidence. A deadline summary requires at least 60,000 ms;
a late-probe summary requires the actual preceding late-probe terminal summary
and at least its elapsed time, because the scene clock started first. Active
callbacks and a late-probe summary must be before 60,000 ms; later callbacks are
cancelled, and later summaries record the deadline reason. Startup poll wall time
must fit its elapsed interval with at most one millisecond of quantization.
Callbacks delivered after either state freezes can only be labelled cancelled.
Scene callback phase is exactly `active|deadline|late_probe|owner_teardown|owner_released`.
Only `active` is noncancelled. `deadline` requires at least 60,000 ms; `late_probe`
requires preceding late-probe summary evidence. `owner_teardown` identifies
observed app teardown without requiring a summary that cannot occur, while
`owner_released` means its Weak reference no longer upgrades and the earlier
close cause is unavailable. Scene summary elapsed time records the freeze itself,
so delayed log delivery cannot relabel which boundary froze it.

The strict parser caps v2 at 64 events, each at 1,024 bytes, and all elapsed/wall
milliseconds at 180,000. It rejects unknown/duplicate fields and events, invalid
values, extra frame registrations, contradictory summaries and causal violations.
Existing v1 logs remain inspectable under their original 32-event parser and
schema; v2 reports schema 2 with separate `prescene_summary`, `scene_summary` and
observations, alongside the unchanged late `summary`. JSON must still equal the
exact validated log projection. No arbitrary JSON/log field, PNG allowance,
launcher variation or acceptance gate is added. `qualifies_acceptance=false`,
primary failure, binary identity, scene, ordinary capture, PNG validation and
watchdog policy remain unchanged. This extension authorizes no repeat launch or
acceptance claim from a diagnostic result.

### Source basis and current validation

Read from pinned official registry source, with corresponding upstream references:

- [Bevy 0.18.1 RenderStartup before extraction](https://github.com/bevyengine/bevy/blob/v0.18.1/crates/bevy_render/src/lib.rs#L500-L519)
- [wgpu-core 27.0.3 submission wait/fence interpretation](https://github.com/gfx-rs/wgpu/blob/v27.0.3/wgpu-core/src/device/resource.rs#L747-L825)
- [wgpu-core 27.0.3 submit maintenance and unlocked callback firing](https://github.com/gfx-rs/wgpu/blob/v27.0.3/wgpu-core/src/device/queue.rs#L1420-L1453)
- Pinned `wgpu` 27.0.1 `api/queue.rs`: retained `SubmissionIndex` and
  `on_submitted_work_done` registration/delivery semantics
- Pinned `wgpu-core` 27.0.3 `device/global.rs::buffer_destroy`: internal unmap
  tolerates idle/failed mapping before resource destruction
- Pinned `wgpu-types` 27.0.1: finite `PollType::Wait`, `PollStatus` and `PollError`

The 51 focused Python candidate tests pass, including v1 and v2 evidence, partial
startup/poll/early-exit logs, slow sparse frames, both cleanup triggers, synchronous
and cancelled callbacks, malformed/contradictory evidence, primary-failure
preservation and rejected unproven PNG upload. The saved `candidate-dfd7754` v1 log
still projects exactly to its existing JSON. With `commercial-staging` enabled,
the exact isolated source compiled with warnings denied and passed all ten Rust
diagnostic lifecycle tests, the opt-in CLI test and eleven capture-related tests
(including unchanged thirtieth-frame arming and batch exit). Strict app all-target
Clippy, strict app private documentation, workspace formatting, architecture,
Python compilation and diff whitespace checks pass. Shared-target crate roots
were explicitly refreshed before compilation to avoid reuse of another worktree's
app feature graph. Independent lifecycle/protocol review approved the final
source after verifying rejection of impossible poll intervals, scene summaries
younger than the late-probe lifetime, and active callbacks at the 60-second cutoff.
It also independently reproduced the unchanged historical v1 projection and found
no remaining default-off, lifecycle/API or acceptance blocker. The lead's native
Linux lifecycle run and the single exact follow-on Windows candidate remain
pending; these source/test records do not claim they have run.


## Historical v1 native Linux validation

The combined weather/diagnostic source was exercised on the cloud desktop's
llvmpipe Vulkan driver. An initial commercial-staging launch without its required
adjacent asset bundle correctly stopped before graphics with exit 2. That layout
policy was retained; diagnostic mechanics were then checked using the development
profile with explicit Swift and the normal source asset root. This is not Windows
candidate or commercial-package qualification.

The first full observation run saved the ordinary scene PNG, received a successful
direct map callback, verified 16 green pixels and resumed the externally woken
async task. It then exposed a deadline-cleanup bug: unmapping an already unmapped
buffer triggered a wgpu validation panic. The subsequent fix uses public
`Buffer::destroy` for final cancellation/release, following the pinned wgpu-core
contract, and separates current mapping ownership from sticky callback evidence.
Independent review and six focused Rust lifecycle tests covered successful,
unchecked, pending, failed and already released states.

The corrected native binary SHA-256 was
`b8d71f9c8af9a6847b0d497ff61cb6acce99ed6f2b7af495dda873c9cf6efbde`.
Its actual structured log parsed successfully with the candidate evidence parser:
map/queue callbacks occurred during normal observation; all 16 pixels were valid;
the async task started, waited, received its external signal and resumed; the
single poll returned `queue_empty` in 2 ms; and the final summary reported
`cleanup=true` at 15,091 ms. The app stayed responsive after that summary without
a panic. The operator subsequently stopped this deliberately non-auto-exiting
QA process (receipt exit 130 / operator interrupt), so this is not a clean-process
exit assertion. Its ordinary scene PNG SHA-256 is
`62ae3523b1cecc8904d1c83a315abda92a76d290bc077fc7e870f106d4d1a865`.

This verifies the diagnostic's Linux happy path and cleanup. It neither explains
the Windows stall nor changes the primary-capture acceptance rule. The subsequent v1 MSVC result is recorded above; this Linux evidence does not
qualify it. V2 requires its own exact native/Windows validation.

This is an opt-in diagnostic, not a screenshot fix or candidate acceptance.
The prior exact Windows candidates reached Bevy screenshot `map_async` on the
Microsoft Basic Render Driver D3D12 adapter, then hit the 180-second process
watchdog without a PNG. That proves the initial screenshot AsyncCompute task
ran far enough to register mapping; it does not identify whether GPU completion,
callback delivery, or executor wake/resume prevented completion.

## Retained late-probe isolation and sequence

`--windows-readback-diagnostic` defaults to false. The ordinary app installs no
probe extraction or render systems and creates no probe buffers or tasks. The
flag does not imply a screenshot, headless mode, an exit, or changed readiness.
The existing capture system arms the late probe when it requests its usual Screenshot.
The request extracts to RenderApp and the probe executes after the public
`bevy::render::renderer::render_system` within RenderSystems::Render. It neither
reads nor replaces Bevy's private screenshot implementation.

The late probe owns a single 4×4 Rgba8Unorm texture, clears it to opaque green with a
render pass, and copies it into a 1,024-byte MAP_READ | COPY_DST buffer at 256-byte
row pitch. No shader, camera, surface, dependency or engine fork is involved.
It retains the actual index returned by its queue submission. Its map callback
is registered directly outside AsyncComputeTaskPool and records the result;
the render thread checks all sixteen RGBA pixels and unmaps after a successful
callback. Padding is ignored and the complete mapped size must be exact.

A separate retained AsyncCompute task records starting, suspends through
`std::future::poll_fn` with a mutex-protected waker, and receives one explicit
signal from a later render frame only after that waker is stored. The render
thread takes the waker out of the lock and wakes it after unlocking. Resumption
is separate evidence from initial task execution. The task does not wait on the
GPU callback and cannot make the map succeed.

For the first five seconds the probe relies on the ordinary renderer's ongoing
submissions. Then it makes at most one public device poll: PollType::Wait with
its retained submission index and a 250 ms GPU wait timeout. The entry, return,
status and actual wall time are recorded. The probe observes until fifteen
seconds and releases its mapping, buffer, texture, saved submission, waker and
retained task on the first render-system opportunity at/after that deadline.
Dropping the task cancels it without blocking; destruction of its future can
still await executor progress. That future owns no probe GPU resources.
Cancellation/late callbacks are labelled and do not change frozen evidence.
The callback closures hold only weak references to signals, never probe GPU
resources. The small finished marker prevents a second probe.

A stalled render frame or arbitrary callback can prevent deadline cleanup from
running. The wgpu timeout bounds waiting for the GPU submission, not arbitrary
callback code, resource teardown or the process. Therefore the unchanged outer
180-second process watchdog is required. A poll entry without return is explicitly
partial evidence; it is not misreported as a returned 250 ms timeout. A normal
screenshot success retains its original immediate batch exit, so a successful
diagnostic PNG can naturally precede the fifteen-second summary.
Batch exit can also interrupt the explicit poll before its return marker, so
interpret a missing return alongside the process outcome and actual PNG proof.

## Evidence and interpretation

All machine-readable events use `FS_READBACK_PROBE` on stderr. When the checker
is explicitly given `--diagnose-readback`, a primary capture failure permits one
launch of the same EXE using the baseline launcher, scene, features, delay and
180-second watchdog, adding only the app's `--windows-readback-diagnostic` flag
(and a distinct diagnostic PNG destination). The old alternate-console
comparison remains absent. The checker always
preserves the primary capture failure and never proceeds to remaining required
acceptance checks on the strength of diagnostic success. The diagnostic record
always has `qualifies_acceptance=false`.

The bounded, independently validated diagnostic log/JSON/optional PNG form a
separate evidence set. A terminal summary reports GPU submission, explicit poll,
map callback/result, pixel verification and AsyncCompute start/wait/signal/resume
separately. A cancelled map callback is not successful mapping. The queue
`on_submitted_work_done` event is callback evidence only: absence is ambiguous,
and it does not independently measure completion without callback delivery.

Valid green pixels establish completion/readback for the tiny owned resource;
a successful submission-specific wait establishes completion of that submission.
Neither establishes correct operation of Bevy's independent scene screenshot
path or screenshot executor future. Progress during the explicit poll suggests
poll/timing sensitivity. It does not prove normal renderer polling is absent.
A task-start event does not prove external wake/resume. Missing summary/events,
a nonzero exit or a process watchdog remain explicitly partial/failed evidence.

## API basis and validation

Source API inspection used the existing official registry copies of
Bevy 0.18.1 and wgpu/wgpu-types 27.0.1:

- bevy_render `renderer/render_device.rs`: public RenderDevice::poll forwarding
- bevy_render `lib.rs`: RenderSystems::Render and public render_system ordering
- bevy_render `view/window/screenshot.rs`: actual separate screenshot task path
- wgpu `api/queue.rs`: SubmissionIndex, submit and callback semantics
- wgpu-types `lib.rs`: submission-specific PollType::Wait and GPU timeout
- bevy_tasks `task.rs`: retained Task drop cancels without a blocking wait

Rust helper tests cover all sixteen pixel positions, exact mapped size, padded
rows, suspension before an external wake, lock release before wake, and cancelled
callback/future handling. The capture scheduling regression checks that the probe
arms only at the same ordinary thirtieth-frame Screenshot request, exactly once.
Linux commercial-staging validation passed eight focused readback/CLI/capture
tests, strict app all-target Clippy, formatting and architecture checks. No native
probe was launched as part of those helper tests.

Parser and harness tests cover complete and partial probe outcomes,
primary-failure preservation, launch/hash identity, malformed or tampered
evidence and nonqualifying diagnostic PNGs. Actual native/Windows results must
be reported separately after their runs; this document does not claim Windows
qualification or a resolved screenshot failure.

## Cleanup correction from the first complete observation

A Linux native run of combined source `f9dc4a36a9a7253e32608af1ee1cf90085be56b1`
saved the ordinary scene PNG and observed a successful map, sixteen green pixels,
external async wake/resume and a `queue_empty` explicit poll. At the fifteen-second
cleanup it then failed with `Buffer is not mapped`: pixel verification had already
unmapped the buffer, but cleanup unconditionally unmapped it again. The PNG and
intermediate events do not make that failed run complete; no summary was emitted.

Cleanup now distinguishes mapping ownership from persistent callback evidence.
Pixel verification drops its view and unmaps exactly once. Deadline cleanup uses
only public `Buffer::destroy` for all ownership states: checked/unmapped,
mapped-but-unchecked, failed, and awaiting a callback. In the pinned wgpu-core
27.0.3 `device/global.rs::buffer_destroy`, that operation performs internal
unmap/cancellation while deliberately accepting an already-idle buffer, then
destroys native resources. Public `unmap` instead reports a validation error on
Idle, including after a mapping error. Callback absence cannot safely identify
backend state: completion or failure may precede callback delivery.

The evidence is frozen before destruction, with the signal mutex released before
the API call because pending-map cancellation can invoke a callback synchronously.
Late/cancellation callbacks still cannot change the summary. CPU lifecycle tests
cover all four ownership states, delayed success/error delivery, one-shot
destruction, persistent callback evidence after unmap and cancellation callbacks
during destruction. The lead must rerun the complete native observation to verify
the correction; these regressions alone do not establish native/Windows success.
