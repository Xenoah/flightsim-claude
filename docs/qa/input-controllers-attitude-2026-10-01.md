# Input controllers and attitude indicator QA (2026-10-01)

## Scope and honest status

- Baseline: `2d2295b`, `0.6.0-alpha.18`, Bevy pinned at 0.18.1
- An input-path defect prevented `[` / `]` trim keys from changing trim. The
  gamepad-combining function was the runtime entry point even without a gamepad,
  but never updated trim. Regression coverage now includes both cases
- Controller mappings, calibration, and diagnostics are tested with synthetic
  inputs. **No physical controller is available in this environment.** This
  does not satisfy Issue #2's named-device hardware acceptance criterion
- Bevy 0.18.1's stock gilrs converter discards unknown native axes and buttons.
  `Other(u8)` support in Bevy's public input types does not itself make these
  channels reachable from real HOTAS devices. The opt-in native adapter preserves
  gilrs native event codes instead; its software path is tested, its physical
  HOTAS behavior is not
- The clipping reproduction is deliberately retained independently from the
  production repair. It was rendered and measured before choosing the repair

## Controller configuration workflow

Create a new, human-editable JSON configuration (never overwrites an existing
file):

```sh
flightsim-app --write-input-config controls.json
flightsim-app --input-config controls.json --input-diagnostics
# For unmapped HOTAS axes/buttons, explicitly opt into the native adapter:
flightsim-app --native-controllers --input-config controls.json --input-diagnostics
```

F10 toggles the diagnostic panel. F11 advances through all connected/disconnected
device and channel pages. Keyboard flight controls remain usable. Pause before
testing axis direction and lever endpoints. `--` means no raw sample has been
received; move the relevant control. Unobserved levers are not assumed to be
centered, because doing so could command 50% throttle on connection.

The configuration has a required `version: 1`, and `pitch`, `roll`, `yaw`,
`throttle`, `brake`, and `flaps` mappings. Set a mapping to `null` for keyboard-only
control of that action. `view_cycle` is an optional button shortcut; set it to
`null` to free the default North/Y/Triangle button for flight controls. Keyboard
C always switches views.

Each mapping contains:

- `source`: an arbitrary exposed axis/analog button, or positive/negative buttons
- `mode`: `absolute` for a stick/lever position; `rate` for moving and holding a
  throttle/flap setting. Brake requires `absolute`, so release cannot latch it
- `curve`: `deadzone` in `[0,1)`, sensitivity exponent `response` in `[0.1,10]`,
  and `invert`. `response: 1` is linear; 2 softens the center

For `source.kind: "axis"`, `input` is `{"kind":"axis","id":"left_stick_x"}`
or `{"kind":"button","id":"right_trigger2"}`. Standard axes are
`left_stick_x`, `left_stick_y`, `left_z`, `right_stick_x`, `right_stick_y`, and
`right_z`. The internal `{"other":17}` ID form is usable only when the backend
actually emits it; the stock backend limitation above applies.

With `--native-controllers`, diagnostics also show `native` channel codes for
every reported physical channel, including those which gilrs labels `Unknown`.
Copy the full decimal u32 code into, for example:

```json
{"kind":"axis","id":{"native":196608}}
```

For an analog button source use `kind: "button"`; in a button-pair source use
`"button": {"native": 12345}`. These numbers are examples, not suggested mappings.
Read the values from the actual device. Native codes are platform-specific;
do not assume a Windows native-code configuration is valid on Linux. The
adapter preserves full u32 codes without truncating them or assigning IDs in
movement order. Standard aliases continue to work alongside native codes.
Only one backend runs: the opt-in adapter refuses to start alongside Bevy's
stock gilrs plugin. When initialization fails, it reports the error and leaves
keyboard control available. Native-channel configurations require the opt-in
flag rather than silently selecting a backend that cannot expose their channels.

`source.kind: "buttons"` contains a required `positive` button source and an
optional `negative`. Each has its own `device` and `button`, so the two directions
can come from different devices. Both together cancel. A single positive button
can drive pitch, roll, yaw, brake, or any other action. For two-way steering use a
pair. Inversion swaps button-pair directions.

The conventional gamepad template uses `{"kind":"first"}`. For a multi-device
cockpit use, for example:

