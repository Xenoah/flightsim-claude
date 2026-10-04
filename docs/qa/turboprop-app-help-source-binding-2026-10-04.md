# Reviewed turboprop app and flight-help source binding

Date: 2026-10-04 UTC. Initially reviewed code:
`559da4790b31da712ed208f7b9055fd6e13ef90b`, tree
`ed60857487cec45585d90d381a3741dfe736324a`. Final native-feedback source review:
`334f7e40233dc1a0772ca8406a89950e017da19d`, tree
`0a397c69916c28081a659f1e903a3cdd4cf85c42`; see the follow-up below.
Latest input follow-up: `0b1e973ebbbee36a29ba26b2710e60a7dbecfdac`, tree
`f352ae87b046b1da3c239d16ff5f13327bbf6566`; see the reset-tap addendum.

This binds the accepted app integration and corrected help layout to the existing
Swift-only Windows engineering candidate. It grants no native acceptance,
aircraft qualification, binary distribution permission or release authorization.
It changes no production Rust, assets, physical profile, replay format or workflow.

## Source review

The app delta was independently accepted at `5d6c9c9615122fe0bfdc1743b1ed1edb01a1a02b`;
integration `0e7472d8d9db4a4ce3baacedb086f74d35a701b2` retained its production/test
bytes exactly and reconciled the accepted pure-host documentation. The subsequent
UI source review traced `ca64394` and `44e4140`, then the paused-attribution fix
authored at `9db33490be9259de7122163ee7292cb4cc32bf98` and integrated at the reviewed
code above. The final integrated text retains the precise legacy help label
`Shift: fine roll/yaw trim`.

The measured HUD puts instruments beside notices, tutorial, help and flight log,
above the actual wrapped attribution height. It retains instrument/help/notice
font sizes and supplies separate complete and compact aircraft references. Replay
uses its own transport reference and suppresses live takeoff guidance. Reference
refresh follows both guidance and replay-status changes, including changed notices
without a guidance change. The new wheel reader only changes bounded UI scrolling,
drains hidden events and resets on resume. Ordinary replay still returns before
the existing Esc/R handler; F5 continues to control replay transport alone.

The initial source review found a real pause-overlay defect: its independently
positioned, nearly opaque panel could cover wrapped footer credits. Binding was
held until the fix above anchored the absolute pause panel inside the measured
HUD body. The footer remains outside that body and is therefore reserved in the
same layout pass. The new layout witness checks long wrapped credit at 640x480,
1180x812 and 1280x720, immediate resizing, clearing/restoring credit, and actual
attribution during reference scrolling. Existing scroll and replay-reference
assertions remain. Standalone pause use retains its viewport-relative fallback.

The app's commercial guard still rejects profile-v3 turboprop starts before CLI
profile assignment and before delegated replay/terrain/session preparation.
The delegated `prepare`, `resolve_sources` and `prepare_startup` functions each
guard their own entry. Original v1/v2 profile inspection exceptions remain;
historical replay v1/v2 still requires the existing explicit compatibility opt-in
and persistent missing-yaw notice. Pure v3/v5 inspection does not authorize app
startup or distribution. No Cedar preset or law-2 change enters this binding.

## Exact boundary extension

All original **47 paths** remain required. Exactly **16** existing pins advance
to their reviewed changed bytes; **31** retain their old hash. The checker keeps
all **13 independent anchors**, all **five historical hashes**, the original
baseline/fingerprint and the separately frozen additive FDM-module guard.
The contract identifier and schemas remain unchanged.

Eight whole-file pins are added, for **55** total:

- `crates/flightsim-app/src/turboprop_session.rs`: delegated pre-read admission,
  complete-state app preparation and aircraft references
- `crates/flightsim-app/src/turboprop_runtime_tests.rs` and
  `turboprop_lifecycle_tests.rs`: admission, telemetry, lifecycle and reference
  witnesses for that delegation
- `crates/flightsim-input/src/lib.rs`: explicit pilot trim on the shared Swift
  path, including the zero-bias branch preserving legacy control bits
- `crates/flightsim-input/src/lateral_trim_regression.rs` and
  `tests/fixtures/legacy-input-v1.json`: independent trim/old-input compatibility
  witnesses
- `crates/flightsim-ui/src/pause.rs`: the complete reference, replay notice
  authority, bounded scrolling and measured footer ownership
- `crates/flightsim-ui/src/attribution_layout_tests.rs`: the existing actual
  wrapped-footer layout witness

The required path-set change is strictly additive. All checker function bodies
are unchanged. Every original candidate test assertion is preserved, with three
additional tests. One stale picker mutation needle is updated from the earlier
boolean jet-vs-preset expression to the accepted explicit Jet-family rejection,
and its corresponding Legacy-family rejection is added. The first test run's
stale-needle failure is retained; no production source is changed to satisfy it.
The all-path canonical-drift and row-removal tests now
also cover every added pin. Thirteen targeted committed mutations reject removal
of any delegated startup guard, weakening the commercial family check, changing
zero-trim compatibility/fine rate/modifiers, losing replay-reference authority,
retaining hidden scroll, removing scroll bounds, showing a live tutorial during
replay, or removing the compact reference's complete-controls cue. Diagnostics
must identify the actual changed path. Thirty-two exported-source mutations cover
each added path's missing file, missing reviewed record, changed canonical digest
and removed contract row, including resealed report/contract hashes.

