# Standalone numerical jet flight and v4 replay

`flightsim-jet-headless` loads a validated exact-decimal profile v2 and runs the
separate transactional jet model without Bevy, a GLB, a window or the old app.
The supplied `numerical-jet.json` is an original numerical boundary fixture with
an intentionally unprovided visual model. It is not a certified or named real
aircraft, a production aircraft preset, or a claim about real engine performance.

Use the repository's coordinated Rust build environment and `-j2`:

```sh
cargo run -j2 -p flightsim-sim --bin flightsim-jet-headless -- \
  --profile docs/examples/aircraft-profiles-v2/numerical-jet.json \
  --scenario takeoff --seconds 60 \
  --trajectory /tmp/jet-takeoff.csv --record /tmp/jet-takeoff.fsreplay

cargo run -j2 -p flightsim-sim --bin flightsim-jet-headless -- \
  --profile docs/examples/aircraft-profiles-v2/numerical-jet.json \
  --replay /tmp/jet-takeoff.fsreplay
```

The CSV contains committed time, CG location, airspeed, vertical speed, pitch,
minimum physical gear clearance and successful-step count. A rejected attempt
adds no time or ordinary input. The status line and terminal error identify the
committed cursor, typed reason, RK4 stage, substep and available ambient query.
Flight rejection returns a nonzero exit status; an independently verified replay
of that recorded rejection succeeds and prints its terminal status. Output files
are flushed with checked I/O before success is reported.

Available numerical drivers:

- `trim`: solve north/down acceleration and pitch moment at the requested
  `--altitude-m`/`--speed-mps`, then hold all effective controls unchanged
- `takeoff`: a separately tuned regression driver with pitch ramp, pitch-rate
  damping and roll/yaw leveling. Its climb is feedback-controlled test evidence,
  not demonstrated natural handling or a hidden live-flight stabilization mode
- `approach`: initialize a solved 35 m/s, 2 m/s descent from 60 m with half flaps,
  then hold controls. Actual rotated-wheel contact is observed; no landing quality,
  body/wing collision, structural damage or jet crash classification is supplied
- `throttle`: begin at the solved trim, hold other controls and apply authored idle
  followed by maximum dry. Zero command means idle, which may have signed net thrust
- `domain-exit`: initialize just beyond the profile's actual upper Mach boundary,
  so the first attempted step visibly rejects and can be saved/replayed at cursor zero

The CLI uses flat ground at ellipsoid height zero. The pure `JetEnvironment` API
also supports the bundled global terrain and fixed bundled climate identities,
wind, turbulence and explicit authored weather parameters. There is no arbitrary
regional source or package replay admission. Weather metadata is preserved;
headless execution does not claim visual weather reproduction.

## API and transaction boundaries

`JetSimulation::advance_with_controller` prepares each effective input using a
cloned controller. All mutable pilot/ramp state must be isolated by that clone;
shared mutable `Rc`/`Arc` interiors or unrelated closure side effects cannot be
rolled back by this API. Successful reports contain only committed inputs.
Failures preserve physical state, interpolation endpoints, ground and per-leg
clearances, elapsed/weather clock, log and contact history, then clear the frame
remainder and latch the terminal reason. A terminal simulation does not call
control preparation again until explicit reconstruction.

`JetRecorder` starts only on a pristine simulation. It records whole contiguous
reports; a count/continuity error closes it before appending the report, retaining
its own authentic final state. `JetReplayPlayer` owns its simulation and uses at
most 240 attempts per call, including the zero-duration terminal attempt. Seeks
always reconstruct from frame zero. Checkpoints are exact full-state evidence,
not incomplete snapshots used for restoration. Terminal probes use a scratch FDM,
so even an unexpectedly successful probe cannot advance the committed replay state.

The [v4 wire and identity contract](replay-v4.md) specifies every field and limit.
Current/legacy APIs and all v1/v2/v3 bytes remain separate and unchanged. The
legacy app path cannot read v4 through its old `ReplayFile` API. The separate
[opt-in native jet path](jet-native-app.md) explicitly dispatches profile v2 and
matching v4 files. Profile loading, identity and replay validity do not establish
trimability, handling or certification. Native visuals, manual controls and audio
retain their own acceptance gates beyond the headless checks described here.
