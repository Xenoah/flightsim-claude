# Local-flight sun shadow reach diagnosis

## Verified mechanism

The original `sun_light_bundle` enabled directional shadows but supplied no
`CascadeShadowConfig`. Bevy 0.18.1 inserts that required component with native
defaults: four cascades, 0.1 m near bound, 10 m first far bound, 150 m maximum,
and 20% overlap. The app also retains the default 2048-square directional maps,
0.02 depth bias and 1.8 normal bias.

This maximum is **camera-forward depth**, not altitude above ground or distance
to a tree. `bevy_pbr/src/render/shadows.wgsl` selects the first cascade whose
far bound exceeds `-view_z`; beyond the last bound it returns fully lit. A flat
ground point 300 m ahead of a camera at 150 m AGL has 335.4 m forward depth when
the camera looks at it. Bias tuning cannot restore a shadow lookup that is
outside every cascade.

The inspected pinned engine implementation is the primary source:

- `bevy_light-0.18.1/src/directional_light.rs`: required config, map resolution,
  defaults and bias semantics
- `bevy_light-0.18.1/src/cascade.rs`: logarithmic split builder, rotation-only
  light frame, integer-ceiled extent and texel-snapped light-space centre
- `bevy_pbr-0.18.1/src/render/light.rs`: extraction multiplies normal bias by
  `sqrt(2)` before uploading it
- `bevy_pbr-0.18.1/src/render/shadows.wgsl`: view-depth selection, volume clipping,
  normal/depth offsets, inter-cascade overlap and fully lit result past the limit

The sun's orientation follows existing solar position/clock systems. Neither
direction nor illuminance is changed. Bevy already stabilizes cascade extents
and snaps their centres; no replacement stabilization algorithm is introduced.

## Initial one-variable control

The candidate explicitly sets only `maximum_distance = 2000 m` on the real sun
bundle. All other defaults remain. This extends local low-flight shadow reach
without coupling it to physical terrain, AGL, the 400 km camera far plane,
scenery streaming, replay state, wall-clock time or camera motion.

Two kilometres is a bounded local-view policy, not a claim that shadows reach
the horizon, the whole 4.5 km scenery footprint, or arbitrary cruising altitude.
The controls at 150/350/1000 m AGL have central ground depths of
335.4/610.3/1414.2 m. Keeping the existing 10 m first slice leaves that cascade's
matrix and texel scale exactly unchanged for the same camera/sun. The four-map
count and 2048-square size remain unchanged; larger volumes can nevertheless
admit more casters and increase draw/culling work. No CPU/GPU frame-time or
hardware-GPU performance claim has been measured for this change.

At the app's 60-degree vertical FOV and 16:9 aspect, applying Bevy's ceiled frustum
diagonal rule gives the following deterministic geometric values. Offsets are
normal-direction magnitudes before projection, **not measured contact errors**.

| Policy | Far bounds (m) | World texel widths (m) | Normal-offset magnitudes (m) |
| --- | --- | --- | --- |
| Original | 10 / 24.662 / 60.822 / 150 | .011719 / .028809 / .070313 / .172852 | .02983 / .07333 / .17899 / .44001 |
| Reach-only | 10 / 58.480 / 341.995 / 2000 | .011719 / .067383 / .393555 / 2.300293 | .02983 / .17153 / 1.00183 / 5.85559 |

The last two columns use 2048 texels and `1.8 * sqrt(2) * texel_width`.
Increasing range therefore trades farther precision for reach and can increase
detached-looking shadows. Keeping map count constant does not keep all middle
or far resolution unchanged. A hard final-distance cutoff remains; Bevy blends
adjacent cascades, but does not fade the last cascade to unshadowed terrain.

## Scoped regression evidence

The tests use the production sun bundle and Bevy's actual
`build_directional_light_cascades` system without a renderer/GPU:

- Before the source change, three reach tests failed and the unchanged-near
  control passed. Afterward all five shadow tests pass.
- Four finite, increasing splits, finite bounded final reach, enabled shadow
  casting and unchanged bias fields are verified.
- The first cascade matches the default matrix and texel width exactly.
- Ground receivers and 15 m crowns at the three low-flight controls are inside
  the selected actual shadow volume at solar elevations 5/30/89 degrees.
- Config bounds remain invariant under 40/60/100-degree FOVs, 1:1/16:9/21:9
  aspect ratios and a common multi-kilometre origin translation. Matrices remain
  finite. XY projection changes stay within one shadow texel plus small f32
  rounding; light-space depth error is below 1 cm in these controls.
- The far-limit test covers strict upper-bound exclusion and an off-axis point
  farther than 2 km radially but still inside 2 km camera-forward depth.
- Six existing daylight-system tests also pass. Focused Clippy with warnings
  denied, formatting and whitespace checks pass; the native app builds.

The synthetic translation test is not a full native floating-origin/replay/map
transition test. These tests establish config wiring and geometric eligibility,
not raster contact, acne, filtering, animation stability or scene performance.
Full integration gates and same-pose native image comparison remain separate.

