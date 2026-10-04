# App replay-v3 migration verification, 2026-10-04

Scope: application recording, startup acceptance, playback and UI migration on
the v3 codec foundation `86de934`. No FDM law, aircraft/profile parameter, control
rate, source pin, dependency or release runner is changed. This isolated branch
does not include the subsequently developed regional download controls or modeled
weather presentation, and does not enable either one through replay.

## Behavior

New live, restarted and map-started flights use complete-identity
`CurrentConditions` / `CurrentRecorder`. F9 saves v3 with `WeatherSelection::Legacy`
and the existing environment/world/clock fields. Source import is `ReplayFile`;
`ReplayFilePlayer` retains that source and original identity on export, including
v2 with the world block disabled. The legacy `Recording` / `Recorder` / `Player`
public APIs retain their signatures and semantics. Both players share a private
cursor for timing, pause, speed, overflow, zero-duration, seek and end behavior.

Normal playback rejects v1/v2. `--legacy-replay-compatibility` allows only the
original frozen revision-2 Light/Swift partial fingerprints together with the
corresponding selected complete baseline configuration. It is an explicit
assumption about missing historical evidence, not recovery of that evidence.
The startup diagnostic and persistent replay banner state that historical
`yaw_rate_p` was not recorded or verified. The notice survives pause, seek,
completion and fault. Complete mismatches cannot fall back to this exception.

Modeled-weather files and replay startup manual-cloud arguments are rejected
before copying recorded conditions into startup. Live manual cloud arguments
continue to affect clouds and permit flight controls, but no frames are recorded
and F9 is unavailable with the persistent `F9 OFF: manual clouds` banner.
Startup and F9 diagnostics explain that recording is disabled and list the four
arguments to remove before recording a new flight. Cloud-quality settings are not
manual weather. Regional package-backed recording and playback remain blocked:
no package ID/version/exact manifest identity was added to v3.

## Verification matrix

The new `replay_migration_tests` drive actual Bevy application systems. For each
of Light Single and Swift Sport, a fifteen-second live flight records actual
fixed-step pilot controls. Known-baseline test fixtures are serialized as v1,
v2-disabled and v3, then decoded through `ReplayFile` and replayed. These test
fixtures do not create a production old-to-new conversion API.

All six combinations verify:

- Exact final rigid-body state, executed simulation time, flight log, crash and
  touchdown state against the original live flight
- Recorded controls win over hostile live input and playback adds no live frames
- F5 discards paused wall-time budget; F6/F7 preserve speed behavior
- F8 rebuilds from frame zero in at most 240 records per update, mutes audio while
  seeking, retains pause, and exactly matches a separately advanced prefix state,
  time and log before reaching the same final state again
- The presentation clock tracks recorded elapsed time at the recorded rate
- Source format, every source byte and partial/complete identity survive pause,
  speed changes, playback, bounded rewind and completion
- The actual status resource and banner text/visibility retain the legacy notice

Startup tests load both aircraft and all three file versions through CLI parsing
and `resolve_flight_sources`. They verify default legacy rejection, explicit
baseline admission, v3 admission, unchanged original files, and rejection before
condition mutation when selected `yaw_rate_p` changes. Policy tests also show the
irreducible historical collision: a recording made with another omitted yaw
coefficient has the same legacy fingerprint and can only be admitted as partial
evidence under an explicit baseline assumption. Display renaming grants no
identity; custom legacy dynamics and missing/old identities remain rejected.

Clear, Rain, Fog and Custom-with-both-layers v3 fixtures prove modeled weather is
not silently played as Legacy. Each of the four manual weather flags is rejected
for every format before startup condition copies. Actual live app systems still
advance controls/physics with those flags and publish the disabled-recording
notice. The F9 handler is exercised on those non-recording flights. UI tests
separately prove notice persistence for running/paused/seeking/complete/fault
states and visibility removal when an inactive notice is cleared. A real
Taffy/Cosmic Text layout test uses the production banner and tutorial, actual
font glyph bounds and 1280 -> 640 -> 320 -> 1280 pixel resizes. Manual-cloud
notices stay clear of the live tutorial. Every legacy running/paused/seeking/
complete/fault banner preserves the full missing-yaw explanation and F8 controls
inside its text box, padded panel and viewport. Returning to live mode restores
the tutorial; clearing the notice hides both its text and background.

