# Functional analog 3D cockpit

The procedural cockpit for Light Single, Swift Sport and legacy-model external
profiles is now an original, aircraft-local three-dimensional interior. Its
reference was a user-provided collage of analog Cessna-style light-aircraft
cockpits. It is not a replica of an identified Cessna model, a certified training
device, or a claim of complete aircraft-system fidelity. No source photograph,
third-party mesh, texture or font is included.

## Viewing and operating

- Hold the right mouse button and drag to look around the cabin
- Home restores the forward panel view; R also resets the view when restarting
- V or the FLIGHT HUD switch shows/hides the ordinary readouts, help and flight log
- Pause reveals the full ordinary HUD. Replay identity, warnings, crash/pause
  notices and data attribution remain independent of this switch
- Hover a physical control for its operating instruction
- Hold the left button on either yoke and drag: left/right commands roll, down
  pulls back, up pushes forward. Releasing returns the existing input axes toward neutral
- Hold either rudder pedal to command that direction; look down to reach the pedals
- Drag the black THROTTLE knob up/down to increase/decrease the throttle setting
- Drag the white FLAPS paddle down/up to extend/retract flaps
- Drag the ELEVATOR TRIM wheel down/up for nose-up/nose-down trim
- Hold BRAKE HOLD to apply the existing wheel brakes. This is a momentary service
  brake, not a new parking-brake system
- PANEL LIGHT switches actual illumination of the instrument faces and needles

Pointer displacement commands the same held inputs as the corresponding keys.
Throttle, flaps and trim move at the aircraft's existing rates and retain their
position after release. Keyboard and configured controller support remain.
Presentation switches and look-around remain available in pause/replay.
Mouse commands do not bypass controller arbitration or add a separate physics
path. Flight controls lock during replay, pause, map capture, focus loss,
crash/fault, or unsupported aircraft modes. An interrupted drag requires a fresh
click; it cannot silently resume after closing the map or unpausing. Center the
yoke to reach controls physically hidden behind it at extreme displacement.

## What each instrument measures

| Instrument | Actual source and limitation |
| --- | --- |
| EAS / knots | Density-corrected equivalent airspeed; not calibrated pitot/static IAS |
| Attitude | Simulated aircraft pitch and roll; earth-relative horizon and pitch ladder |
| Altitude / feet | Geodetic ellipsoid height, with 1,000-ft and 10,000-ft revolutions; no pressure setting or barometric model |
| Body yaw / deg/sec | Body-axis yaw angular rate; not a gyro turn coordinator or slip ball |
| True heading | Simulated true heading; no magnetic variation or compass deviation model |
| VSI / ×100 ft/min | Simulated vertical speed; bounded display range ±2,000 ft/min |
| Throttle / percent | Actual throttle command, not measured RPM, torque or engine power |
| Flaps / percent | Actual normalized flap setting |
| Flight data stack | Ground speed, AGL, true wind direction/speed, ellipsoid altitude and flight/warning state |
| Control state stack | Throttle, flaps, separate trim when available, and actual brake command |

The eight instruments, 3D needles, two yokes, pedals, throttle knob, flap paddle,
trim wheel and switches are coupled to state. The center stack is deliberately
labeled FLIGHT DATA / CONTROL STATE; it does not impersonate a radio.

Live yokes and pedals show the pilot's untrimmed axis displacement. Replay only
contains effective control commands, so replay animation uses those recorded
commands and explicitly labels this approximation. Separate trim is unavailable
in replay and displays N/A; its wheel rests at neutral rather than inventing
historical trim. Physical controls cannot modify replay.

## Scope still missing

Fuel quantity/burn/selector, mixture, magnetos/starter, battery/alternator/bus
failures, circuit breakers, radio tuning/COM/NAV, transponder and pressure/gyro
instrument dynamics are not modeled. No inert imitation controls for those
systems were added. Full operation of all systems in a real aircraft is still a
separate development task. Fixed cabin hardware (seats, upholstery, screws,
window frame and carpet) is visual structure.

Bounded-model jets/turboprops retain their existing model-owned interiors and
2D instrument fallback. This change does not replace those interiors or alter
physics coefficients, aircraft/replay identity, terrain or licensing gates.

Validation is recorded in the [cockpit QA report](qa/functional-cockpit-2026-10-10.md). Automated ECS and
software rendering do not establish physical GPU, controller, audio or pilot
handling acceptance.
