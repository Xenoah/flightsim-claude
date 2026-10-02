# Replay file numeric boundaries (2026-10-01)

## Scope

The format-v1 replay reader is a trust boundary: recordings can come from other
machines, and app playback restores the frame-zero keyframe directly. Numeric
corruption must produce an ordinary `ReplayError`, rather than silently changing
controls or repairing a different attitude into an apparently valid flight.
The format version and layout are unchanged.

## Reproduction and verification

`crates/flightsim-sim/tests/replay_file_boundaries.rs` builds format-v1 bytes
independently of `Recording::write_to`, then mutates individual encoded fields.
This avoids letting writer validation hide reader defects. Before changing the
reader, the first eight tests produced **7 failures / 1 pass**: the reader accepted
NaN stored duration, NaN keyframe position, NaN controls, NaN conditions,
out-of-domain latitude, zero quaternion, and overflowing summed duration. The
independent valid-v1 byte round trip passed.

Final verification at **2026-10-01 18:38 UTC**:

- Debug and release replay suites: **50 passed** each (24 new boundary/player
  tests, 19 existing hostile-file/player tests, 7 flight-fidelity tests)
- Entire `flightsim-sim --all-targets`: **226 passed**, 0 failed, 0 ignored
- Sim doctests: **2 passed**
- Sim all-target clippy with `-D warnings`: passed
- Scoped rustfmt and `git diff --check`: passed

All commands sourced `/workspace/shared/flightsim-tools/env.sh` and used
`CARGO_TARGET_DIR=/workspace/shared/flightsim-replay-target CARGO_BUILD_JOBS=1`.
The focused commands were:

```sh
cargo test --offline -p flightsim-sim \
  --test replay_file_boundaries --test replay_hostile --test replay_fidelity
cargo test --offline -p flightsim-sim --release \
  --test replay_file_boundaries --test replay_hostile --test replay_fidelity
cargo test --offline -p flightsim-sim --all-targets
cargo test --offline -p flightsim-sim --doc
cargo clippy --offline -p flightsim-sim --all-targets -- -D warnings
```

The before-fix reproduction was run before editing `replay.rs`. The initial
post-fix visual-end test used only 1 second beyond the very large maximum date;
that rounded back to the exact bound in `f64`, so its test expectation was wrong.
The final fixture advances 61 seconds past the bound and is correctly rejected.
This records a corrected test, not an unreported passing claim.

## Numeric contract

- Stored frame durations are finite and nonnegative. Zero duration is valid for
  the first GUI frame, paused/no-step frames, and exact legacy round trips.
  A long finite frame is not capped to the fixed-step clamp: that would alter
  the recorded playback schedule. The nonnegative sum must remain finite.
- Stored control scalars use the existing `ControlInputs` domains: aileron,
  elevator and rudder in `[-1, 1]`; throttle, flaps and brakes in `[0, 1]`.
  Read-time clamping of corruption is no longer accepted.
- All keyframe components must be finite. Vector squared magnitudes must be
  representable. Attitudes must have unit length within the existing `1e-12`
  round-trip tolerance; valid components are never normalized or rewritten.
- Start latitude/longitude use their geodetic domains, altitude and angle fields
  must be finite, and the start position must have representable ECEF magnitude.
  Nonnegative wind speed/turbulence intensity must have representable squared
  magnitude. Headings remain multi-turn angles rather than being normalized.
- The visual time rate is finite and nonnegative. It is deliberately not capped
  at the CLI's newer 3600x limit, so numerically safe legacy rates remain valid.
  Scaled elapsed seconds must remain finite and the computed visual epoch must
  stay within the supported calendar range.
- `replay::MAX_VISUAL_EPOCH = 784_354_017_363.5` is UTC
  `2147483647-12-31 00:00:00`, the last midnight of the visual calendar's `i32`
  year range. Its final day is reserved as rounding headroom. This is a numeric
  representation bound; solar accuracy at remote dates is not guaranteed.
- Epoch zero remains the "not recorded" sentinel. The reader validates the
  offset in that case; the app must validate the eventual resolved origin plus
  that offset using the same exported bound.

These are file/data-representation constraints, not an aircraft performance
or geographical flight envelope. Passing the reader does not prove that the
flight remains physically stable, matches local terrain, or uses this build's
physics. Runtime divergence checks and aircraft fingerprint checks still apply.

## Compatibility considerations

Valid existing format-v1 files retain their bytes, including near-unit
quaternion rounding and zero-duration frames. Corrupt files previously repaired
by sanitizing controls, normalizing arbitrary quaternions, replacing zero/NaN
quaternions with identity, or passing nonfinite fields are intentionally rejected.
The format does not gain checksums or a guarantee that arbitrary finite byte
corruption can be detected.

Writer preflight validation rejects invalid records before writing any bytes,
without changing `Recorder::new`, `record`, or `finish` signatures. Aircraft names
longer than 256 UTF-8 bytes now return `TooLarge` instead of being silently
truncated; the previous truncation could split a UTF-8 code point and produce a
file that its own reader rejected. Valid serialized names keep their bytes.
`check_reproducible_with` validates numeric data before checking the aircraft
fingerprint, so a matching fingerprint cannot bypass validation.

The in-memory recorder and `Player::new` remain permissive and infallible;
constructing either is not proof of validation. Runtime callers must defend
against invalid manually constructed records. The app's separate runtime work
owns those checks. I/O failure after a valid writer preflight can still leave
partial output; this is not an atomic file-save implementation.

Frame/keyframe count caps, sorted/in-range keyframe indices, short-read errors,
and stream-oriented read semantics are unchanged. Existing no-frame recordings
and no-keyframe recordings remain readable at the data layer. Whether the app
can reproduce a particular recording is a separate startup decision.


## Player budget defense

`Player::accumulate` now computes the candidate budget first and accepts it only
when finite. Finite input multiplied by speed can overflow; adding two finite
budgets can also overflow. Both cases ignore that update without discarding the
previous finite budget. Separate regression tests consume the preserved budget,
then demonstrate that a later ordinary accumulation still works. No real-frame
cap or new `Player` API is introduced.

## Remaining boundaries

This work verifies the pure-Rust replay data layer on Linux in debug and release.
It does not establish a GUI visual result, cross-CPU bitwise determinism,
Windows-specific behavior, or an end-to-end release asset. The app must still
check runtime state/drift/elapsed time and resolve an omitted epoch safely; those
checks are covered by the separate app replay-runtime work.
