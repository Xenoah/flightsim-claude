# Graphics quality candidate, 2026-10-03

Status at 15:05 UTC: `6cad220` passed its source/CPU and native lifecycle checks
but subsequently FAILED cold-switch visibility. The supported render-world
pipeline gate in `5648438` passed fresh focused checks and an observed native
cold-switch visibility retest, warmup cancellation/retry, map-start/pause controls
and a run with the explicit surface-detail override. All nine scene/tier images
passed mechanical checks and independent visual review without a practical
geometry or lighting blocker. Target-hardware and native minimized-target
acceptance remain incomplete; exact LIGHT pixel identity is not claimed.

The tested pre-fix candidate is `6cad2203320ce0bfcbf39e133ecbc6396ab62b75`, tree
`f4af8f8c21cc4242d0a3f446344d50a858843aca`, based on public
`0f0f6bc138080f43cfb1ba27b067069a7318fe3b`. The baseline tree
`7274acbab30917d2578694b33fcdf5fdb609ec3b` was verified to match the earlier local
`47fb2ed` baseline. An execution-environment reset required reconstruction of
uncommitted graphics source, followed by independent review and fresh checks.
The results here use those fresh checks, not the lost pre-reset build evidence.

The ordinary default-feature executable is `flightsim-graphics-6cad220`, SHA256
`0d82e0bad0fc5177f12b5c761d1ed26e1c9bfd46f2da5d71d5a1c0c238d02f50`.
It was built with Rust 1.93.0 using
`cargo build --locked -j 2 -p flightsim-app`; this is a development build, not a
release or `commercial-staging` build. The binary identity here identifies the
pre-fix candidate; replacement-binary evidence must be recorded separately.

## Scope

LIGHT is the unchanged authored baseline/default. HIGH and ULTRA change
atmosphere integration samples and add bounded, generated sky illumination to
existing materials; ULTRA optionally increases shadow resolution. This is a
modest lighting/shadow foundation, not photorealistic scenery. No geometry,
material assets, aircraft/FDM/replay identity, control rates, physical terrain,
selector limits, or terrain upload budgets are changed. See
[the controls and budget table](../graphics-quality.md).

A disposable non-mesh helper owns Bevy's atmosphere source and final cubemaps.
Only its public `EnvironmentMapLight` is cloned to the existing flight camera.
Removing that clone and despawning the helper reclaims the engine's otherwise
private creation-only caches. Teardown runs in PostUpdate, after the Update
schedule boundary flushes Bevy's generator setup commands. The helper is absent
when the flight camera is inactive, including startup with the world map open.
It also requires a valid nonzero physical target and viewport, following the
public camera extraction data after CameraUpdateSystems. Missing/zero target and
viewport regressions verify suspension and recovery without a preset change.

## Fresh source and CPU verification

| Check | Result |
|---|---|
| App tests | 193 passed, 1 ignored |
| Renderer unit tests | 296 passed |
| Renderer integration tests | 75 passed, 2 ignored |
| UI tests | 180 passed |
| Total | 744 passed, 3 ignored |
| Render/UI documentation tests | Passed; zero runnable doctests |
| App/render/UI all-target clippy | Passed with `-D warnings` |
| Architecture and formatting checks | Passed |
| Ordinary app build and `--help` | Passed |

App/render results were recorded on `62c01b0`; the only subsequent change to
`6cad220` corrected a HUD test assertion for the intentional GFX line. Independent
review verified that test-only diff, and the full UI suite, lint and build were
rerun on `6cad220`. Earlier failed runs are retained: `de4fcbd` introduced two
missing-resource presentation failures, fixed by initializing the default
quality resource without overwriting an explicit tier; `62c01b0` then exposed
the stale UI suffix assertion, fixed by checking both the VIEW and GFX lines.
Those failed runs are not represented as passes.

The three ignored tests require an optional regional fixture, external Haneda
tiles, or a separate quiet-window planning-latency measurement. These are explicit
coverage limits. All five pinned legacy profile/FDM/replay file SHA256 values and
the fingerprint `0505e6644bb29a53` were verified unchanged.

