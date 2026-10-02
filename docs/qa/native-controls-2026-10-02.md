# Native keyboard flight verification — 2026-10-02

## Result and tested build

**Pass for the tested conditions.** Light Single and Swift Sport each completed two actual keyboard-controlled runway takeoffs, continued flying after pitch-key release, and remained airborne. Their first flights also demonstrated a deliberate stall, recovery with genuine altitude loss, and settled flight after manual trim and power adjustment. No automatic trim, attitude hold, altitude hold, or aerodynamic stabilization was added.

- Production source: `f7a03445517a9b74c907a245e84189448c753ad0`
- Tested Linux application SHA-256: `15c0b4e61b523f6e9231b03d7397092af921864317f260e9aaef76d9f113b97c`
- Subsequent source `730b359` changes documentation only
- Native conditions: calm wind, ISA atmosphere, clean configuration, nominal bundled mass and trim, level sea-level fallback terrain; Linux with software Vulkan
- Light used the procedural fallback model; Swift used its original bundled model

The reviewed input mapping applies the sampled pilot command at each 120 Hz physics step. The bundled pitch ramp is 0.25/s, with 5/s spring return; full authority remains available. Releasing the key returns the transient pitch input to zero while preserving trim and throttle. The nose is free to respond physically. See [fixed-step controls and replay QA](fixed-step-controls-replay-2026-10-02.md), [longitudinal physics QA](longitudinal-physics-2026-10-02.md), and [ground-rotation QA](ground-rotation-input-2026-10-02.md).

## Actual takeoff and release

Each second flight followed an in-app restart. The operator accelerated, applied one rotation input, and then left pitch, roll and yaw untouched during the recorded climb. All four pulls began on the ground and caused prompt liftoff; none was mistaken for a later natural liftoff under default trim.

| Aircraft / flight | Pull-start EAS | Sampled pull | Pitch at release | Release to liftoff | Liftoff EAS | Maximum post-cue AoA |
|---|---:|---:|---:|---:|---:|---:|
| Light 1 | 75.22 kt | 1.267 s | 2.81° | 0.633 s | 80.53 kt | 5.16° |
| Light 2 | 76.01 kt | 1.525 s | 9.85° | 0.167 s | 80.43 kt | 10.02° |
| Swift 1 | 75.19 kt | 1.233 s | 4.15° | 0.392 s | 83.54 kt | 6.22° |
| Swift 2 | 77.19 kt | 1.100 s | 3.20° | 0.467 s | 85.10 kt | 5.45° |

All four maxima were below the modeled clean lift peaks, 13.12° for Light and 13.15° for Swift, and below the configured warning-on thresholds. There was no crash, numerical divergence, or return to the ground.

Requested key durations differed from the actual sampled durations. These numbers describe the tests; they are **not recommended tap durations**. The guide correctly says to begin gently near 75 kt EAS and ease off at the first visible nose rise. A pitch near 3° is only a level-runway reference, not a target for every runway.

Light's first flight continued for 113 seconds after liftoff before the pre-stall checkpoint, with unchanged full power and +0.09 trim. Swift continued for 185 seconds at full power and +0.08 trim. Swift exhibited a climb phugoid: brief altitude dips reached 0.77 m in flight 1 and 2.72 m in flight 2, with continued climb afterward. Thus successful release does not imply constant pitch or monotonically increasing altitude.

## Stall, recovery and manual cruise

Approximately five-second deliberate pulls retained enough authority to produce deep stalls. Warning activation and clearing were visually observed in the native application. The following losses are measured from the recovery crest to its subsequent trough, not from initial altitude.

| Aircraft | Maximum AoA | Recovery crest → trough | Altitude lost |
|---|---:|---:|---:|
| Light | 43.57° | 1036.63 → 894.90 m | 141.72 m |
| Swift | 41.84° | 1784.29 → 1671.01 m | 113.28 m |

Both recovered and climbed again in these sufficiently high tests. These results do not promise recovery from a low-level stall. Recorded surfaces show fast return toward retained trim when reversing input, followed by the slower opposite-command ramp. Saturation of the final surface means exact raw key transitions cannot always be recovered from surface records alone.

After manual trim and power changes, controls were left unchanged. The earlier oscillations decayed:

| Aircraft | Retained trim / power | Unchanged-control interval | Final EAS | Final vertical speed | Last 60 seconds |
|---|---|---:|---:|---:|---|
| Light | +0.043 / 51.04% | 220 s | 85.20 kt | −9.9 ft/min | EAS 84.81–85.41 kt; height range 1.53 m |
| Swift | +0.047 / 29.58% | 436 s | 83.95 kt | −17.7 ft/min | EAS 83.91–83.97 kt; gradual 5.17 m descent |

These are settled manual-control conditions, including a small residual descent, **not altitude hold**.

## Independent reconstruction and automated gates

All twelve saved snapshots of the four flights passed independent format, aircraft-identity and exact `1/120 s` step checks. Every stored keyframe matched bit-for-bit when reconstructed through the public simulation/replay APIs. AoA maxima and recovery minima above use all 120 Hz states, not just one-second keyframes. Snapshots within a flight overlap and are not twelve independent flights.

All 17 final verification gates passed, including 1,683 Rust tests across the two main groups:

| Gate group | Result |
|---|---:|
| Headless Rust tests | 938 passed |
| Native-feature Rust tests | 745 passed; 2 existing ignores |
| Commercial-feature tests | 143 passed |
| Documentation tests | 5 passed |
| Python checks | 135 + 5 passed |

Strict clippy, formatting, architecture, documentation, default/commercial builds and Windows GNU cross-checks also passed. Automated tests cover shared-command app trajectories at 6/30/60/144 Hz, plus fixed-step replay and grouped/jittered frame durations. Same-update HUD and software audio-bridge warning propagation is tested; **physical audio output was not verified**.

The separately supplied native screenshots show actual airborne flight at 119 seconds for Light and 102 seconds for Swift. Both were inspected at 1180 × 812; the updated 75 kt EAS / first-nose-rise help fits the captured viewport. The screenshot timer names should not be read as time since liftoff.

## Limits

This is not aircraft certification or a guarantee for arbitrary terrain, wind, density altitude, loading, flap state or delayed input. Ground numerical tests retain explicit hot/high, increased-mass and late-release failure boundaries. EAS is displayed as equivalent airspeed, not calibrated IAS. Finite engine power and density effects remain; no arbitrary altitude ceiling or altitude floor was introduced.

Untested here: physical controllers and their hardware drivers, hardware audio, Windows MSVC/native Windows execution, a controlled native 30/60/144 FPS sweep, other viewport sizes, real-aircraft handling fidelity, and recovery guarantees outside the documented post-stall model and available height. Passing compile checks or numerical fixtures does not substitute for those tests.
