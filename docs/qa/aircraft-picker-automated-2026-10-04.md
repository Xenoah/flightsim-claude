# Aircraft picker: automated verification (2026-10-04)

All executed local gates below passed for the production code at
`1ce3fa723d4a0b69737e372df49414adad480a78` (tree
`c2ceaab55fe8818f837bc23f030de7e29bb7587b`), independently source-reviewed over
`965b24ff20cfc351ec0774d14d2fd04da41b03c0`. The initial evidence update at
`337cf6b` changed no code; the later bounded presentation follow-up is recorded
below. This is not native or release acceptance. The source
adds the app-owned aircraft transaction described in [ADR-0016](../adr/0016-transactional-new-flight-aircraft.md)
and the [map picker guide](../aircraft-picker.md). It does not change any original
physical profile, FDM/simulation law, replay schema, asset/candidate allowlist,
quality default or publication policy.

## Executed checks

Using the configured Rust 1.93 build environment and serialized `-j2` lane:

- App/UI/audio all-targets: app **308 passed, 1 existing optional-fixture ignored**;
  real-GLB integration **1 passed**; audio **93 passed**; UI **220 passed**
- Commercial-staging app all-targets: **299 passed, 1 existing ignored**, plus
  real-GLB integration **1 passed**
- Region-downloads app all-targets: **319 passed, 1 existing ignored**, plus
  real-GLB integration **1 passed**
- Pure-layer all-targets group (core/FDM/world/sim/content/tilegen/assetgen/net):
  **1,178 passed**; benchmark harness checks also passed
- Renderer/input all-targets group: **515 passed, 2 existing ignored**;
  benchmark harness checks also passed. The ignored cases require external
  normalized Haneda tiles or a quiet manual latency measurement window
- Pure-layer doctests: **6 passed**
- Pure-layer release benchmark `--no-run` compilation: passed; no new performance
  measurement or speed claim is made
- Final workspace all-targets Clippy `-D warnings`: passed
- Final app/UI/audio all-targets, all-features Clippy `-D warnings`: passed
- Strict workspace/private documentation (`RUSTDOCFLAGS=-D warnings`,
  `cargo doc --workspace --no-deps --document-private-items`): passed
- `cargo fmt --all --check`, `scripts/check-architecture.sh` and
  `git diff --check`: passed

The commercial test run exposed an old weather-fixture assumption: four fixed
Light Single v3 fixtures were tested against the distribution default, which is
Swift in that feature. The test now explicitly selects Light Single. The fixtures,
production default, identity mismatch gate and CLI inspection exception are unchanged.
The separately reviewed LF/CRLF test corrections from `c02fc806` are also included.

## Boundaries exercised

The new app tests use Bevy's real GLB loader and SceneSpawner without a GPU or audio
device. They exercise every original family direction and repeated selection,
legacy v3 and jet v4 export/identity mismatch, canceled loading before instantiation
and after scene readiness, shared scene handles, malformed GLBs, missing scene 0,
empty scenes, unsupported embedded cameras/lights, invalid environment preparation,
weather/destination/month reselection, and replay locking. Existing regional tests
also route legacy-to-target-jet rejection through all seven raw/active/selected/
pending/ready source stages and reject late mismatched inspection generations.

A corrupt GLB test exposed that the requested Scene0 handle can remain pending
when its parent document fails. Preparation now holds and checks the GLB document
as well as scene/dependency load state, before any scene instantiation. Another
regression verifies paused/terminal jet-to-legacy replacement publishes cleared
notices and newly seeded HUD values in the commit update.

Audio tests verify the actual source kind, retired source silence, asset/bridge
ownership, repeated replacement and enabled/master preservation. UI tests cover
coordinate and child-modal ownership, cancellation/generation changes and real
layout at 1024x720 and 1280x720.

The existing real-GLB hierarchy test emits Bevy B0004 insertion-time warnings,
then verifies the complete final parent/transform/visibility hierarchy. This
checkpoint does not claim that warning text is absent.

## Preserved native candidate, not executed

A separate immutable Linux QA binary was built from the exact source/tree above
with `cargo build --locked -j2 -p flightsim-app`, default features, no explicit
features, Rust/Cargo 1.93.0, the configured development profile with debug information
disabled, no incremental compilation and `RUSTFLAGS=-D warnings`. Build log and
receipt were preserved beside the binary for the native acceptance owner.

- Binary: `flightsim-app`, **150,345,824 bytes**
- SHA-256: `e695db07f54ef73de254ba98f32a09bf11315221a9db96ec8ee9daeb9da23f84`
- `commercial-staging`: disabled; `region-downloads`: disabled
- Native launch by this implementation task: **not performed**

## Gates at the original automated snapshot

Subsequent [native checks](aircraft-picker-native-2026-10-04.md) and
[candidate binding review](replay-candidate-picker-pin-review-2026-10-04.md)
record the later results. The following paragraph describes this earlier snapshot.

Independent source review approved the production code at the recorded commit.
Actual native picker/camera/model/flight/replay/region acceptance, picker-specific
Windows execution and speaker listening remain separate. No native screenshot,
publication, merge or release acceptance was performed by this implementation task.
Existing candidate source pins were deliberately not refreshed for the changed
production files; final binding and publication remain the lead's separate gate.

## Bounded readability and regression follow-up

The follow-up over `337cf6b` changes presentation text and tests only. App supplies
the active aircraft's raw name and UI supplies one Current/Locked prefix. A
40-character name budget preserves all original names, including the longest
Launch label; concise known notes retain the 56-character detail budget. Preparing
and regional-rejection notices use two complete lines within the existing 42x2
navigation budget, exposing cancellation/the terrain blocker and the unchanged
flight guarantee without suggesting a reset workaround.

Real-font tests preserve complete Swift, Meadow and Kestrel names and check node
bounds in live and locked maps at 1024x720, 1180x812 and 1280x720. The existing
worst-case long-text layout checks continue to pass. The same-update flight
observer now seeds a distinct stale display at 5000 m and +26.4 m/s before the
jet-to-legacy commit, checks that it survives preparation, then compares the
committed HUD against the authoritative simulation and reseeded display. Its
fixture explicitly includes the production capture -> Start -> advance -> sound
-> HUD ordering at zero physical dt; the earlier fixture omitted the advance
system and therefore did not establish that transitive scheduling edge.

Executed follow-up checks: **309 app tests passed, 1 existing optional fixture
ignored; 222 UI tests passed**. App/UI all-targets Clippy with `-D warnings` passed
for default and all feature configurations, and formatting/diff checks passed.
No admission predicates, lifecycle behavior, controls, physical data, replay
formats or source/binary publication policy changed. Final native captures and
binding remain lead-owned; this implementation task does not launch the binary.
