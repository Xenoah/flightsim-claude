# Replay-v3 codec verification, 2026-10-04

Scope: pure `flightsim-sim` codec/types/tests, based on the accepted complete
identity and authored-weather parameter foundations at `e58f023`. No aircraft
law, profile, control, app/CLI default, rendering, regional-package identity,
dependency, release or Windows candidate source pin is changed.

## Evidence and resource boundaries

`ReplayFile` explicitly dispatches and retains v1/v2/v3. The historical `Recording`
reader remains v1/v2 only, and its automatic writer retains its original format
selection. Explicit v1/v2 writers and format-preserving wrapper export are tested
independently, especially v2 with an all-zero world block. New current recording
objects hold only complete aircraft evidence; there is no fake legacy fingerprint
or automatic conversion of old recordings.

The v3 envelope uses nested bounded readers, never an entire-file or weather
buffer. Name, C and W caps are 256, 4096 and 512; typed schema 1 accepts exactly
W=0/62/86/96/120 and C=136+N+W, with exact consumption before frame counts.
Actual schema-1 conditions cannot exceed 512 bytes. Existing frame/keyframe limits
and numeric/control/time/world rules are retained. The maximum v3 encoded size is
22 + 512 + 56*1,000,000 + 108*8,334 = 56,900,606 bytes. This is not a new in-memory
allocation allowance: frame/keyframe vectors retain their historical caps.

Independent Python `struct.pack` encoding in `replay_v3_reference.py` produces
11 checked-in fixtures: v1, v2-disabled, v2-world, v3-absent-weather, all six authored
presets and Custom with both layers. Rust's independent test encoder must match
those files, and production decoding/encoding must preserve their exact bytes.
The Python verifier prints each SHA-256 and does not rewrite files by default.
`replay_identity_reference.py` independently verifies the unchanged legacy and
complete Light/Swift identities from the shipped profile JSON.

Boundary tests include every truncated prefix of all block shapes; oversized C,
W, names, frame/keyframe counts; malformed UTF-8; unknown formats/identity metadata;
weather schema/source/model/preset/phase/morphology tags; reserved flags; lengths
inconsistent with flags; trailing block bytes; each weather scalar's nonfinite and
range failures; layer thickness; precipitation kind/rate contradictions; mislabeled
presets; environmental/world/clock failures; control bounds; keyframe bounds and
unit quaternion validation; and preserved signed zero. Instrumented cursors check
that invalid envelopes do not consume frame counts and that inconsistent layer
lengths fail before variable layers are read. Unvalidated weather cannot enter a
recording object because its validated fields are private.

Every independently configurable aircraft scalar mutation in the existing complete
identity suite now goes through v3 serialization and compatibility classification.
Exact Light/Swift state, elapsed time, keyframe, log, crash and touchdown comparisons
cover record/decode/replay with variable render cadence and actual fixed-step input
recording. Both absent and explicit Rain metadata survive. That test deliberately
makes no weather-rendering claim; scenarios remain data at this layer.

## Validation commands

Use the coordinated Rust 1.93 `build-env.sh` and shared target with `-j 2`:

- `cargo test -j 2 -p flightsim-sim --all-targets`
- `cargo clippy -j 2 -p flightsim-sim --all-targets -- -D warnings`
- `cargo test -j 2 -p flightsim-sim --doc`
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-sim --no-deps --document-private-items`
- `python3 docs/qa/replay_identity_reference.py`
- `python3 docs/qa/replay_v3_reference.py`
- Changed-file rustfmt, `git diff --check`, `bash scripts/check-architecture.sh`

All listed checks passed on 2026-10-04 using Rust 1.93: 337 sim tests across 35
all-target binaries, 3 doctests, strict clippy and strict documentation, both
independent Python encoders, changed-file formatting, diff whitespace checks and
architecture checks. This includes 17 new v3 integration tests and the existing
identity suite extended to 48 tests with serialized field mutations and the new
Light/Swift fidelity check. No existing acceptance tolerance was widened or
refreshed.

## Remaining integration gates

At this codec-only milestone the app still used legacy recording/player APIs.
The later [app migration](replay-app-v3-2026-10-04.md) implements the explicit
policy and current recorder. The remaining integration constraints identified
here were to select an explicit legacy compatibility policy, use `ReplayFile` on
import/export, and choose v3 complete identity for new flights. Manual cloud
overrides need an explicit Custom mapping decision. Unsupported weather playback must remain
blocked until presentation supports the full scenario, including ambient/fog
extinction independent of cloud quality. Regional-package replay remains blocked
because no ID/version/exact-manifest-hash identity is added here.

No app, GPU, native runtime, full-workspace, cross-platform floating-point, Windows
package or publication qualification was performed by this sim-only task. The
existing source-pinned Windows candidate must be reviewed and requalified by its
owner; this change does not refresh pins or claim that existing qualification
covers changed replay source.
