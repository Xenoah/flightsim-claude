# Aircraft profile v1 authoring guide

An aircraft profile is a local UTF-8 JSON file that selects the existing fixed-gear,
three-wheel, single-propeller flight model, an exterior model, cockpit eye position,
input rates and synthesized sound. It is data, not executable plugin code. This
publishes the current v1 format without changing its parser, built-in aircraft,
model transforms or physics. A valid file is not proof of realistic or stable
flight performance.

- [Reusable JSON Schema](../schemas/aircraft-profile-v1.schema.json), JSON Schema
  Draft 2020-12; schema identifier `urn:flightsim:aircraft-profile:v1`
- [Required-fields-only example](examples/aircraft-profiles/required-only.json)
- [Same example with explicit pitch rates](examples/aircraft-profiles/explicit-pitch-controls.json)
- [Existing built-in aircraft and flight checks](qa/aircraft-profiles-2026-10-01.md)

The examples are original, fictional authoring fixtures, not new selectable
built-ins or calibrated aircraft variants. Their dynamics are identical and their
controls behave identically: the second example spells out the first example's
inherited pitch rates. No `schema_example.glb` is supplied. Use `--no-model` for a
placeholder, or supply your own suitably licensed, self-contained GLB as described
below. These files have passed profile validation; they have not been flight-tuned.

## Load a profile

From the repository root:

```sh
cargo run -p flightsim-app -- --aircraft docs/examples/aircraft-profiles/required-only.json --no-model
cargo run -p flightsim-app -- --aircraft assets/aircraft/swift_sport.json --view chase
```

For an installed build, use its executable in place of `cargo run ... --`.
`--aircraft` treats a choice ending in lowercase `.json` as a filesystem path;
a relative profile path is relative to the process working directory. Other
choices are built-in IDs (`light-single`, `swift-sport`). Reading is bounded to
**128 KiB = 131,072 bytes**, including whitespace. Larger files, invalid UTF-8,
malformed JSON, duplicate struct fields, unsupported versions, missing required
fields, unknown fields and invalid values fail profile loading. Unknown IDs and
unreadable files also fail; no other aircraft is silently selected.

Associate the schema with a file in your editor's settings, or pass it to a
Draft 2020-12 validator. Do **not** insert `$schema` into the profile: it is an
unknown v1 field and the runtime rejects it. Schema validation does not install
anything or load a model. The app's loader is the authority for runtime acceptance.

All fields in each schema `required` list must be present. There is no merging
with a built-in or inferred physical default. The only optional fields are
`controls.elevator_rate` and `controls.elevator_centering_rate`; omission or JSON
`null` inherits the corresponding common rate. All other numbers require actual
JSON numbers, not strings or `null`. Write `version` as the integer token `1`.

## Model files, asset root and axes

`model.path` is resolved **under the app's resolved `assets/` directory**, never
beside the profile. For example, `"aircraft/my_plane.glb"` means
`<resolved asset root>/aircraft/my_plane.glb`, even when the profile is elsewhere.
The profile does not copy, download or install that file.

The development build searches upward for `assets/` from these candidate bases,
in order: `BEVY_ASSET_ROOT`, `CARGO_MANIFEST_DIR`, the executable's directory, then
the current working directory. It takes the first existing asset directory. Each
base must be resolvable on disk. `BEVY_ASSET_ROOT` is therefore a search base
containing an `assets/` directory, not a profile-relative override. The
`commercial-staging` build instead requires `assets/` adjacent to the executable
and never searches the developer checkout. Explicitly selected missing models
fail startup; `--no-model` explicitly selects the procedural placeholder.

The profile path must be nonempty and at most **256 UTF-8 bytes**, relative, with
lowercase `.glb` suffix. Absolute paths, `..` path components, leading `./`, colon
and backslash are rejected. Use ordinary forward-slash-separated components such
as `aircraft/my_plane.glb`. For compatibility, the existing platform path parser
normalizes internal `./` and repeated `/`; this is not a new portable-path package
format. Filesystem rules can reject a path that passed profile parsing.

Authoring scope is a **self-contained glTF 2.0 binary GLB** with its buffers and
images embedded and visible geometry in **scene 0**; the app explicitly requests
that scene. The `.glb` suffix alone does not prove that dependencies are embedded,
that extensions are supported, or that the model is loadable. The schema inspects
no GLB bytes, external-resource references, textures, licenses or geometry. The separate [aircraft data package v1](aircraft-packages.md) now supplies a
bounded static/untextured GLB manifest and explicit offline validation/basic
import. This standalone profile format does not install dependencies or
package files and is not a general untrusted-asset sandbox. The
commercial staging path check also checks canonical root escape; ordinary
profile validation itself is lexical and is not a sandbox for untrusted assets.
The separate [regional terrain package system](content-packages.md) does not
install aircraft profiles.

