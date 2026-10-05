# Cockpit replay-help clearance — 2026-10-05

## Scope and reproduction

This is a UI-only correction for the compact replay help covering the cockpit
V/S dial in the ordinary and analytical native 830×582 captures. The original
images were inspected directly: `hdr-baseline-narrow-cockpit-040/native.png`
and `hdr-proof-narrow-cockpit-042/native.png` in the retained native QA set.
The unresized case 041 is not evidence for this window size.

The new real Taffy/Cosmic Text regression reproduces that geometry on the
unchanged implementation, before the fix:

| Element | Measured bounds at 830×582 |
|---|---|
| Six-pack | x312–518, y354–489 |
| Compact replay help | x507–818, y417–485 |

The initial test fails on their intersection. Previous complete-HUD tests
measured the text instruments but did not spawn the separate cockpit dial panel.

## Correction

Only cockpit replay help receives a maximum width equal to the space to the
right of the centered six-pack, with the existing 12px screen inset and an 8px
gap. The panel width is shared with the actual dial layout. At compact sizes,
five short rows retain pause/resume, speed, rewind, view, map preview, LAN exit,
input diagnostics and the ignored-flight-controls notice. Help stays 14px;
notices, HUD text and instrument labels retain their existing font sizes.

The width is computed from the current logical viewport and UI scale before
layout. The updater joins the existing committed-flight display set so a view
change uses the same published state as instrument visibility. Leaving cockpit
replay restores the original help width and text selection.

No dial moves relative to the modeled dashboard. No physics, controls, replay
codec or interpretation, notice content, attribution content, renderer,
tonemapping, Cargo feature, candidate, rights or release file changes are included.
No app `main.rs` wiring is needed.

## Checks

- The new 830×582 regression failed before the production change and passed
  afterward, using actual layout and font measurement with all six dials spawned
- All 262 UI library tests pass: the previous 259 plus three new regressions
- New coverage includes 640×480, 830×582, 899×600, 900×600, 1180×812 and 1280×720;
  both sides of the compact/full reference boundary; playing, paused,
  seeking, complete and faulted replay; persistent legacy/recording, jet and
  turboprop notices; wrapped credit; complete help/notice glyphs; resize in one
  update; and restoration of Chase, Free, Tower and live help
- Existing test/helper bodies and candidate acceptance fixtures are unchanged
- `cargo fmt --all --check` and `bash scripts/check-architecture.sh` pass
- `cargo clippy --locked --offline -j2 -p flightsim-ui --all-targets -- -D warnings`
  passes; the subsequent [ordinary/analytical native verification](analytical-build-native-2026-10-05.md)
  confirms the 830×582 correction in both modes and repeated view/map/resize
  behavior in the analytical build

Commands use the task-local Rust 1.93 environment, `--locked --offline -j2`,
and warnings denied. The full UI command is
`cargo test --locked --offline -j2 -p flightsim-ui --lib`.

## Preserved small-window limitation and failed intermediate checks

This does **not** qualify the entire 640×480 cockpit. The unchanged long
turboprop notice with `STALL WARN N/A` occupies x338–628/y12–289. The fixed
six-pack occupies x217–423/y252–387, so those panel areas intersect. The long
left HUD can also reach the dial region. These are separate layout constraints;
this scoped fix neither hides those warnings nor moves the dials to mask them.
Panel geometry is not a native pixel classification of every overlapping glyph.

The first width-only correction failed the new 640×480 test: help reached
y305–440 while attribution began at y433. A six-row attempt then left the log
at y414–448, also intersecting attribution. The five-row cockpit reference
resolves those newly introduced text/footer collisions while retaining every
action and notice. Those failed attempts are retained in the task QA evidence;
they are not included in the final passing count.

At 640×480 the new tests therefore establish complete help/notices, their
separation from each other and attribution, and clearance of help from dials.
They explicitly do not claim clearance of the separate long notice/HUD from
dials. The original 830×582 defect and normal-size notice clearance are checked
separately. Arbitrarily smaller windows or longer external notice text are not
qualified. Physical-GPU, Windows, performance and release acceptance remain open.
