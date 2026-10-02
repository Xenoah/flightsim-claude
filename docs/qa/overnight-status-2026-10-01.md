# Overnight integration status — 2026-10-01

Documentation snapshot frozen at **20:03 UTC**. Publication was checked at 19:55 UTC;
final local build/visual evidence below has its own completion times. Later remote
CI, merges and releases must be checked against their exact commits.

## Reading this record

This is an evidence snapshot of the October 1 repair/integration work, starting
from main `2d2295b6d61de91df3bf8e03ff3e75d3e5b37fc0` (`0.6.0-alpha.18`).
It describes the integrated source and the named checks below. A source change,
a focused passing test, a merged subsystem PR, and a published Windows release
are different milestones. **This record does not declare every issue closed,
all defects eliminated, or alpha.21 released.**

Publication snapshot, checked at **19:55 UTC**:

- [PBF PR #40](https://github.com/Xenoah/flightsim-claude/pull/40): exact head
  `bd0dcebfb04ef1878ec653a9bf4ebb804e03c96e` passed all seven CI jobs and was
  merged to `agent/alpha20-integration` at `01eb8d4f4f801a069a681a47c6618de0bace244a`
- [Geoid PR #41](https://github.com/Xenoah/flightsim-claude/pull/41): exact head
  `da9b83a9c48a0c2f815e3baea595f5cba7d090d9` passed all seven jobs and was merged
  to that integration branch at `c122ab027c0e02e1b7869999acd393ac05e7cc90`
- [Capture hotfix PR #42](https://github.com/Xenoah/flightsim-claude/pull/42): head
  `cd8c4b13b32eee33a7bd8c1d7d4b9b0d1aebb9a5` passed all seven jobs plus independent
  review. Main source `f3d32d816cc625d10e4309f3506c150280e749a5` is now the
  **published alpha.20**; its actual release evidence is recorded below
- [Attitude PR #43](https://github.com/Xenoah/flightsim-claude/pull/43): exact head
  `8d883dac0ac43c6f11f3f272af9939876e9223db` passed all seven jobs in
  [CI 36905167305](https://github.com/Xenoah/flightsim-claude/actions/runs/36905167305)
  and independent review, then merged to staging at
  `75ff9464a54718933dad3bac097da90a2ac65d16`
- [Input PR #44](https://github.com/Xenoah/flightsim-claude/pull/44): exact head
  `f59941c66709dfa994b44166c63b3c8dbf0814fe` passed all seven jobs in
  [CI 36907926320](https://github.com/Xenoah/flightsim-claude/actions/runs/36907926320)
  and independent review, then merged to staging at
  `f18723e83ca641dbc563406521825f2414cc7688`
- [Turbulence PR #45](https://github.com/Xenoah/flightsim-claude/pull/45): exact head
  `9123e63d1fd446c66cc87502fe9fbf3d659869cb` passed all seven jobs in
  [CI 36916091192](https://github.com/Xenoah/flightsim-claude/actions/runs/36916091192)
  and independent review, then merged to staging at `6d3865a`
- [Aircraft PR #46](https://github.com/Xenoah/flightsim-claude/pull/46), head
  `8e16bf7514252b695d1210cd8b56045bf302eb96`,
  [Network PR #47](https://github.com/Xenoah/flightsim-claude/pull/47), head
  `edc63b065cf22cd58e674abe13098609753bb5d6`, and
  [Runway PR #48](https://github.com/Xenoah/flightsim-claude/pull/48), head
  `33c5109593e54656708b761ad0f6d682a0f40908`, are draft PRs with CI pending.
  Remaining runtime/final integration and release gates are still in progress
- Staging retains the historical name `agent/alpha20-integration`, but the broader
  feature release is **alpha.21, not published at this snapshot**. Staging merges
  do not mean those features are on main or included in alpha.20. Verify the final
  main SHA, complete CI, both Windows aircraft smokes, release URL and actual assets
  before reporting the feature release as published

### Verified alpha.20 release

[Release v0.6.0-alpha.20](https://github.com/Xenoah/flightsim-claude/releases/tag/v0.6.0-alpha.20)
was published at **2026-10-01 19:08:32 UTC** from source
`f3d32d816cc625d10e4309f3506c150280e749a5`.
[Main CI 36907426362](https://github.com/Xenoah/flightsim-claude/actions/runs/36907426362)
and [Release 36909649587](https://github.com/Xenoah/flightsim-claude/actions/runs/36909649587)
both succeeded. The extracted Windows bundle ran under WARP, loaded the bundled
model, saved a complete PNG, and exited with status 0 and its batch-completion marker.

The actual release artifact `11187266761` was downloaded. Its source/version
metadata, executable, aircraft asset and license files were inspected; the complete
published PNG was visually inspected. The independently calculated SHA-256 of
`flightsim-claude-v0.6.0-alpha.20-windows-x86_64.zip` matches the published checksum:

```text
252a402b5eed5ba42722449dde3beff6acd5ce516aa000e04335e09e81919953
```

This establishes the capture hotfix release. It does not establish the alpha.21
feature package, two-aircraft release smoke, physical-GPU behavior, or normal
interactive Windows graphics teardown.

## Demonstrated capabilities and repairs

### Terrain data and PBF boundaries

[Data-boundary QA](data-boundaries-2026-10-01.md) records inputs, reference hashes,
commands, limitations and independent review findings.

- A narrowly patched, licensed `vendor/osmpbf` 0.3.7 validates primitive integer
  arithmetic, indices, parallel arrays, UTF-8 and enums before exposing iterators.
  Framing and zlib end-of-stream/full-input checks reject malformed input normally
- Seventeen hostile-PBF tests passed in debug and optimized release. The normal
  synthetic airport retains the pre-fork 400-byte golden FSAP and reason-specific
  skip reports. This is not a whole-file memory sandbox or exhaustive fuzzing claim
- Tile generation checks horizontal CRS, units and vertical datum at the CLI and
  library boundary. Local GeographicLib 16-bit PGM EGM2008/EGM96 grids must match
  the source datum; conversion applies `h = H + N` at original valid pixel centres
  before resampling. The runtime `.fsdem` byte format stays unchanged
- Independent checks used all 500,000 published NGA-harmonic reference points for
  each five-minute grid: EGM2008 maximum/RMS error 0.274256968/0.011791141 m;
  EGM96 0.115708552/0.004570351 m. These are bilinear-grid approximation results,
  not spherical-harmonic evaluation or a claim of cubic interpolation accuracy
- A provider-documented Copernicus N35 E139 source was explicitly declared EGM2008
  because the actual TIFF omitted its vertical GeoKey. Two bakes produced the
  same hashes for all 429 tiles; 32 low-coverage candidates were skipped
- Output uses per-file atomic replacement. Invocation provenance is INCOMPLETE
  before mutation and COMPLETE only after success. It does not certify old tiles
  outside the invocation, provide a directory transaction, or permit concurrent writers

Raw DEM, geoid grids and regional derived tiles are not bundled. Obtain the
specified source and grid separately and retain their provenance and licenses.
Old tiles are not silently normalized. Use a fresh output directory when replacing
an older unnormalized bake.

### Controllers, trim and attitude display

[Controller QA and configuration guide](input-controllers-attitude-2026-10-01.md)
contains the JSON schema examples and physical-device worksheet.

- The runtime trim path now processes `[` / `]` with and without a controller
- Multiple devices can map pitch/roll/yaw/throttle/brake/flaps to exposed axes or
  buttons; calibration, inversion, deadzone and response are persisted in versioned
  JSON. F10 opens diagnostics; F11 changes device/channel pages
- Missing/unobserved samples are not treated as centred throttle levers. Ambiguous
  device identity is refused; disconnect clears samples and releases brakes,
  while reconnection requires fresh values
- Stock Bevy gilrs discards some unknown native HOTAS channels. The explicit
  `--native-controllers` adapter preserves complete native codes; those codes are
  platform-specific. The software routing is tested, real HOTAS hardware is not
- The original rotated-child clipping bug was reproduced with the pinned Bevy
  renderer before replacement. The stationary production material has a circular
  alpha mask. Thirty-six positive/negative-bank and extreme-pitch cases had zero
  classified sky/ground pixels outside the circular aperture within the checker's
  one-pixel antialias allowance. The existing ±30° display pitch saturation remains

The [committed attitude evidence](attitude-boundaries-2026-10-01.md) includes the
actual [before](images/attitude-before.png), [positive-bank](images/attitude-positive.png)
and [negative-bank](images/attitude-negative.png) renders. These are rendered
production/reproduction cases, not mockups or physical-GPU certification.

Actual app captures also exercised round-gauge plugin integration and the native
backend's no-device fallback. A separate, clearly labelled simulated-device render
checked the complete diagnostics page after its tutorial-overlap/padding repair.
Neither is proof that a physical controller was connected.

### Two aircraft and approach starts

[Aircraft-profile QA](aircraft-profiles-2026-10-01.md) records schema boundaries,
original asset provenance and unchanged before/after scenarios.

- `light-single` and `swift-sport` select different dynamics, external model,
  camera eye, engine sound and input rates. Version-1 JSON rejects invalid
  parameters, unknown fields and unsafe model paths instead of silently falling back
- Swift Sport was made with the repository's Blender script. Its editable `.blend`
  and exported `.glb` have no downloaded external mesh, texture or logo. Blender
  studio renders are asset evidence; app GLB captures separately verify runtime loading
- The existing fixed-gear piston/propeller FDM is used for both aircraft. Light
  Single retains its historical turbine sound default, while Swift uses piston
  sound. The `--engine` option changes sound only
- Approach starts now use profile-specific force/moment-balanced trim, pitch and
  throttle, rather than reusing parked/takeoff values. Both profiles keep TAS within
  1 m/s, descent angle 3 ± 0.5°, and pitch within 1° of its initial value for every
  step of a 30-second hands-off, near-sea-level, still-air approach segment
- At 30 s, Light measured 34.912 m/s TAS, −1.787 m/s vertical speed, 2.935° descent;
  Swift measured 35.908 m/s, −1.837 m/s, 2.932°. The two unchanged 120-second
  director-driven takeoff checks also passed

These are generic game aircraft and bounded initialization tests. They do not
certify real-aircraft handling, arbitrary altitude/weather/mass, flare/touchdown,
terrain clearance, or hands-off takeoff. There is no hidden auto-trim/controller
in the 30-second approach test.

### Replay and time

The app's `replay_runtime` starts from the recording's frame-zero state. Rewind
restarts the simulation and re-executes recorded frames in batches of at most 240
per update, restoring the physics clock, fixed-step remainder and flight log as
well as position/orientation. A state-only keyframe assignment cannot do that.
The display's time-of-day follows playback time through pause and rewind.

Focused regressions exercise variable frame times, airborne frame zero, exact
same-build state equality, bounded rewind/log restoration, and visible stopping
on positional mismatch. Aircraft fingerprint mismatch and invalid replay files
fail startup. F9 atomically reserves a new sequence name so it cannot overwrite
an existing recording through a check/create race.

[Replay numeric-boundary QA](replay-file-boundaries-2026-10-01.md) records the
independently encoded corrupt files that exposed seven failures before repair.
Reader and writer now reject nonfinite/out-of-domain durations, controls,
keyframes and conditions, overflowing total durations and unsupported visual
calendar epochs. Valid format-v1 bytes, near-unit quaternion rounding and zero-dt
frames are preserved. Fifty focused replay tests passed in both debug and release;
this is numeric/file compatibility, not cross-build flight compatibility.

App guards also cover manually constructed records, nonfinite runtime state/drift,
and the resolved visual epoch when the file stores its omitted-epoch sentinel.
An additional independent review identified finite-but-unrenderable positions:
initial state, post-step state and LOD observation now require coordinates and
squared lengths that remain finite after the f32 render conversion. Actual CLI
probes using 1e30 and 1e39 position components both returned exit 2 before rendering,
with no screenshot created. The runtime retains the last good world Transform on
fault. The final alpha.21 app test run passed 95 tests; warning-denied clippy passed.
Runtime regressions check replay pause/seek/fault/completion audio, the finished
banner, F8 recovery instead of inactive restart prompts, recorded location/weather
before airport and difficulty resolution, and suppression of live tutorial/time
controls. Chase/free camera history resets on origin changes and restart/rewind;
the tower camera retains its terrain-supported world anchor and LOD observation.

Replay requires the matching build, aircraft and terrain. The file does not
establish universal compatibility across toolchains, operating systems or changed
physics. Old turbulent recordings may diverge after this session's clock and
field changes even if the aircraft fingerprint still matches; drift is detected
at recorded checkpoints, not every possible state component on every frame.

### Turbulence

[Turbulence QA](turbulence-2026-10-01.md) deliberately separates numeric evidence
from its still-empty human playtest worksheet.

- Gust time advances per executed physics step rather than once at a render-frame
  endpoint. Fixed controls reproduce bit-identical states across the tested frame
  groupings on the same build
- Reported TAS now uses the same gust-relative velocity as the aerodynamic path
- ECEF sampling and vector basis remove the old date-line/pole seam. The same seed
  now describes a different atmosphere; measured statistics change even though
  preset intensities and correlation scales were not retuned
- All 24 cruise/approach-configuration scenarios and three additional ten-minute
  severe cruise runs passed the unchanged numerical envelopes, without saturation

These are automated Light Single flights. Preset names are not calibrated weather
categories, and numerical control reserve is not a measure of human workload.

### Traffic and local sessions

[ADR-0010](../adr/0010-local-traffic-sessions.md) defines the boundary:
`flightsim-net` is pure Rust and feeds observations to app. It never overwrites
own-aircraft physics.

`TrafficSource`, deterministic synthetic circuits, bounded interpolation, identity,
range/bearing/heading/relative-height display, stale freezing and expiry are
implemented. The version-1 UDP host/client supports create/join/leave, capacity
limits, packet validation, reordered sequences, timeout/reconnect and fresh join
identity. Actual loopback-socket tests exercise these paths. Traffic renders a
generic proxy; remote aircraft profile/model transfer is not part of this protocol.
The final alpha.21 binary's Swift/synthetic-traffic capture (local capture; full-size PNG not published in this snapshot)
was inspected: it shows the original Swift model and three labelled synthetic
contacts with bearing, range, relative height and heading on the synthetic airfield.
This capture is not real-DEM or live ADS-B evidence.

[Two-native-instance QA](high-altitude-lan-alpha21-2026-10-01.md) additionally verifies
the final candidate at the application boundary on loopback. The host showed two
participants and an orange remote-aircraft proxy; the client connected, detected a
deliberate eight-second host outage, and automatically joined a new session after
restart. F12 subsequently produced `LAN LEFT`; it was pressed after the host had
closed, so this does not prove receipt of a Leave by a live host. All three host/
client process executions returned 0. The saved host/client images and timestamped
logs are linked in that record; the full-size native PNGs are retained locally. The short visible STALE interval was not captured.

The initial reduced-size PNG review suspected missing text fragments. Independent
original-resolution inspection and fresh serial headless, native and resized-native
captures showed complete help text. White needles cross the tiny PWR/V/S/TAS
labels and readouts; this explains the apparent gaps and also occurs live. No
separate missing-glyph defect was established, and no runtime patch was made.
This remains a bounded readability observation, not a clean-all-pixels claim.

The default host binds `127.0.0.1:41520`. Explicit LAN binding is for trusted peers
only. Nonces are session identities, **not authentication credentials**. There is
no encryption, public matchmaking, NAT traversal, hostile-Internet security,
authoritative collision system or real ADS-B provider. Physical multi-machine
LAN/WAN behavior has not been demonstrated by a loopback test.

### Actual-scene captures and remaining visual gaps

Four initial actual offscreen captures covered diagnostics, Swift/synthetic traffic,
real-DEM night and a roughly 3 km altitude drop. PNG creation/exit succeeded, but
visual inspection found defects: the cockpit camera used an outdated aircraft
Transform during descent, and flat-threshold runway/light geometry could be
buried by the real DEM. They are discovery evidence, not four visual passes.

The camera now follows the current render Transform. Pavement/paint share a
bounded, at-most-10 m terrain-draped grid; lamps sample local corner elevations
and have vertical emitting faces. [Runway QA](runway-terrain-drape-2026-10-01.md)
records geometry limits and tests. Fixed matched night and altitude captures were
inspected; the night default-start image shows the runway lights and round gauges,
and the altitude image retains the cockpit attached during descent.

The original altitude probe is a falling aircraft at about 9,393 ft and −65.6°
pitch. It does **not** show a level-flight high-altitude horizon or establish moving
LOD transition/depth acceptance. That capture also exposed a wrapped V/S numeric
label. The current source uses compact signed k-units with a `V/S fpm` label and
an `OVR` guard. The named regression
`vertical_speed_readouts_fit_the_dial_in_extreme_descent` covers sign, width and
nonfinite/over-range values; the updated UI suite passes 124 tests. The final
alpha.21 binary's actual extreme-descent capture (local capture; full-size PNG not published in this snapshot)
was then inspected: the compact negative k-unit readout fits inside the dial.
This closes the observed label-wrap recheck without turning a descent into
level-flight evidence.

A night still cannot establish the absence of every terrain seam. DEM sampling
and the visible LOD mesh use different triangulations, so some terrain can still
cover a draped runway between samples. Do not increase all lifts merely to hide
that mismatch.

A separate [real-DEM cruise fixture generator](../../crates/flightsim-sim/examples/record_visual_flight.rs)
now provides a suitable flight for the missing visual check. After a 120-second
settling period it starts a fresh simulation and records 360 seconds / 21,600
frames over the same 429 normalized tiles. It travels 17,533.177 m westward, with
ellipsoidal altitude 3017.088931–3017.092641 m and pitch 3.014843–3.015513°.
There are four aircraft-anchor 4 km rebases (81.917, 163.833, 245.750 and 327.667 s),
zero missing-terrain physics frames, no crash/divergence, and exact serialized
round-trip, keyframes, and final state/elapsed/log/ground/interpolation replay.
The director target was 3035 m; the measured settled range above is the result,
not a claim of perfect target-altitude hold. Repeated generation preserves the
fixture and terrain identity.

The fixture above is numeric evidence. Separately, the **native app completed the
full 360-second playback on the final alpha.21 candidate**, with actual
render-origin rebases logged at 84.100, 167.867, 251.650 and 335.317 seconds.
These camera/render events differ from the fixture's aircraft-anchor rebases. The log records four live terrain tiles during
the cruise, completion capture, and batch exit status 0. Native controls also
paused at displayed 0:29, rewound to 0:19 with F8, resumed and changed playback
speed through x4/x8 before completing without mismatch. The earlier uninterrupted
integration run is superseded by this final-binary repetition.

The actual completion PNG (local capture; full-size PNG not published in this snapshot)
was inspected: it shows the aircraft in chase view, rendered terrain and a level
horizon, `REPLAY COMPLETE 6:00 / 6:00`, 9,899 ft ellipsoidal altitude, about 3°
pitch, zero rounded vertical speed and 9.5 NM travelled. The companion
real-DEM night cockpit PNG (local capture; full-size PNG not published in this snapshot) shows
visible runway-light rows and readable illuminated round gauges at the ground
start. A separate 1.5 NM night approach showed only a few tiny light pixels at
about 2.6 km; useful approach-light visibility has not been established.
[High-altitude QA](high-altitude-2026-10-01.md) contains the source, hashes,
CSV, replay metadata and actual app log. Cruise, night, drop, Swift/synthetic
traffic and the two-native-instance lifecycle were all repeated with executable
SHA-256 `be80a129cf2fbfe4d488673b7be3561d27c1a8ec5bf7aee2a81bbdec694923b6`.
The final cruise capture completed at 20:00:22 UTC and was inspected before this
20:03 UTC documentation freeze. Its PNG SHA-256 is
`1945c5f302fe2b88bd4490f5ed2f6d28494de46f0981295a52e16d817e1d2e1f`.
These are final local-candidate artifacts, not alpha.21 Windows release assets.

These are bounded sampled observations and a completed native playback, **not an
all-LOD-transition or hardware-GPU smoothness certification**. Four live tiles and
zero missing physics samples do not establish the absence of every transient mesh
gap, depth artifact, or near-ground runway occlusion. Wider #6 acceptance remains
open, with the original falling-only gap now replaced by actual stable-cruise proof.

The offscreen path renders the real scene and UI to a 1280×720 image target without
a display server. Native Linux windowed testing separately exercised actual app
startup, Swift model, camera changes, pause/reset, diagnostics and trim change/reset.
Neither path had a physical controller, GPU or audio output device.

## Issue acceptance and remaining work

All ten issues below were open when their current bodies were read on October 1.
The software-only rows are candidates for closure **after** the relevant final
commit is integrated, exact-head CI passes, and release evidence is verified.
This table does not change GitHub state.

| Issue | Implemented/evidenced acceptance | Remaining condition / disposition |
|---|---|---|
| [#2](https://github.com/Xenoah/flightsim-claude/issues/2) Physical gamepad | Device/channel diagnostics; saved calibration/deadzone; synthetic disconnect/reconnect recovery | Record at least one actual device model and physical results. Keep open |
| [#5](https://github.com/Xenoah/flightsim-claude/issues/5) Turbulence handling | Deterministic light/moderate/severe scenarios, attitude/load/control-margin budgets; numeric and subjective reports separated | A person must fly the reference aircraft and record handling impressions. Keep open |
| [#6](https://github.com/Xenoah/flightsim-claude/issues/6) Real-DEM visual validation | Sourced/licensed bake, camera/runway repairs; 360 s / 17.533 km exact replay and native completion, four actual render rebases, inspected horizon/night PNGs | Broader unsampled LOD/depth transitions, night near-terrain continuity and residual mesh occlusion. Keep open; the stable-cruise evidence is now present |
| [#9](https://github.com/Xenoah/flightsim-claude/issues/9) HOTAS/remapping | Multiple devices, six control actions, exposed arbitrary axes/buttons, inversion/deadzone/response, save/reload | Software criteria have evidence; close only with release integration and explicitly retain hardware limits. Its hardware-blocked label is not evidence of a test |
| [#10](https://github.com/Xenoah/flightsim-claude/issues/10) Multiple aircraft | Versioned data-driven profiles; two distinct types; FDM/display/input switch; invalid selections/load failures tested | Final integrated assets/CLI/reset and release checks. Candidate after release; real-aircraft fidelity is not claimed |
| [#13](https://github.com/Xenoah/flightsim-claude/issues/13) Surrounding traffic | Source abstraction, deterministic synthetic traffic, position/orientation/identification, missing/stale handling; final-binary Swift/traffic image inspected | Final main/release check. Candidate for the issue's synthetic scope; live ADS-B stays future |
| [#14](https://github.com/Xenoah/flightsim-claude/issues/14) Sync foundation | Local create/join/leave, interpolation, loss/delay/reconnect, version contract; final two-native-process join/host-outage/rejoin/F12 and exit-0 evidence | Final main/release check. Keep the client capture-text anomaly, uncaptured STALE interval, public Internet and real multi-machine validation separate |
| [#22](https://github.com/Xenoah/flightsim-claude/issues/22) Height normalization | Strict datum/metadata, explicit offline geoid path, unchanged format contract/provenance, independent reference points, real bake/readback | #41 is on integration, not yet main/release at snapshot. Final local ground/render consistency and release evidence required |
| [#23](https://github.com/Xenoah/flightsim-claude/issues/23) Malformed PBF boundary | Chosen licensed safe fork, hostile arithmetic/string-table fixtures returning errors, baseline deterministic bytes/skip report | #40 is on integration, not yet main/release at snapshot. Candidate after integration; no OOM/exhaustive-input guarantee |
| [#33](https://github.com/Xenoah/flightsim-claude/issues/33) Attitude clipping | Required small Bevy reproduction before repair; circular production material; measured/rendered extreme-angle matrices | PR #43 exact-head CI/review and staging merge passed; final main/release gate remains. Retain pitch saturation/GPU limits |

## Reproduce the application workflows

Run from the repository root with Rust 1.93.0 and the workspace's pinned Bevy 0.18.1.
Use the [data QA](data-boundaries-2026-10-01.md) instructions first for real terrain.
The example output paths below are local filenames, not hosted deliverable links.

```sh
cargo run -p flightsim-app -- --help
cargo run -p flightsim-app -- --list-aircraft
cargo run -p flightsim-app -- --aircraft swift-sport --view chase --traffic synthetic
cargo run -p flightsim-app -- --aircraft assets/aircraft/swift_sport.json --approach 1.5

cargo run -p flightsim-app -- --write-input-config controls.json
# Edit controls.json using raw values from F10/F11, then relaunch:
cargo run -p flightsim-app -- --input-config controls.json --input-diagnostics
cargo run -p flightsim-app -- --native-controllers --input-config controls.json --input-diagnostics

# Separate processes on one computer; do not expose the host to the Internet:
cargo run -p flightsim-app -- --host
cargo run -p flightsim-app -- --join 127.0.0.1:41520 --callsign PILOT2

cargo run -p flightsim-app -- --headless-screenshot swift.png --aircraft swift-sport --view chase
cargo run -p flightsim-app -- --screenshot window.png --exit-after-screenshot
cargo run -p flightsim-app -- --tiles copernicus-haneda-tiles --time 23:00 \
  --headless-screenshot real-night.png --screenshot-delay 3
# This is an altitude/camera stress probe, not a level-flight horizon test:
cargo run -p flightsim-app -- --tiles copernicus-haneda-tiles --start 35.55,139.78 \
  --drop 3000 --time 12:00 --headless-screenshot altitude-drop.png --screenshot-delay 3

# A separate settled flight for high-altitude/moving-LOD inspection (not an image):
cargo run -p flightsim-sim --example record_visual_flight -- \
  --tiles copernicus-haneda-tiles --output cruise-360.fsreplay --duration 360
cargo run -p flightsim-app -- --tiles copernicus-haneda-tiles --aircraft light-single \
  --replay cruise-360.fsreplay --time 09:00

# F9 in a live flight saves a new name; select its aircraft again for replay:
cargo run -p flightsim-app -- --aircraft swift-sport --replay flight-001.fsreplay
```

F5 toggles playback pause; F6/F7 change speed; F8 rewinds ten seconds.
F10/F11 show/page controller diagnostics. F12 leaves a client session; close the
host process to stop hosting. Input configuration is created only at a new path.
Headless capture exits after writing; ordinary `--screenshot` keeps running unless
`--exit-after-screenshot` is present.

## Verification ledger and release history

The subsystem rows below are **separate overlapping runs** and must not be added
together. The final local aggregate row is a distinct grouped execution: its core
and render groups account for 1,404 tests, plus five doctests, without adding older
focused runs again. Later focused runs supersede earlier numbers in their scope.
Final exact-remote-commit CI remains a separate gate for the assembled changes.

| Scope | Most recent recorded evidence at this snapshot |
|---|---|
| Tilegen | 155 all-target tests; strict clippy/rustdoc, formatting and diff checks; release hostile-PBF subset 17 |
| World | 184 all-target tests plus benchmark smoke and one doctest; benchmark smoke is not a performance measurement |
| FDM + sim turbulence pass | 339 tests (137 FDM + 202 sim), three doctests, clippy/rustdoc; includes 19 new turbulence tests |
| Input | 71 tests including native-code serialization/ECS routing and same-frame reset; integrated Cargo run also records 71 |
| UI | 124 latest tests, including compact extreme-V/S and help-toggle regressions; actual render examples and circular pixel checks. Final-binary extreme-descent PNG confirms the compact V/S label fits |
| Render/runway | 172 unit + 23 integration tests and all-target clippy |
| App | 95 latest alpha.21 tests passed, including finite render-coordinate boundaries; focused all-target clippy passed and two actual malformed-position CLI inputs returned exit 2 |
| Replay numeric/file boundary | 50 focused tests in both debug and release; complete sim all-target run 226 tests and 2 doctests. Later app tests verify runtime defenses separately |
| Real-DEM cruise | Two exact 360 s generations, five generator tests/clippy; 17.533 km and four numeric anchor rebases. Native playback completed, four actual render rebases, inspected completion/night PNGs; broader transition coverage remains open |
| Network | Protocol/traffic and actual loopback tests passed; final native host/client additionally demonstrated visible proxy/connection, host outage/new-session rejoin, F12 and exit 0. Client PNG text-fragment anomaly retained |
| Latest local aggregate | Completed at 19:56:20 UTC: core group 833 tests / 55 binaries, render group 571 / 13 binaries, five doctests / six binaries; fmt, warning-denied workspace clippy, strict rustdoc, architecture, benchmark compilation (`--no-run`) and app build passed. No performance result is inferred |
| Physical hardware | No controller, hardware GPU, speaker listening, or real multi-machine LAN pass |

The actual alpha.19 Windows release attempt
[run 36898555726](https://github.com/Xenoah/flightsim-claude/actions/runs/36898555726)
built source `5782496024b872e5b54eeb45c0c021fe41203201`. It saved a complete,
visually inspected PNG at about 104 seconds but did not terminate by 180 seconds,
so the smoke **failed**. A graphics-teardown stall is an inference from the logs,
not a captured stack-trace diagnosis. An earlier failed run checked out older
alpha.18 source despite showing newer workflow metadata; do not conflate them.

The separate hotfix explicitly flushes/syncs/closes the PNG, flushes stdout/stderr,
and returns batch status 0/1 without waiting for graphics destructors. Subprocess
regressions verify status/output and that interactive mode does not terminate.
Native Linux windowed capture exited 0. The later exact-source alpha.20 Windows
smoke and public archive also passed, as recorded in the verified release section.
This supersedes the earlier pending-hotfix status without changing the alpha.19
failure record. Normal interactive Windows teardown remains a separate check.

Release verification must retain immutable tags, identify the exact source SHA,
require successful process exit plus a complete PNG/model-fit/completion marker,
inspect the emitted image, verify packaged assets/checksum, and confirm public
release attachments. A version bump or a green subsystem PR alone is insufficient.
The alpha.21 workflow additionally requires both profile JSON files and GLBs plus
the editable Swift Sport Blender source, and separately captures Light Single
cockpit and Swift Sport chase/synthetic traffic from the extracted ZIP. Those
final feature-release runs and their two public PNGs are not yet verified here.
