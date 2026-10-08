# P3: structured device-error observations over unchanged S

This is a separately reviewed diagnostic policy, not a renderer repair, cause
identification, new native acceptance, or release authorization. Preparation is
local only; no workflow was launched. Current main's green CI/MSVC/ordinary
Windows results remain separate from this historical-source investigation.

## Evidence and purpose

Source S remains `fe7373e96396131ef4a1c2a9af9cfabdb8aab82b`, tree
`6dc4d8ed190ccc43733182d03e089f84f0eaaef6`. This additive policy is based on the
published P2 commit `2939a781a91c31db6ffc34c6979448871f0e46bc`, tree
`2e4172748d2ee990c0e5d874b12792b176db36b1`. No P2 file is modified.

P2 run 37826099523 established the present leaf `Parent device is lost`, but not
its initiating failure. Its old scanner observed an additional ERROR-marked
267-byte physical line at byte 1089504, three physical lines / 456 bytes before
the exact fatal heading at 1089960. P2's narrow selector omitted that line without
counting the rejection. The line's logger/message cannot be recovered from its
hash. This policy does not assert which message it contained, and the fixture
with matching span dimensions is explicitly synthetic.

The downloaded historical JSON artifacts contain no complete private stderr,
executable, or original command journals. P3 cannot retroactively project those
missing bytes or turn a later run into evidence about that historical line.

## Small additive design

- `project-device-error-facts.py` independently reads the same fully bound stderr
  and emits structured facts only. No arbitrary target, message, label, driver
  prose, path, token, panic location or environment string is exported.
- `observe-device-failure.py` calls P2's unchanged source validation/binding code
  and `project` function. It preserves all four P2 sidecars, adds
  `device-error.json`, and binds both observations in `device-origin.json`.
- The isolated `diagnostic/analytical-device-fe7373e` workflow retains the single,
  byte-identical P2 runtime invocation of S (the same effective arguments as S),
  Windows 2022, original
  180-second scene capture deadline, successful original upload prerequisite and
  one original strict-validator poststep. Revalidation checks the same current
  S/P/build/executable/bundle/command-journal/full-stream bindings. It performs no
  additional frozen-target audit or Microsoft/runtime-fact recollection.
- The six-file export allowlist contains P2's present-origin.json,
  source-validation.json, runtime-failure.json and present-error.json plus the
  two new device sidecars. Original policy and evidence are preserved.

All runtime acceptance and release flags remain false, including when every
observed marker can be classified. No native instrumentation, GetDeviceRemovedReason,
application/dependency edit, new agreement, or release step is added.

## Bounds and accounting

The stream is limited to 64 MiB, each retained physical-line prefix to 64 KiB,
and the new device JSON to 16 KiB. Every byte, including oversized tails and
post-fatal text, contributes to the full hash. Each exported row carries its
original physical byte span and SHA-256. The first eight and last eight marked
pre-fatal lines are retained. All other pre-fatal rows are counted by their
fixed disposition; they never silently vanish when INFO/debug/noise intervenes.
There is no deduplication of repeated failures.

This is a new finite fact-selection boundary: P2 still exports at most four
nearby sanitized text lines and keeps all its original limits. P3 adds a separate
16-KiB structured sidecar with at most sixteen first/last observations; it does
not enlarge P2's text allowance. Early queue submission, command recording or
memory-query failure can invalidate the device before later INFO/debug messages
and the secondary Surface::present panic, so limiting all observations to the
last eight lines could hide the initiating operation again. Temporal order alone
still does not identify that initiating failure.

The old scanner's case-insensitive ERROR/fatal regex over ANSI-stripped physical
line prefixes is reproduced exactly as an accounting rule, not as a trusted
severity parser. The wrapper reconciles total marker count with that unchanged
scanner. The first exact unprefixed, uncolored fatal literal is a separate byte
span. Later marked lines are explicitly counted as post-fatal. Pre-fatal counts,
that boundary marker, and post-fatal counts must sum to the old total.

Nearby counts cover the same eight physical lines / 4096-byte radius as P2 but
include rejected markers as well. The projector also retains earlier errors.
A marked line can be recognized, unknown-target, unknown-template,
malformed-prefix, unsafe-label/path, unsafe-token, oversized,
ambiguous-prior-content, malformed-encoding/control, unknown-HRESULT, unknown-D3D12-metadata or
ambiguous-stream-framing. The row-cap omission count and per-disposition omission
counts are separate from those classifications. Oversized lines can hide markers
beyond the old scanner's prefix; the packet explicitly describes this limitation.

`classification_complete` concerns only this conservative grammar and retained
pre-fatal marker set. It is false when the fatal boundary is absent, rows are
omitted, any classified row is unresolved, any line is oversized, or framing is
quarantined. `producer_authenticated` is always false. Neither complete structural
coverage nor a numeric HRESULT establishes root cause or validates a driver claim.