Pure turboprop physics and v5 internals are not newly represented as Swift runtime
qualification: commercial admission rejects that family. Their complete tracked
bytes remain in the raw source inventory, their accepted pure review remains
applicable, and independent v5 wire checks are rerun. This extension closes the
new delegated admission, shared-input and reference boundaries; it does not
expand the candidate's aircraft or binary allowlists.

## Verification and limits

The bounded Python run passes **180 tests**: candidate 66, stager 21, readiness
38, authorization 43, release workflow 8 and source-CI workflow 4. All six
independent legacy/v3, jet/v4 and turboprop/v5 identity/wire scripts pass without
`--write`; existing fixture bytes are not regenerated. Full clean-tree collection
checks every checkout byte against its raw Git blob, and exported source evidence
passes the unchanged validator. A separate AST/hash audit checks preservation of
all old test assertions, checker functions, historical values and independent
anchors. Exact final commit, tree, path/byte counts, hashes, mutation logs and the
full inventory are retained in the accompanying integration review receipt.

The UI author's final 239-test suite and scoped UI Clippy logs were independently
read and hashed. They are supplied execution evidence, not tests rerun by this
source-binding review. No Cargo, shared build, GPU, CUA, Windows binary or native
interaction was executed here. Native rendering, Esc/wheel behavior, actual
save/reopen/transport, physical controller/audio checks and exact Windows
candidate qualification remain separate gates on the final integrated source.
Rights, dependency review, distribution authorization, workflow/backend/watchdog
and verification contracts remain unchanged. Publication is lead-owned after
native acceptance.

## Native-feedback source follow-up

Lead-owned native inspection at 1180x812 found that the full help block could
cover the centered aircraft even while the text panels avoided each other.
It also found underlying HUD text visible through the 0.92-alpha pause panel.
These findings were not detectable from source-box non-overlap alone and do not
retroactively grant native acceptance to the initial binding.

The bounded correction authored at `46aaeadb23a07152fe43ac4125c66d420049ce8e`,
integrated at `334f7e4`, right-aligns the content-sized help block and makes the
pause background opaque. Existing font sizes, wrapping, compact mode, measured
footer ownership, reference contents, scrolling and replay behavior are unchanged.
The real-layout witness now requires the desktop help block to remain to the
right of the central 40-60% scene band and the pause background to have alpha 1.
The integrated three source/test files were independently compared with the
reviewed author bytes, preserving the precise legacy lateral-trim label.

Exactly three pins advance: UI `lib.rs`, `pause.rs` and `top_layout_tests.rs`.
All 55 required paths, other 52 hashes, 13 independent anchors, five historical
hashes, checker code and complete candidate test source remain unchanged from
binding `7eefb123fe4199c2a8936dd2fa6dff869e0b5ca9`. The candidate's 66 Python tests,
clean full-source collection and exported-source validation pass again. The
author’s final 239-test UI and scoped Clippy logs are supplied evidence, not
reruns by this reviewer. Final native visual acceptance remains lead-owned.

## Native-feedback reset-tap addendum

The lead's native input check found that a brief K reset could be pressed and
released before the frame sampler observed it. The real Bevy `ButtonInput`
regression reproduced `pressed(K) == false` with `just_pressed(K) == true`.
The first source fix admitted that edge, and a second red regression established
that Ctrl+K with both keys released in the same frame needed separate suppression.
Those red runs are diagnostic reproductions, not validation passes. The final
author commits `a265f528db5cb74e43d84e95e0e5b46b694b3823` and
`06d3f00057857275f07d5e2aea6b3ee01a806f05` are integrated at `0b1e973`;
its two input source/test files and QA document match the final author bytes.

The independently reviewed sampler preserves the old held-K path, including
the existing current-state modifier suppression. A released K tap additionally
requires a same-frame K press edge and no held, newly pressed or newly released
Ctrl/Alt/Super key. Shift remains allowed. The conservative edge path cannot
recover event ordering and intentionally suppresses an unrelated plain tap if
a shortcut modifier changed within the same frame. No queue or persistent
state is added: a tap may expire if its frame executes no accepted control step.
Existing sampling, suspension, fixed-step transaction and reset code is unchanged.
The new tests exercise both modifier sides, previous-frame holds, later plain
taps, held K, Shift, repeated polling and a real no-control-step ECS sample.
The old physical-key witness now explicitly clears frame edges before its old
release assertion; held-key and independent-axis assertions remain.

Only `flightsim-input/src/lib.rs` and `lateral_trim_regression.rs` hashes advance
from binding `bd24743f81357d8c49b76d18d15ed08721e5fc13`. All 55 paths, other
53 pins, 13 independent anchors, five historical hashes and checker functions
remain exact. One existing Python mutation's needle is updated to the extracted
`shortcut_modifiers` array expression; the mutation still reverses the held-key
modifier guard and keeps every previous test assertion. No test is removed or
relaxed. The candidate's 66 Python tests, complete clean source inventory and
exported-source validation pass on this follow-up.

The author's final 107 input tests and strict scoped Clippy/format/architecture
checks are supplied evidence, not reruns by this reviewer. See
[the input QA record](lateral-trim-reset-tap-2026-10-04.md) for both reproduced
failures, positive checks and scheduling limits. No Cargo, native or external
publication action was performed in this binding review. Final native input
acceptance remains the lead's separate gate.