The six focused renderer tests and three app tests cover default values,
20-cycle exact rollback and ownership, authored environment restoration,
unsupported capabilities, inactive/missing/zero-sized camera targets and
recovery, CLI validation, surface-detail override, repeated keys and same-frame
map input capture. CPU ownership tests emulate generated image handles; they do
not exercise private render-world caches, shader compilation or actual pixels.

Evidence under the local `flightsim-qa/graphics-quality/` artifact root:
`62c01b0/tests.log`, `6cad220/checks-summary.json`,
`6cad220/ui-test-receipt.json`, `6cad220/ui-tests-final.log`,
`6cad220/doc-tests.log`, `6cad220/clippy.log`, `6cad220/architecture.log`,
`6cad220/build-normal.log` and `6cad220/binary-receipt.json`.

## Native renderer lifecycle

The exact candidate binary ran on the Linux native desktop using software Vulkan
(llvmpipe LLVM 19.1.7 / Mesa 25.0.7), in a 1180×812 client area. The fixed Balzers
day replay used Swift Sport, CHASE view, cloud cover zero, time rate zero and the
existing regional terrain/scenery fixture. The initial run used a fresh isolated
shader-cache directory. The command and binary identity are recorded in
`logs/native-cold-6cad220.json`.

The log records 21 complete LIGHT→HIGH→ULTRA→LIGHT roundtrips: an initial slow
roundtrip and 20 numbered repetitions. Sampled views retained the aircraft and
buildings, and no renderer errors were recorded. Settled ownership was:

| Settled state | CPU / active GPU images | Helper / intermediate owners | Meshes / StandardMaterials |
|---|---:|---:|---:|
| LIGHT before first map use | 16 / 16 | 0 / 0 | 527 / 37 |
| HIGH or ULTRA before first map use | 19 / 19 | 1 / 1 | 527 / 37 |
| LIGHT after map use | 18 / 18 | 0 / 0 | 527 / 37 |
| HIGH after map use | 21 / 21 | 1 / 1 | 527 / 37 |

The flight camera remained `93v0`. Returning to LIGHT reclaimed the generated
helper, its three CPU/active GPU images and the intermediate texture owner after
settling. These public ownership counts exclude retained texture-cache and driver
allocations; they are not measurements of total GPU memory or proof of GPU
pipeline readiness.

Opening the map in HIGH retained the selection, deactivated the flight camera
and removed its generated environment/helper. F4 while the map was open caused
no tier transition. Closing the map recreated the helper on the same camera.
F8 rewind retained HIGH and the camera/helper, while the existing replay reset
temporarily rebuilt geometry; the scene returned to 527 meshes and 37 standard
materials. A subsequent map open/close and HIGH→LIGHT sequence remained bounded.

The first visible map added two persistent non-helper images. Their exact owners
are unclassified; subsequent opens did not add more. This observation is recorded
without attributing them to font atlases or calling total image count identical
to a never-opened-map baseline.

The detailed input sequence, timestamped ownership observations, error scan and
limitations are in `logs/reviewer-native-cycles-map-6cad220.json`, referencing the
989,015-byte prefix of `logs/native-cold-6cad220.log` with SHA256
`69d2d511037dde4c3f1789e4c7c4038e11fa1de3a34f757190cd3d8f23a4ced9`.
Its screenshots were inspected visually, without exported lossless pixel
comparison. The run ended by the QA operator's interrupt (exit 130); this was not
a normal-exit screenshot smoke test.

## Pre-gate descriptive software-renderer intervals

`native-initial-steady-intervals.json` records one ordered LIGHT, HIGH, ULTRA
observation of that fixed scene on `6cad220`, before the pipeline gate. These
numbers do not establish the fixed candidate's cost. Differences of sample-count
× rounded cumulative mean, before the 2048-sample cap, give the following
approximate mean intervals.
The first ten seconds after each quality switch and intervals classified as
terrain loading were excluded.

