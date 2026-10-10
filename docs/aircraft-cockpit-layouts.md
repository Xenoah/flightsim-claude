# Aircraft-specific cockpit layouts

The aircraft names below are the repository’s original/fictional labels. Real
aircraft are ergonomic references, not identifications or flight-model claims.
All geometry, bitmap lettering and display artwork are authored in code.

| Profile | Matching model | Interior |
| --- | --- | --- |
| `light-single` (legacy) | `aircraft/light_single.glb` | Analog trainer, dual yokes, enclosed high-wing cabin |
| `meadow-trainer` (legacy) | `aircraft/meadow_trainer.glb` | Analog high-wing trainer, dual yokes, warm light panel/olive upholstery |
| `swift-sport` (legacy) | `aircraft/swift_sport.glb` | Open canopy view, dual center sticks, original primary flight display |
| `kestrel-jet-trainer` (jet) | `aircraft/kestrel_jet_trainer.glb` | Side-by-side analog jet trainer, dual sticks, one aggregate throttle lever |

The app checks profile family, ID, declared model path and the model actually
accepted at startup. Missing/custom Kestrel models and other bounded profiles
retain their model-owned cockpit and 2D instruments. Unknown legacy profiles
keep the generic trainer fallback. Explicit placeholder mode retains legacy
layouts. Choosing a layout never changes the physical model or admission rules.

CLI built-ins remain Light Single and Swift Sport. The development aircraft
picker also offers Meadow and Kestrel. Commercial-staging still admits only
Swift. Cedar’s experimental visual asset and Peregrine’s numerical fixture do
not become selectable aircraft. No fighter or stealth model is supplied.

## Functional inventory

All layouts support roll/pitch by dragging either yoke/stick, rudder pedals,
throttle, flaps, trim, momentary service brakes, instrument illumination,
look-around/reset and ordinary HUD visibility. Home centers the view 15° down
for yokes or 24° down for sticks, keeping the panel and primary controls in view. Animation uses the existing
controls; sticks pivot in pitch/roll, yokes rotate/translate. Kestrel’s single
lever represents its aggregate throttle command, not multiple engines.

Swift’s display shows EAS knots, pitch/roll, ellipsoid altitude feet, vertical
speed ft/min, true heading and body yaw deg/sec. The central screens show the
existing flight/control data. There is no synthetic navigation map, flight
director, radio, autopilot, engine RPM/N1, fuel, electrical or mixture model.
The two round secondary gauges show throttle command and normalized flaps.

Kestrel’s EAS scale is 0–400 kt and its VSI scale is ±6,000 ft/min. These are
display ranges, not real-aircraft or simulated operating limits. Its existing
Mach-envelope/status notice remains the authority for the modeled envelope.
Other analog layouts retain 0–200 kt and ±2,000 ft/min ranges. All layouts use
the same truthful units and sources described in the [cockpit guide](3d-cockpit.md).

Kestrel’s parking brake remains independently latched with B. Its service-brake
handle is momentary; releasing it cannot release parking brake. Replay locks
flight interaction and labels effective axes/unknown separate trim. The separate
service-brake handle is unknown in replay and rests neutral; BRK TOT retains
the actual combined recorded braking. Existing
focus, map, pause, crash, scene-change and controller-arbitration rules remain.

## Visual sources inspected

- High-wing trainer: the user-supplied analog Cessna-style collage, actually
  viewed. Exact aircraft model is unknown. Current [Skyhawk manufacturer
  material](https://cessna.txtav.com/en/piston/cessna-skyhawk) supports general
  cabin/yoke ergonomics, not an assertion that this analog panel is current Skyhawk avionics
- Sport: [Diamond DA20 manufacturer page](https://www.diamondaircraft.com/en/flight-school-solution/aircraft/da20/overview/)
  documents two seats, panoramic canopy and center sticks. The manufacturer’s
  [cockpit photograph](https://www.diamondaircraft.com/fileadmin/_processed_/a/f/csm_DA20_Cockpit_G500TXi_f0c80a25ea.jpg)
  was actually inspected: curved dark panel, large left glass display, narrow
  center stack, right-side gauges and two sticks. Swift uses only these broad cues
- Sport control/panel research: [Van’s RV-12 training supplement](https://www.vansaircraft.com/wp-content/uploads/2021/01/RV-12-FTS-Rev-7-1.pdf),
  sections 5-5/5-6/5-13 and 6-3/6-5/6-7. Text was accessible; PDF pixels were not.
  RV-12 engine, flap detents and branded EFIS artwork are not imported
- Jet trainer: [USAF T-37 description](https://www.af.mil/News/Photos.aspx?igphoto=2000030990)
  supports the side-by-side training architecture. The actually inspected
  [1969 cockpit photograph by B. G. Jones / Seattleretro](https://commons.wikimedia.org/wiki/File:T-37_cockpit.jpg)
  is the photographer’s Commons publication, not an official USAF photo.
  It shows gray-green subpanels, a thick curved glare shield and dual sticks.
  Kestrel does not copy twin-engine, retractable-gear, radio, ejection or pressure systems

No source photographs are distributed or used as textures. Display artwork is
original and has no Garmin/Dynon branding. The pictured parked T-37 stick locks
are not operational controls and are not reproduced.

## Scene boundaries

Generated cockpit and exterior GLB are mutually exclusive. This removes Swift’s
opaque canopy obstruction and Kestrel’s overlapping static panel/stick meshes;
owned procedural rails represent the interior canopy framing. Exterior nose and
wings consequently are not visible from inside. In chase/tower views the original
GLB returns unchanged. Unknown bounded models keep their full authored GLB.
New-flight staging, cancellation and reset use the existing owned asset lifetime.

Software offscreen captures and automated picking are useful checks, but cannot
establish native human mouse/window, physical GPU, controller or flight-training
fidelity. See the [aircraft-specific qualification report](qa/aircraft-cockpits-2026-10-10.md)
for exact source, test results, capture hashes and remaining limits.