## Same-pose range-only raster control

The actual software-Vulkan app was captured at the existing forest-hillside
150 m AGL replay pose near 47.1742644457 N, 9.5925149085 E, at the same lighting,
viewport and data settings. Both runs settled at 89 terrain surfaces and 204
bridges, retained 22 of 24 optional overlay groups, saved their screenshots and
exited successfully. The unretouched `forest-before.png` and
`forest-reach2km.png` were inspected side by side. The latter restores visible
tree shadows throughout the view, while some small-tree shadows remain blurred
or detached. This supports the range diagnosis, **not final contact quality**.

The original binary SHA-256 is
`af24ca99220e7d72ed04878287c05f523eaf8f25b7d9656a9dd6ee9f745d1ccf`;
the range-only diagnostic binary is
`c8379354199716349f406577b415002f8bead19bbd1c694e0ef245f7139b9d22`.
Images and logs are retained with local QA evidence. This diagnostic checkpoint
does not approve publication or establish moving-camera or hardware-GPU quality.

## Remaining diagnostic comparisons

Use the same settled hillside pose, source files, solar time and surface-detail
setting for the original and range-only binary. Verify which new ground shadows
appear, then inspect tree bases, crowns, roofs and aircraft for contact separation
and acne. A successful reach comparison does not close the prior shadow-gap
limitation. If middle/far resolution is unacceptable, compare a separately
identified fixed-split candidate before changing normal bias; if bias is then
changed, compare it separately with the same map configuration. Keep ground-level
aircraft/runway, hillside low sun and distant cutoff views as controls.

No DEM, terrain normals, procedural object positions, foundations, palette,
physical collision, flight dynamics or replay format are changed here.

## Fixed-split alternative: not selected

A separate diagnostic control (`a13e86e`, binary SHA-256
`ce151268942d845f254d5d2d0adf8202bb6bee1fc53bfd894d444725f6e0b8e3`)
used 32 / 128 / 512 / 2048 m far bounds with the same map count, resolution,
overlap and biases. At 60 degrees / 16:9 its texel widths were
0.037109 / 0.147461 / 0.588867 / 2.355469 m. This kept the 335.4 m ground target
out of the last-cascade blend but coarsened the close 10 m slice and the default
35 m-back / 10 m-up chase view's aircraft slice.

Its actual hillside capture retained the same settled 89/204 cut and optional
overlay counts. Two independent image reviews found preserved coverage but no
decisive gain over range-only; some shadows became softer. In screen rectangle
x=210..1259, y=80..499, range-only to fixed-split mean absolute RGB change was
0.1232/255; 634 pixels darkened and 2709 lightened by more than five mean channel
levels. These are change-localization statistics, not contact/quality scores.
The fixed splits are therefore **not selected for production**. The smaller
range-only correction preserves the original closest cascade, while the known
normal-bias/contact limitation remains. Near-aircraft/runway verification and
final integration gates remain separate; no diagnostic images are published here.

## Shared-target verification precaution

The first fixed-split test run caught a stale library from another checkout:
new tests observed the old 150 m config. Cargo's shared-target dependency paths
were relative and the other library artifact was newer. That failed log is
retained as an artifact/source mismatch. Touching all current-checkout Rust
sources and crate manifests forced a rebuild, after which five cascade and six
daylight tests, focused Clippy and the native build passed. Runtime source/test
and working-diff hashes matched across the app build; a complete source manifest
also matched during versus after it.

For every checkout switch: take an exclusive build lease; record source content
hashes; refresh all workspace Rust/manifests, including shader includers, without
changing contents; confirm identical hashes; run behavior tests that distinguish
the intended source; and record/copy the immutable binary before releasing the
lease. No cache deletion is necessary. Final integration repeats this protocol
on its own exact source rather than relying on an earlier Cargo success.

## Near-aircraft/runway acceptance control

A fresh pair used the same previously recorded 56-second Swift takeoff replay,
1280×720 offscreen viewport, clear weather, and identical data/assets. The original
and reach-only binaries listed above both saved their images and exited with
status zero. Both ended at 47 ft AGL, 91 kt EAS, +1730 ft/min and 13.4-degree pitch;
the settled terrain cut was 44 surfaces and 110 bridges. All four required airport
overlays remained visible, with maximum authored displacement 0.004 m.

Independent full-frame and enlarged pixel inspection found no new aircraft
self-shadow defect, detached or misplaced aircraft ground shadow, runway-marking
occlusion, or horizon change. The shadow silhouette and position remain
consistent, with small edge differences. The pair differs at 2052 of 921600 pixels
(mean absolute RGB-channel difference 0.00436 on a 0–255 scale). Those counts are
an image-difference description, not a visual-quality or performance score.

This control and the forest comparison accept the conservative reach-only policy
for restoring local airborne shadow coverage. The fixed-split experiment remains
rejected. Tree contact softness/separation, the final-distance cutoff, moving-view
stability, and hardware-GPU performance remain distinct limits; they are not
claimed solved by this acceptance.
