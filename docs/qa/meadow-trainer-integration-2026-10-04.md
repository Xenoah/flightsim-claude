# Meadow Trainer source and native integration, 2026-10-04

This adds an original high-wing exterior and external profile. It does not add
a newly calibrated flight model: every numeric dynamics field, all controls and
the camera eye equal Light Single, including `yaw_rate_p`. The built-in aircraft
catalog, default aircraft, FDM, input code and existing models are unchanged.

## Asset and profile checks

`python3 tools/blender/validate_meadow_trainer.py` passed on the integrated tree.
It reads the actual GLB binary: chunk/accessor bounds, finite coordinates,
normalized normals, valid nondegenerate triangles, mesh budgets, all three
physical wheel contacts, intended CG/camera containment, and unit scale.
The profile also passes the public draft-2020-12 v1 JSON schema. Original
Light/Swift profile/model/source hashes remain exact. The source-tool CI now
runs the asset validator without Blender or graphics dependencies.

The actual delivered GLB is 140,864 bytes, 3,759 vertices, 5,756 triangles,
46 primitives and eight materials. It contains no images, external buffers,
resource URLs, animations or downloaded geometry. Its SHA-256 is
`c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412`.
The editable Blender source and deterministic generator are included; the author
verified a byte-identical second GLB and re-imported the canonical export.
All three actual Cycles preview images were independently inspected.

## Actual application checks

The cloud desktop ran the existing immutable `8c0f729` application with the new
asset checkout as its asset root and the external JSON path. That executable's
SHA-256 is `29e9dc1cf01064cf96eb4a47efb1c78b3eb83013540e9c85fd3554ffd30413a5`.
It has the same unchanged renderer/FDM/profile loader used by this source slice;
later accepted main changes concern package portability, schema tests and the
optional downloader. No application code change is needed for the new asset.

Vulkan selected Mesa llvmpipe CPU rendering. These checks establish appearance,
input and lifecycle behavior, not hardware GPU performance. The machine had no
audio device; sound selection logged `piston`, but audible output was not checked.

- `meadow-native-ground-01`: correct forward/up orientation, stable parked gear
  and shadow, actual model-fit log `8.30 m ... scale 1.0000`. The existing `=`
  throttle binding accelerated the aircraft and retained its setting on release.
  The unedited final app screenshot shows liftoff at 78 kt EAS, 72 ft AGL,
  +972 ft/min and 87% throttle; process exited 0. CUA's Page Up input did not
  change throttle in this run, so that UI route is not qualified by this check.
- `meadow-native-takeoff-02`: the next run was interrupted at the operator level
  before pilot input. The application still reached its scheduled screenshot and
  exited 0. It supplies no additional takeoff or control evidence.
- `meadow-native-cockpit-cruise-03`: airborne start at 1,000 m AGL, 65% throttle,
  normal difficulty, calm wind, original trim. The shared cockpit showed the
  established interior without the exterior blocking its view. A short W input
  changed pitch, release returned control to the existing trim, and flight
  continued. Cockpit/chase/free/tower/cockpit switching completed. The final
  unedited screenshot at three minutes airborne shows 75 kt EAS, 5.5 degrees
  pitch and +195 ft/min; process exited 0. This is brief native acceptance, not a
  complete approach/landing/stall or long-duration stability qualification.

The two useful app screenshot SHA-256 values, retained with the supplied QA
captures, are `d4ae1f79160e5468bb618c23fcc4b0491fcc841e1ac2acff32280fd60b3c62fe`
(ground-to-liftoff) and
`d8afcb779f163109779ddfa66c71b4828e4bfc1048e74bfe826df0cc4a965ce7`
(cockpit/flight). Studio renders above are explicitly a different form of evidence.

Independent source/asset review accepted this bounded slice with no blocker.
The lead also ran 55 commercial staging/readiness tests, 24 Swift Windows
candidate policy tests and both profile-schema tests successfully. All five
frozen legacy source hashes still match. These policy tests do not themselves
run or qualify a Windows binary.

## Distribution boundary and remaining work

The three new aircraft files are recorded as original source-only assets in the
rights inventory. Both binary packaging allowlists remain byte-for-byte unchanged:
the Swift-only candidate still stages only Swift GLB/JSON. The default dependency
inventory is recollected from the unchanged Windows default-feature metadata;
only the asset-manifest hash changes, with the same 359 packages, notice bytes,
four unresolved records and `not_reviewed` state. The source readiness check has
zero integrity blockers and six existing review blockers. No license receipt,
commercial release approval or binary-candidate qualification is manufactured.

The aircraft currently requires its external profile path; it is not a third
built-in entry. Propeller/wheels/control surfaces are static and glazing is opaque.
It reuses the common cockpit. New turbine/jet/supersonic force models, family-specific
instruments/guidance and additional original models remain separate milestones.
