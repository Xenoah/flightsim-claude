# Opt-in native dry-jet sessions

The native app can load an external exact-decimal aircraft profile v2 and replay
its matching v4 recording. The original fictional Kestrel Jet Trainer provides
an authored exterior and cockpit; the numerical headless fixture still supplies
no exterior. These are experimental model integration paths, not a real-aircraft
performance, handling, certification, damage, or landing-grade claim.

```sh
cargo run -j2 -p flightsim-app -- \
  --aircraft assets/aircraft/kestrel_jet_trainer.json --view chase

# Explicit flat-zero ground, standard atmosphere, calm control practice.
cargo run -j2 -p flightsim-app -- \
  --aircraft assets/aircraft/kestrel_jet_trainer.json \
  --global-terrain off --climate off --wind 0/0 --turbulence calm

# Replay requires the same exact physical profile and compatible terrain.
cargo run -j2 -p flightsim-app -- \
  --aircraft assets/aircraft/kestrel_jet_trainer.json \
  --global-terrain off --replay flight-001.fsreplay
```

Build prerequisites are in [HANDOFF](HANDOFF.md). Native development checks and
their limits are recorded in [Kestrel flight QA](qa/kestrel-native-flight-2026-10-04.md).

## Startup, controls and contact

The ordinary built-in selection, profile-v1 decoder, legacy simulation, v1-v3
replay identity gates and commercial candidate defaults retain their old paths.
The external file version is probed without decoding its physical numbers; each
original byte sequence then goes to its original bounded decoder (128 KiB for
v1, 1 MiB exact-decimal for v2). Display names and sound choices never select FDM.

A parked jet begins with its engine at authored idle and its parking brake ON.
B queues a parking toggle; Space remains the momentary service brake. The
physics and recorder receive `max(service, parking)`. Toggles and rate-based
pilot controls commit only when a complete fixed step succeeds. A no-step frame
or rejected step retains the pending toggle. Pause, map capture and focus loss
release transient input without releasing the parking latch. R reconstructs the
start and resets the recorder: parked starts restore the brake; airborne starts
restore the configured initial levers with the parking brake off.

Zero throttle means running idle, never a modeled engine shutdown. There is no
hidden position lock, takeoff script, stabilization driver or engine spool state.
The regularized contact/friction law can produce finite idle creep. A GPU-free
app regression measures ten seconds of Kestrel idle with parking applied and
requires displacement below one metre while also confirming that it is nonzero.
The measured three-dimensional displacement was 0.044505229 m (4.45 cm), including
gear settling, at the tested flat-zero/calm/ISA start. This is not a guarantee for
other slopes, weather or profiles; the test exposes the value with `--nocapture`.

The persistent banner shows idle/dry thrust, parking state (including a pending
toggle), effective brake and the authored Mach interval. Playback shows the last
recorded effective brake; v4 does not store a separate parking-switch history.
Jet help does not use the legacy propeller rotation/climb cues. The jet uses its
authored GLB cockpit; the generic high-wing propeller interior is not added.
Turbine sound for a dry jet has afterburner disabled, including an explicit
`--engine turbine` sound override. Audio smoothing remains presentation only.
Stall warning evaluates the current Mach-sampled lift peak and current flaps;
an unsupported peak is shown unavailable rather than guessed. A paused, completed
or seeking replay retains the actual visual warning while sound stays muted.

## Terrain and atomic new flights

Supported physical surfaces are bundled global terrain and explicit flat ground
at ellipsoid elevation zero. Raw `--tiles` and selected, active or pending regional
packages are rejected before replacing a flight. Selections are not silently
cleared. Package-backed replay is unsupported.

Map Start prepares the entire candidate startup, weather, controls, physical
session, recorder, clock and camera anchor before committing any of them.
Unsupported initial pressure, temperature or Mach keeps the previous flight and
pending request intact with a visible error. Jet map and airborne starts use the
profile's explicit approach speed/pitch/control hints instead of the legacy
90 kt / 2 degree / 65 percent defaults. Those hints are initial conditions, not
proof of trim at arbitrary wind, atmosphere or terrain height. Weather choices
remain pending until a successful Start; Cancel preserves the current weather.

A physical operating-domain rejection freezes at the last committed state and
shows the reason/cursor. The app retains valid render coordinates and uses v4's
1e-9 quaternion tolerance; legacy presentation keeps 1e-12. No legacy crash
threshold or five-grade landing report is assigned to jets.

## Recording and replay

F9 exports a non-consuming snapshot, including the authentic last checkpoint.
Repeated saves continue the same live recording. Frame-zero and terminal-at-zero
snapshots remain representable. Manual cloud overrides retain the existing
visible recording/F9 prohibition; authored weather is recorded. A recording-cap or
continuity error freezes the authentic saved prefix and displays a recording-only
notice while live flight continues. Changing live sun speed with comma/period also
stops the jet recording before another physical step: v4 has one initial sun rate.
F9 saves that valid prefix; R explicitly starts a new recording at the current rate.

A live jet session owns its simulation and recorder. A replay session owns only
its `JetReplayPlayer`; presentation, elapsed time, weather and effective controls
all read that player's authoritative simulation. F5 pauses, F6/F7 change playback
speed, and F8 queues a ten-second rewind. Reconstruction runs from frame zero,
at most 240 attempts in one app update, including a terminal check. Repeated F8
uses the pending/current seek target. Sun time follows recorded physical elapsed
while paused, seeking, completed or terminal. Non-playing replay states mute audio.

The app tests cover legacy golden replay behavior, exact profile routing,
30/60/144 Hz committed jet controls, rejected-step rollback, no-step parking,
repeat exports, terminal-at-zero, pause/focus release, repeated restart, bounded
F8/speed/pause, model-aware weather and atomic source/domain rejection.
Actual native manual takeoff, warning/recovery response, approach/contact,
camera selection, recording and lifecycle checks passed within the bounded
[development QA](qa/kestrel-native-flight-2026-10-04.md). Speaker listening,
Windows full-scene capture and full-envelope handling remain unverified.
