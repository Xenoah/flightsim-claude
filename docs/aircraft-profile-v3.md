# Exact aircraft profile v3 and turboprop identity

`flightsim_sim::aircraft_profile_v3::AircraftProfileV3` is an explicit pure Rust
loader for `running_turboprop_table`, [physical law revision 1](adr/0018-bounded-running-turboprop.md). It returns an
immutable `TurbopropAircraftConfig`, presentation/input metadata and an explicit
`RunningTurbopropStart`. It performs no asset read, warmup, flight initialization
or app/replay dispatch. Existing profile v1/v2 decoders and replay v1–v4 retain
their meanings; the v4 identity gate remains exclusively schema-2 dry jet.

The public [JSON Schema](../schemas/aircraft-profile-v3.schema.json) describes
authoring structure. The [numerical example](examples/aircraft-profiles-v3/numerical-turboprop.json)
contains original synthetic values for parser/identity tests. It is not a Cedar
aircraft preset, tuned governor, measured engine map, certified performance, or
an assertion that the complete envelope can be flown. Its GLB path is deliberately
unprovided and is never opened by this loader.

## Three separate admission gates

1. **JSON data shape:** required/unknown fields, data types and scalar/array bounds
   are described by the public schema
2. **Exact profile construction:** original bytes and number tokens are bounded;
   metadata, physical components, cross-component containment, rotor-subtracted
   inertia and the three default engine values are validated fallibly
3. **Actual operating support:** full rigid/engine state and the actual position,
   attitude, wind, ground and atmosphere must pass the separate FDM evaluator

Passing either earlier gate does not imply the next. The pressure, temperature,
Mach and shaft-speed intervals form only a bounding box. Actual support is its
intersection with forward axial inflow, propeller advance ratio and pitch, positive
absolute axial spin, axial helical-tip-Mach proxy and crossflow ratio. The proxy
does not bound all instantaneous blade velocities from transverse inflow and
pitch/yaw rotation. A tailwind while parked or backward taxi can give negative
axial inflow and is unsupported in this first map family. Unsupported queries are
errors, not inputs to clamp or repair.

Paired CT/CP construction checks continuous `CP-J*CT >= 0`, positive-thrust nodal
ideal-disk power and nondecreasing CP with pitch. Every actual map query also
checks the stronger ideal-disk bound. A bilinear map can pass nodal admission and
fail that stronger bound internally; a regression fixture demonstrates this.
Pitch-monotone CP establishes feedback direction, not governor stability.
Numerical fixtures and sampled checks do not certify full-map physical accuracy,
stable handling or a real aircraft's performance.

## Authoring shape and limits

Outer keys are exactly `version`, `id`, `dynamics`, `model`, `controls`,
`camera_eye_m` and `engine_sound`. The integer version is `3`. `dynamics` requires:

- `kind: "running_turboprop_table"`, `revision: 1`
- `airframe`: unchanged neutral fields and exactly three ordered gear contacts;
  inertia is the complete locked-rotor tensor, with `[Ixx,Iyy,Izz,Ixz]` input
- `turbine`: schema 1; 2–16 pressure and temperature knots; up to 256
  `{idle_w, maximum_w}` cells, temperature fastest; rise/fall seconds and torque
  limit. Output shaft power already includes gearbox/accessory losses
- `propeller`: schema 1, `isolated_axial_propeller` convention; diameter, represented
  rotor axial inertia and integer sense −1/+1; 2–32 J and blade-pitch knots; up to
  1,024 paired `{ct, cp}` cells, pitch fastest. Pitch is radians at 0.75 radius
- `governor`: schema 1; fixed reference rad/s, dimensionless gain, fine/coarse
  rates rad/s and minimum/maximum pitch stops
- `aero`: unchanged Mach-aero schema 1; all 25 coefficients at 2–32 knots
- `envelope`: pressure, temperature, Mach, relative-shaft rad/s and absolute-spin
  rad/s intervals, plus maximum axial helical tip Mach and crossflow-tip ratio
- `running_start`: turbine fraction, relative shaft rad/s and actual blade pitch

Ordered axes and intervals must increase by at least `1e-9`. J starts at zero.
Turbine pressure is 0.1–2, temperature 0.25–2, and each power is 0–5,000,000 W
with maximum at least idle. Rise/fall constants are 0.1–30 s, torque 1–100,000 N m.
J is 0–4; pitch is 0–π/2; CT/CP each lie in −2–2. Diameter is 0.5–6 m and rotor
inertia 0.1–1,000 kg m². Shaft bounds/reference are 20–1,000 rad/s, governor gain
0.0001–10 and pitch rates 0.001–1 rad/s. Tip/crossflow maxima are respectively
`(0,0.8]` and `(0,0.1]`, including representable positive subnormal endpoints;
such tiny structural limits are not a usable flight envelope.

The full pressure/temperature envelope must fit turbine axes, Mach must fit the
aero schedule, and governor stops must fit the propeller pitch axis. Reference
speed must fit the relative-shaft interval. Both locked and rotor-subtracted
inertia must meet positive-definite conditioning and finite inverse requirements;
the reduced tensor is rejected before any assertion-based constructor call.

