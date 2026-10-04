# Exact-numeric aircraft profile v2

`flightsim_sim::aircraft_profile::AircraftProfileV2` is an additive pure Rust
loader for explicit `dry_jet_table` dynamics revision 1. It validates local data
and returns immutable `JetAircraftConfig` plus presentation/input metadata.
It performs no network access or GLB load and does not make a jet selectable in
the current app. Profile v1, its 128 KiB limit and ordinary numeric decoder,
legacy FDM revision 2 and replay v1–v3 APIs/identities are unchanged.

The public authoring schema is
[`schemas/aircraft-profile-v2.schema.json`](../schemas/aircraft-profile-v2.schema.json).
The [numerical example](examples/aircraft-profiles-v2/numerical-jet.json) is an
original synthetic boundary fixture, not a real aircraft or validated handling
model. Its `aircraft/unprovided_numerical_jet_fixture.glb` path is deliberately
unprovided: parser/headless validation must not read it. It supplies no jet
exterior, model rights, asset package or performance evidence.

## Explicit structure

The required outer keys are `version`, `id`, `dynamics`, `model`, `controls`,
`camera_eye_m` and `engine_sound`. `version` is the integer token `2`.
`dynamics` has exactly these required fields:

- `kind`: the case-sensitive string `dry_jet_table`
- `revision`: the integer token `1`; this is the physical model revision
- `airframe`: neutral name, mass, full inertia, geometry and three fixed gear
  definitions, plus friction/transition fields, with no propeller fields
- `thrust`: `{schema: 1, pressure_ratios, temperature_ratios, mach, cells}`
- `aero`: `{schema: 1, knots: [{mach, aero: {all 25 coefficients}}]}`
- `envelope`: `{pressure_ratio: [min,max], temperature_ratio: [min,max],
  mach: [min,max]}`

Component schema, physical revision, profile version and replay version are
separate identifiers. A display name, turbine sound or model path never selects
a dynamics law. Unknown fields/tags/revisions and duplicate keys at every
object level are errors. Integer tags must use integer notation; `2.0`/`2e0`
are rejected even though JSON Schema considers them integral.

SI field names and the exact table semantics are inherited from
[the component contract](subsonic-foundation.md) and
[the runtime contract](subsonic-runtime.md). Pressure and temperature ratios are
ambient **static** values. Thrust is installed aggregate signed **net** thrust;
zero command means authored idle. Cells flatten pressure/temperature/Mach, Mach
fastest. All 25 aero coefficients are supplied at every knot, without an
additional stability/control sign restriction. Numerical acceptance cannot
establish trim, stability, controllability, engine performance or flight safety.

The neutral airframe keeps the legacy structural numerical ranges without
constructing a dummy propeller. Envelope endpoints are inclusive and increasing
by at least `1e-9`; pressure lies in 0–2, temperature in 0.25–2 and Mach in 0–0.95.
The entire envelope must lie inside the thrust axes and aerodynamic schedule.
Every component endpoint passes the strict physical constructor before a profile
is returned. There is no clamp or fallback force.

## Exact numeric and resource boundary

All v2 physical **and metadata** floating-point fields use `ExactF64`, whose
field is private. The loader takes the original bounded UTF-8 string or bytes:

1. Reject more than 1 MiB, including whitespace, before JSON decoding. File
   loading reads at most the limit plus one byte.
2. A string/escape-aware, allocation-free preflight caps container depth at 128.
   The ordinary serde_json recursion guard also remains enabled. This explicit
   preflight matters because RawValue's iterative subtree scanner does not charge
   the normal recursion counter for a wrongly typed object/array numeric value.
3. Deserialize concrete strict structs directly, without `Value`, `from_value`,
   internally tagged/untagged buffering or owned per-number raw copies.
4. Borrow each original `&RawValue`; cap its token at 128 bytes before converting
   with Rust's `str::parse::<f64>()`. Reject non-numbers, nonfinite/overflow and
   nonzero decimal values that round down to binary64 zero. Preserve signed zero,
   correctly rounded halfway cases and representable subnormals.
5. Bound each sequence while reading, without trusting a size hint: at most 32
   knots per thrust axis/aero schedule, 4096 cells and three gear contacts. The
   first excess element is rejected before it is deserialized. Constructors
   subsequently require at least two knots, exactly three gear contacts and an
   exact bounded dimension product.
6. Reject trailing non-whitespace data, validate metadata, construct the physical
   components and check envelope containment.