```json
{
  "kind": "named",
  "name": "Name exactly as reported by the device",
  "vendor_id": 1234,
  "product_id": 5678,
  "instance": 0
}
```

The panel displays vendor/product IDs in decimal. The JSON stores full Unicode
device names, but the default UI font can only display ASCII; non-ASCII characters
are shown as `?` in diagnostics. If a name is not ASCII, obtain the exact OS name
from the operating system rather than copying the substituted display string.
When vendor/product is omitted, an ambiguous same-name match is refused instead
of guessing. Slots for identical devices are retained after disconnect, so unit
1 never silently becomes unit 0 while flying. Bevy has no per-unit serial number;
identical units must be connected in a consistent order after restarting.

### Calibration

Record raw values at both travel endpoints and at rest, then edit `calibration`:

```json
{"min": -0.91, "center": 0.03, "max": 0.88}
```

For pitch/roll/yaw, the two sides of center are normalized independently to -1 and
+1 before deadzone, response, and inversion. For an absolute throttle/brake/flap
axis, min/max map to 0/1; center is unused but must remain strictly between min
and max. Analog buttons/triggers normally need min=0, center=0.5, max=1. Inverting
an absolute lever swaps 0 and 1. Out-of-range finite values are clamped; non-finite
samples never become control commands. Save the edited file and relaunch with
`--input-config`; malformed files report an error instead of loading defaults.

Spring-centered sticks yield to keyboard input inside their deadzone. Absolute
lever endpoints, including zero, remain active. Holding a keyboard throttle/flap
key takes priority over an absolute lever while held; releasing the key restores
the lever's actual position. Disconnection clears device samples, releases
controller brakes, centers unattended control surfaces, preserves held
throttle/flap settings, and restores keyboard control. Reconnection requires
fresh analog samples rather than replaying a stale full-deflection position.

## Attitude clipping reproduction

```sh
cargo run -p flightsim-ui --example attitude_clip_repro -- /tmp/attitude-clip.png
python3 crates/flightsim-ui/examples/check_attitude_pixels.py /tmp/attitude-clip.png --shape square --expect-leaks
```

The example uses a real Bevy offscreen Image render target and screenshots that
target. It neither creates a window nor opens a display-server socket. It keeps
the original oversized rotated child with `Overflow::clip`, for bank angles
0/30/45/90/135/180 degrees and pitch offsets -11.52/0/+11.52 pixels.

Window bounds are x=`48+160*column` to x+64, y=`80+160*row` to y+64. The background
is black; clip windows are magenta; sky/ground use the original instrument colors.
Measure blue/brown pixels outside these bounds, allowing a one-pixel antialias
fringe, and inspect the image. Tests that merely bound `horizon_placement.offset`
cannot verify rendered clipping.

Relevant pinned upstream source:
`bevy_ui_render-0.18.1/src/lib.rs`, `prepare_uinodes`, around line 1596; its
vertex clipping explicitly does not support rotation/scaling. This is a source
explanation, not a substitute for the reproduction screenshot.

## Execution results

- `cargo test -j 2 -p flightsim-input -p flightsim-ui --all-targets`: **65 input
  tests and 121 UI tests passed**, zero failures. Both render examples compiled
- Subsequent native-adapter source verification: **71 input tests passed** in
  the direct compiled harness, including native-code serialization, actual ECS
  sample routing, and same-frame reset ordering. Direct clippy with workspace
  lint flags and `-D warnings` also passed. The lead's final combined Cargo run
  verifies the integrated optional backend and app CLI
- `cargo build -j 2 -p flightsim-ui --example attitude_clip_repro`: passed
- Reproduced original clipping failure at 16:11 UTC with the pinned Bevy renderer
  using Vulkan, llvmpipe LLVM 19.1.7 / Mesa 25.0.7. There were **85,024 sky/ground
  pixels outside instrument bounds**, plus severe deformation and uncovered
  magenta regions in several dials. Image was inspected, not just counted
- The repair uses an embedded WGSL UI material on one stationary dial-sized
  quad. Only its internal sky/ground boundary moves; no geometry extends beyond
  the instrument. It also keeps pitch displacement aligned to the banked normal
  and verifies the rendered right-bank ground side. The pre-existing ±30-degree
  display pitch saturation is retained; this is not a full spherical attitude
  instrument redesign
