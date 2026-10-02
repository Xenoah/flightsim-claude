# Aircraft profiles and original Blender model

## Scope and limits

Two named startup selections are provided: `light-single` preserves the existing dynamics and parked controls, while `swift-sport` changes mass, inertia, wing geometry, power, aerodynamic response, model, cockpit eye position, engine sound and keyboard control rates. Both use the existing bounded fixed-gear piston-propeller FDM. Neither is certified real-aircraft performance data. The legacy Light Single still defaults to turbine sound because that was an explicit prior project choice; `--engine piston` remains available.

Each JSON profile is version 1, in SI units, with unknown fields rejected. Input is limited to 128 KiB. Names and identifiers are bounded ASCII. Dynamics are checked before assertion-based FDM constructors: finite positive mass/inertia, positive-definite tensor, valid wing/engine/gear domains and stability/control signs. Model paths are relative `.glb` paths below the assets directory, with parent traversal, absolute paths, colon and backslashes rejected. JSON errors and unknown aircraft ids fail startup with nonzero status instead of silently launching another aircraft.

## Commands

```
cargo run -p flightsim-app -- --aircraft light-single
cargo run -p flightsim-app -- --aircraft swift-sport --view chase
cargo run -p flightsim-app -- --aircraft assets/aircraft/swift_sport.json
cargo run -p flightsim-app -- --aircraft swift-sport --headless-screenshot swift.png
```

A custom profile's model path is relative to the app's resolved assets directory, not the profile's directory. `--model`, `--model-forward`, `--model-up` and `--engine` explicitly override the selected profile. A recording is checked against the selected aircraft's complete dynamics fingerprint; select the same aircraft for replay. Failed replay loading or fingerprint validation now exits instead of starting an unrelated live flight.

## Original model provenance

Created using Blender 4.3.2 with the repository's `tools/blender/build_swift_sport.py`. The script makes the fuselage, canopy, airfoil wings, tail, fixed gear, navigation lights, propeller and conforming paint bands. It exports only aircraft meshes to GLB, then saves an editable compressed `.blend` with studio camera/lights. No external mesh, texture or logo was downloaded. Blender axes are +X forward/+Z up; GLB export is +X forward/+Y up. The profile supplies those axes to the existing ModelFit correction layer.

Rebuild:

```
blender --background --threads 2 --python tools/blender/build_swift_sport.py -- assets/aircraft
```

Studio images are actual Cycles CPU renders. Denoising is disabled because this Blender build lacks OpenImageDenoise. They show the asset, not the game. Actual app screenshots and simulation checks are recorded separately below when verified.

## Verification status

- Original GLB and editable Blender source successfully generated
- Both actual three-quarter Blender views inspected; paint bands adjusted to conform to wing geometry
- Eight profile tests pass, including physics fingerprint compatibility, invalid-file boundaries, both 120-second takeoffs, and 30-second hands-off approaches
- Actual app model loading and startup/reset wiring require separate integration-owner verification
- No physical GPU or flight-control hardware certification


## Approach initialization correction

The old startup used the clean/takeoff trim (`0.09` / `0.08`) with full flaps,
a level body pitch, and throttle `0.45` / `0.40`. Both bundled aircraft developed
large unwanted pitch/speed transients. The diagnostic previously checked only
that the simulation remained finite; that was not sufficient to establish a
usable approach start.

Each profile now has separately validated `approach_trim` (−1 through +1) and
`approach_pitch_rad` (−π/4 through +π/4). These are required version-1 fields;
nonfinite or out-of-range values fail validation. `pilot_controls(true)` selects
the approach trim; parked throttle, flaps and trim remain unchanged.
`AircraftProfile::approach_state(runway, distance, glideslope)` uses the shared
simulation approach placement/velocity and applies the profile's starting body
pitch. It changes no FDM coefficients and installs no feedback controller.
The app must use this helper for `--approach`; reset then reuses the stored
initial state and the same approach controls.

### Derivation