These are bounded parser/data policies, not measured process-memory or frame-time
claims. The original file is held once; DTO and validated runtime copies are each
bounded by their declared limits. The borrowed raw slice is never an allocation
of the whole numeric subtree. Malformed syntax still belongs to the JSON parser.

Only serde_json's `raw_value` feature is added. Neither `float_roundtrip`,
`arbitrary_precision` nor `unbounded_depth` is enabled. Locked upstream
[Cargo.toml](https://raw.githubusercontent.com/serde-rs/json/v1.0.151/Cargo.toml),
[RawValue](https://raw.githubusercontent.com/serde-rs/json/v1.0.151/src/raw.rs) and
[decoder](https://raw.githubusercontent.com/serde-rs/json/v1.0.151/src/de.rs)
confirm independent raw-value and ordinary-f64 paths. Cargo feature unification
still makes executable legacy regression checks necessary.

`to_json()` emits finite shortest decimal numbers. Reparse with this loader to
retain actual constructed physical bits, including negative zero. Different
spellings such as `1`, `1.0` and `1e0` denote the same floating field bits; spelling
and whitespace are not model identity. The legacy default parser changes the
example `-30.400000000000002` by one ULP; this new decoder preserves
`0xc03e666666666667` while ordinary serde_json retains legacy
`0xc03e666666666666`. Any model identity must consume validated constructed
physical bits, not the original JSON text.

## Metadata and authoring limits

Metadata names/ranges retain v1's familiar semantics: lowercase ASCII ID (1–48
bytes), a relative lower-case `.glb` path (at most 256 UTF-8 bytes), explicit
perpendicular model axes, visible length 1–100 m, camera components ±10 m,
control rates 0.01–10, centering 0–10, trim ±1, approach speed 10–150 m/s,
approach pitch ±π/4 and throttle/flaps 0–1. No wider jet approach-speed range is
silently introduced. Missing/null pitch-specific rates inherit the general rate.
Model axes and sound categories accept the documented v1 aliases and normalize
them to signed lowercase axes and `piston`/`turbine` after validation. This keeps
export size bounded even when input aliases contain near-limit whitespace. Sound
remains presentation only. Return values expose metadata immutably; physical unit types
are supplied by the FDM configuration and `camera_eye()`.

Path checking is lexical only. It rejects absolute paths, backslashes, colons and
parent components. It does not resolve symlinks, inspect GLB content, enforce an
asset-root filesystem sandbox or grant redistribution rights. Lexically accepted
paths can still contain NUL, reserved names or other filenames that a particular
OS cannot load; this is not a cross-platform filename-validity guarantee. Model loading,
package confinement and rights review belong to the later app/MOD boundary.
No declared provenance or successful schema validation establishes those rights.

The JSON Schema validates the parsed data model. It cannot express duplicate-key
rejection, raw number spelling/length, serialized byte caps, UTF-8 path byte
length, correctly rounded machine arithmetic, inertia determinant, axis order,
cell products/monotonicity/vacuum or cross-component envelope containment.
The shared corpus records schema and runtime outcomes separately for those
intentional differences; a schema pass is never runtime acceptance.

## Verification

The shared Python/Rust corpus covers shape, required/unknown/duplicate fields,
tag revisions, metadata aliases, physical ranges, bounded arrays, the 1 MiB edge,
number-token bounds and runtime-only cross-field rejections. Focused numeric tests
pin independent binary64 patterns for halfway rounding, exponents, signed zero,
subnormal/normal boundaries, overflow, underflow and invalid JSON tokens. Export
checks compare actual runtime airframe/envelope values and complete component
bytes and check every metadata floating field. Hostile sequences prove that the
first excess element is rejected before its malformed contents are parsed.

Run these in the repository's serialized Cargo lane:

```sh
cargo test -j2 -p flightsim-core -p flightsim-fdm -p flightsim-sim
cargo clippy -j2 -p flightsim-sim --all-targets -- -D warnings
python3 schemas/tests/test_aircraft_profile_v2_schema.py
python3 schemas/tests/test_aircraft_profile_schema.py
```

The executed checks and legacy before/after comparison are recorded in
[profile boundary QA](qa/aircraft-profile-v2-2026-10-04.md).

This profile foundation has no native/GPU acceptance, real aircraft performance
validation, asset importer, app selector or new replay codec. Those integrations
retain their separate review gates.
