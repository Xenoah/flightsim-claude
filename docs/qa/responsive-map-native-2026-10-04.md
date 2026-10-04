# Responsive world-map native acceptance

Date: 2026-10-04 UTC. Native app cases 39 and 40 passed on integrated runtime
`9474d059dd0a00fef46a214bbf85b9d4f1636acf`, Linux CPU llvmpipe. The subsequent
Windows test-witness correction and merged source bindings change no runtime
source used by this binary. This is UI/lifecycle evidence, not GPU performance.

## Live map

At 640x480 the header/footer remain readable, and wheel scrolling exposes all
map, aircraft, weather, wind and departure controls. A real pointer selected a
Meadow draft while Swift remained active. Wind editing accepted 270 degrees,
13.25 knots and Light turbulence, retained that draft through aircraft preview,
and exposed Apply/Cancel by scrolling. Canceling the entire map discarded both
aircraft and wind drafts: reopening showed Launch/Swift, zero wind and the
original zero turbulence bound. The paused native recordings before/after Cancel
are byte-identical: 175,411 bytes, SHA-256
`441a4f115d4a6ca119923e3cf03fe18a0df30ac77c0f4e84c63d03fdb3c83ea4`.

Global credits reached page 3/3. Installed/Downloads and all catalog provenance
reached page 2/2, including wrapped hashes and offline restrictions. This was an
existing locally seeded offline QA catalog; its placeholder URL was not fetched
or presented as a usable public dataset. Preview did not activate terrain.

Repeated resizing covered 640x480, 900x600, 1180x812 and 1280x720. At the corrected
900x600 boundary, scrolling to zero exposes the complete north instruction;
all lower controls remain reachable. Back at 640x480 a real pointer selected the
current aircraft position and pressed Start. A new Swift flight appeared at
1000 m AGL with calm wind, confirming the canceled wind draft did not leak.

## Replay map

Case 40 loaded the actual Cedar profile-v3/v5 recording. At 640x480 aircraft
selection and wind editing stayed locked; clicking them had no effect. Scrolling
reached disabled Start, which could not replace the flight. Repeated open/click/
return cycles preserved the displayed paused cursor 0:44, 85 kt, 1347 ft,
-637 ft/min, heading 76 degrees, pitch -0.7 degrees, bank -38.6 degrees,
turbine fraction 0.366, shaft 180.03 rad/s and blade pitch 23.58 degrees.
This is a display comparison; complete numerical v5 reproduction is documented
separately in the turboprop app acceptance. Both native runs exited normally.

## Evidence and limits

The [receipt](responsive-map-native-receipt.json) pins the native launcher logs,
actual captures, cancellation snapshots, independent source review, retained
245-test UI evidence and the combined 71-test candidate check. The merged
contract retains 118 whole-file pins and all 102 independent anchors; existing
validator functions, rights gates and physical admission remain unchanged.

Case 39's automatic PNG caught the wind editor during text entry; case 40 shows
the replay-locked controls. These are actual app frames, not mockups. The native
X11 text-entry provider was unavailable; physical digit-key events completed the
same editor inputs. No app workaround or input remapping was introduced.

An inherited limitation remains: wheel input can scroll a covered pause
reference underneath an open map. This changes no flight state and did not
invalidate cancellation/replay checks. Speaker output is unqualified because
the cloud runtime has no audio device. Exact Windows candidate validation and
binary rights/release gates are separate; no binary release follows from this
UI acceptance.
