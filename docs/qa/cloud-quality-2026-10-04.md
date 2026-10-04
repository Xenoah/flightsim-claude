# Cloud quality qualification — 2026-10-04

This milestone adds independent Off/Light/High/Ultra clouds. F3 cycles settings;
Shift+F3 restores Light. The weather inputs remain the same across settings.
See [the user guide](../cloud-quality.md) for provenance and scientific limits,
and [ADR 0013](../adr/0013-bounded-cloud-quality.md) for resource ownership.

## Scope and evidence

The implementation was compared with the accepted graphics baseline tree
`6f2c0b08907702ef0af77bcf834b1d89ff508a6d`. Cloud implementation code was tested
through local commit `2d361963af375b9cc43523ae11839efa6b2ff9ef`; subsequent
qualification-report edits do not alter runtime code. Actual native and fixed-view
captures used immutable development binaries with recorded SHA-256 and fixture
hashes. The cloud desktop runs Vulkan through **llvmpipe software rendering**.
These results do not qualify hardware GPU frame rate or Windows distribution.

Fixed 1280×720 views covered Light/High/Ultra, below/inside/above the modeled
layer, 12 km cruise, dusk, night, cockpit, monthly climate, the north pole and the
date line. All capture processes exited successfully. Independent image review
accepted the bounded near-field enhancement: rounded rises and darker troughs
became readable while the distant arrangement stayed stable. This is a single
procedural cloud layer, with visible sampling/shape limitations, not photographic
weather reconstruction.

Native tests exercised cold High startup with Light fallback, Off→Light→High→Ultra→Off
and restoration to Light. A real defect found during this sequence removed the
camera fog component, changed Bevy's PBR specialization, and temporarily hid the
scene. Off and volume mode now retain an inert fog component. The corrected
native cycle kept terrain and aircraft visible, and an actual Bevy pipeline-key
regression verifies that this component transition preserves the specialization.

A live flight verified that the modal map blocks F3, closing it preserves High,
pause permits Ultra and Shift+F3 restores Light. The aircraft continued through
an origin rebase at latitude 47.103437°, longitude 9.503400°, with the upper
renderer ready and cloud resources unchanged across that rebase. Screenshots
afterwards showed terrain, aircraft and clouds intact. This is a smoke test plus
numerical anchoring tests, not a frame-by-frame temporal artifact measurement.
Native window-manager resize requests did not change the cloud desktop window;
runtime resize therefore remains unqualified by that UI attempt. Target-size
and replacement bounds are covered by unit tests and tier changes.

## Calibration and cost

The old Light seed-1 mask covered 1.835% for a requested 20% and 95.012% for a
requested 70%. Per-seed histogram calibration corrects this bias while retaining
two planes, one material and the 256² RGBA image. Four-seed/seven-coverage
independent sampling found less than 0.008 percentage-point raw area error and
less than 0.096 percentage-point error with 512² bilinear resampling. The
additional histogram scratch is 16 KiB; the existing image buffer is reused.

Upper-tier coverage validation sampled 1,253,376 positions over six latitudes,
three held-out seeds and 17 drift phases. Maximum per-latitude ensemble bias was
0.511 percentage points; an individual snapshot differed by up to 8.271 points.
The sampled patch was approximately 381 km wide. These are procedural projected
support statistics, not screen opacity or the actual cloud amount at an airport.
Dateline, poles, f32 projection, drift and rebase tests are separate checks.

The upper field uses a 64³ RG8 source (524,288 CPU bytes) and a complete mip chain
(599,186 GPU bytes). Maximum two-target storage is 2,764,800 bytes for High
(640×360) or 6,220,800 for Ultra (960×540), plus a 336-byte uniform. Actual native
1180×812 sizing was 523×360 High / 784×540 Ultra. Returning to Off or Light
reported zero owned volume target, noise, source and uniform bytes. These are
logical owned resources; driver allocation and in-flight memory are not measured.

High uses at most 40 view × 4 sun samples; Ultra uses 64 × 6. Including bounded
near detail, worst-case texture lookup caps are about 55.3 million and 265.4
million respectively, before early exits. Upper cloud quality is optional and
does not claim to be inexpensive on an integrated or software GPU.

An isolated Criterion comparison used Rust 1.93, app optimization level 1,
dependency level 3, 30 samples and two reversed-order repeats. Light seed
regeneration measured 12.97–13.67 ms before and 13.10–14.69 ms after calibration;
cover regeneration measured 12.97–13.28 vs 13.06–13.32 ms. New 64³ source
generation measured 30.84–31.27 ms. Host drift prevents a precise regression
percentage claim. Later finite-input and inert-fog guards were not separately
retimed. Density mip generation, upload and GPU execution are excluded.

The monthly Balzers fixture requested 49.214% cover and a modeled base of
2,622.795 m ellipsoidal height, with a 1,200 m thickness. The fixed departure
reference deliberately replaces the old layer that followed local ground height.
It uses coarse model topography, so this correction does not claim pixel parity
or a measured cloud base over the regional DEM.

## Checks and remaining limits

Combined validated coverage: 1,917 Rust test passes, zero remaining failures,
three explicit fixture/device ignores, 178 Python passes and 51 untimed benchmark
smoke cases. Formatting, workspace all-target Clippy with warnings denied,
benchmark compilation, strict rustdoc and architecture checks passed. Two initial
HUD fixture failures were fixed by initializing cloud presentation resources and
allowing absent CLI provenance in the minimal presentation boundary; both focused
retests and all 198 app tests then passed. No flight-dynamics code changed.

The existing commercial/release guards continue to report their outstanding
review and authorization blockers. This milestone does not clear them or create
a binary release. Remaining visual scope includes hardware/Windows qualification,
native resize, arbitrary transmissive scene objects, terrain cloud shadows,
multiple cloud layers and precipitation. Monthly climatology remains a modeled
scenario input; live meteorological observations are not supplied by this work.
