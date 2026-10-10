# Terrain three-source stitch bindings, 2026-10-09

This bounded source migration succeeds the terrain-centroid migration. It does
not qualify native appearance, a complete replay, another driver, a Windows
build, dependency-review applicability, distribution or publication.

## Provenance and preserved evidence

The migration's base is the **unpublished local centroid checkpoint**
`5eaaff379f19cc986fa5600619f1491213e9da0d`, tree
`a469ee76cd0d7aabf3dd2c07114d44c0bbdf5d37`. Its public base is
`42a7ddeae36f3022c2c77e54d5dce33f71b11bca`, tree
`625c39f9b182b16a0dada2dc4131434d178a0831`. The local checkpoint is not a
published release or a required fetch target. Verification uses the checked-in
whole-file witnesses and exact manifests; it never requires that unpublished
commit object. Each execution binds its own actual final clean commit, complete
source tree and canonical/checkout bytes, even if publication uses a different
commit ancestry.

`scripts/terrain-stitch-source-migration.json` retains the previous centroid
migration digest `203d43b06ef860813fd3f21f482f4751d3ad0b625872086a7eb76d3b91d04db5`.
The centroid and component-terms manifests, earlier source histories, vendor
provenance and all historical QA/build/release records remain byte-for-byte
unchanged. The prior 930-member replay contract is retained verbatim at
`scripts/history/5eaaff3-replay-candidate-contract.json`, SHA-256
`1b72efcc010124b0a02708f0c4b75c07c79a272e352ac697b71934fc9aa20869`.

Only two existing compiled inputs change:

- `crates/flightsim-world/src/seams.rs`, whose previous bytes remain at
  `scripts/history/5eaaff3-seams.rs`, SHA-256
  `4c69ca50946663ead1c82907793b37af75e54f886340c8986bfd4a573a8397ef`
- `crates/flightsim-world/tests/terrain_seam_geometry.rs`, whose previous bytes
  remain at `scripts/history/5eaaff3-terrain_seam_geometry.rs`, SHA-256
  `a88108b3c2e7dedb5d483d4578101d7e37f90b3d6cab0ec71e18aa51f2be929f`

The migration records their exact whole-file old-to-new digests. Every other
prior replay pin is checked against the retained contract, so a wholesale repin
cannot redefine the accepted boundary. All 497 frozen runtime identities, the
404-member original replay boundary, 102 independent anchors and 55-member
core-pipeline boundary retain their exact prior bytes. The runtime map itself
is unchanged; only the two new explicit historical relocations are added.

## New source boundary

The replay contract has 934 members: the previous 930, two historical Rust
witnesses, the historical replay contract and the new migration. The closed
crate boundary remains 495 members; no compiled path is added. The analytical
boundary grows from 47 to 50 members by adding this record, the focused Python
migration test and [ADR-0032](../adr/0032-terrain-three-source-corners.md).
The capture boundary retains four members. The three current contracts carry
new terrain-stitch admission identities and the same new migration digest;
only affected current source/tool/test/document hashes and inherited contract
hashes advance.

For exactly three source vertices with the full closed link cycle, and a
nonzero-area triangle after local f32 encoding, the geometry emits the whole
original source triangle in both windings in each incident ribbon. It omits
the redundant mean-anchor faces while retaining vertex storage and repeated
cap ownership. Exact encoded degeneracy falls back to the earlier fan, as do
open three-source paths and other topologies. No height, source boundary,
source polygon, LOD selector, planner, DEM/FDM/replay state, MSAA setting or
shader is changed by this third migration. Its geometry and validation limits
are separate from the earlier shading-only centroid evidence.

## Source validation witnesses

The focused tests check all 934 pins against repinning, reject previous current
identities, and exercise the actual clean committed-source capture chain in a
fresh repository without the unpublished checkpoint object. Mutations cover
both changed files and every new source witness/manifest, omission, case aliases,
ignored extra crate targets and omitted/substituted/resealed exported evidence.
The earlier source-boundary suites retain their adversaries and independent
anchors; only explicit current identity/count expectations advance.

These fixtures are source-validation witnesses, never production build or
execution receipts. Test results must be attached to the exact tested source.
The earlier 121-method centroid source review remains evidence for its own
frozen checkpoint, not a pass for this changed source. Historical preparation,
dependency/full-review applicability and authorization maps are not refreshed.
Changed source must fail old exact applicability conditions until a separate
same-source review and genuine build satisfy those gates.
