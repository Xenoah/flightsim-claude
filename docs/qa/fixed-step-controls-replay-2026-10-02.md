# Fixed-step control callback and replay verification

Date: 2026-10-02. Scope: pure `flightsim-sim` integration and replay identity.
Baseline: input sampling commit `98c3652`, with reviewed fixed-clock boundary
correction `8a82cb1`. This note is not evidence that the application wiring,
native input, UI, publication or all flight-envelope behavior was tested here.

## Behavior

- `Simulation::advance_with_controls` requests effective controls for each actual
  fixed FDM step and exposes that step's initial rigid-body state to the recorder
- `advance` remains the constant-control wrapper over the same path
- The original `FixedStep` owns accumulation and the 0.25 s clamp; no new clock or
  Bevy dependency enters the simulation crate
- The first impact or divergence stops further control callbacks and FDM calls
  inside that frame. `steps` and physical elapsed count executed calls, and unused
  terminal frame time is discarded. A finite crash displays the actual impact pose
- A domain-separated `FDM_MODEL_REVISION` suffix is part of the existing aircraft
  fingerprint. Old alpha.21 identity is rejected for reproduction, while valid
  v1/v2 byte reads/writes and historical durations remain intact

## Regression cases

`tests/fixed_step_controls.rs`:

1. Constant-input wrapper/callback equality, including interpolation and flight log
2. Changing controls at 30/60/144 render Hz: callback pre-state equals a separately
   stepped 120 Hz reference at every callback, and all run exactly 1,200 steps in
   ten seconds
3. Jittered render cadence with sub-step, zero-step and multi-step frames produces
   only fixed-duration records, with the actual effective controls
4. Byte round-trip and full replay from frame zero reproduce all rigid-body state,
   physical time, log, contact and crash state exactly
5. Repeated seeks from frame zero to arbitrary targets reproduce those same fields,
   including 240-record batch boundaries
6. Half-step remainder survives zero-duration pause calls; pause and negative time
   append no records; a 60-second frame is clamped to 30 physics callbacks
7. Invalid pre-state calls no callback and counts zero steps; a finite extreme
   state that diverges on its next step counts exactly one and freezes immediately
8. Impact within one 0.249-second frame matches sequential fixed steps and byte
   replay exactly, including impact time, flight log, contact and frozen pose;
   subsequent calls record nothing, and recovery does not resurrect the budget

`tests/replay_model_identity.rs` checks the historical Light Single config-only
identity `e8ca8e4cac33cd04`, captured from pre-change native baseline recordings.
With identical config, the current domain/revision suffix must change the identity.
Old v1 and v2 records retain their exact bytes and original fingerprint on
read/write, but current-model reproduction yields an explicit aircraft/FDM mismatch.

## Local checks

- `cargo test -j 2 -p flightsim-sim`: 255 passed, 0 failed, including two doctests
- `cargo clippy -j 2 -p flightsim-sim --all-targets -- -D warnings`: passed
- `cargo doc -j 2 -p flightsim-sim --no-deps`, with `RUSTDOCFLAGS=-D warnings`: passed
- `cargo fmt --all --check`: passed
- `bash scripts/check-architecture.sh`: passed

Tests used the supplied Rust toolchain, shared build target, debug info disabled,
incremental compilation disabled, and two jobs. Raw logs remain outside the
repository. Independent source review found no sim/replay blocker; application
sample/update/record integration still requires its own review and tests.

## Resource implications

Limits are unchanged. At 120 recorded steps/s, 1,000,000 records cover about
2 h 18 m 53 s; serialized frames use 56 MB plus at most 900,072 bytes of keyframes
and small metadata. This is serialized size, not an in-memory allocation promise.
Checkpoints every 120 records now represent one simulated second. The application's
240-record playback/seek update bound represents two simulated seconds per update
for these recordings; no wall-clock timing guarantee or enlarged work bound is made.