The selected speed, full flap setting and requested 3-degree descent were held
fixed. At the synthetic runway's 1.5 NM start (153.589 m ellipsoid altitude,
8 m runway elevation), the existing ISA density and normal gravity were used.
Solving the existing aerodynamic/thrust equations for zero vertical and
longitudinal acceleration and zero pitch moment gives the following values.
The equilibrium equations in body axes are:

- α = θ + γ, where θ is body pitch and γ is the positive descent angle
- L cos α + D sin α = mg cos θ
- T + L sin α − D cos α = mg sin θ
- Cm0 + Cmα α + Cmδe trim + Cmflaps flaps = 0

Lift/drag include the existing smooth stall blend and induced drag. Throttle
uses the existing propeller power/density model, not a changed force law.
Rounded profile values are checked against the production FDM force/moment
functions: initial acceleration < 0.01 m/s² and angular acceleration < 0.0001
rad/s². These are initialization consistency checks, not real-aircraft validation.

| Profile | TAS target (m/s) | Pitch (rad / degrees) | Trim | Throttle | Flaps |
| --- | ---: | ---: | ---: | ---: | ---: |
| Light Single | 35 | −0.07269918 / −4.16536° | 0.15570889 | 0.39155148 | 1 |
| Swift Sport | 36 | −0.08404719 / −4.81555° | 0.13761162 | 0.25748579 | 1 |

The mild nose-down body attitudes are a consequence of these generic models'
full-flap lift at the chosen speeds. They are not presented as real-aircraft
landing attitudes or evidence of real-world handling fidelity.

### Unchanged scenario and acceptance

Both baseline and corrected runs use the synthetic runway, 1.5 NM start,
3-degree initial velocity descent, ISA, still air, flat empty-tile terrain,
1/120-second simulation steps and the selected profile's controls held
unchanged. There is no autopilot, flight director, feedback or auto-trim.

The new regression checks **every step for 30 seconds**, including the spawn
transient: TAS within 1 m/s of the selected speed, descent angle 3 ± 0.5 degrees,
pitch within 1 degree of its initial value, and no crash or numerical divergence.
The tolerances express the intended approach scenario independently of the
configured trim values. It also checks force balance, runway position/velocity/
heading preservation at four headings, distinct parked controls, and rejection
of invalid approach fields.

| Profile / initialization | Time (s) | TAS (m/s) | Pitch (degrees) | Vertical speed (m/s, up positive) | Descent angle (degrees) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Light / baseline | 10 | 43.1 | −11.7 | −5.90 | approximately 7.9 |
| Light / baseline | 30 | 40.5 | −9.6 | −4.28 | approximately 6.1 |
| Light / corrected | 10 | 34.973 | −4.124 | −1.805 | 2.958 |
| Light / corrected | 30 | 34.912 | −4.100 | −1.787 | 2.935 |
| Swift / baseline | 10 | 44.5 | −9.5 | −4.13 | approximately 5.3 |
| Swift / baseline | 30 | 41.8 | −8.0 | −3.00 | approximately 4.1 |
| Swift / corrected | 10 | 35.972 | −4.773 | −1.855 | 2.957 |
| Swift / corrected | 30 | 35.908 | −4.748 | −1.837 | 2.932 |

Baseline descent angles above are reconstructed from the rounded logged TAS
and vertical speed (still air); corrected angles are measured directly.

Both unchanged 120-second takeoff regressions also pass: Light lifts off at
24.975 s and ends at 349.7 m AGL; Swift lifts off at 16.117 s and ends at
672.4 m AGL. This takeoff check uses the existing flight director and therefore
is not a hands-off takeoff claim.

Reproduce the focused checks:

```sh
cargo test -j 2 -p flightsim-app aircraft_profile -- --nocapture
```

Limits: the calibration is for a 3-degree near-sea-level approach in still air,
not arbitrary altitude, mass, wind, turbulence, glideslope, custom profiles,
terrain clearance or hands-off landing through flare/touchdown. These settings
do not compensate for weather or changing air density while descending. Real
flight-control hardware, pilot judgment and subjective handling remain separate
validation work.
