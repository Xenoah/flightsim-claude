# Local areas, draw distance and water: integrated QA, 2026-10-04

This milestone adds prepared local DEM packages, bounded visual distance settings
and an initial procedural water reflection material. It does not yet download
GitHub datasets, convert arbitrary repository ZIPs, implement all-weather presets
or add the requested aircraft families. Those remain later milestones.

## Reproducible runtime evidence

Runtime checks used Rust 1.93.0, Bevy 0.18.1 and software Vulkan llvmpipe on the
cloud Linux desktop. The optimized development profile uses local-crate opt-level
1 and dependency opt-level 3. It is not a hardware GPU performance qualification.
All listed PNGs are captured by the actual application; no image enhancement was
used. Windows native interaction, controller hardware and audio are not covered.

Immutable executables distinguish the rendering and area-integration checks:

| Runtime source | Executable SHA-256 | Scope |
|---|---|---|
| `bec77bb8623b69a97a1b5295e78a81baf577a6b1` | `6cea4d59755c50a3fee6a346225d46a105720e57924ea7cf229faa567b9c49c2` | Matched water/distance views and live water lifecycle |
| `5eb3894c3fa5d319299c239fad08046ff10a436a` | `920970feacb81aa9f7dc5a2a222fb3a6d349b4ac799894c28e0923001615688d` | Real prepared-package import/list and native activation |
| `7cc864af06312b6ef411ee3f737216c5a85ff5c0` | `386042d494a7850214792b653190edff02c14c75707efb2bd14ff640693493fb` | Native keyboard selection and regional-to-global restoration |
| `8c0f729f2be9e53c242e47a50f655799da9e94ac` | `29e9dc1cf01064cf96eb4a47efb1c78b3eb83013540e9c85fd3554ffd30413a5` | Final native logical-arrow credits paging retest |

These are development commit identities, not claims about a public binary release.
The final source/CI identity is correlated separately with the publication receipt.

## Water: visible benefit and limits

A fixed June 21 Pacific view at 0°N, 150°W, 3,000 m AGL, heading 70° and local solar
07:30 compares default Light with High, with Graphics Light and Clouds Off. The
same zero-duration replay fixes camera/aircraft state. High adds a broad sun path
and angular sky response on the existing water surface. This is a shader change;
there are no added wave vertices or displaced physical water.

| Capture | SHA-256 |
|---|---|
| `proof01-light-pacific-glint-3000m.png` | `4404ccd3d49c63d792d2521ed63a710a0c754905dd402b8d7cd74b68e0b805a7` |
| `proof01-high-pacific-glint-3000m.png` | `01b22b314aba97815e069315075fe69c4604b94b53ce8238a21908d34002a42c` |

Additional actual captures cover 120 m AGL Ultra, midnight, an opposite heading,
California coastline and inland/South Bay, Lake Superior, the date line and
89.95°N. All completed with exit 0 and upper pipeline readiness, without shader
errors. The stronger inland view confirms land remains land at that viewpoint;
it does not qualify small islands or detailed coastlines everywhere.

The near-field crossed-wave lattice remains conspicuous. This is an initial
reflection improvement, not a fully realistic sea. The mask is approximately
20–25 km, so detailed bays, small islands and locally imported shorelines can be
unresolved. The stepped distant horizon already occurs in Light. There is no
foam, surf, displacement, scene-object reflection or reflection of actual cloud
shapes. Static images do not prove absence of temporal shimmer.

Native checks covered cold High preparation → Light cancellation → High retry,
map input capture, pause, High/Ultra changes, distance changes, Graphics High with
Ultra water, a live origin rebase and return to Light. The observed upper mask is
3,145,728 bytes; returning to Light reports zero owned mask bytes, proxies and
materials. Shared driver/engine pipeline caches are outside those counters.

The 192 mask preparation batches and cold shader compilation are preparation
costs. The isolated CPU measurements are [recorded separately](water-material-numeric-2026-10-04.md).
Existing frame-statistics phase labels follow terrain/scenery readiness and do
not yet classify water preparation; their apparent steady averages must not be
used as comparative water frame-performance results.

## Draw distance: actual resource/quality tradeoff

Matched regional views use the original Balzers terrain/scenery fixture at
47.0674°N, 9.5034°E and the same fixed replay, with Graphics/Water Light and Clouds
Off. The complete displayed cut matches the desired/live cut in every receipt.

