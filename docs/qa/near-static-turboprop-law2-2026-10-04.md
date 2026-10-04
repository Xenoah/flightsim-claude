# Near-static turboprop law 2: pure-FDM validation

Date: 2026-10-04 UTC. Candidate based on
`b1142919248426646592aa634231e38d23de509a`.
Status: independently accepted as an isolated pure-FDM foundation at
`d18d953dcaa9775783b0a4adb16a540698f09e5a`. No production profile, identity,
replay codec, app or preset added.

## Verified result

The original Cedar boundary remains a law-1 rejection at K2, internal substep 2,
with exact historical Linux GNU J bits for -4.8781986805208105e-6 and all
16 original state words unchanged. Explicit law 2 accepts the identical 1/120 s attempt. The fixture's
state, calm environment, full brakes, engine state and contact configuration
are unchanged. This is one-step pure-FDM evidence, not a completed calm-stop,
aircraft, native application, empirical or stationary-parking qualification.

Forward coefficient/load queries and 120 successive forward-flight steps
match the original law exactly, including every complete state word. The old
law source, all old fixtures/goldens, profiles and replay codecs are unchanged.

## Initial implementation checks

These results describe the original implementation checkpoint
`9f6a9449226bc6be5fee8d471afcbff2ad4c71f6`. The exact-fixture correction and
subsequent independent review are recorded separately below.

The provided toolchain environment was sourced, using the existing shared
target, two Cargo jobs, debug information disabled and warnings denied.
No second build cache or native/GPU process was started.

