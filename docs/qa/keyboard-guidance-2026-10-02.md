# Keyboard flight guidance QA — 2026-10-02

Scope: `flightsim-ui`, keyboard flight guide and links from README/global-map.
No physics, input mapping, app integration, world-map layout or assets changed
in this component. Parent integration owns the corresponding HUD publication and
native takeoff acceptance.

## Changes

- Primary HUD speed, cockpit dial needle/readout and rotation decision all use
  app-supplied EAS. TAS remains distinct in `HudState.airspeed`; EAS is not labeled
  as calibrated IAS.
- Rotation enters at 75 kt EAS and leaves below 72 kt EAS. The earlier 65/62 kt
  EAS suggestion is superseded: its supporting release tests began airborne and
  did not establish a successful ground rotation.
- Gentle S/Down input is followed by release at the first visible nose rise,
  then observation and small corrections. The detailed guide scopes the approximate
  3-degree hint to level-runway practice. No automatic absolute-pitch decision is
  made: an uphill runway may already have positive pitch before rotation. No fixed
  press duration or continued pull until liftoff is prescribed. Initial-climb advice
  still targets 70–80 kt EAS; 3 degrees is not a sustained climb attitude.
- Conditional trim guidance replaces unconditional nose-up trim. Help explicitly
  maps S/Down to pull, W/Up to push, and says throttle/trim remain set.
- Early sink gets CHECK CLIMB with full-power confirmation and neutral small S/W
  corrections. Only low EAS gets BUILD AIRSPEED / ease-pull advice. A fast descent
  does not receive an unconditional push-forward instruction.
- Explicit approach starts/restarts have `for_approach()` seeds, avoiding inference
  of pilot intent from altitude or vertical speed alone.
- Shared AoA warning overrides unfinished tutorial lessons and appears in the
  main HUD even with the guide hidden. Unsupported warning data gets STALL WARN
  N/A; this takes precedence over stale warning state. Lesson completion still
  suppresses tutorial prompts; the main HUD remains the warning display.

## Integration contract

App must populate three added fields each update: `equivalent_airspeed`,
`stall_warning`, `stall_warning_unavailable`. The boolean warning and availability
must come from the same status used by sound. This UI component does not calculate
an aerodynamic warning threshold or change the controls. App must select
`TutorialState::for_approach()` for explicit approach startup/restart; ordinary
runway/fly/map initialization retains the default lesson seed.

## Revised ground-rotation cue

The integration owner selected 75 kt EAS with the slower 0.25/s keyboard/rate-mode
pitch ramp and 5/s centering after ground-start numerical reproduction and
independent reaction-delay review. These input settings are integrated separately,
not changed by this UI component. The reported 12-case check covered both bundled
profiles, release feedback at 3/5 degrees and 0–0.3 s reaction latency: no ground
return or crash in those cases, with peak AoA 12.127 degrees below the modeled
13.12-degree lift peak. This is bounded numerical evidence, not native acceptance
or a certified operating envelope. Final native validation remains with the lead.

The detailed guide no longer calls an airborne 65 kt release result evidence of
runway rotation. Additional UI regression covers 65 kt remaining ACCELERATING,
75 kt entering ROTATE, the same first-rise guidance across different initial
pitch attitudes (without an automatic absolute-pitch decision), and liftoff at
75 kt moving to normal climb without an immediate BUILD AIRSPEED/push-down instruction.

Revised-source verification in the integrated tree: **177 UI library tests**
passed, including **34 tutorial tests**. The full native package group was used
for compilation/feature resolution with the tutorial filter; the resulting UI
test executable also passed its complete suite. Native package-group all-target
clippy passed with `-D warnings`. Rustfmt and whitespace checks passed. No GUI
was launched by this component; native takeoff and final window fit remain
separate integration gates.

## Earlier component verification (before this cue revision)

The counts below apply to original component commit `6ece2c1`. Revised-source
verification is recorded above; the earlier app initializer failure is historical.

Shared native-feature package selection: render, input, ui, audio, app. Flags:
`RUSTFLAGS='-D warnings'`, `RUSTDOCFLAGS='-D warnings'`, dev/test debug 0,
incremental 0, jobs 2. Library-only target selection preserves that package
feature resolution while skipping app's binary initializer pending integration.

- All **176 UI library tests** pass, including **33 tutorial tests**
- Full native library package group passed
- Native package-group `clippy --lib -- -D warnings` passed
- `cargo fmt --all -- --check`, architecture and whitespace checks passed
- Attempting `clippy --lib --tests` in this isolated component tree reached the
  expected app compile error: its old HudState initializer lacks the three new
  fields. This is an explicit integration requirement, not a full-target pass
- Independent source review approved the final guidance logic after the fast-sink
  correction and its regression test

Focused tests cover density-separated TAS/EAS at 4 km, both bundled profile trim
fixtures, repeated rotation hysteresis, low- and high-speed early sinks, recovery
and level cruise, explicit approach seeds, warning/N/A priority, and repeated
hide/pause/crash/restart visibility. ASCII, two-line prompts and bounded help-line
lengths are checked. These tests do not qualify a new display size or prove a
complete native takeoff. No GUI was launched for this component; final native
window fit, actual input takeoff/release behavior and app startup/restart wiring
remain integration gates.

See [keyboard flight guide](../keyboard-flight.md) for the user-facing explanation
and [longitudinal physics QA](longitudinal-physics-2026-10-02.md) for the separate
numerical evidence behind the candidate training speeds.
