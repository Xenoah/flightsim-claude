# Authored weather and replay integration — 2026-10-04

This record supplements [the app bridge checks](weather-app-controls-2026-10-04.md),
[the replay migration](replay-app-v3-2026-10-04.md) and
[the reviewed candidate contract](replay-candidate-contract-2026-10-04.md).
The six explicit presets are authored visual scenarios, not live observations or
weather forecasts. Legacy/monthly remains the default. No flight-force, wind,
turbulence, contact/friction, icing or aircraft-control change is included.

## Integrated source and automated checks

Region catalog source `509424e7bc9f2ed57d5004421ef5e83ba2ab2cfd` and reviewed
weather source `6097bc952a43e559acbbe98c5bb983222844a286` were merged as
`48bf8595dc5383b77f3f452710e0c1fe3a0342ee`. The single merge conflict was the
additive weather/catalog command-line dispatch; both flag groups were retained.
That combination passed 256 enabled-app tests and 199 UI tests, then built.

Independent integration review found and corrected a same-frame ordering edge:
an application-side ZIP drop/catalog initialization could open Regions after map
input while pending weather was selected in an unspecified order. The explicit
region-update → weather-selection order and combined real-schedule regressions
are in `b68b0ff1ce63ff1fc441a72d9bbd997f9b39a014`. That source passed 257 enabled
app tests (one existing optional fixture ignored), 16 default region tests,
strict app Clippy/private-item docs, formatting and architecture checks. Tests
cover both module registration orders, offline/LAN F12 ownership, inert preview
and cancellation, unchanged nonempty recording bytes, and package → Base Start
with exact Storm/seed/ground-reference/v3-recording behavior.

The lead reran the simulation tests, integration targets and doctests (337 reported
passing checks across 33 groups), both independent reference encoders without
rewriting golden files, and the enabled application build. Candidate-only commit
`14114ec992610257aa1253764842ede0b09f7fbd` adds the separately reviewed 26-file
source contract and 13 unchanged independent anchors; 146 focused Python checks
passed. Its scope does not claim Windows execution, image acceptance or rights
clearance.

## Actual application checks

Linux cloud desktop, llvmpipe CPU Vulkan, 1180×812 native window. No physical-GPU,
controller, audible-output, Windows or frame-rate qualification is claimed.
The app's normal screenshot/save/exit path produced the following images; no
image content was edited. Shader preparation and application-frame intervals are
separate from GPU performance. Some test builds ran concurrently with observation,
so these sessions are not timing benchmarks.

The first visual binary used source `48bf859` with SHA-256
`faa3f4ae9de6393cb65420d13e1c531ce86294e4c0165aefd9af97ec0cc1872d`.
The final input-order binary used source `b68b0ff` with SHA-256
`e03ae56151a7390e192a2e453466f4ebde3af852cb1bfd7cf707ec98ca4e9ca4`.
Candidate-only script/docs changes do not alter those application bytes.

| Native check | Observed result | PNG SHA-256 |
|---|---|---|
| Fog at a flat reference, chase view | Ground at rest; strong authored 250 m fog remained when cloud quality changed from Off to High | `ecbe6154b6fb914f5268aaa181f056e912f2f7474a21d40f5a5cb4ed386151cf` |
| Clear at the same location/heading/view | Surface and aircraft remained visible without the fog contribution | `f6a0d396955900201ef12e114d38ae2f722eb18a8ea56e29ef73a2bad8939818` |
| Rain in flight | Streaks, Light → High → Ultra readiness, inside-cloud view, and opaque cockpit occlusion | `d9f3aa72fff3f45833dfa07e376a8ae813fb23178f8852145a8465dd5b5a5f26` |
| Snow, Light/High | Small flake proxies distinct from streaks; sparse appearance remains a limitation | `7d98fe8569df5f3fdf55f6b3eab70f523c1d539ab55e88533f0c1bf638e56a1a` |
| Actual v3 Rain recording replay | Completion, bounded rewind, pause and unchanged executed weather time | `c20f11634a19140c4719767fba31e7623033e0081cffbb3f6f46ea6a8e2b584e` |
| Storm at midnight | Cloud/rain scene stayed dark; particles did not turn into self-emitting white streaks | `c5e9baa8a3e4a6f9d64a86188f9ceba124024e92396581eacac1d2179273f7ae` |
| World map weather selection | Cloud → pending Rain → Cancel retained Cloud; reopening restored Cloud; explicit Rain + Start changed the new flight | `3cbab0350e6fa7d2e07d264e8ba096cc3cd7efc1702db758b9fc1019b0a8b8ad` |
| Explicit legacy compatibility | Existing fixed-view v2 file displayed the full missing-yaw warning through completion | `15a8202372b8e50cffa9e8d06a9d3019f31eb1d54b8df87d5dab0e004bb96415` |

