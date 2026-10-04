# Exact aircraft profile v2 boundary QA (2026-10-04)

Scope: additive pure `flightsim-sim::aircraft_profile` loader/schema on component
foundation `b609a94` and reviewed runtime `0f1b2b0` (identical runtime cherry-pick
`d240247` in this worktree). Legacy app v1/profile assets, FDM revision 2, replay
v1–v3 codecs/identities, release candidate pins and publication are untouched.

## Executed checks

- Five new decoder unit tests: original decimal to independent binary64 patterns,
  halfway tie/neighbour values, signed zero, exponent spelling, normal/subnormal
  endpoints, overflow/nonzero-underflow, wrong types, malformed tokens, 128-byte
  numeric-token bound, depth preflight strings/escapes and pre-element array caps
- Ten public-loader integration tests, including the shared 62-case schema corpus,
  exact current physical export/reparse, all 25 aero coefficient fields, every
  metadata float, signed-zero and subnormal runtime construction, all entry-point
  byte limits, nested duplicates, trailing data and explicit deep-nesting error
- Near-1 MiB inputs independently padded through each axis/sound alias; canonical
  categorical spelling keeps exports reloadable without changing numeric bits
- Maximum 4096-cell/32-aero-knot profile loads, exports below 1 MiB and reparses to
  identical component bytes
- Strict sim all-target Clippy, workspace format check, sim rustdoc with warnings
  denied, architecture script and both Python v1/v2 schema checks passed
- Twelve actual app `aircraft_profile`/schema-contract tests passed under the new
  shared feature graph, including v1's 128 KiB boundary, builtin JSON→physical
  identity, unchanged optional metadata defaults, takeoff and approach regression

The final core/FDM/sim aggregate passed **615 tests including doctests** after
the export normalization and complete final test additions. No GPU, native
interaction, actual jet handling or asset-import validation is claimed.

## Independent legacy numeric comparison

An isolated Cargo package with locked `serde_json 1.0.151` and only its default
and std features, outside the workspace, decoded the problematic decimal as
`0xc03e666666666666`. The feature-unified sim test gets the same legacy bits while
`ExactF64` retains `0xc03e666666666667`.

The same isolated baseline traversed every numeric value in each unchanged v1
JSON asset, appending f64 little-endian bits and object keys in sorted map order
to FNV-1a. The unified test pins these regression checksums:

| Asset | All-number checksum |
| --- | --- |
| light_single.json | `c3f87d8d44626953` |
| swift_sport.json | `d1c69b200bc8a736` |
| meadow_trainer.json | `c3f87d8d44626953` |

These include metadata numbers and are test checksums only, not replay identity
algorithms. Light Single/Meadow share numeric data intentionally. Existing app
and sim identity/golden tests separately exercise actual physical construction.

`cargo tree -e features -i serde_json` confirms `raw_value`; no `float_roundtrip`,
`arbitrary_precision` or `unbounded_depth` appears. Upstream source inspection
identified that RawValue's iterative scanner does not charge the normal depth
counter; this motivated the allocation-free v2 lexical preflight. The ordinary
serde recursion guard remains enabled. This finding was reviewed independently.

## Review and limits

Independent boundary review examined raw-token ownership, rounding and signed
zero, resource/error precedence, unknown/duplicate field handling, immutable
configuration access, constructor/schema ranges, cross-field runtime differences,
export bounds and absence of any legacy mutation. The reviewer approved the
final code at 09:32 UTC with no remaining blocker and independently ran both
Python schema checks and the whitespace diff check. The final aggregate then
passed on that same reviewed code.

Model path validation remains lexical only; an accepted path may contain NUL or
OS-reserved names and still be unloadable. No symlink, asset-root confinement,
GLB validity or rights decision occurs. The synthetic fixture uses an explicitly
unprovided GLB path, so profile/headless tests cannot be mistaken for a new jet
exterior or a packaging approval. The table does not claim measured engine or
aircraft performance. Candidate source migration and native app selection remain
separate gates.
