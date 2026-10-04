# Aircraft profile v4: explicit near-static turboprop law 2

`flightsim_sim::aircraft_profile_v4::AircraftProfileV4::{parse,from_bytes,load}`
loads the [schema](../schemas/aircraft-profile-v4.schema.json) and constructs the
accepted pure `flightsim_fdm::turboprop::near_static::TurbopropAircraftConfig`.
It is an explicit API, with no app/host/replay dispatch or preset registration.
`configuration`, `running_start`, `model`, `controls`, `camera_eye`, `engine_sound`
and `id` expose the validated values. `to_json` preserves their binary64 bits.

Use the [complete numerical example](examples/aircraft-profiles-v4/numerical-near-static-turboprop.json).
Its model path is intentionally unprovided and is never opened by this loader.
All outer fields are required. The outer version is integer 4; dynamics kind is
`running_turboprop_table`, integer revision 2. The old airframe, turbine,
governor, aero, envelope and explicit `running_start` forms are described in
[profile v3](aircraft-profile-v3.md). A v3 document does not select this law.

## Propeller schema 2

`propeller` requires exactly `schema`, `forward`, `negative_advance_ratio`,
`negative_rows` and `near_static_domain`. `schema` is integer 2. `forward` is
the complete unchanged schema-1 propeller definition, including convention,
diameter, rotor inertia, rotation sense, J and pitch axes and paired CT/CP cells.
It must have at most 30 forward J knots; its first J is either exact zero sign.
It shares the exact J=0 row and pitch axis with the negative extension.

`negative_advance_ratio` contains exactly [-0.01,-0.005]. `negative_rows` has
exactly two arrays in that order, each with one `{ct,cp}` pair per forward pitch
knot, pitch fastest. Pairs must be positive and at most 2, with nondecreasing CP
as pitch increases. Both rows and the shared J=0 closure must satisfy the
accepted static-power floor, including its finite/nonzero derived-scale guard.
No rows are generated at runtime or silently extrapolated.

`near_static_domain` requires these exact supported commitments:

| Key | Supported value |
| --- | --- |
| maximum_adverse_inflow_ratio | 0.10 |
| maximum_transverse_inflow_ratio | 0.10 |
| static_power_epsilon_multiplier | 64.0 |
| interpolation | signed_pitch_then_advance_ratio |
| static_power_bound | positive_static_actuator_disk_floor |
| inflow_scale | current_positive_thrust_hover_velocity |

These are fixed law declarations, not author-tunable limits. Static floor is
sqrt(2/pi) CT^(3/2), allowing 64 epsilon max(abs(CP),abs(floor)) roundoff in its
comparison. The ratio scale is vh=sqrt(T/(2 rho A)), A=pi D²/4, from current
positive dimensional thrust. Negative J also requires -Va/vh <=0.10 and
hypot(Vy,Vz)/vh <=0.10. Nonfinite/underflowed scales reject. Forward queries,
including both zero signs, retain law-1 arithmetic. See [ADR-0020](adr/0020-near-static-turboprop-law.md)
for the pointwise force and integration contract and scientific limitations.

## Three distinct admission gates

1. JSON Schema checks objects, required keys, nominal number ranges, tags and
   array bounds. It cannot prove physical inequalities, exact token grammar,
   duplicate absence, source byte limits or coupled shape/positive-definiteness.
2. The production loader parses original UTF-8 bytes (1 MiB), checks depth 128,
   exact physical/metadata numeric tokens (128 bytes), finite binary64, nonzero
   underflow rejection and signed-zero/subnormal preservation; constructs all
   physical components and verifies coupled bounds, row dimensions and floor.
3. The FDM checks actual body/engine state, atmosphere, wind, operating envelope,
   negative-flow ratios and every integration stage. A valid profile or explicit
   running-start tuple is not an equilibrium or a promise of supported flight.

Fixed scalar declarations compare correctly rounded bits. Different spellings
with the same supported binary64 value are accepted; an exact-decimal schema
validator may disagree about an adjacent decimal spelling. Integer tags still
require integer tokens (e.g. 4, not 4.0 or 4e0). Unknown or duplicate keys reject
at every level. `load` reads at most 1 MiB+1 and never loads referenced assets.
Negative-row subnormal CT may be syntactically valid but rejected when its
derived static floor underflows. Initial values and metadata are preserved
separately and excluded from physical identity.

## Identity schema 4 byte contract

`ModelIdentity::for_near_static_turboprop` selects algorithm 1 / schema 4 / kind 3 /
law 2. `canonical_near_static_turboprop_bytes` emits:

| Order | Type and content |
| --- | --- |
| 1 | Bytes `flightsim/model-identity` followed by NUL |
| 2 | u16 algorithm=1, u16 schema=4, u16 kind=3, u32 law=2 |
| 3 | u32 byte length; COMPLETE schema-3 forward canonical byte sequence |
| 4 | u32 byte length; schema-2 extension below |

The nested schema-3/law-1 header binds retained forward physical configuration;
the outer schema-4/law-2 header selects the runtime identity. The nested bytes
contain every physical input, not a short old fingerprint. See
[v3 identity order](aircraft-profile-v3.md). No new public gate uses the nested
header to admit a recording. All integers and raw f64 words are little endian.

| Extension order | Type and content |
| --- | --- |
| 1 | u16 component schema=2 |
| 2 | u16 interpolation=1, u16 static-power-bound=1, u16 inflow-scale=1 |
| 3 | u32 knot count=2, f64 -0.01, f64 -0.005 |
| 4 | u32 row count=2 |
| 5 | For each row: u32 pair count, followed by f64 CT then f64 CP for each pitch |
| 6 | f64 adverse maximum=0.10, f64 transverse maximum=0.10, f64 epsilon multiplier=64.0 |

The three semantic tag 1 values correspond to the three exact strings above.
Unknown tags have no supported meaning. Signed zeros in retained components
remain distinct bits. FNV-1a64 (offset cbf29ce484222325, multiplier 100000001b3,
modulo 2^64) hashes the whole canonical sequence; it is not authentication.
The example is 1,582 bytes with fingerprint `f3bb440d4797f5c4`; its complete
retained forward component is 1,379 bytes and preserves old fingerprint
`a302a97f8e778c26` when independently hashed as schema 3.

The separate `supported_near_static_turboprop` checks only the outer tuple.
It is not fingerprint equality, replay admission or runtime support. Old jet
and law-1 turboprop helpers still reject this tuple. Replay v5 stays law 1.
The separate [near-static host and replay v6](replay-v6.md) use this complete
identity for explicit law-2 reproduction. No app dispatcher or preset follows.

## Provenance and portable checks

The sample retains all physical values from the original v3 numerical fixture.
Its six additional positive pairs were authored as simple small increments
above that fixture's J=0 row, with a margin above the static-power floor. They
are explicit test data, not measured, fitted or copied aircraft performance.
No Cedar preset, startup/parking solution or negative-flow fidelity is claimed.

`docs/qa/near_static_turboprop_identity_reference.py` independently encodes the
JSON using Python binary64 and explicit little-endian packing, retaining the
independent v3 reference encoder for the full forward component. Rust tests
compare every byte, perturb 164 configurable physical scalars and check all
five fixed scalars and three semantic tags. Corpus cases distinguish schema,
physical-loader and runtime-only boundaries. LF and CRLF, exact rounding,
export/reparse, signed zero, subnormal and nonzero underflow cases are portable
checks. Local passing tests alone do not claim another OS was run, nor that
floating-point trajectories/libm agree across platforms.
