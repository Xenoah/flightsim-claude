# Small-window world-map layout

Date: 2026-10-04

## Problem and change

Native 640x480 acceptance exposed aircraft/weather controls overlapping or
clipping below the map. The 308px sidebar and fixed header/footer also consumed
more room than this viewport allowed. Existing map layout tests covered 720px
high desktop layouts, but not this small-window interaction path.

The map now has one measured, vertically scrollable body. Below 900 logical
pixels wide or 600 high, the raster and full-width sidebar stack. Larger windows
retain the side-by-side map, its 740px maximum width and 2:1 raster. Header and
recording disclosure wrap outside the scroll body. Text and button sizes stay
unchanged. Long unbroken identifiers use word-or-character wrapping without
truncating additional content.

Credits, Regions/Downloads and Wind have independent bounded scroll surfaces.
Their rows wrap or stack as needed, and each overlay retains a scroll/return hint
outside its clipped panel. Only the active modal consumes wheel input, including
opening and closing frame ownership. Closing the entire map clears scroll;
returning from a child keeps the map's place. No key is reassigned to scrolling.
The existing display system owns this presentation work, so app registration,
Start/Cancel, physical conditions, replay locks, source and download policy do not
change. No strict replay manifest is modified here.

## Automated evidence

`crates/flightsim-ui/src/world_map_layout_tests.rs` uses actual Bevy/Taffy layout,
Cosmic Text shaping, UI clipping and pointer hit testing without a GPU. At
640x480, 899x600, 900x600, 900x720, 1024x720, 1180x812 and 1280x720 it checks:

- Every map/child text byte is shaped; text fits its measured node, stays within
  the horizontal bounds, and is reachable across the finite vertical scroll range.
  Glyph bounds also fit all intermediate non-scroll clipping ancestors
- Sibling controls and text do not overlap, button heights are retained, and the
  wrapping map header/footer remain outside the body
- Pixel- and line-unit wheel input clamp at the top/bottom; only the active modal
  moves, and hidden wheel input cannot scroll a later opening
- Actual pointer clicks select an aircraft, open/cancel wind, submit Start and
  invalidate that request by Close; clicks inside the clipped portion of Start
  cannot submit from the footer, and clicks inside a clipped raster portion
  cannot change the geographic selection
- Actual Regions clicks preview a download, request acquisition, cancel and
  return without selecting installed terrain or starting a flight
- Paginated credits retain the final notice and their controls remain clickable
- Resizing after scrolling reflows and clamps rendered bounds in the same layout
  pass; replay still blocks aircraft, wind and Start clicks

Validation on this patch:

- `cargo test -j 2 -p flightsim-ui --lib`: **245 passed**
- `cargo clippy -j 2 -p flightsim-ui --all-targets -- -D warnings`: **passed**
- `cargo fmt --all --check` and direct rustfmt check for the included test file:
  **passed**
- `bash scripts/check-architecture.sh`: **passed**

Wheel input regressions also reject non-finite events and overflowing sums of
finite events; scroll offsets remain finite and unchanged for these batches.

Existing desktop map, coordinate editor, transaction generation, region/download
and wind suites remain part of the full UI test run. The measured fixture now
includes Bevy visibility propagation for genuine pointer tests. No screenshot,
GPU, speaker, platform or new physical-law claim follows from these checks.

## Boundary review follow-up

At 900x600, centered desktop columns placed the north map label at y45–61 while
the scroll body began at y62. That top slice could not be reached by scrolling.
Desktop columns now start at the body's top edge. Their widths, 2:1 raster, fonts
and controls are unchanged. Stacked layouts keep horizontal centering. The
initial `bb7ba19` binary predates this correction and is intermediate evidence.

A separate concern about fixed-height region rows did not reproduce glyph loss.
With a long canonical-shaped key at 900x720, the 43px row spans y281–324 and its
44px text line box spans y280–324, but actual glyph ink remains y282–320. At
900x600 the row spans y261–304 and ink y262–300. The stronger ancestor-clip checks
pass at both sizes and on the 899x600 stacked side of the breakpoint. Region row
runtime behavior was therefore left unchanged.

## Native acceptance still required

On the final integrated app build:

1. At 640x480, open M and wheel through the full map/settings body. Read aircraft
   names, weather, wind, coordinate/month/layer controls and recording disclosure
2. Click aircraft arrows, open/cancel and apply the wind editor, then Start a new
   flight; verify Cancel/Close leaves the active flight intact
3. Open Credits and reach its final page, then Regions Installed/Downloads with
   long rows/source notices; check scroll, explicit actions, cancel and return
4. Resize 640x480 -> 1180x812 -> 1280x720 and back while scrolled. Also check
   900x600 and 899x600 around the layout breakpoint. Check raster aspect, reachable
   north label, absence of overlapping controls, header/footer and text sizes
5. Open map during replay and verify the aircraft lock, disabled wind/Start,
   and return to unchanged playback

The lead owns native evidence, final source binding and publication. This record
must not be treated as native acceptance before that separate result is attached.
