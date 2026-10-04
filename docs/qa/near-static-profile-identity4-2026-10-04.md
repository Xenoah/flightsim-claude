# Profile v4 and complete near-static identity v4: implementation QA

Date: 2026-10-04 UTC. This is an additive pure profile/identity implementation
checkpoint, pending independent review. No app, picker, host, replay format,
preset or FDM numerical behavior was changed.

## Source and compatibility

Base: `067005235b13ce2a212acb398a685712598b120b`, incorporating independent
acceptance of the isolated law-2 foundation. Profile 4, physical identity 4,
propeller wrapper 2 and ADR-0022 were centrally allocated after an unused-number
check. Replay 6 remains reserved and unimplemented; replay 5 remains schema 3 /
kind 3 / law 1 with all its old masks, errors and terminal gates.

The source audit checks every base Git blob: **2,214 baseline files unchanged**.
The four changed existing files are ARCHITECTURE, sim module exports and the
two v3 DTO/module files. The latter changes are exclusively crate-private
visibility for reuse; their parser/numeric behavior is unchanged. Every old
golden, identity implementation, profile-1/2 loader, FDM and replay source is
unchanged. Added source blobs are all below 200 KiB. The receipt records each
added file's SHA-256 and the complete unchanged-file inventory outside Git.

## Checks and outcomes

- **1,216 pure Rust tests passed**, zero failures/ignores across 86 result
  groups: the 1,193 accepted baseline tests plus 23 additions
- Fourteen new profile tests include a **168-case shared schema/loader corpus**,
  one-MiB load/bytes/string boundaries, depth, duplicate/unknown keys, numeric
  tokens, signed zero, subnormals, underflow, maximum 32-by-32 combined map,
  the adjacent 33-knot combined rejection, and export/reparse
- Eight new identity tests pin every independent golden byte; perturb all
  **164 configurable physical scalars** (including both yaw_rate_p knots and
  all 12 negative CT/CP scalars); check sense/order and metadata/start exclusions;
  exercise actual v4/v5 readers' rejection and original writer roundtrips
- One new private encoder unit test proves all **five fixed scalar commitments
  and three semantic tags** alter canonical bytes/digest. The production parser
  independently rejects changed declarations; this does not expose a tunable law
- All five Python profile schema suites passed, including the shared v4 corpus
- Independent old/new Python identity references passed: old 1,379-byte
  `a302a97f8e778c26`; new 1,582-byte `f3bb440d4797f5c4`
- Sim all-target clippy with warnings denied, full-workspace formatting,
  architecture checks, diff checks and sim public documentation with rustdoc
  warnings denied passed

All initial Rust tests passed. A final added corpus case distinguishes an
individually valid 31-knot forward map from an unsupported 33-knot combined
map; the full pure suite and schemas were rerun afterward. No physical limits,
controls, gear, start state, law-1 bytes or old support gates were loosened.

The runtime distinction is exercised directly: a parsed profile accepts a small
negative-flow state at zero duration and rejects a state with excessive actual
transverse/vh while its J and old crossflow/tip limit remain supported. Separate
coverage retains the prior forward-map example whose structural/nodal admission
does not certify an interior pointwise ideal-disk bound.

## Exactness and provenance

The full retained forward canonical bytes are nested, length-delimited, under
the new outer schema-4/law-2 header. They identify retained physical
configuration, not the runtime law; no short old fingerprint is substituted.
The extension includes the new negative pairs, fixed knots, domain scalars and
closed semantic tags. See [the byte contract](../aircraft-profile-v4.md) and
[ADR-0022](../adr/0022-near-static-profile-and-identity.md).

The independent Python reference reads original JSON decimal tokens and uses
Python binary64 plus explicit little-endian packing. Rust reads original
borrowed numeric tokens directly through its correctly rounded parser. Tests
cover exact decimal values, signed zero, representable subnormals, underflow,
LF/CRLF and shortest-decimal roundtrips against the same golden. These portable
encoding checks were run locally on Linux; Windows/macOS execution, remote CI
and cross-platform numerical/libm trajectory equality were not established.

The v4 example retains the original numerical v3 physical component exactly,
then adds six simple authored pairs above its static row. No Cedar profile or
preset is registered. It is synthetic data, not empirical reverse-flow evidence,
a performance rating, a startup/parking solution or aircraft qualification.

Source JSON SHA-256:

- Retained v3 example: `7c5a091eb33821b8cbd48d7be8881d21fac02e85773b5457e96431eb7ebbac9f`
- New v4 example: `d53f6e59cf823a6ef3bbc87cf10ccfe077c433a23b1e404e06bfd26d6cc8b950`
- New independent golden: `9cae1fea4fb03fdde8cc4262a9fdfa060bd38013cd6b7c87eb8f96a67a7d858c`

## Reproduction and receipts

Use the existing shared target and provided build-env; Rust/Cargo 1.93.0,
`RUSTFLAGS=-D warnings`, no debug info or incremental cache, and two jobs:

```sh
cargo test --offline -j2 -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen
cargo clippy --offline -j2 -p flightsim-sim --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/check-architecture.sh
RUSTDOCFLAGS='-D warnings' cargo doc --offline -j2 -p flightsim-sim --no-deps
python3 -m unittest discover -s schemas/tests -p 'test_aircraft_profile*schema.py'
python3 docs/qa/turboprop_identity_reference.py
python3 docs/qa/near_static_turboprop_identity_reference.py
git diff --check
```

The external `nearstatic-profile-identity4` QA bundle contains exact commands,
logs, test counts, source inventory and SHA-256 receipt bound to the clean Git
checkpoint. Its archive is verified by reading every member against that
inventory. No second Cargo cache, benchmark, native/GPU process, remote CI,
publication or source/QA/immutable-binary cleanup was performed for this task.
Independent engineering review and all later host/replay/native/preset gates
remain separate.
