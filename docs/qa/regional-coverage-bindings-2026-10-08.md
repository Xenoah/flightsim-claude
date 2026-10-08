# Reviewed regional-coverage source bindings, 2026-10-08

This is source-boundary maintenance for [ADR-0028](../adr/0028-bounded-primary-terrain-coverage.md),
following the [candidate maintenance procedure](../release/swift-windows-candidate.md#reviewed-source-contract-maintenance).
It grants no Windows, package-replay, native-device or distribution approval.
The verified remote reviewed runtime is
`cdd3f9d23314ffb91037c7f6f1ece3df7233891d`, tree
`b87e2f2dec6d8987e63063301cf064eb673c65cd`, parent
`dca2dc86b317766096af01cc00e334efa8c8db0a`. It contains the exactly recovered
runtime plus the separately reconstructed hexf evidence dossier. This binding
uses the verified remote commit, not the reconstructed local commit identity.

## Recovery and evidence limits

An executor reset erased the original clone, patch, local screenshot copies, raw
logs, executable and evidence ZIP. Two owner-retained original fixed-pose
before/after stills survive separately; the other native artifacts remain lost. The runtime source and documentation were reconstructed
from retained implementation commands and independent review reads. Official Rust
1.93.0 formatting reproduced every retained protected-file hash and the entire
pre-reset tree `905fcefdfc090e4c0cbe8a9e8e3d73bb9000be2c` exactly. A full source
snapshot, patch, recipes and migration fragments are now durably saved.

Apart from those two original stills, the old native artifacts have not been
recovered. The [runtime QA](regional-high-altitude-2026-10-07.md)
reports the earlier observations with an explicit loss notice. It does not claim
fresh captures. The pre-reset results were: 1348 pure/core/content tests, 1292
ordinary app/render/input/UI/audio tests (three existing ignored), 405 app tests
with region-downloads (one existing ignored), strict Clippy/docs/format/architecture,
and six successful actual-scene PNG captures. The original +100 m PNG matched its
baseline byte-for-byte; +3000 m displayed two primary regional meshes, and a moving
finer-LOD capture displayed five. These are historical observations associated
with the exactly recovered source, not substitutes for new remote CI or a release.

The pre-reset quick release Criterion comparison at Balzers +3000 m reported
SSE-only 3.8693 microseconds (3.8659–3.8702) and source coverage 5.9881 microseconds
(5.9488–5.9979). Command: `cargo bench --locked -j 2 -p flightsim-world --bench terrain -- lod_select/balzers --quick`.
It measured selection CPU time only, with maximum L12; selected levels stop at
L9/L10 here as they do with the app's L13 cap. No FPS/hardware or fresh-run claim
is made, and no measurement log is fabricated.

## Reviewed source boundary

Five of the 149 previous whole-file replay source pins change: app `main.rs`,
`screen_capture.rs`, `world_runtime.rs`, content `install.rs`, and renderer
`terrain_selection.rs`. They respectively add render-only indexed source creation
and provenance diagnostics, unchanged-readiness reporting, transactional renderer
source replacement, advisory coverage from validated manifest IDs, and passing
hints through the unchanged bounded streaming/atomic-cut path.

The remaining 144 prior pins are preserved exactly. Eleven added whole-file pins
bring the protected boundary to 160: world `coverage.rs`, `lod.rs`, `terrain.rs`,
`global.rs`, `draw_distance.rs`, `lib.rs`, `tile.rs`, `draw_distance_tests.rs`,
`benches/terrain.rs`; core `geodetic.rs`; and content `tests/balzers_package.rs`.
They bind the new implementation, topology, conservative WGS84 radius primitive,
extracted bounds tests, benchmark and actual package regression.

All 102 independent anchors and historical legacy hashes/fingerprint remain
unchanged. Physical laws, codecs, aircraft admission, package replay refusal and
release gates do not change. Entire files are bound; no projection, normalization,
ignored helper or automatic pin-refresh feature is added.

## Minimal dependent contracts

The replay contract changes only the five reviewed pins, eleven additions and
its verified remote runtime source pointer. The analytical path set stays fixed;
only main, candidate checker, replay contract, analytical source-count comment
and test count assertion receive new digests, with the same reviewed source.

The capture runner changes only its two inherited contract hash literals. Its
contract copies those values and updates only the runner hash. Original capture
provenance `75753f5e1dfe8d56d321775cb800b29bf058b3e3`, the four-file boundary,
other three source hashes, modes, commands, Windows restriction and unqualified
native/distribution status remain intact. This follows the
[earlier documented migration](aircraft-package-bindings-2026-10-05.md#minimal-dependent-contracts).
All existing candidate/analytical validator functions and candidate test/helper
functions are unchanged at Python AST level; two tests are added and the
analytical count assertion moves from 149 to 160.

No rights, dependency approval, publication receipt, stager copy set, release
checker, Cargo input or workflow is changed. A later build-only workflow triggered
by the capture binding must inspect its own exact source; old artifacts do not
qualify it.

## New adversarial witnesses

Seventeen bad-source mutations are committed only in disposable fixtures, never
compiled. They cover coverage record/directory limits and overflow, maximum LOD,
local radius, complete four-way leaf budget, forwarding, physical sampling order,
topology, conservative geographic bounds, draw-distance policy and its witness,
package hint/replay admission, read and mesh budgets, and actual app activation.

Each of eleven added paths has four exported-evidence attacks: omitted file,
omitted reviewed record, rehashed replacement and removed contract row. Those
44 cases supplement the existing all-file/independent-anchor drift tests.

## Final verification

After recovery, `cargo test --locked -j 2 -p flightsim-world -p flightsim-content --features flightsim-content/downloads --all-targets` passed 329 tests with no failures or ignored tests. Formatting passed, the runtime tree remained exact, and all 102 independent anchors matched. These fresh pure-data checks are separate from the historical app/native results above.

The integrated migrated Python suite ran 318 tests in 116.899 seconds: 317 passed
and one existing Windows-only junction test was skipped. This includes the 17 committed
semantic mutations and 44 export attacks. The earlier stale-pin failure is
retained as historical evidence rather than rewritten as a pass.

Clean-source admission and release-preflight receipts are retained separately
so writing a receipt cannot change the source SHA it attests. Each consumer must
check its own exact clean HEAD and run the full target-platform recipe. All nine
binary-release blockers remain required; source acceptance grants no release
or licence approval.
