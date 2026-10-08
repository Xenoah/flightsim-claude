# ADR-0029: Bound render work during native screenshot capture

- Status: implemented locally; Windows runtime qualification pending
- Date: 2026-10-08

## Evidence

The single Windows diagnostic at commit
`a11d09f161dc99113f176c4a377d80fbafd7eb85` timed out at the unchanged
180-second process watchdog. The selected screenshot map registered successfully
with submission dependency 39. Across 136 successful fence reads, completion
advanced from 9 to 35 while successful submissions grew from 46 to 181. The last
read was 140.383 seconds after screenshot preparation. Maintenance kept returning
successfully. This demonstrates a growing submission backlog before the map can
be promoted; it does not establish a permanently frozen GPU, a driver defect or
a missing callback-service path.

Source: [run 37733675230, job 113168300051](https://github.com/Xenoah/flightsim-claude/actions/runs/37733675230/job/113168300051).
The exported ZIP is 660140 bytes, SHA-256
`f87e5f81bc7b0c78c010b663118d322c2a8d2ada3e80d6c053cb5e7c6b4ba336`.
All 418 exported events are contiguous; its last snapshot reports no dropped or
undrained events, but a process-final undrained tail is still possible.

## Decision

Only an explicit native `--screenshot`/`--headless-screenshot` session installs
an outer render schedule. It always runs Bevy's entire original Render schedule,
including preparation, graph submission, screenshot collection and cleanup.
Before doing so, it obtains one of two render-frame credits. Each completed
Render schedule registers the public wgpu queue-completion callback covering
all submissions made before registration. The callback only sets its own atomic
flag. A completed prefix releases credits; incomplete callbacks cannot do so.
This bounds uncompleted render-frame batches, not individual internal wgpu
submissions or GPU bytes.

When both credits are occupied, the gate withholds the next Render schedule.
It services completion with a wgpu wait of at most 100 milliseconds per call,
rechecking the completion flags and cancellation between calls. A wait timeout
never returns a credit. Other poll errors fail closed through a runtime error;
they do not admit another frame or claim screenshot success. This servicing is
necessary because this state machine intentionally stops the submissions that
normally service callbacks. No extra empty submissions, sleep, engine patches
or synthetic completion signals are used. Bevy does not re-export `PollError`;
app directly names the already-locked `wgpu-types =27.0.1` with default features
disabled to distinguish timeout from other errors without string matching. No
package version or backend feature is changed.

Bevy's extraction function remains intact. Its pipelined renderer continues to
use the same sub-app and original Render schedule. An extracted screenshot can
wait for a credit, but no render preparation/collection step is skipped. CPU
readiness, minimum delay, the 30-frame floor and the previous committed-scene
observation retain their existing meaning. GPU credit completion does not prove
shader readiness or visual acceptance.

The shared session ends when the screenshot event arrives, whether PNG saving
subsequently succeeds or fails, or when the request is removed. Late callbacks
own isolated flags and cannot complete a new session. Ordinary launches install
no session, callback or scheduling wrapper. The one-shot CLI is not converted
into a repeated-capture API. WebGPU does not support blocking device polling, so
this native gate is not installed for wasm targets.

## Costs and rejected alternatives

- Capture frame cadence changes and may block the render thread; a windowed
  capture can consequently stop receiving further input while GPU work is stuck.
  Each GPU wait is finite, but a native callback or driver call is not a process
  deadline. The independent 180-second watchdog remains mandatory for acceptance.
- Two queued frame batches preserve limited CPU/GPU overlap while preventing
  the observed unbounded growth. This is a work bound, not a measured speedup,
  deadline guarantee or proof the Windows candidate now succeeds.
- Skipping just `render_system` is unsafe: Bevy marks screenshots Capturing in
  extraction, clears prepared screenshot state every preparation pass, then
  dispatches the map task from rendering. Skipping that frame can lose capture.
- Blind extra polling does not repair the measured producer/consumer imbalance;
  queue submission already performs maintenance.
- Scene/quality reduction, changing backend/adapter, longer watchdogs, fewer
  readiness frames and engine upgrades would change the qualification target.

Deterministic tests cover prolonged maintenance without completion, backlog
bounds, repeated and reordered completion, credit reclamation, cancellation,
late callbacks, error debt, and unchanged ordinary registration/extraction.
Existing readiness and PNG/exit tests remain applicable. Actual Windows capture,
visual review and all frozen source/capture/release contracts are still required.


## Follow-up: preparation pacing

The ordinary 3b098bc1 Windows attempt timed out before CPU readiness: retaining
the whole render sub-app during credit waits also paced bounded Main preparation
by GPU throughput. The cap still prevented unbounded submitted frame debt. See
[ADR-0030](0030-batch-capture-preparation-admission.md) for the separate loading
admission correction and its remaining native-acceptance requirements.