| Tier | Classified steady intervals | Derived mean interval |
|---|---:|---:|
| LIGHT | 78 | 274.424 ms |
| HIGH | 139 | 328.170 ms |
| ULTRA | 69 | 382.106 ms |

These are descriptive application intervals from a single ordered software run,
not randomized benchmark results, GPU timestamps, whole-run throughput, or
hardware FPS predictions. No concurrent local build or second GPU test ran;
normal compositor/OS activity remained. Per-tier percentiles cannot be recovered
by subtracting cumulative histograms. No target-hardware performance conclusion
is drawn from these numbers.

## Visual acceptance and remaining limits

- First cold-switch continuity: FAILED on `6cad220`; the observed native retest
  PASSED on `5648438` as detailed below. Retained ambient illumination does not
  prove uninterrupted mesh visibility while new environment-map pipelines
  compile. The first cold-switch attempt had a 17-second observation gap before
  settled HIGH was observed, so it does not qualify transition continuity.
  A subsequent fresh-cache run captured 160 samples from 14:25:00.687 to
  14:25:27.205 UTC. Twenty samples from 14:25:02.491 through 14:25:05.124 showed
  only sky/HUD; terrain and buildings returned at 14:25:05.328 and the aircraft
  at 14:25:08.396. Evidence: `cold-transition-failure-6cad220.json` and
  `logs/native-cold-contiguous-6cad220.log`. These are sampled CUA observations,
  not every rendered frame. Warm roundtrips do not excuse this failure.
- Native lossless LIGHT equality against the preserved ordinary baseline, and
  exact LIGHT restoration after upper tiers: practical day-view restoration
  observed, exact equality not met. Candidate native LIGHT control versus
  roundtrip PNGs differed at 68 of
  958,160 pixels, in 28 groups of 1–7 pixels. Review found 65 near high-contrast
  edges and no aircraft/HUD differences: practical appearance was restored,
  exact identity was not, and the cause is unclassified. Evidence:
  `native-light-pixel-comparison.json` and the two native LIGHT launch receipts.
  The fixed candidate reproduced that roundtrip PNG exactly after cancellation
  and retry (`native-fixed-light-comparison-5648438.json`). Against the public
  `0f0f6bc` baseline day capture at 1280×720, fixed LIGHT differed at only five
  pixels by one channel level outside the intentional GFX label region.
  Dusk differed at 17 pixels (maximum channel delta 15), and night at eight
  (maximum 11); all were tiny isolated differences. Full statistics are in
  `fixed-5648438-pixel-comparisons.json`.
- Candidate day/dusk/night captures across all tiers: all nine completed normal
  exit, PNG/hash, settled-terrain/overlay, requested-tier, same mesh/material
  count and no-error checks. Independent review found no practical geometry or
  lighting blocker. Full baseline statistics are below.
- Startup with the map already open, pause/resume quality controls, and the
  `--surface-detail off` override through native tier changes: functional native
  checks passed on `5648438`; details and scope are below.
- Native minimized/zero-sized target interruption and recovery: pending; the
  source guard has CPU regression coverage only.
- Unsupported-adapter/4096-shadow fallback on actual hardware: not exercised;
  CPU capability tests do not replace that check.
- Target GPU/Windows performance and visual acceptance: not run. The llvmpipe
  observations cannot establish FPS, GPU memory usage or appearance on the
  intended hardware.

No publication or release clearance is asserted. The default tone-mapping LUT
rights issue and ordinary release gates remain unchanged.

## Cold-switch fix and verification

The gate source is `56484387368a284f9e0da4ab239e31855f9ad096`, tree
`93c0c2351b4513ec1cf316a1eebf8a674ab29179`; independent source review found no
blocker. The documentation draft is separate from that frozen implementation.

The renderer now inspects Bevy's exact specialized pipeline IDs after material
specialization and before draw queueing. Only the flight view's app-owned
environment addition is gated; an authored baseline environment is excluded.
The current prepared visible mesh/material cohort must have fresh specialization
ticks and ready pipeline objects. Empty, absent, stale or failed entries do not
qualify as ready. A newly prepared or newly visible variant is rechecked each
frame.

