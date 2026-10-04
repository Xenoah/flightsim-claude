# Aircraft profile v1 public-schema verification

## Scope

The public [schema](../../schemas/aircraft-profile-v1.schema.json) and
[authoring guide](../aircraft-profiles.md) document the existing v1 boundary from
source baseline `4177daf`. The only production-source-file edit is a `cfg(test)`
module declaration in `main.rs`; the original `aircraft_profile.rs` remains
byte-identical to the baseline. No parser, defaults, built-in
profiles, model transforms, flight physics, replay code or Rust dependency
manifest changes are included. Integration adds the schema checker to the
existing source-boundary CI job, using the tested jsonschema 4.26.0.

The two original, fictional JSON examples describe one documentation-only design
with inherited versus explicitly equivalent pitch rates. Neither is registered
as a built-in; no model file is supplied and no flight-quality claim is made.

## Checks

Passed on Linux, using Rust 1.93.0 and the existing shared toolchain:

- `python3 schemas/tests/test_aircraft_profile_schema.py`: Draft 2020-12 schema
  self-validation, 89 acceptance/rejection recipes, both examples, both unchanged
  built-in JSON profiles; two Python test methods pass
- `cargo test -j 2 -p flightsim-app aircraft_profile`: 12 tests pass, including the
  existing takeoff/approach regressions and both relocated schema-contract tests
  with all 89 corpus cases, after the final registration/loader changes
- `cargo clippy -j 2 -p flightsim-app --all-targets -- -D warnings`: pass
- `cargo fmt --all --check`: pass
- `bash scripts/check-architecture.sh`: pass with the project toolchain loaded
- `git diff --check`: pass; relative authoring-guide links resolve

The corpus is shared between an actual Draft 2020-12 implementation (`jsonschema`,
already present in the development environment) and the production
`AircraftProfile::load` file path. It exercises actual accepted and rejected
files rather than comparing two manually transcribed lists of Rust rules.
Explicitly different expectations record inertia, UTF-8 path-byte length, total
file bytes, duplicate keys and integer-token spelling. Invalid NaN/Infinity tokens
are rejected at the JSON decoding boundary; numeric overflow is also covered.
Both documented examples load through `AircraftProfile::load`, including its
file-size and UTF-8 checks. Required-only and explicit-control examples produce
identical pitch inputs over both pressed and released rate-mode controls for parked and approach starts.

The Python tool is optional for local development and pinned in CI. Rust
contract coverage runs within the existing app test target. The checks recorded
here are focused validation, not a new whole-workspace or graphics test.

## Limits

JSON Schema cannot replace Rust finite-machine-number conversion, serialized byte
limits, the positive-definite inertia condition or filesystem/GLB/simulation
checks. Its scalar bounds and sign rules do not establish physically plausible
combinations. No downloaded model, GPU/native-hardware test, new aircraft package
importer, performance benchmark, licensing determination or publication is part
of this milestone. The two examples have not been flight-tuned.

Standard glTF +Y up/+Z forward is cited in the guide. Legacy Light Single (-X),
Swift Sport (+X) and default model override (-Z) adapters remain unchanged.
