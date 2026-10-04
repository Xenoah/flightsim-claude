# Near-static turboprop app integration verification

Status: local automated qualification passed for source
`3b2d95f79ca61ce9fc45250a412c5040b70da397`. Independent review, combined-source
rebinding/CI and native v6 acceptance remain separate gates. This work did not
publish a release, register a Cedar preset or change any physical bound.

The pure integration checkpoint is
`93bad37f27403ee566a59290bc25fe6dc4bec48e`, on app base
`7f5663c064d31b0361530777a058e763ac7f0e85`. Accepted law/profile/identity/host/codec
and Cedar qualification commits were applied unchanged before app edits.
ADR-0020, 0021, 0022 and 0023 remain distinct; app admission is ADR-0024.

## Result

- Explicit original-byte profile-v4 loading and separate typed law-2 live/v6
  replay ownership; old profile-v3/law-1/v5, jet and legacy paths remain separate
- Complete body/engine state, transactional controller/trim/parking controls,
  recorder and environment ownership through startup, restart, candidate
  preparation, cancellation, terminal rejection and bounded replay reconstruction
- Existing source, weather, wind, model, camera and audio transactions reused
  with their generation checks; no asynchronous candidate can activate early
- Read-only law-2 presentation at the committed physical clock; truthful modeled
  turbine x, shaft rad/s, blade pitch, static model rotor and synthetic audio
- App-only wrong-family replay feedback identifies the matching profile version
  and `--aircraft FILE`. Codec read/dispatch boundaries are unchanged; commercial
  messages explicitly require a development build for the experimental families

Existing map layout modules are unchanged. The only edit to an existing pure
runtime source is the near-static host's read-only presentation module/export.
All 2,017 protected existing files, including every one of the 93 fixture files
at the pure checkpoint, are byte-identical. The comparison covers pure crates,
render/input/UI/audio/network/content, assets, schemas, scripts, tools and
release manifests. No source-pin manifest or old golden was rewritten.

## Executed checks

The [machine-readable receipt](near-static-app-integration-receipt.json) records
exact commands, per-run counts, source hashes, log sizes and SHA-256. Counts
between configurations overlap and must not be summed as unique coverage.

| Final configuration | Passed | Existing ignored |
|---|---:|---:|
| App, all targets, ordinary | 363 | 1 |
| App, all targets, commercial-staging | 323 | 1 |
| App, all targets, region-downloads | 374 | 1 |
| Pure simulation, all targets | 513 | 0 |

App totals comprise 362/322/373 unit cases respectively plus one production GLB
hierarchy integration test each. The ignored case requires optional external
regional scenery/terrain fixtures and receives no pass credit.

Also passed strict workspace all-target Clippy; app all-target Clippy with
commercial-staging and region-downloads together; workspace private-item
rustdoc with region-downloads and warnings denied; formatting; architecture
checks; and diff whitespace checks. The shared Cargo target was used serially,
with affected crate roots touched before building to prevent stale cross-worktree
artifacts. No duplicate cache or native app launch was used in this qualification.

## Transaction and replay witnesses

New tests exercise both old-law to new-law and new-law to old-law new-flight
transactions. Their active baseline first executes a physical step and owns
nonzero trims, pending parking input, full host history and an authentic
recording. Cancel, changed destination/month/weather/forces/generation, late GLB
completion, malformed/missing assets, regional/raw sources and invalid forces
preserve physical/engine state, both endpoints, history, cursor, recorder bytes,
controls, scene root, source, clock, camera and sound owner.

A separately parsed finite 150 m/s approach hint is structurally valid metadata
but rejected by the unchanged numerical model's Mach 0.3 operating envelope.
The test checks complete physical canonical-byte equality before/after changing
that hint, then verifies the active flight remains unchanged. Successful
replacement alone resets trims and owns the selected profile, model/audio,
wind/turbulence/weather and complete new recorder. No model bounds were changed
to create either the failure or success.

Runtime witnesses include authored signed-zero engine startup without warmup;
read-only presentation/accumulator equality; 30/60/120/144 Hz effective controls
and full-state equality; non-consuming v6 exports; F5–F8 transport and visual
clock restoration; recorded controls overriding live HUD/audio input; no legacy
landing grading; all-state/controller rollback for an actual negative-flow
transverse-domain terminal; terminal-at-zero reproduction; v5/v6 cross-rejection;
exact source/weather restoration and forbidden overrides; manual-cloud record
disabling; restart reset; and authentic prefix closure on sun-rate changes.

The new legacy-entry diagnostic tests feed only wrong-family version headers
through the existing reader. They verify v5/v6 point to their matching profile,
unknown formats remain unsupported, ordinary malformed errors retain their
original explanation, and aircraft/wind selection is unchanged.

## Corrected early test assumptions

The first compile found missing new-test imports and an existing exhaustive
recorder test helper requiring the added enum arms. These were corrected.
An initial invalid-wind test assumed a large finite steady wind must invalidate
map startup. That was wrong: the existing map constructs a wind-relative
velocity. The corrected tests distinguish a nonfinite force input from a finite
schema-valid candidate outside the actual physical envelope. This was a test
assumption correction, not a production physics bug or a wind-bound change.
Initial red logs remain in the external QA archive alongside the passing runs.

## QA-only Cedar profile for native follow-up

External QA contains `cedar-v4-native/cedar-utility-turboprop-law2-v4.json` with
SHA-256 `89ccf5ba39fd909927b11ec3b9047b57b9bdf4f4d20915f419373902843fb6b4`.
It retains the exact original profile source
`a4b13ac7c59830967666bb874e25416587a5fbf9945cbad248afa5bc7c9344e1`, changing
only version 3 to 4, law revision 1 to 2 and the explicit accepted propeller
extension. No asset, preset or picker registration follows.

Independent source checks preserve all 609 retained numeric words (excluding the two explicitly changed version tags) and all
controller/start/model/camera/audio metadata. Production exact-token loaders
confirm all 4,835 retained canonical forward bytes and original identity
`f83d082e814871c0`. New canonical bytes match the independent encoder exactly:
5,390 bytes, identity `3b9ca42bba4c6eaa`, canonical SHA-256
`6828da741412dedbd598db15ac88535afa35fa4d418fbd330454bbac7f44d81c`.
The explicit negative rows retain SHA-256
`2467ad7537216ae1470caa73e94eeba44f737686ef251fb5ac046a1cf6ca1fd4`.
Schema validation and production export/reparse also passed.

This artifact prepares native full-state/ground-boundary checks; it does not
itself perform them. Original Cedar braked-idle creep is approximately 0.20062 m
per ten seconds, so stationary parking remains unqualified. The law is still
limited to J[-0.01,0), adverse/transverse induced-speed ratios at most 0.10 and
the conservative static power floor. No broad reverse-flow or vortex-ring-state
fidelity, flight/wind qualification, native visual interaction, physical
controller support, speaker listening or release acceptance is inferred here.
