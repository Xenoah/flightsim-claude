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