- The first rectangular `attitude_matrix` rendered pitch -90/0/+90, roll
  0/30/45/90/135/180, then the
  mirrored negative banks: **all 36 cases had zero outside-dial pixels**. Every
  dial had at least 4,001 classified interior sky/ground pixels. Remaining
  interior pixels are the antialiased sky/ground boundary
- Follow-on round-frame verification at 16:39 UTC: dial backgrounds now use
  `BorderRadius::MAX`, and the horizon shader has an independent analytic
  circular alpha mask (parent overflow alone cannot clip a circle). **All 36
  positive/negative bank cases had zero sky/ground pixels outside the circular
  aperture**, allowing a one-pixel fringe. Each circle contained 2,931–3,012
  classified solid-color pixels; the remaining circle pixels are intentional
  antialiasing along its rim and horizon. Both images were inspected. The checker
  defaults to a circle; use `--shape square` for the retained original reproduction
- The pixel checker was independently checked against a clean synthetic matrix
  and an intentionally injected outside-dial pixel. The latter failed the fixed
  image assertion as required; a blank image is also explicitly rejected
- The early render checks linked the exact repository example source directly
  against the already compiled pinned Bevy artifact while Cargo's changed
  feature graph rebuilt. The subsequent Cargo example build and all-targets
  tests passed as above. No claim of physical-GPU validation is made
- Expected environment warnings: no audio device, and gilrs could not create a
  udev hotplug monitor. Therefore no physical input/hotplug result is claimed
- Actual app screenshots showed the round cockpit gauges and the optional native
  backend's no-device fallback, but diagnostics originally overlapped the takeoff
  instruction. The panel now starts at 135 px and is bounded 12 px above the
  screen bottom; its text has a real padded parent. A full 14-channel page and
  all six footer statuses were rendered at 1280×720 with a clearly labeled
  **simulated QA device**: no tutorial overlap or bottom clipping. Traffic uses
  its own padded parent and disappears while diagnostics is open. These are UI
  layout results, not hardware-device results. All 121 UI tests and direct
  clippy remained green after this layout change

Session artifacts (not bundled into the release):

- `/workspace/shared/flightsim-qa/attitude-clip-before.png`
- `/workspace/shared/flightsim-qa/attitude-fixed-positive.png`
- `/workspace/shared/flightsim-qa/attitude-fixed-negative.png`
- `/workspace/shared/flightsim-qa/attitude-round-positive.png`
- `/workspace/shared/flightsim-qa/attitude-round-negative.png`
- `/workspace/shared/flightsim-qa/default-diagnostics.png` (actual app, before
  the diagnostic panel position/padding correction)
- `/workspace/shared/flightsim-qa/swift-traffic.png` (actual app, before padding)
- `/workspace/shared/flightsim-qa/diagnostics-layout-fixed.png` (full simulated
  device page, latest UI source)
- `/workspace/shared/flightsim-qa/traffic-layout-fixed.png` (latest UI source)
- Matching `*-pixels.json` files and renderer logs in the same directory
- `input-ui-tests.log`, plus standalone `direct-input-tests.log` and
  `direct-ui-tests.log` (65 and 121 passes, respectively)

Repeat the fixed rendering check:

```sh
cargo run -p flightsim-ui --example attitude_matrix -- /tmp/attitude-fixed.png
python3 crates/flightsim-ui/examples/check_attitude_pixels.py /tmp/attitude-fixed.png
cargo run -p flightsim-ui --example attitude_matrix -- /tmp/attitude-negative.png --negative-bank
python3 crates/flightsim-ui/examples/check_attitude_pixels.py /tmp/attitude-negative.png
```

## Physical-controller checklist (not executed)

1. Record model, connection type, OS, vendor/product, and backend
2. Confirm every stick/pedal/throttle axis and button appears in diagnostics
3. Test min/center/max, then save calibration, invert, deadzone, and response
4. Relaunch and verify all mappings/calibration persisted exactly
5. Drive pitch/roll/yaw/throttle/brake/flaps simultaneously from multiple devices
6. Verify keyboard coexistence, trim keys, and view shortcut conflicts
7. Unplug each device while displaced; verify no stuck brake/control or panic
8. Reconnect and verify identity, fresh samples, and full-range recovery
9. Record any backend-unexposed controls explicitly; do not mark Issue #2 complete
