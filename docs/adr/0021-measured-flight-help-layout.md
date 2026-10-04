# ADR-0021: Measure flight help with the instruments and notices

- Status: implemented; native acceptance is a separate gate
- Date: 2026-10-04

## Context

The native 1280x720 HUD placed graphics, cloud and monthly-weather labels over
the first help lines. Instruments were top-anchored while help was independently
bottom-anchored. Existing tests protected attribution and stall notices separately,
so neither checked this collision. Moving a fixed vertical offset would only move
the failure to another window size or a longer notice.

## Decision

Use one bounded root. Its footer reserves actual wrapped attribution height; its
body measures the instrument column beside a grid containing notices, contextual
guidance, help and the flight log. Preserve 18px instruments, 16px notices and
14px help. Below 900 logical pixels wide or 600 high, select a concise flight-key
reference and combine the four log values into two rows. Aircraft-specific compact
guidance is supplied explicitly by the app, independently of its complete text.

The existing Esc pause state displays the full aircraft reference and display
settings, with an always-visible resume/scroll hint. Mouse-wheel scrolling is
bounded to measured content, is ignored while unpaused, and resets on resume.
No flight key is reused for reference navigation. Replay has its own complete
compact playback-key list and suppresses live takeoff guidance.

## Alternatives and costs

- Smaller fonts make already-small controls harder to read and do not bound long
  notices; the established font sizes are retained.
- A scroll-only flight reference hides essential controls while flying. Scrolling
  is limited to the complete reference during the existing paused state.
- Independent offsets do not account for text wrapping, attribution changes or
  resize; shared Bevy/Taffy layout does.

Help moves beside the instruments. Very narrow 320px windows retain the existing
stacked warning protection, but are not claimed to fit every flight panel.
No physics, input mapping, replay format, release or attribution content changes.

## Evidence

Real Taffy/Cosmic Text tests measure every flight panel together at 1280x720,
1180x812 and 640x480, including live/replay, long trim notices, wrapped attribution
and same-update resizing. A separate 640x480 pause test scrolls the complete
reference to its end and verifies resume resets it. Platform rendering and native
interaction remain separate acceptance evidence.