## Reviewed grammar and source provenance

The fixture provenance JSON records seven Cargo.lock archive checksums and twenty-two
inspected source-file SHA-256 values. The preparation verified those crate archive
checksums before deriving the fixture records. Unit tests verify fixture version
identities against the unchanged lockfile. These are source-shaped synthetic
messages, not recovered native evidence.

- Bevy log 0.18.1 uses tracing-subscriber 0.3.23 Full formatting to stderr. The
  accepted shape is optional exact UTC timestamp, uppercase ERROR, an exact
  Rust-style logger target and message. No optional missing target, fuzzy
  timestamp, whitespace folding or dynamic span prefix is accepted. Formatter
  SGR colors are removed conservatively; other controls and invalid UTF-8 are
  rejected. A timestamp/target shape does not authenticate the producer.
- wgpu-hal 27.0.4 `auxil/dxgi/result.rs` logs `{description} failed: {err}`.
  Forty reviewed fixed description literals are paired with this exact logger,
  including Signal fence, QueryVideoMemoryInfo and GraphicsCommandList::close.
  The logger is the adapter module, not the originating call-site module.
- windows-result 0.2.0 Display emits either an eight-digit uppercase HRESULT or
  arbitrary message text plus the parenthesized HRESULT. Only a full terminal
  source-shaped failure code is projected. Prose, quotes, nested/extra code
  candidates, credentials and paths are never harvested as numeric facts.
- Direct descendant loggers include dx12::command allocator reset,
  dx12::device fence wait, dx12::descriptor exhaustion, dx12::suballocation and
  core::device::resource's fixed trace-disabled observation. Unknown descendants
  retain a fixed target category and explicit unresolved disposition. The
  WAIT_EVENT Debug tuple is recognized conservatively; its number is not exported
  or misidentified as an HRESULT. Arbitrary shader/configuration text is unknown.
- The dxgi exception handler removes the D3D12/severity prefix before logging.
  Only a complete metadata-only message can provide a whitelisted D3D12
  category/severity and an independently checked ID/name pair
  from windows 0.58.0. The candidate trailer shape follows the driver's trailer
  form visible in the handler, but the precise message in P2 is unknown. Any driver prose, object-name prefix, unknown, mismatched, duplicated or
  quoted trailer produces no facts, even if an ID-looking suffix is present. All free driver prose remains private; a metadata name containing AT_FAULT is merely a copied
  fixed enum observation, not this policy's attribution of fault.

## Privacy and ambiguity

ResourceErrorIdent in wgpu-core 27.0.3 inserts unconstrained resource labels
verbatim using `with '…' label`. A label can contain quotes, newlines, complete
logger prefixes or fake fatal headers. Finding that label syntax anywhere before
the boundary permanently quarantines later classifications, including apparent
fatal attribution. All severities participate in framing checks, including
WARN exception messages. Malformed controls/encoding, oversized lines, unknown
source ERROR templates and incomplete/unsafe dynamic messages also fail closed.

Facts remain tentative until the complete stream is inspected. A visible
non-logger continuation after relevant logging, even after an apparent fatal
heading, revokes all tentative pre-fatal facts and the apparent boundary. The
retained, omitted and nearby disposition counts are revised consistently. This
is intentionally conservative: an unrelated unframed line can suppress useful
facts too. Exact panic-location shapes and the fixed cause/panic-note/Bevy system
postlude are allowed as framing, but never exported. Bevy ECS 0.18.1's two
executor source files support the system-panic postlude shape.

Quarantine does not stop marker accounting, hashing or omitted-row accounting,
and never resets when apparently ordinary logging resumes. A wholly valid
forged record sequence with no observable continuation remains indistinguishable
from ordinary logs, so every fact is still explicitly unauthenticated.

An unframed text stream cannot prove the authorship of a perfect forged record.
The policy deliberately exposes that limitation instead of treating a textual
match as authenticated logger metadata. It never infers native removal reason,
causality, the initiating operation, or absence of an unlogged earlier failure.

Tests cover the P2 independent-review privacy cases: Windows punctuation and
relative paths, lowercase and punctuation tokens, multiline labels, embedded
label delimiters, injected headers/known causes, and original-upload ordering.
Additional hostile fixtures cover operation/target mismatches, source-real
HRESULT and WAIT_EVENT grammar, invalid/duplicated D3D12 metadata, ANSI/control and
UTF-8 tricks, intervening logs, oversized tails, first/last cap accounting,
full-stream mutation, file links/reparse points, and S/P/executable/journal/export
mutation. These tests establish only local policy behavior.
