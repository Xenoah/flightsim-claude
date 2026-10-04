# Bounded Windows readback diagnostic

## Subsequent native Linux validation

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
the Windows stall nor changes the primary-capture acceptance rule. Actual exact
MSVC execution and interpretation of its separate events remain outstanding.

This is an opt-in diagnostic, not a screenshot fix or candidate acceptance.
The prior exact Windows candidates reached Bevy screenshot `map_async` on the
Microsoft Basic Render Driver D3D12 adapter, then hit the 180-second process
watchdog without a PNG. That proves the initial screenshot AsyncCompute task
ran far enough to register mapping; it does not identify whether GPU completion,
callback delivery, or executor wake/resume prevented completion.

## Isolation and sequence

`--windows-readback-diagnostic` defaults to false. The ordinary app installs no
probe extraction or render systems and creates no probe buffers or tasks. The
flag does not imply a screenshot, headless mode, an exit, or changed readiness.
The existing capture system arms the probe when it requests its usual Screenshot.
The request extracts to RenderApp and the probe executes after the public
`bevy::render::renderer::render_system` within RenderSystems::Render. It neither
reads nor replaces Bevy's private screenshot implementation.

The probe owns a single 4×4 Rgba8Unorm texture, clears it to opaque green with a
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

All machine-readable events use `FS_READBACK_PROBE` on stderr. The candidate
replaces the old alternate-console comparison with one launch of the same EXE,
using the baseline launcher, scene, features, delay and 180-second watchdog,
adding only this flag (and a distinct diagnostic PNG destination). It always
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
