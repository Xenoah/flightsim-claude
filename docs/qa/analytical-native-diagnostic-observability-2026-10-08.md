# Analytical native failure observability, 2026-10-08

This patch prepares read-only diagnostic helpers P on exact production source
S `fe7373e96396131ef4a1c2a9af9cfabdb8aab82b`, tree
`6dc4d8ed190ccc43733182d03e089f84f0eaaef6`. It changes no simulator code, build
flags, stager, acceptance rule, watchdog, qualification/publication schema or
ordinary workflow. It does not launch or authorize another native run.

## Existing evidence and limits

Run 37784631203 / runtime job 113336213187 completed the original native build
capture, platform fact collector, both distribution handshakes and the expected
unreviewed extracted-readiness result. `default-swift` then completed with exit 1
and outcome `failed`. Its stderr was 1,090,271 bytes, SHA-256
`d429acf7c190e547cf3ce6fd4cdaa39cfd79b0e8d39b31a80ede66b1da2eda7e`.
The raw stream and private command journal are not present in the downloaded
artifact. The exact underlying error and elapsed time cannot be recovered from
that hash. `images={}` means no accepted/exported PNG; the existing runner rejects
exit 1 before PNG validation, so private PNG existence is unknown.

This was not an external supervisor timeout. It can still be an application
failure or in-application deadline. In particular, Bevy 0.18.1's pipelined render
runner converts a renderer-thread panic into AppExit::error (exit 1). A shader,
GPU-validation or device failure therefore remains possible. Source inspection
found no proven analytical runtime defect. Reinhard specialization and the no-LUT
placeholder binding are different code paths, not established causes.

The successful ordinary run used Windows 2025; the analytical job used Windows
2022. Their feature configuration, host image/driver and log filters are not a
controlled comparison. Do not attribute this failure to analytical tonemapping
from those runs alone.

The original native factual packet remains immutable. Its compiler identity and
executable SHA `34336b00434134ca3a74f70df7f5328bef515636a37192ec56035659fff7944d`
are preserved. OCR and Default desktop probes reported available. The 428,554-byte
linker trace was retained privately but yielded `no_matching_command`; volume is
not evidence of a particular parser defect or selected linker.

## Narrow corrections

`collect-analytical-runtime-facts.py` normalizes copied Windows environment keys
and rejects conflicting case aliases. Python's Windows os.environ normalizes
keys to uppercase; copying to a normal dict lost its case-insensitive lookup.
The old mixed-case ProgramFiles(x86) lookup therefore skipped VS/SDK discovery.
This is a demonstrated collection defect, independent of the renderer failure.
New facts record the corrected discovery root privately and revalidate it.

The strict Windows linker-command parser is retained. Source-shaped tests cover
quoted drive paths, spaces, Unicode, escaped quotes, foreign paths, wrong Cargo
fingerprint suffixes, embedded false /OUT text and duplicate outputs. New optional
`final_link.trace_observation` contains only bounded scalar counts of recognized
line/command/output stages. A missing/ambiguous match still establishes no selected
linker, implicit SDK search or static archive-member contribution. Old schema-v1
packets without the counters remain valid under the new explicit validator;
old consumers are not implicitly changed to accept the new field.

`project-analytical-runtime-failure.py` binds observations to the entire original
stdout/stderr hashes and private command journal. It records actual elapsed time,
external supervisor deadline, exit/outcome, stage/error/panic marker counts,
selected fixed message fragments and whether the private PNG is absent, invalid,
valid or oversized. Valid PNG bytes plus exit 1 never create acceptance, and PNG
bytes are not exported by this helper.

`analytical-wgpu-error-details.py` recognizes exact locked wgpu 27 source templates:
operation, known shader label/stage, numeric shader line/column, group/binding,
expected/actual texture dimensions/sample types/formats, sampler properties,
known feature/limit failures, and AdapterInfo backend/device/vendor IDs. Known
Microsoft Basic Render Driver identity and numeric driver versions are preserved;
unknown adapter/driver strings are hash/length observations only. Paths, shader
source, arbitrary labels and arbitrary error strings are suppressed.

Streams are at most 64 MiB each and scanned incrementally with a 64 KiB line-prefix
buffer. Full physical lines are hashed, including oversized lines; unscanned
suffixes are explicitly reported as incomplete. Up to eight selected line spans
and sixteen distinct structured details per stream are retained. Unknown error
lines remain hashes. The public failure packet is at most 64 KiB. This is useful
for the identified wgpu binding/shader/device/save categories, but unfamiliar
wording, oversized lines, or arbitrary panic text can still leave root cause
unestablished. It is not a guarantee that one additional run resolves every fault.

## Supported isolated S/P post-step route

Keep canonical S and reviewed P in separate, disjoint clean checkouts. Do not
replace scripts inside S or reuse a cached/reconstructed private executable.

1. On a separately reviewed diagnostic branch, use the existing read-only CI
   gate with explicit source SHA S. It must verify completed successful same-repo
   main-push CI for S. Qualification's production exact-head gate is unchanged.
2. Use Windows 2022 and the existing exact official toolchain. Run S's unchanged
   runtime qualification command once, with fresh private/export roots. Its real
   dual build, inventory, source checks, staging, 180-second captures and limits
   remain intact. If the first scene fails, S stops as before; if it succeeds,
   S may continue its existing bounded remaining sequence. All approvals remain
   false. This is one diagnostic invocation, not an identical blind retry.
3. Before the ephemeral runner closes, run P's
   `observe-analytical-diagnostic-sidecars.py` as an always post-step. Pass explicit
   S source/private/export paths, S SHA, reviewed P SHA and fresh separate P
   private/export roots. The helper first checks both canonical source identities,
   then imports **S's own original strict qualification validator** and revalidates
   S's result. It requires S's actual successful frozen native build audit.
4. P's corrected collector observes installed/native facts against the exact
   existing S build trace and audited executable. It performs read-only queries
   and does not rebuild or launch the simulator. Failure projection reads S's
   original private logs and PNG. Original S evidence is never rewritten.
5. Independently revalidate the sidecar export on that same host. Upload only
   `diagnostic-origin.json` (8 KiB), `runtime-facts.json` (512 KiB), and optional
   `runtime-failure.json` (64 KiB), separately from unchanged S artifacts. Origin
   binds S/P commit+tree, original qualification/build/executable, and current
   native Windows/image observations. No executable, archive, raw log, notice
   snapshot, source text or private path is an output artifact.

The post-step accepts `--source-repo`, `--source-sha`, `--source-private`,
`--source-export`, `--policy-sha`, `--private` and `--evidence`. Its read-only
recheck replaces `--evidence` with `--validate-evidence`. The additive `.github/workflows/analytical-runtime-fe-diagnostic.yml` implements
only this isolated diagnostic route. Its sole push branch is
`diagnostic/analytical-runtime-fe7373e`; the read-only exact-S CI gate has a
five-minute cap and the separate Windows 2022 job has the existing 360-minute cap.
It keeps original S failure status, validates all artifact paths explicitly, and
performs no main movement, publication or approval write. The integration owner
must review and publish that exact P branch before the one native invocation.

These sidecars are diagnostic evidence under P. The frozen publication prototype
must not consume their changed fact schema implicitly. Any later qualification
or publication integration needs its own matching reviewed validators and exact
source/run/archive/review identities; this patch grants none of them.
