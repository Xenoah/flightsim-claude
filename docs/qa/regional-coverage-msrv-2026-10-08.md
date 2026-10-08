# Regional coverage MSRV correction, 2026-10-08

The coverage directory scanner added in ADR-0028 used a let-chain that requires
Rust 1.88. This regressed the Rust 1.85 normal-library boundary retained by
[ADR-0007](../adr/0007-bevy-version.md), despite passing the ordinary 1.93 builds.

Against published `7043e8ce794fab47b2fa30a64e2f9deea27e1fc0`, the runtime change
replaces that conditional with an ordinary depth/file guard and a let-else.
Malformed canonical paths still skip exactly one directory entry; no trailing
work is skipped. Counting, overflow rejection, symlink exclusion, payload-free
discovery and all source/selection/streaming limits are unchanged. No dependency,
Cargo input, compiler declaration or lint allowance changes.

A minimal Linux CI lane builds only `flightsim-core` and `flightsim-world` normal
libraries with Rust 1.85.0 and the locked dependencies. Existing Rust 1.93 tests,
bench compilation and all release checks remain required. This does not extend
the 1.85 claim to Criterion dev targets or the Bevy application.

## Fresh local checks

- Official Rust 1.85.0 (`4d91de4e4`), normal-library command:
  `cargo build --locked -j 2 -p flightsim-core -p flightsim-world --lib`.
  Published 7043e8ce failed with E0658 at the let-chain in `coverage.rs:99`.
  The corrected source passed with the same lockfile and warnings denied.
- Rust 1.93.0: `cargo test --locked -j 2 -p flightsim-core -p flightsim-world -p flightsim-content --features flightsim-content/downloads --all-targets`
  passed 407 tests, with no failed or ignored tests; benchmark smoke targets passed.
- The same 1.93 crate/feature/target set passed Clippy with `-D warnings`.
  Workspace formatting and architecture checks passed.
- Independent review found the conditional behavior equivalent. Official
  compiler components were verified against their distribution manifest hashes.

## Evidence identity and remaining gates

This is a new runtime revision. The recovered source tree and native captures in
[the earlier coverage binding record](regional-coverage-bindings-2026-10-08.md)
remain historical evidence for their original source; the new source is not
byte-identical and no fresh native capture is claimed here. The two surviving
owner-retained comparison stills do not attest this revision.

The affected whole-file coverage pin and dependent contract bindings require a
separate reviewed migration against a verified remote runtime commit. No source
contract is automatically refreshed. All independent anchors, package replay
refusal and nine binary-release blockers remain authoritative. This correction
and its MSRV build grant no native-device, performance, licence or release approval.

## Reviewed binding migration

The verified remote runtime correction is
`b7efc0aa8d16945496e470734c52acb8ca7ee139`, tree
`6c751cbbad81c5be997560e97b1405629756abd9`, parent
`7043e8ce794fab47b2fa30a64e2f9deea27e1fc0`. The separate binding revision
changes exactly the whole-file `coverage.rs` hash in the 160-path replay boundary;
all other 159 pins and all 102 independent anchors are retained. The replay and
analytical reviewed-source pointers use that verified remote runtime commit.
Only the analytical replay-contract digest, capture inherited-contract literals
and capture runner digest change transitively. The original capture provenance
`75753f5e1dfe8d56d321775cb800b29bf058b3e3` remains unchanged.

No validator logic, test, mutation witness, source path set, release workflow or
rights/admission gate changes. The ordinary CI workflow only gains the additive
MSRV lane described above; it is outside the frozen source-contract path sets.
Strict core/world/content documentation checks and the existing four CI smoke
workflow tests also passed; the resulting CI YAML parses with the intended
locked normal-library command.

The complete `scripts/tests` suite ran 318 tests in 119.073 seconds: 317 passed
and the existing Windows-only junction test was skipped. The unchanged regional
mutation tests still include 17 committed bad-source cases and 44 export attacks.
Clean-source and release-preflight receipts are retained separately against the
final clean commit, so creating those receipts cannot modify their source identity.