When upper variants are pending, the gate clears only the ENVIRONMENT_MAP bit
and that view's per-entity specialization cache, then runs Bevy's public cached
specialization system to select exact baseline variants for current geometry.
Other view-key bits and other views are retained. The engine derives the desired
upper key and retries next frame, while its shared specialized cache deduplicates
compiled variants. No guessed shader descriptors, extra scene geometry, blocking
GPU waits or private engine components are used. Terminal pipeline errors remain
errors and retain baseline material draws.

State is scoped to the current extracted flight request and current unique
pipeline IDs; map suspension, cancellation and camera replacement do not retain
old requests. LIGHT performs no prewarming. Pending compilation can add one
native material-specialization pass over the visible scene. Shared shader and
pipeline caches can persist after the helper and its images are reclaimed; the
earlier ownership counts never measured those caches or total driver memory.

Fresh checks on `5648438` passed: nine focused renderer tests (including three
new pipeline-gate regressions), three app controls/CLI tests, app/render/UI
all-target clippy with `-D warnings`, formatting and architecture checks. All
five pinned legacy file hashes remain unchanged. The focused command was
`cargo test --locked -j 2 -p flightsim-render -p flightsim-ui -p flightsim-app graphics_`;
other tests were filtered, so this is not a repeat of the entire earlier suite.
Evidence is in `5648438/checks-summary.json` and its sibling logs. An initial
test-only Tick import compilation failure was corrected before this frozen
commit; its failed log remains under `pipeline-gate-draft/`.

The ordinary build and `--help` passed. The immutable executable is
`flightsim-graphics-5648438`, SHA256
`ae374dae216e70efade988a39189d2fd9c21810268686e5a0187f549f9c06ad3`
(146,205,264 bytes); `5648438/binary-receipt.json` records the source identity,
build command and documentation-only working-tree differences.

The fresh-cache native first-switch retest captured 160 samples from
14:45:54.071 through 14:46:19.667 UTC, following input at 14:45:53.297. All four
unique sampled JPEG states retained terrain, buildings and the aircraft; no
sky-only or missing-aircraft state was observed. HIGH lighting was first observed
at 14:46:01.388. The native log recorded four unique visible variants entering
the gate at 14:45:55.086 and becoming ready at 14:45:56.582, with no renderer
errors in that observation. Evidence: `native-fixed-cold-assessment-5648438.json`,
`logs/native-fixed-cold-5648438.log` (SHA256
`b41d9a698adbbda06c18278ccadd5e08e5a77ea9b16ca86f82c3fe8cad38c2bb`) and
its launch receipt. The capture is sampled visibility evidence, not proof about
every rendered frame.

That fixed-candidate run completed six LIGHT→HIGH→ULTRA→LIGHT roundtrips. Its
final settled LIGHT state retained camera `93v0`, 527 meshes and 37 standard
materials, with 16 CPU and active GPU images and no helper/intermediate owners.
Terrain settled at 119 displayed/live/desired surface tiles and 271 visible
bridges. No renderer errors were recorded; the run ended with a QA operator
interrupt (exit 130), not a normal-exit smoke result.

First-use compilation still delayed the appearance change. The run's logged
maximum frame interval was 4863.394 ms, so this is not a hitch-free or instant
switching claim, nor hardware performance evidence. The earlier 744 passes and
lifecycle checks remain historical evidence for `6cad220` only; fixed-candidate
checks are recorded separately here.

Warmup cancellation/retry passed in a separate native run. HIGH was selected at
14:51:44.861, warming began at 14:51:46.292, and Shift+F4 restored LIGHT at
14:51:47.156 before any ready event. Retrying HIGH at 14:52:07.735 produced the
complete brighter scene, then LIGHT was restored at 14:52:19.641. Inspected
screenshots showed no disappearance and the log contained no renderer errors.
Evidence: `native-fixed-cancel-assessment-5648438.json` and
`logs/native-fixed-cancel-5648438.log`. The run wrote
`captures/native-fixed-cancel-5648438.png` and exited normally with code 0.
The PNG SHA256 is `270e2247ed7831a61c01cc0dcfd96ea74ef25ee0cf132ae40b10218dc3286999`.
Final ownership was 16 CPU/active GPU images, no helper/intermediate owners,
527 meshes and 37 standard materials.