| Command | Result |
| --- | --- |
| `cargo test --offline -j2 -p flightsim-core -p flightsim-fdm` | 314 tests passed, zero failures or ignores |
| `cargo test --offline -j2 -p flightsim-world -p flightsim-sim -p flightsim-tilegen` | 878 tests passed, zero failures or ignores |
| `cargo clippy --offline -j2 -p flightsim-fdm --all-targets -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `bash scripts/check-architecture.sh` | Passed |
| `git diff --check` | Passed |

The counts include unit, integration and documentation tests. These are local
same-build results, not remote CI or cross-platform bit-identity evidence.

The initial 13 new tests cover:

- Exact Cedar old-law rejection and rollback plus unchanged-input law-2 success
- Signed negative knots, exact/adjacent-outside table and pitch edges, and
  continuity of thrust, absorbed power and torque through either zero sign
- A 7,514-point negative-cell sample against an independently evaluated static
  power floor, with minimum CP-CPh 0.0067327197054048125
- Rejection of continuation-only power below the static floor, nonpositive
  CT/CP, invalid dimensions, nonmonotone pitch feedback and nonfinite/underflow
- Exact and immediately adjacent values around both fixed 0.10 flow ratios
- Runtime adverse/transverse limit rejection while retaining old crossflow and
  envelope precedence; negative velocity underflow cannot become forward zero
- Both rotation senses, torque/power conversion, signed thrust work and
  independent scalar rotational energy and angular-momentum balances
- Actual new transverse-guard exits at Initial, K2 and K4, including substep 1
  after earlier accepted work, with bitwise whole-call rollback
- Inherited K2/K3/K4 and weighted-endpoint rejection cases in the separate law-2
  driver, raw-state/environment guards and whole-call rollback

The per-stage checks are ordinary force evaluations, not injected failures.
K1 starts from the same state already evaluated at Initial or the previous
accepted endpoint; unchanged environment/control cannot newly fail a physical
flow bound solely because that stage is labeled K1.

## Fixture and design provenance

Both FDM-only fixture files derive from the original Cedar candidate at
`be5094dd49a88d5a3709c7435dca7bfca287e7df`. The component JSON contains only the
existing six physical definitions; it does not introduce or parse a production
profile schema. The boundary JSON is copied intact:

- `cedar-law1-calm-braking-boundary.json`: SHA-256
  `a9819ee75f94dae91adeed8e63d661c9ae08b64e7e443431c23c3f8e69ac181f`
- `cedar-law1-components.json`: SHA-256
  `851847ec222304d548144184ab4b6994471f6d861e45475ed71d4dbfb81bcd75`

The supplied scientific brief, “Bounded near-static adverse-inflow propeller
extension”, has SHA-256
`2fc8900dadca88d5e3015bd7fcd54649285f601788c15616300188af921a4a96`.
Its fixed rows, static-power condition and 0.10 ratios were adopted before
qualification; no numerical limit was widened to obtain these results.
The implementation contract, copied-driver boundary and primary mechanics
references are recorded in [ADR-0020](../adr/0020-near-static-turboprop-law.md).

## Remaining gates

Independent pure-law review is complete. Complete Cedar braking/near-static
duration, release/renewed acceleration, 120/240/480/960 Hz convergence and the
full flight/contact matrix remain separate. Record every negative force-stage
ratio during that work, not only committed endpoints. Do not widen bounds if
the unchanged trajectory exits this operating set.

There is no measured negative-inflow dataset for this authored approximation.
The existing braked-idle creep and the <0.001 m/s for ten seconds stationary
parking criterion remain distinct. Production schema allocation, identity and
replay integration, native operation and publication are not claimed here.

## Exact-fixture correction and independent acceptance

The original fixture decoder used ordinary serde_json numeric conversion.
Matching the failure J and state alone did not prove all model bits matched
the production profile's exact-token reader. This test/provenance gap was
corrected in the separate test-only commit
`d18d953dcaa9775783b0a4adb16a540698f09e5a`, without changing production Rust.
The test now parses numeric lexemes with Rust's correctly rounded parser and
compares all **589 recovered component words** with an independent Python
inventory generated directly from the original production JSON.

The independent review accepted that corrected checkpoint at 19:01 UTC with
no unresolved blocking finding. It reran **1,193 candidate tests** (core 79,
FDM 236, sim 444, tilegen 175, world 259), including all **14 near-static unit
tests**. Four separate review-only sim tests bring the aggregate to **1,197**,
with zero failures or ignores. FDM all-target clippy, formatting and
architecture checks also passed.

Independent evidence includes:

- The actual unchanged production profile reader pins physical fingerprint
  `f83d082e814871c0`; law 1 reproduces the exact original K2/substep2/J rejection
  and rollback, while law 2 accepts the identical attempt in six substeps
- 720 forward steps across six trajectories, both rotation senses and three
  speed/shaft initializations preserve every state word and report
- 1,260 negative-runtime queries across both senses, all 14 pitch knots,
  three shaft speeds, three altitudes and five negative J values accept at
  transverse flow 0.09 vh; the corresponding 0.11 vh queries reject atomically
- A 70-digit Decimal audit independently confirms the 7,514-point sampled
  power margin; profile/identity gates still reject law 2
- A Git-tree audit confirms 2,205 baseline files are unchanged, including all
  old fixtures, goldens, profile/identity/replay code and Cargo.lock

The durable independent `engineering-review.md` has SHA-256
`313bdce577062190382ee51444e35e9e1f738b93911ab5b5ad2f1f96c4a2a0ef`;
its `acceptance.json` receipt has SHA-256
`d075fd33b835ebf597cab1d6bff4637fecdef469d5d6d6946d2654260022d288`.
They retain exact commands, all 85 result groups, source hashes, the independent
harness and original production-profile input. This is same-build local
foundation acceptance, with the aircraft and host gates above still open.

## Cross-platform derived-output witness correction

Windows core CI for `fc9750d701fc0508dc4b374c4903920a0fa35e91`
([run 37234646790, job 111531224096](https://github.com/Xenoah/flightsim-claude/actions/runs/37234646790/job/111531224096))
passed 130 of 131 FDM unit tests, including all 589 recovered component bits.
The one failure was the historical Linux output assertion after the expected
K2/substep-2 `OutsidePropellerDomain(Below, Within)` rejection already passed:

- Linux reference J: `bed475ed0aff54c1` (-4.8781986805208105e-6)
- Windows observed J: `bed475ed05f3e017` (-4.878198608830617e-6)
- Absolute difference: approximately 7.16902e-14

This is a derived trajectory value, not a fixture input token. Contact forces
use ECEF-to-geodetic altitude, trigonometric local frames and near-zero friction;
those intermediate calculations are not promised bit-identical across system
math implementations. The log establishes platform-dependent derived arithmetic,
but does not isolate one particular library function as the cause. No JSON
newline, float decoder, production law or tolerance is changed.

The test-only witness now reconstructs two complete law-1 RK4 substeps and the
third substep's K2 state using the public derivative, turbine and governor APIs.
It holds one governor sample per substep, applies the analytic turbine response,
normalizes each offset quaternion, and evaluates each accepted endpoint. Its
substep size is exactly `(1/120)/6`, independently pinned against the accepted
law-2 report. It uses the caller's unchanged environment at every evaluation,
including the distinction between moving local ground and a held ground plane.
It calls neither `step` nor the private integrator and never reads law-2 output
to construct the reference. This is an independent integration/diagnostic
witness using shared law-1 force functions, not an independent physical model.

On every platform, independently computed signed J must be finite and strictly
negative and must match the failed full-step diagnostic bit for bit. Direct
law-1 evaluation of that reconstructed K2 state must have the identical domain
reason and all nine optional diagnostic words, preserving absence and signed
zero. Stage K2/substep 2, all 16 rollback words, component inventory and original
physical input identity remain exact. Law 2 must accept the original attempt in
six substeps. The historical output bit pins remain additional regressions on
x86_64 Linux GNU. Windows receives the same independent witness, with no widened
numerical tolerance and no ignored test.

The same correction covers the qualification helper, the replay-v6 bare-FDM
boundary, and its held-plane law-1/v5 witness. The held-plane latitude/longitude
reference is similarly scoped to Linux GNU while its exact same-runtime
geodetic, host, direct-FDM and replay identity comparisons run everywhere. Other
new exact comparisons were inspected: they compare source identity, same-runtime
trajectories/diagnostics, authored domain edges or algebraic endpoint values.
The existing negative-flow limits and their adjacent-outside rejection tests
are unchanged. All original fixtures, recorded evidence and v3/v4/v5/v6 codecs
remain unchanged.

Local validation of this correction (Linux GNU, Rust 1.93.0, existing shared
build target, offline, `-j2`, warnings denied):

- All 236 FDM tests passed, including the 14 near-static unit tests
- All 3 Cedar qualification tests and 28 near-static v6 replay tests passed
- All 14 profile-v4, 11 v6 codec and 8 near-static identity tests passed
- FDM and sim `cargo clippy --all-targets -- -D warnings` passed
- `cargo fmt --all --check`, architecture checks and `git diff --check` passed

These are local Linux results. A new Windows CI run is still required before
claiming that the corrected cross-platform assertions pass on Windows.
