# Profile v3 and schema-3 identity boundary QA

Date: 2026-10-04 UTC. Status: local profile/identity validation passed; final
independent review and integration acceptance remain separate.

Base: `b38f41e94b6231037c54c538a5b99c33b19ca7dc`. Pure FDM dependency:
`cbe064f973457953910e6f59edfac94308edb684` plus qualified correction
`7fdf76ad138431050a1daf9136ae0412754dd3dd`, cherry-picked without modifying the
foundation sources. The correction checks the actual matrix determinant and
inverse before the assertion-based mass constructor. This report covers the
additive sim profile/identity layer only; foundation evidence is in its
[separate report](turboprop-fdm-foundation-2026-10-04.md).

## Executed validation

- Python v3 shared schema corpus: 120 cases pass their independently specified
  structural outcomes; the actual Rust loader passes all 120 separately specified
  loader outcomes, including intentional schema/loader differences
- Existing Python v1 and v2 schema suites pass unchanged
- Independent schema-3 Python witness: exactly 1379 bytes, FNV-1a64
  `a302a97f8e778c26`, matches committed golden
- Existing independent schema-2 witness: exactly 1095 bytes, FNV-1a64
  `9493dfe3f9ba6f76`, matches its unchanged golden
- Independent numerical fixture sampling: 17×17 points in each of four propeller
  cells, 1156 queries. Minimum sampled `CP-CPideal` is `0.03921045903391185` at
  `J=2`, pitch `0.12 rad`, `CT=.05`, `CP=.14`, `CPideal=.10078954096608816`
- 18 new Rust tests pass: 12 profile tests and 6 complete-identity tests
- Broad pure core/FDM/world/sim/tilegen regression: 1,152 tests/doctests pass in
  82 test results, with zero failures; includes unchanged legacy/jet numeric and
  byte fixtures and the new profile/identity tests
- Strict sim all-target Clippy and sim Rustdoc pass with warnings denied
- Workspace formatting, architecture dependency policy and `git diff --check` pass
- All 22 baseline sim fixture files remain byte-identical to `b38f41e`

Reproduction for the independent checks:

```sh
python3 schemas/tests/test_aircraft_profile_v3_schema.py
python3 schemas/tests/test_aircraft_profile_v2_schema.py
python3 schemas/tests/test_aircraft_profile_schema.py
python3 docs/qa/turboprop_identity_reference.py
python3 docs/qa/jet_identity_reference.py
python3 docs/qa/turboprop_profile_reference.py
```

## Rust reproduction

Checks ran in the serialized Cargo lane with two jobs, the shared pure build
target, warnings denied and debug information/incremental compilation disabled.
The commands were:

```sh
cargo test -j2 -p flightsim-sim --test aircraft_profile_v3 --test turboprop_model_identity
cargo test -j2 -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen
cargo clippy -j2 -p flightsim-sim --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -j2 -p flightsim-sim --no-deps
cargo fmt --all -- --check
bash scripts/check-architecture.sh
```

The new Rust tests check the corpus's separate loader outcomes,
exact physical/metadata/initial-state bits, export/reparse, 1 MiB/128-byte/depth
bounds, early rejection of hostile excess array elements, maximum table shapes,
fallible physical rejection, all 152 independently configurable physical floats,
rotation sense and ordered cells/gear, nonphysical exclusions, exhaustive golden
bytes and explicit rejection of schema-3 identity by the actual old v4 reader.
The original-token near-singular inertia regression explicitly exercises both
LF and CRLF source forms, asserts both intended substitutions occurred and
requires a normal error without unwinding. It catches the determinant
reassociation failure at the public untrusted-JSON boundary. Broad tests and
Clippy were rerun after this portability fix.

An admitted-map/interior-disk-failure fixture exercises the distinction between
profile construction and actual query support. Old legacy and jet fixture files
are not edited; their regression suites passed in the broad run.

## Scope limits

The numerical example is original synthetic authoring data. Neither its profile
admission nor the sampled margin certifies the continuous nonlinear disk bound,
governor stability, full flight envelope, a named aircraft or measured performance.
Actual support intersects atmospheric/shaft boxes with J, forward axial flow,
pitch and tip/crossflow limits. Parked tailwind and backward taxi can be unsupported.
`running_start` is structurally checked, explicitly retained and excluded from
physical identity; it is not an equilibrium and no hidden warmup is performed.

No app preset, new family selection, sim stepping/restart, replay v5 codec,
native/Bevy/GPU/audio, asset, desktop, release or publication work is included.
The physical foundation and any later host integration retain separate acceptance.