A separate startup run used `--graphics-quality high --world-map
--surface-detail off` at the existing Haneda runway. While the map was open,
camera `93v0` remained inactive without a helper; F4 retained HIGH. Closing the
map activated the same camera and created its helper. Esc opened PAUSED; F4
selected ULTRA and Shift+F4 restored LIGHT while that overlay remained visible,
then Esc resumed the LIGHT flight view. The explicit surface-detail-off command
remained in force and mesh/material counts stayed at 208/36 across tier changes;
the source/CPU override regression separately verifies that quality never
overwrites that setting. There were no renderer errors. Final LIGHT had 18 CPU
images and no helper. This check ended by operator interrupt, exit 130.
Evidence: `native-map-start-assessment-5648438.json` and
`logs/native-map-start-5648438.log`. This fixture and its counts are separate from
the regional Balzers appearance matrix.

The fixed-candidate interval receipt `native-fixed-steady-intervals-5648438.json`
records one ordered LIGHT/HIGH/ULTRA observation at the same fixed day view:

| Tier | Classified steady intervals | Derived mean interval |
|---|---:|---:|
| LIGHT | 129 | 283.506 ms |
| HIGH | 140 | 330.151 ms |
| ULTRA | 82 | 379.936 ms |

These use differences of count × rounded cumulative means before the sample cap,
excluding initial warmup, at least ten seconds after each quality input and
intervals classified as terrain loading. They are descriptive software-renderer
intervals, not a randomized comparison, total throughput or GPU timestamps. No
per-tier percentiles, hardware FPS or total GPU memory can be inferred. No
concurrent build or second GPU test ran; compositor and brief source-preparation
work remained. The pre-gate and fixed tables do not isolate the gate's cost.

## Fixed day/dusk/night image matrix

`fixed-5648438-matrix-assessment.json` records all nine combinations of
LIGHT/HIGH/ULTRA and day/dusk/night. Each offscreen software-Vulkan run produced a
1280×720 PNG with a verified hash and normal exit 0, and no renderer errors. The
recorded settled scene contains 119 displayed/live/desired surfaces, 271 visible
bridges, 15 registered overlays at revision 5/5, 527 meshes and 37 standard
materials. Offscreen LIGHT had 17 CPU/active GPU images and zero helpers; upper
tiers had 20 images, one helper and one intermediate owner. These offscreen
counts include the capture target and are distinct from native-window counts.

Independent inspection of the original images found cooler fill on shaded faces
in daylight, changed ULTRA shadow edges and a modest dusk lift. The tested night
view was effectively unchanged: exact comparison found only 21 changed pixels outside the HUD for
either upper tier versus LIGHT, with mean absolute channel difference 0.0000149
on 8-bit RGB values. Scene geometry remained present in all nine captures, and
independent review found no
practical geometry or lighting blocker. This does not establish universal
brightening or photorealism. All nine hashes and normal-exit receipts, plus the
fixed cold/cancel/map assessment log hashes, were independently verified.
Capture-run wall times are not benchmark measurements.

`fixed-5648438-pixel-comparisons.json` compares the preserved public baseline
against fixed LIGHT at 1280×720, excluding only the intentional GFX-label
rectangle `[0,270,200,294]` (x0,y0,x1,y1):

| Scene | Changed pixels outside HUD | Maximum channel delta |
|---|---:|---:|
| Day | 5 | 1 |
| Dusk | 17 | 15 |
| Night | 8 | 11 |

These tiny isolated differences support practical baseline appearance, without
claiming exact pixel identity. The receipt also records each upper-tier
comparison; its figures describe these fixed images, not a blanket quality
ranking.
