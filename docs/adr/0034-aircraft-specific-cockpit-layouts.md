# ADR-0034: Select cockpit presentation independently of aircraft physics

- Status: implemented; successor source/native release qualification is separate
- Date: 2026-10-10
- Extends ADR-0033's generic cockpit with explicitly supported layouts

## Decision

Render owns `CockpitLayout`, original geometry, display artwork and bounded
animation. App selects the layout using profile family, ID, declared model path
and the actual accepted model. This metadata selection never selects dynamics,
changes profile bytes, or alters replay identity, admission or distribution.

Light Single/Meadow retain yokes and analog instruments. Swift has two pivoting
sticks and an original primary flight display. Kestrel has two sticks, analog
instruments with appropriate display ranges and one aggregate throttle lever.
References establish architecture rather than exact aircraft identification.

The active scene, not a blanket bounded-model exception, owns visibility and
interaction capability. Exterior views show GLB and hide generated interiors;
cockpit views with a generated interior hide GLB; fallback cockpit views retain
the GLB and 2D instruments. Staged parts acquire active markers only at commit.
This prevents opaque Swift canopy and duplicate static Kestrel panels/sticks.
It also excludes external wing/nose geometry from the generated cockpit view.

Unknown bounded models, missing/custom Kestrel models retain their fallback.
Unknown legacy profiles retain the generic trainer. No external profile is
silently treated as a specific real aircraft. Existing control, pause/replay,
focus, modal and fixed-step ownership remains. Kestrel parking brake stays a
separate existing system; service-brake release does not unlatch it.

## Validation

Test exact and rejected layout bindings, generated/fallback visibility through
commit/cancel, all-layout ray targets, default camera framing, stick extrema,
known instrument units/signs and wider jet scales, live fixed-step and replay
input locks. Inspect actual software-rendered captures for each reachable layout.
Native mouse/window and hardware qualification remain separately reported.
