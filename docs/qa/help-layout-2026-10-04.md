# Flight HUD/help layout regression — 2026-10-04

Base: `5d6c9c9615122fe0bfdc1743b1ed1edb01a1a02b`.

The native Windows 1280x720 `default-swift.png` showed the final instrument rows
(GFX, CLD, monthly model) over the beginning of help. Top and bottom roots reserved
space independently. The fix measures instruments beside a notice/tutorial/help/log
column, with attribution reserving its actual wrapped height below both columns.

Instrument/help/notice fonts remain 18/14/16 logical pixels. Small viewports use
explicit compact flight references and a two-row log. Live Esc opens the complete
reference inside the existing paused state; the wheel scrolls it with a persistent
resume hint. No keyboard, physics or replay transport behavior changes.

## Automated evidence

- Final `cargo test -j2 -p flightsim-ui --lib`: **238 passed**
- Final `cargo clippy -j2 -p flightsim-ui -p flightsim-app --all-targets
  --features flightsim-app/region-downloads -- -D warnings`: **passed**
- `cargo fmt --all --check`, `bash scripts/check-architecture.sh`, and
  `git diff --check`: **passed**
- Combined UI/app all-target tests with `region-downloads`, before the final
  replay-reference hardening: **app 361 passed / 1 existing ignored**, black-box
  integration **1 passed**, UI **237 passed**. Final replay-reference changes
  were rerun in the full UI suite and final UI/app clippy; no app input or transport
  code changed in that hardening.

The new tests run real Bevy UI/Taffy and Cosmic Text font layout without a GPU.
They measure all visible instrument/help/log/tutorial/notice/footer rectangles
at 1280x720, 1180x812 and 640x480, plus same-update resize and live/replay changes.
They retain the complete STALL WARN/N/A glyph checks and test a long explicit
roll/yaw-trim notice together with wrapping attribution.

At 640x480 the paused reference test scrolls the actual text to its measured end
and checks its scroll position resets on resume. A separate unchanged-guidance
live→replay→live regression checks replay keys and updated notices replace live
throttle/trim/restart instructions. Its explicitly supplied paused+replay state
checks reusable UI rendering; ordinary replay still ignores Esc and F5 continues
to control only the existing replay transport.

The reference has no keyboard handler. Its wheel reader changes only UI scroll
position, drains events while unpaused, and is separate from flight input. Existing
app pause/input-release and recorder-freeze regressions passed in the app group.

## Limits and remaining acceptance

Native rendering and mouse-wheel/Esc interaction need verification on the final
integrated build. This source-only evidence does not claim a native pass. At 320px
width the existing stacked stall-warning protection remains tested; fitting every
flight panel into that below-desktop viewport is not claimed.

## Paused attribution follow-up

Independent review found that the opaque pause panel still used a viewport-bottom
inset and could cover upper lines of wrapped attribution. The pause panel is now
an absolute child of the measured HUD body above the footer, created after the
HUD. Its position follows that body's current layout; no previous-frame text
height or offset is copied. Standalone use without a HUD retains its viewport
fallback. Replay wording, pause visibility, input bindings and controls are
unchanged.

The full UI suite now passes **239 tests**. The added real-layout witness keeps
pause visible while resizing through 640x480, 1180x812 and 1280x720 and clearing /
restoring credit. Its credit really wraps at every size, and the complete measured
footer remains outside the opaque overlay. The existing 640x480 live/replay
reference-scroll test now also sets attribution. Focused UI all-target clippy,
formatting, architecture and whitespace checks pass. Native acceptance still
belongs to the final integrated build.

## Native scene-clearance follow-up

Native 1180x812 inspection showed that left-aligning help within the broad side
column put its text over the centered chase-view aircraft. Help now remains
content-sized but aligns to the right edge. The measured whole-HUD regression
requires desktop help to stay to the right of the central 40–60% scene band;
640x480 retains the same compact reference and bounded edge layout.

The full paused reference now has an opaque dark background so underlying flight
text cannot bleed through it. Footer ownership, font sizes, controls, scrolling
and replay behavior are unchanged. The opacity regression rejected the former
0.92-alpha background before the property correction. Native visual acceptance
must still be repeated on the integrated build containing these corrections.