| Preset | Displayed terrain tiles | Visible bridges | Reported geometry KiB |
|---|---:|---:|---:|
| Short | 68 | 160 | 12,505 |
| Standard | 119 | 271 | 22,361 |
| Long | 215 | 471 | 43,832 |

These count renderer-managed geometry, not total process/VRAM consumption. No
hardware FPS improvement is inferred. Nearby buildings, fields and roads remain
consistent in the three images, with no exposed sky cracks or missing terrain
patches at this viewpoint. Short noticeably flattens the distant mountain
silhouette into a broad plateau-like ridge; Long preserves more relief and adds
distant scenery. Short is therefore an explicit quality/resource tradeoff.

Native F2 cycling and Shift+F2 restoration passed while paused and in flight,
including after regional activation. The coarse globe remains present; these
controls change local terrain/scenery detail rather than the camera horizon.
Unit tests independently cover the polar conservative-distance regression,
whole-globe coverage, unchanged hard budgets, cancelled scenery work, partial
uploads and unchanged physics/replay bytes. Stills do not establish universally
smooth transitions.

## Real prepared terrain package

The validation fixture wraps the existing 765 GLO-90-derived L10–13 DEM tiles
without modifying their bytes. It has ID `liechtenstein-glo90@1.0.0`, declared
coverage 9.31640625–9.84375°E / 46.93359375–47.4609375°N, nominal source resolution
90 m and EPSG:4979 runtime heights. Source EGM2008 DSM heights were normalized
using the documented matching geoid data; this is not surveyed ground or a
resolution increase. No OSM city geometry is included in this DEM-only package.

Prepared ZIP: 6,551,156 bytes, SHA-256
`fc4de5f479d769b0aa6f028ab965a8798f9c11bdbae157bffc57ecd45df66b2a`.
Manifest SHA-256:
`34e8759340c315c745a9d83746944d082bb94b8139e2806756749572b7d53384`.
The package retains bounded source notices, original license text and source
hashes. PDF form-feed page separators were normalized to blank lines for text
metadata; no license words were removed. These records describe provenance and
do not substitute for release-rights review.

Production CLI import/list passed. Native selection showed separate pending and
active sources, displayed credits, returned to the map with Escape and activated
only after Enter/Start new flight. A new regional DEM flight, matching location,
AGL display and regional replay notice appeared. F9 produced the explicit v1/v2
replay-unavailable reason. Map dismissal retained the selected active flight and
paused state. The first activation briefly displayed a loading frame before the
new complete terrain cut appeared.

The visible G/0/1–5 keyboard controls also passed the full regional-to-global
round trip: choosing Global kept the current regional flight active until Start,
then restored baseline terrain and removed the regional notice. F9 subsequently
saved 1,527 frames (13 seconds) of the baseline flight.

Native testing exposed a valid logical-arrow navigation gap for keypad/remapped
physical keys. Integrated InputPlugin → app synchronization → UI Text tests
reproduced the failure and proved persistence across 90 idle updates. The fix
accepts semantic navigation once, retains modifier guards and keeps numeric
selection on the labelled physical top-row keys. The final native Right/Left
paging, repeated return to page 2 and Ctrl+Right guard all passed.

Cloud-desktop bound mouse events did not activate Bevy buttons. The observed
keyboard path succeeded; OS drag-and-drop delivery is not claimed as tested.
Automated app/UI tests cover drop-event routing, cancellation, stale completions,
source cleanup, return to baseline and invalid packages; they do not replace
native platform event testing.

## Gates and publication

The full baseline aggregate on `5eb3894` passed 1,989 Rust tests, 178 Python
tests and 53 untimed Criterion smoke cases, with zero failures and three explicit
fixture/manual-timing ignores. Workspace all-target Clippy, formatting, strict
private-item docs, architecture and release-profile benchmark compilation passed.
After the keyboard changes, all app/UI tests passed again (219 app and 194 UI,
plus the existing app fixture ignore), with strict app/UI Clippy and formatting.
The final production changes received an independent read-only review.

[Local-area automated cases](local-regions-2026-10-04.md) and
[archive boundary checks](content-packages-2026-10-04.md) retain their scope.
The canonical Windows MSVC default-feature inventory now has 359 packages, 705
package notice files plus two supplements and 348 registry checksums. Its status
remains `not_reviewed`. Existing rights, review and release-authorization gates
remain active; this milestone does not establish a distributable commercial
binary or publish a new alpha release.
