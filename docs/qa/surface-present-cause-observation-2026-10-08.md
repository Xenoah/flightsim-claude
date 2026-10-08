# P2: bounded Surface::present cause observation

This additive policy keeps application source S at
`fe7373e96396131ef4a1c2a9af9cfabdb8aab82b` / tree
`6dc4d8ed190ccc43733182d03e089f84f0eaaef6`. It is based on actual prior policy
`42357a5667aa96c22f1278d715534b3962079805`. No prior file, production source,
qualification/publication schema, build recipe, 180-second capture deadline or
acceptance gate is changed. The earlier policy/branch remains separate.

The prior diagnostic established exit 1 after 38.703 seconds, an absent private
PNG, and a panic before the screenshot CPU-ready/request markers. Exact hash
matching against locked public wgpu sources identified the fatal line as
`Error in Surface::present: Validation Error\n` (44 bytes, SHA-256
`4cbeb207d73b1fb2f2a49d74979523ee282e8a364e8e2c99bd388fd6a3a8e18a`).
“Validation Error” is the formatter's generic heading: it does not distinguish
surface state, destroyed texture, device loss, out-of-memory or backend failure.
The following `Caused by:` tree was not captured as readable text. No renderer
repair follows from the known heading alone.

## One unchanged reproducer, separate failure observation

The only new push trigger is `diagnostic/analytical-present-fe7373e`. The workflow
verifies successful original same-repository main-push CI for exact S, then checks
out clean S and P2 into separate directories. Windows 2022 and the original Rust
1.93.0 toolchain remain fixed. It executes S's full runtime qualification once,
without overlaying source or changing any original command or timeout. The
workflow retains five-minute source-CI and 360-minute Windows job caps.

After the original invocation, `observe-present-failure.py --validate-source`
loads **S's own unchanged strict validator** and calls it once. That still checks
both frozen target trees and original evidence. It records the validated source,
report, command-stream/journal, successful build receipt, current executable and
bundle bindings in a fresh P2 private root. Earlier failures can preserve their
original validated evidence without being called native presentation failures.

The original allowed S artifacts upload immediately after that validation,
before excerpt processing. Excerpt extraction requires that upload to succeed.
Only a failed native scene enables the P2 excerpt step. Observation and final
export verification recheck the exact S/P sources,
receipt/root identities, original reports, current executable/strict bundle and
full command streams. They do not repeat the full target-tree audit and do not
run the Microsoft/platform collector. The full original validator is called once
in this post-step path; its existing internal validations remain unchanged.
This replaces the prior sidecar's repeated 14-plus-5 frozen-build validations,
without making these diagnostic bindings a substitute for qualification.

## Selected error text and privacy bounds

`project-present-error-excerpt.py` accepts only the expected complete stream
SHA-256 and size. It finds the first exact known fatal header and selects:

- At most four relevant preceding `Present failed` / known surface-device ERROR
  lines within eight physical lines and 4 KiB of that header
- The immediately following `Caused by:` tree, at most eight lines and 2 KiB
- Explicit stop, truncation, unknown-source-variant and repeated-header counters

The stream is capped at 64 MiB and read incrementally with a 64 KiB line buffer.
Each preceding error text is at most 512 escaped JSON bytes. The entire selected
text packet is at most 16 KiB. Full line spans and hashes bind the sanitized text
to the original private bytes. Unknown cause wording remains readable after
sanitization and is identified as unrecognized; it is not replaced solely by an
opaque hash. The original complete stream stays private.

Sanitization occurs before clipping. Any detected absolute, drive-relative or
relative path suppresses the entire selected line, because unquoted filenames
can contain spaces and punctuation and both path boundaries can be ambiguous.
Resource labels are inserted verbatim by wgpu: every label-bearing cause stops
selection, including apparently closed
labels, so embedded delimiters/newlines cannot leak continuations or fabricate a
known nested cause. Any label-bearing line before the fatal header blocks
selection entirely; its severity/target cannot make an injected header
trustworthy. The packet reports those omissions explicitly. URLs,
quoted strings, credential-key values, bearer/basic credentials, JWTs, recognized
API-key patterns and token-like long values are also redacted. Known HRESULT
identifiers and bounded numeric error codes survive. Panic locations, backtrace
lines and unrelated/interleaved logs or environment data terminate the cause
selection. Oversized lines and all caps are visible as incomplete/truncated
observations. No arbitrary log or environment export is enabled.

These are technical application-error excerpts from the exact requested source.
They neither prove root cause by themselves nor create runtime/visual/rights or
publication acceptance. Conservative redaction can remove useful detail; if the
cause remains unsupported, that must be reported rather than guessed.

## Exact artifact boundary and validation

Original S exports remain its existing fixed JSON files and eight permitted Swift
PNGs. P2 exports only:

- `source-validation.json`: at most 64 KiB of original-validation identities/hashes
- `runtime-failure.json`: at most 64 KiB of prior bounded stage/PNG/adapter facts
- `present-error.json`: at most 16 KiB of selected sanitized error/cause evidence
- `present-origin.json`: at most 8 KiB binding those observations to S and P2

All runtime-accepted/release-authorized fields remain false. Original process
failure stays failed even if the diagnostic succeeds. No raw log, private path,
source payload, model, executable, archive or private approval is uploaded. The
frozen publication consumer is unchanged and does not consume these new schemas
implicitly. P2 does not recollect or amend the earlier corrected Microsoft facts.

Local tests cover full-stream binding, known locked cause forms, readable unknown
causes, sensitive paths/tokens, HRESULT preservation, caps/interleaving, Windows
reparse-point rejection, one strict S-validator invocation, subsequent mutation
rejection, exact artifact membership and unchanged source/workflow commands.
No Windows job or renderer change was executed while preparing this policy.
