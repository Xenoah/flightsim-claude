# Pure-sim jet presentation and native integration boundary

This additive API prepares the native adapter without changing the jet FDM law,
simulation revision, identity, fixed duration, or replay-v4 bytes. Legacy
`Simulation`, profile v1, and replay v1–v3 remain unchanged.

## New-flight initialization

`JetSimulation::parked(config, start, heading, environment)` owns the terrain,
climate, wind and FDM bridge for both `JetTerrain::Flat` and `BundledGlobal`.
The start point selects latitude/longitude; ground determines CG altitude.
The authored three-wheel triangle is rotated onto the sampled slope, including
unequal leg lengths, without changing the aircraft configuration. The forward
direction is projected along the slope, following the legacy parked convention.
At exact poles, heading is measured in the core's canonical longitude-zero NED
frame, shared by the sampled ground and pose regardless of supplied longitude.

Degenerate contact triangles, vertical support planes, support planes above the
CG, invalid coordinates/headings, actual FDM wheel-clearance residuals exceeding
1 mm, and unsupported initial force-law queries are errors. The 1 mm tolerance
accounts for geodetic curvature across the tangent-plane wheel footprint and
matches the existing FDM contact margin. This is an uncompressed geometric
placement, not a static suspension solution or guarantee of stable support.
Gravity and authored idle thrust take effect on the first tick. There is no
parking lock or hidden brake. `restart_parked_at` replaces the whole flight only
after placement succeeds; errors preserve the old state/history/clock.

The existing `jet_parked_state` remains the unchanged flat-ground numerical
fixture. `from_state` remains authoritative for replay and explicit airborne
starts, including unsupported initial states used as terminal-at-zero evidence.

## Read-only presentation

`JetSimulation::presentation()` returns `JetPresentationSnapshot`:

- `pose`: existing `InterpolatedState` for rendering only
- `atmosphere`, `aero_angles`, `mach`, and optional Mach-scheduled `aero_coefficients`
- `climate`, the held `ground`, CG `agl`, and ordered `gear_clearances`
- `on_ground`: the existing contact-history hysteresis
- `stall_fraction`: absolute alpha / scheduled stall angle, zero below 5 m/s
  or without supported coefficients, and otherwise not capped at one

Only pose interpolates. Diagnostics describe the committed state at executed
simulation time using the sim-owned environmental bridge; no terrain is sampled
and no physical state or clock is changed by reading. Unsupported current table
or operating-envelope queries produce absent coefficients, never a clamped or
extrapolated sample. The terminal latch supplies the reason for stopped physics.
Presentation alone is not a force-law admission or replay-permission check.

Convenience accessors are `interpolated`, `aero_angles`, `atmosphere_sample`,
`climate_sample`, and `stall_fraction`. The native app must use these results
instead of reconstructing wind/climate/ground or substituting legacy aero data.

## Recording and replay

`JetRecorder::export(&self)` returns a complete owned `JetRecording`, with a
mandatory final checkpoint on its clone. Repeated exports neither close the
recorder nor leave transient final checkpoints in the ongoing recording.
Empty and terminal-at-zero recordings remain valid. `finish(self)` retains its
previous byte output and consuming behavior.

`JetReplayPlayer::recording`, `last_controls`, and `seek_target` expose read-only
source/input/seek information. `last_controls` reports the last committed input,
or neutral at cursor zero; rejected terminal inputs stay in the terminal event.
`set_speed` uses the existing replay range 0.1–8x, with NaN reset to 1x and
infinities mapped to the appropriate bound. Frame-time admission remains capped
at 0.25 real seconds before scaling. No replay call performs more than 240
attempts, including seek and terminal attempts. A terminal following the 240th
successful step is verified on the next call, which may have zero elapsed time.

`JetReplayPlayer::interpolated` and `presentation` use the player's own
accumulator, never the live simulation's unused fraction. Pauses, seeks, faults,
completion and pending terminal verification show the current committed pose.
Successful seek and unpause preserve that pose until ordinary playback executes
a new tick. Speed, seek and presentation do not change recording bytes.

## Verification boundary

`jet_presentation.rs` checks flat/global slope starts, unequal/degenerate gear,
atomic restart, climate/wind/Mach-scheduled warnings, non-mutating interpolation,
repeated exports at periodic-checkpoint boundaries, terminal-zero export, and
replay fraction/speed/pause/seek/end behavior. The parked unit test checks the
actual FDM contact geometry at eight headings and four signed 15-degree slopes.
`jet_replay_v4.rs` additionally verifies the 8x 240-attempt terminal boundary.
Existing jet identity, v4 independent golden bytes, legacy byte roundtrips,
headless acceptance and numerical flight suites remain the compatibility gates.

These APIs and headless checks are not native/GPU, controller, handling, real
aircraft performance, or release approval evidence.