Static review first caught the long manual-cloud text crossing the tutorial in
narrow windows. The short UI label retains full explanation in startup/F9 logs.
The real-layout check then found the legacy glyph width was 481 px at a 640 px
viewport, exceeding the padded content width of 470 px. Bevy's final text layout
uses the whole text node width, so the banner now puts padding/background on a
parent and text in a separate child. The bounds were not relaxed: the final test
checks child containment within padding as well as all measured glyph bounds.

Pure sim tests preserve independently encoded v1, v2-disabled, v2-world and v3
fixture bytes through player operations. Cursor parity covers pause, overflow,
invalid elapsed time, zero-duration frames, clamped/nonfinite speed, seek and end.
The existing independent complete-identity, hostile codec, exact trajectory,
world/climate and numerical suites remain in the all-target run.

## Checks actually run

Linux, Rust 1.93, default features, coordinated `build-env.sh`, shared target
and `-j 2`:

- Initial `cargo test -j 2 -p flightsim-app -p flightsim-ui --all-targets`: 228 app
  and 196 UI tests passed. One pre-existing optional real-scenery fixture test was
  ignored because its external sample was not configured
- After the diagnostic/banner fixes, `cargo test -j 2 -p flightsim-app -p flightsim-ui --all-targets replay`:
  39 app and 11 UI replay-related tests passed, including the real-layout matrix
- Final strict app/UI/sim all-target clippy and whole-workspace formatting passed;
  the added explicit padding/clear assertions were also rerun in the focused
  real-layout test, with app clippy and strict app/UI documentation checks
- `cargo test -j 2 -p flightsim-sim --all-targets`: 339 passed across 36 binaries
- `cargo clippy -j 2 -p flightsim-sim -p flightsim-app -p flightsim-ui --all-targets -- -D warnings`: passed
- `cargo test -j 2 -p flightsim-sim -p flightsim-ui --doc`: 3 sim doctests passed;
  UI had no doctests
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-sim -p flightsim-app -p flightsim-ui --no-deps --document-private-items`: passed
- `cargo fmt --all --check`, `bash scripts/check-architecture.sh`,
  `git diff --check`: passed
- `python3 docs/qa/replay_identity_reference.py` and
  `python3 docs/qa/replay_v3_reference.py`: all independent hashes/bytes unchanged

Recovery initially exposed four scenery test helpers that fed new v3 bytes into
the historical v1/v2-only reader. They now use the explicit `ReplayFile` reader.
The recovered pure player parity test also referenced a nonexistent Swift
constructor; that timing-only test uses Light Single, while both real bundled
profiles are exercised by the app matrix above. During matrix development, the
key helper needed to release held keys between separate presses; final runs use
distinct F5/F6/F7/F8 presses. No production control behavior or acceptance
tolerance was adjusted to make the tests pass.

After another worktree used the shared target, a focused retry initially reused
a pre-v3 sim artifact and reported missing exports despite correct checkout and
Cargo metadata. Updating only this checkout's sim `lib.rs` timestamp forced a
confirmed build from its v3 sources; no source bytes, registry dependencies or
immutable binaries were removed or altered. The final focused tests and clippy
then compiled against the correct source. Integration must also invalidate
incompatible workspace-crate artifacts when switching isolated worktrees.

## Qualification limits

This is application-system and codec verification, not native GPU, screenshot,
physical controller, acoustic, Windows package, cross-platform floating-point
or publication qualification. The app matrix's airborne fixture has no touchdown;
existing terminal/crash tests remain in the retained suites. Real font/layout checks cover the three listed widths with the default font and
scale; they do not qualify every window size, DPI scale or native GPU output.

The commercial Windows candidate intentionally pins replay source. Its pin and
qualification runner are untouched. The source owner must review and requalify
the changed source with current native/Windows evidence; previous results cannot
be relabeled as acceptance of this migration. No release or binary is published
by this change. The separate [candidate-pin audit](replay-candidate-pin-audit-2026-10-04.md)
locates the exact frozen source, identifies the expanded helper boundary and
distinguishes a disclosed partial-identity smoke check from complete reproduction.