`running_start` is mandatory and has no implicit defaults. Its turbine fraction
is in `[0,1]`, shaft speed inside the relative-shaft interval and pitch inside
governor stops. `running_start()` returns validated fraction and SI wrappers.
These initial conditions are not solved equilibrium. No live time is advanced
to settle the engine. A future session/replay integration must use and store the
actual complete initial state and evaluate its actual environment explicitly.

Names, model axes/paths, sound aliases, control rates and camera metadata use
the existing [v2 metadata semantics](aircraft-profile-v2.md#metadata-and-authoring-limits).
Sound never selects the physical law. Asset path validation is lexical only,
does not open/resolve the file or prove confinement, and conveys no asset rights.

## Original-byte and exact-number policy

The three entry points `parse`, `from_bytes` and `load` enforce the same 1 MiB
UTF-8 budget, including whitespace. File reads stop after one additional size
check byte. The string/escape-aware preflight limits nesting to 128 containers;
serde_json's ordinary recursion guard remains enabled as well.

Concrete strict structs decode once from original bytes. Each floating field
borrows its original numeric token, caps it at 128 bytes, and uses Rust's
correctly rounded binary64 conversion. Non-numbers, overflow/nonfinite values
and nonzero decimals that underflow to zero are rejected. Signed zero, surviving
subnormals, halfway rounding and one-ULP distinctions are retained. No Value or
buffered tagged enum mediates the input. Sequences reject their first excess
element before deserializing it. Duplicate and unknown fields, trailing data,
unknown component/convention tags and floating spellings of integer tags/sense
are errors. The unchanged legacy decoder's floating behavior is separately pinned.

`to_json()` emits shortest decimals, and reparsing through v3 preserves all
physical and initial-condition bits. Only categorical aliases are normalized.
The schema cannot express raw token spelling/length, duplicate keys, byte/depth
limits, UTF-8 metadata byte length, exact machine arithmetic, ordered spacing,
cell products or coupled physical constraints. The shared schema/loader corpus
records structural and loader outcomes separately.

## Candidate complete identity schema 3

`ModelIdentity::for_turboprop` and `supported_turboprop()` are additive methods.
`ModelIdentity::supported()` continues to accept only jet algorithm 1/schema 2/
kind 2/law 1; turboprop cannot enter replay v4 through this change. A supported
tuple is not fingerprint equality, authentication or replay admission.

Accepted allocation for this additive interface: profile 3;
identity algorithm 1, schema 3,
kind 3, FDM law 1; turbine/propeller/governor component schema 1 and isolated
propeller convention 1. The unchanged Mach-aero component remains schema 1.
No existing version is reused with a new meaning. The byte layout
and its version meanings are fixed. The separate [replay v5 codec](replay-v5.md)
now preserves the complete engine state; this profile loader itself performs no
replay or app dispatch.

Canonical bytes are little endian and preserve validated f64 bit patterns:

1. ASCII `flightsim/model-identity\0`; algorithm/schema/kind u16; law u32
2. Neutral airframe in schema-2 order: mass; all nine stored locked inertia
   matrix entries in column-major order; wing area/span/chord; rolling/braking/
   lateral friction and transition speed; u32 count 3; ordered gear contact xyz,
   spring, damping, max stroke, bottom-stop travel and max recoil for each leg
3. Envelope: pressure min/max, temperature min/max, Mach min/max, relative-shaft
   min/max, absolute-spin min/max, maximum tip Mach and crossflow ratio
4. Length-prefixed turbine component: u16 schema; counted pressure axis, counted
   temperature axis; cell count and each idle/maximum pair; rise/fall/torque
5. Length-prefixed propeller: u16 schema and u16 convention 1; diameter/inertia;
   signed i8 sense; counted J and pitch axes; cell count and paired CT/CP
6. Length-prefixed governor: u16 schema; reference/gain/fine rate/coarse rate/
   minimum stop/maximum stop
7. Length-prefixed unchanged `MachAeroSchedule::canonical_bytes()`, including
   its existing ASCII domain, schema, counted Mach axis and all 25 fields/knot

All counts and byte lengths are u32; all physical values are f64. The three new
component payloads start directly at their schema, without an ASCII prefix.
Their identity is scoped by the outer family/schema and unambiguous lengths.
Cached rotor-subtracted inertia is derived and is not an independently authored
field. Presentation, names and `running_start` are excluded. Different initial
states of the same model therefore retain the same physical identity.

The change detector is FNV-1a64: initial `0xcbf29ce484222325`, XOR each byte then
multiply by `0x100000001b3` modulo 2⁶⁴. It can collide and is not a security hash.
The independently authored [Python witness](qa/turboprop_identity_reference.py)
produces the [1379-byte golden](../crates/flightsim-sim/tests/fixtures/turboprop-identity-v3.json)
with digest `a302a97f8e778c26` for the numerical example. Existing legacy/jet
golden bytes are kept unchanged. Tests individually perturb all 152 configurable
physical floating fields, sense and ordered data, and check the exclusions.

Validation commands and their executed outcomes are recorded in
[profile QA](qa/aircraft-profile-v3-2026-10-04.md).