[glTF 2.0's coordinate convention](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#coordinate-system-and-units)
is right-handed, **+Y up, +Z forward, -X right**, with metre units. For newly
authored standard content, specify `"forward": "+z", "up": "+y"`. The aircraft
physics body frame is **+X forward, +Y right, +Z down**. `ModelFit` maps the declared
model forward to body +X and model up to body -Z using a rotation, without
reflecting the mesh.

Existing assets intentionally keep their legacy adapters:

| Existing model/adapter | Forward | Up |
| --- | --- | --- |
| Light Single bundled profile | `-x` | `+y` |
| Swift Sport bundled profile | `+x` | `+y` |
| Legacy `ModelFit::default()` / model-override fallback | `-z` | `+y` |

The legacy `-z` default is **not** the standard glTF front direction. It remains
unchanged for compatibility. Do not rotate an existing mesh or change its profile
axes merely to match a label. Declare the actual exported model axes. Accepted
axis spellings are `+x`, `-x`, `+y`, `-y`, `+z`, `-z`; existing aliases also allow
unsigned positive axes, letter case and surrounding Unicode whitespace. Forward
and up must use different coordinate axes, even when their signs differ.

`model.length_m` is the desired exterior length along the declared forward axis
(1–100 m). After mesh loading, the app measures descendant bounds in model space
and applies a uniform scale. It does not infer a new center of mass or recenter
the model: author its origin appropriately. Missing/degenerate extents are not
validated by this schema. The scale does not change mass, wing geometry or gear
contact locations. `camera_eye_m` is already in body metres, not model units.

`--model`, `--model-forward`, `--model-up` and `--engine` are explicit CLI overrides.
When supplying `--model`, also specify its axes: the override uses the legacy
model fallback rather than inheriting the profile's axes. The selected profile's
`length_m` still supplies the target length. `--no-model` does not waive JSON or
physics validation.

## Units and fields

All bounds below are inclusive unless marked strict. They are supported input
domains, not a claim that every combination describes a plausible aircraft. No
feet, pounds, horsepower, knots or degrees are inferred from values.

| Field | Meaning and domain |
| --- | --- |
| `version` | Integer token `1` |
| `id` | 1–48 ASCII characters, each `a`–`z`, `0`–`9` or `-`; no registry/uniqueness check |
| `dynamics.name` | 1–96 printable ASCII bytes with at least one non-space character |
| `camera_eye_m` | Exactly `[forward, right, down]` from center of mass; each -10–10 m |
| `engine_sound` | Prefer `piston` or `turbine`; sound only, does not change propulsion |

The sound parser also accepts `prop` / `propeller` for piston and `jet` /
`turbofan` / `fighter` for turbine, ignoring case and surrounding Unicode
whitespace. Those are existing aliases, not additional physical engine types.

### Dynamics

| Field inside `dynamics` | Units and bounds |
| --- | --- |
| `mass_kg` | 100–100,000 kg |
| `inertia_kg_m2` | Exactly `[Ixx, Iyy, Izz, Ixz]` in kg m²; first three 1–10¹⁰, Ixz -10¹⁰–10¹⁰; see cross-field check below |
| `wing_area_m2` | 1–1,000 m² |
| `wing_span_m` | 1–100 m |
| `mean_chord_m` | 0.1–20 m |
| `max_shaft_power_w` | 1–10⁸ W |
| `propeller_efficiency` | 0.01–1, dimensionless |
| `static_thrust_n` | 1–10⁶ N full-throttle sea-level static limit |
| `rolling_friction`, `braking_friction`, `lateral_friction` | Each 0–2, dimensionless |
| `friction_transition_mps` | 0.01–10 m/s |
| `landing_gear` | Exactly three complete wheel/strut objects |

Each gear object contains `contact_m` (three body coordinates, each -100–100 m),
`spring_n_per_m` (1–10⁸ N/m), `damping_ns_per_m` (0–10⁸ N s/m), `max_stroke_m`
(0.001–10 m), `bottom_stop_travel_m` (0.001–10 m) and `max_recoil_mps`
(0.01–20 m/s). Contact positions refer to full strut extension from the center of
mass. The order is preserved; supply all three explicitly.

The inertia tensor uses rows `[Ixx, 0, -Ixz]`, `[0, Iyy, 0]`, `[-Ixz, 0, Izz]`.
After the scalar bounds, runtime requires
`Ixx * Izz - Ixz * Ixz > 1e-9 * Ixx * Izz`. A positive determinant alone is not a
substitute for the supported positive-definite tensor and its inverse. The JSON
Schema cannot express this multiplication between fields.

### Aerodynamics

Every field in `dynamics.aero` is required. `alpha` and `beta` slopes are per
**radian** of angle of attack/sideslip. Control derivatives multiply normalized
commands, not degrees or radians: convert per-radian control-surface source data
using the intended maximum deflection before authoring the coefficient. Moment
rate derivatives multiply nondimensional rates `p b/(2 V)`, `q c/(2 V)`,
`r b/(2 V)` (span `b`, chord `c`, airspeed `V`). The schema describes each field.

| Fields | Bounds / sign constraint |
| --- | --- |
| `lift_alpha`, `roll_aileron`, `pitch_elevator`, `yaw_beta`, `yaw_rudder` | Strictly >0, at most 100 |
| `roll_rate_p`, `pitch_alpha`, `pitch_rate_q`, `yaw_rate_r` | At least -100, strictly <0 |
| `lift_flaps`, `drag_flaps` | 0–100 |
| `stall_angle_rad` | 0.05–0.7 rad |
| `stall_blend_rate` | 0.1–100 rad⁻¹, sharpness of stall blending |
| `drag_min` | 0.0001–1 |
| `oswald_efficiency` | 0.05–1 |
| `lift_zero`, `side_beta`, `side_rudder`, `roll_beta`, `roll_rate_r`, `roll_rudder`, `pitch_zero`, `pitch_flaps`, `yaw_rate_p`, `yaw_aileron` | -100–100 |

### Controls and approach initialization

| Field inside `controls` | Units, bounds and behavior |
| --- | --- |
| `surface_rate` | 0.01–10 normalized units/s; aileron/rudder ramp and inherited pitch ramp |
| `elevator_rate` | Optional number 0.01–10 or null; omitted/null inherits `surface_rate` |
| `centering_rate` | 0–10 normalized units/s; zero disables spring return |
| `elevator_centering_rate` | Optional number 0–10 or null; omitted/null inherits `centering_rate` |
| `throttle_rate`, `flap_rate`, `trim_rate` | Each 0.01–10 normalized units/s |
| `default_trim` | -1–1; parked-start elevator trim |
| `approach_speed_mps` | 10–150 m/s initial approach true airspeed |
| `approach_pitch_rad` | -π/4–π/4 initial body pitch in radians; separate from glideslope |
| `approach_trim` | -1–1 initial approach elevator trim |
| `approach_throttle`, `approach_flaps` | Each 0–1 initial approach fraction |

Pitch overrides affect keyboard and rate-mode inputs; absolute analog positions
remain direct. Parked starts initialize throttle and flaps to zero and use
`default_trim`. Approach starts use the approach values. These are initial
conditions, not an autopilot, auto-trim controller or hands-off landing promise.
Changing mass, coefficients, altitude, weather or glideslope may require tuning
and pilot input. Schema `default: null` is an annotation for the two optional
fields; validators need not insert or mutate values.

## Validation layers and known schema limits

1. A Draft 2020-12 validator checks structure, required/unknown fields, array
   lengths, scalar ranges, stability signs and nonparallel axis strings.
2. `AircraftProfile::load` bounds the file bytes, parses with Serde and calls the
   existing profile/FDM/model validators. Floating-point values must be finite;
   unsupported combinations fail before constructing the FDM.
3. Startup resolves assets and loads scene 0 through Bevy. Model compatibility,
   appearance, performance and flight behavior require their own checks.

The standalone schema cannot enforce serialized size, path length in UTF-8 bytes,
raw numeric spelling or duplicate keys after a JSON parser discards them. JSON
Schema's integer type accepts mathematical `1.0` / `1e0`; the runtime's `u16`
version parser requires an integer token. Its `maxLength` counts characters, so a
multibyte path can pass the schema but exceed the runtime's 256-byte limit.
Standard JSON excludes NaN and Infinity; validators receiving nonstandard
in-memory numbers or arbitrary-precision decimals do not reproduce all Rust f64
conversion/rounding behavior. Runtime finite checks and cross-field inertia
validation remain authoritative, including values close to strict boundaries.
Schema success also says nothing about filesystem existence, symlink confinement,
embedded dependencies, safe resource budgets, model fit or simulated stability.

The shared [fixture corpus](../schemas/tests/aircraft-profile-v1-cases.json)
explicitly labels cases where schema validation passes but runtime rejects. It
covers real accepted/rejected files, optional/null inheritance, axes and sound
aliases, unknown fields at every object level, physical domains, legacy path
normalization, duplicate keys, integer spelling, inertia and byte boundaries.
The unchanged built-ins and both authoring examples also pass the schema.

Run the checks without launching graphics:

```sh
# Python 3 with the optional development tool jsonschema >= 4.18 installed:
python3 schemas/tests/test_aircraft_profile_schema.py
# Uses the production file loader against the same corpus, including byte caps:
cargo test -j 2 -p flightsim-app aircraft_profile_schema_contract
# Existing profile/flight regression suite:
cargo test -j 2 -p flightsim-app aircraft_profile
```

No Rust dependency or CI tool installation is added for the schema. The Rust
contract tests are part of the normal app test target; the Python schema check is
an explicit authoring/development check. Tests do not grant model redistribution
rights or certify real flight-control hardware.

Implementation references: [profile loader](../crates/flightsim-app/src/aircraft_profile.rs),
[FDM boundary validation](../crates/flightsim-fdm/src/definition.rs),
[model fitting](../crates/flightsim-render/src/model.rs),
[asset startup policy](../crates/flightsim-app/src/distribution.rs), and
[JSON Schema validation specification](https://json-schema.org/draft/2020-12/json-schema-validation).
