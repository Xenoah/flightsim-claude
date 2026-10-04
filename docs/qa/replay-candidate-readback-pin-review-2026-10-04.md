# Replay contract rebind for the opt-in readback diagnostic

This is a separate, bounded source-pin review after the
[replay candidate migration](replay-candidate-contract-2026-10-04.md) at
`14114ec992610257aa1253764842ede0b09f7fbd`. The reviewed diagnostic source is
`c80596c0f73a03d2dff45efe75ddd15265dfc9ca`, directly atop that migration. Its
[diagnostic implementation and limits](windows-readback-diagnostic-2026-10-04.md)
remain separate from replay identity and Windows acceptance.

## Exact reviewed change

Only `crates/flightsim-app/src/main.rs` changes among the 26 frozen implementation
files. Its complete raw Git/checkout SHA-256 changes from
`11783c63f5d1919cce097c0348068df945c755f8ab2988cf0e44192f5df17599` to
`632121d2baf3ceaaadf4ac27ff1254fa5e5c6394aaea680bb4ef007eeb21736d`.
The manifest's reviewed-source pointer now names the diagnostic source above.
The other 25 implementation hashes, all 13 independent encoder/golden anchors,
the historical baseline and five historical hashes, and the legacy partial
fingerprint remain unchanged. No fixture is rewritten or regenerated.

The main-file delta adds one module, a false-by-default startup boolean, an
explicit `--windows-readback-diagnostic` option/help line, its configuration call,
and a test that normal screenshot options remain unchanged. Replay admission,
CurrentRecorder initialization, persistent identity notice, weather/region
selection and frame playback paths are untouched. The reviewed configure gate
returns before installing resources, extraction, render systems, tasks or GPU
buffers when the option is false. With the option enabled, ordinary screenshot
scheduling alone arms the separate probe; the flag does not imply a screenshot,
headless capture or exit.

No new replay-admission helper was extracted. The capture/probe modules remain
outside the frozen **replay identity** implementation set, as renderer/capture
behavior already was; their complete canonical identities and checked-out bytes
are bound by the full exact-source inventory for each run. This rebind does not
represent the small diagnostic image or a successful map/poll as replay or real
scene-capture proof. The diagnostic has its own reviewed implementation, harness
and evidence boundaries; its success cannot replace a primary capture failure.

## Checks and limits

AST comparisons against 14114ec confirm that replay contract validation,
source-input collection, exported source-evidence validation, legacy capture/
notice/rejection checks, required replay-test commands, the literal
`region_downloads=false` guard, staging bundle checks and readiness checks are
unchanged. The diagnostic's PNG validator additionally checks file size before
reading bytes; its existing CRC, dimensions, decompression and pixel-row checks
remain. Diagnostic workflow/evidence changes are reviewed in the separate
readback change, not relabeled as part of the original replay migration.

The tests-only outcome matrix at `5e70e141ba99323add0658c1ac28a54a7347127d`
changes no pinned implementation bytes. With that follow-up and this manifest,
all 158 focused Python checks pass: 65 candidate/stager tests and 93 readiness,
authorization and workflow tests. Both independent reference encoders preserve
the original values and golden bytes. A separate
reviewer verifies the exact main delta and every retained source/independent pin.
The clean post-commit full-source inventory and exported source-evidence checks
are rerun locally. These are source and Python checks only: no Cargo command,
native launch, Windows workflow, binary publication or rights approval is
performed by this rebind. The exact integrated source still needs actual Windows
qualification and screenshot inspection; diagnostic outcomes remain
non-qualifying evidence.