All eight listed runs exited 0. Fog/Clear compare the same fixed flat-reference
scene; they are not a flight-performance comparison. An earlier Alpine ground
fog run rolled downhill out of its fixed initial fog layer and is not used as
dense-fog evidence. The layer deliberately did not follow the aircraft. One
initial legacy helper invocation referenced a missing fixture and stopped before
launching the app; the corrected existing fixture above was run under a new label.

Rain's native F9 save wrote 6,512 frames (54.2667 executed seconds) as v3, SHA-256
`665381061f7d742c29090f30925d521003d7959d051afb34847e397835071dbc`.
On replay, the log retained seed 42, departure height 17.2225 ellipsoidal metres,
layer 617.2225–2617.2225 m and the recorded rate. After rewinding and pausing,
`elapsed_s=44.2666666666648` remained identical through repeated five-second
diagnostic observations. The completed replay likewise held its final time.
The independent state/clock tests establish numerical reproduction; native images
alone do not. A separate native Fog F9 save also wrote v3.

The legacy fixed-view fixture is only a startup/completion/UI check, not a full
historical flight reproduction. Without `--legacy-replay-compatibility`, the same
binary rejected it before renderer startup with exit 2 and the explicit missing
`yaw_rate_p` limitation. Opt-in assumes the supported baseline; it cannot recover
what the old recording did not store.

Map suspension disabled the flight camera and returning recreated the bounded
weather resources. A pending Legacy choice during the Storm run was canceled;
the active scenario remained Storm. The map Start test retained seed 42 and
re-resolved the departure ground reference, with the new flight carrying Rain.
The source tests cover additional same-frame modal/coordinate and LAN boundaries.

## Resource observations and visual limits

The Rain run's ready High cloud target was 523×360 with 2,259,360 logical target
bytes; Ultra was 784×540 with 5,080,320 target bytes. Both reported 599,186 noise
bytes, 336 uniform bytes and 524,288 CPU source bytes. These are the renderer's
owned-resource diagnostics, excluding driver caches, allocator overhead and
unrelated assets. Fog without a cloud layer allocated no upper cloud target/noise.
The precipitation caps and logical vertex budgets remain as documented in
[the modeled-weather contract](../modeled-weather.md); they are bounds, not a
measured hardware frame-time claim.

Visual limits are visible in the actual captures: sparse snow; opaque geometric
rain/flake proxies; low-resolution depth-edge fringes around aircraft/particles;
approximate cloud silhouettes and Light deck/horizon bands. Fog is camera-local
homogeneous attenuation and does not replace the blue atmospheric sky or render
distant fog banks from outside. Storm has no lightning or convective dynamics.
There is no claim that this completes photorealistic or every-weather simulation.

## Altitude boundary and publication checks

Review during native acceptance identified precipitation still appearing above
its own authored cloud top. Reviewed renderer commit
`3638fd9287da9cfa76e7d17c5aba7416d7466475` bounds complete proxies using core
WGS84 altitude, a 0.75 m extent margin and a 2 m area fade. A camera whose complete
32 m field lies above the top releases the owned entity, mesh and material.
Below-boundary seed/time/sample positions and quality caps remain unchanged.
Cloudless Custom scenarios retain their explicit absence of a known ceiling.
Eight precipitation unit tests, seven modeled-weather ECS checks, strict renderer
all-target Clippy, formatting and architecture checks passed, with independent
review of polar/dateline/rebase/negative-height geometry and cleanup/reentry.

The final application rebuilt from that source; binary SHA-256:
`0af184b827578f67e541c03be30ea95518db0a21fa9d72e260b477bbd0227368`.
The same 5000 m AGL launch visibly had rain above the 2617.2225 m cloud top before
the fix and no drops afterward. Both native runs exited 0. Final main-world asset
observations were 362 → 361 meshes and 38 → 37 standard materials, with 16 images
in both scenes, consistent with releasing the precipitation owner. These are
asset counts, not GPU memory or frame-time measurements.

| Ceiling comparison | PNG SHA-256 |
|---|---|
| Above cloud, before correction | `4019986403e54bcc016eed35583a198b087382d052d7a837c5b7a06d95415059` |
| Above cloud, after correction | `df9e424d27764a730d707cf12af209e7511eea1de39b091078d3faf6f14646c1` |
| At the ceiling, corrected | `c0a094a0146cc906dfaa30cc1b0d130e8a52e9487d13cbe81e379322ce4026d5` |
| Below the cloud, corrected | `466132e09844a6e86b0e33f6a74082a374cf892e796309da3a25537d51ceee11` |

The latter two runs used fixed v3 visual fixtures produced through the public
CurrentRecorder/WeatherScenario APIs, complete-identity checked and round-tripped
without advancing physical time. The near-ceiling aircraft is 2590 m AGL, placing
the chase camera approximately at the 2600 m AGL top; drops remain below it.
The 200 m AGL fixture retains rain. Both captures exited 0. These fixtures are
render-boundary evidence, not new flight dynamics or weather observations.

Final-head CI and Windows candidate execution remain separate publication checks.
The existing dependency/rights and exact-binary authorization gates remain active.
