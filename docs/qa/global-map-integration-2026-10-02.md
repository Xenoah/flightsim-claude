# Global map integration verification — 2026-10-02

This is an engineering verification record for frozen production source
`c66eb9838ee1c93a0aee0f74639018e0a468faa4`. It is not a published release,
certified flight model or commercial-rights clearance. Final aggregate checks,
north/south-pole 3D captures and the exact commercial candidate are complete
within the scopes below; their visual and platform limitations remain explicit.
Commands and user-visible behavior are in [the world-map guide](../global-map.md).

## Final source and aggregate gates

The `final-0734` run, from 07:34:25 to 07:41:55 UTC, records the exact source above
in `build-logs/final-0734-source.txt`. All **17 gates exited 0**:

- **909** pure-Rust/headless all-target tests; zero failures
- **701** native render/input/UI/audio/app default-feature all-target tests;
  zero failures and the two known ignored tests described below
- **128** commercial-feature app tests in a separate run; zero failures. This is
  a second feature configuration, not 128 additional distinct default cases
- **Five doctests** and **104 Python tests** (99 main and five terrain regressions)
- Workspace all-target Clippy and commercial app all-target Clippy with warnings
  denied; strict default/private-item and commercial app documentation checks
- Rust formatting, diff and architecture checks
- Default and commercial native app builds
- Windows GNU all-target checks for default and commercial configurations

The split test groups required by `CLAUDE.md` were used with Rust/Cargo 1.93.0,
locked dependencies and two build jobs; `cargo test --workspace` was not used.
The test commands also exercise benchmark smoke entry points. Those successes
are not statistical performance measurements.

The two ordinary aggregate exclusions are unchanged: the Haneda runway-footprint
test requires its specific external normalized DEM fixture, which is unavailable
here; the quiet-window Commands/Assets timing probe remains opt-in. The latter
was run explicitly for the retained measurements below. An independently rebuilt
Alpine fixture does not substitute for the missing Haneda fixture, and the
ordinary aggregate run did not turn either ignored test into a pass.

Exact final executable SHA-256 values:

- Default: `4883d8f87cadd9051e637e1f45cc3ea4449936bb5670719290f3b824d5fe240f`
- Commercial: `ea36a56eb5df5074dfda950e1de6629100a0902ae10e4c9ef56c86e88110e94a`

Evidence is retained under the local QA artifact's `build-logs/final-0734-*`
summary, per-gate logs, exit files and binary hashes. These native binaries are
optimized dev-profile builds, not qualified production release packages.

## Implemented behavior and regression scope

The application has an offline world raster, exact-coordinate entry, eight region
presets, monthly climate preview and an explicit new-flight transition. Regional
DEM ancestors take priority over the global baseline. Physical ground, climate
and replay remain independent of camera LOD. Global terrain samples are roughly
19.5 km apart at the equator; generating finer mesh tiles does not create finer
source data.

The initial date is fixed, and climate remains at the chosen annual phase for a
flight. Monthly NOAA reanalysis alters temperature, density and speed of sound
through `sim`; pressure remains ISA. Biome colours, cloud geometry and snow cues
are model approximations. There is no live-weather service, account or key
requirement. Map MSL elevation is orthometric `H`; HUD ALT is ellipsoidal `h`, with
`h = H + N`. AGL/`--fly` use the active physical ground sampler. The map preview
continues to identify its coarse-global source even when a regional DEM is active.

Permanent six-minute integration tests cover ocean/dateline and Sahara flights
in January and July: finite state, terrain/air/climate samples, continuity,
dataset identity, more than 10 km of travel, actual date-line crossing, replay-v2
serialization, and exact rigid-body bits at checkpoints and completion. These
are numerical test-control fixtures, not an autopilot or subjective handling
assessment. Historical six-region duration experiments are supplemental; the
fresh aggregate results above are the retained reproducible gate.

Native interaction checks across the development session include:

- Rapid latitude/longitude click/edit, repeated digits, negative longitude and
  decimal comma; out-of-range entry and Start rejection
- Month preview without moving the current aircraft or changing the active flight
- Explicit Tokyo → Alps → Sahara relocation, resetting flight, recording and
  terrain state
- Closing the map preserves an already-paused flight; Esc dismisses credits
  before the map without also toggling flight pause
- All three data-credit pages, repeated open/close, free-camera selection, and
  map rendering/model fitting while the 3D camera is inactive
- `--fly` hides unrelated circuit guidance at startup, while ordinary runway,
  approach, replay and the H override retain their existing behavior
- Replay map selection, clicking Start and pressing Enter cannot relocate the
  recorded aircraft; native F5 pause, F8 rewind to zero and F5 resume worked
- With global terrain and climate explicitly off, an attempted Alps map start
  stays preview-only; closing returns to the original Tokyo synthetic runway,
  with zero global terrain assets, a saved image and successful process exit

These earlier interaction runs are distinguished from the exact frozen-source
captures below; final automated coverage was rerun on `c66eb98`. The short native
replay-control check is not a completed six-minute native playback claim.

The recorded input stream folds trim into effective elevator rather than storing
a separate historical trim setting. Replay HUD trim zero does not mean its
physical trim effect vanished. Manual cloud-render overrides are not serialized:
use the same cloud CLI settings when comparing scenes. Exact physical replay and
identical screenshot pixels are separate contracts.

## OSM scope and local provenance

The existing optional local PBF → FSAP pipeline remains airport-only: regional
runways/taxiways, aprons, holding positions, signs and lights. It does not add
global roads, cities or building coverage. The new provenance companion records
input/output hashes and declared snapshot/converter identity, publishes an
immutable record atomically without overwriting an existing one, and keeps
redistribution unreviewed. Its **31 regression tests passed** in the final Python
gate. Hashes do not authenticate the user-attested source/conversion relationship.
No real OSM database is bundled; ODbL regional packs and their attribution,
source-access/share-alike and distribution review remain a separate workflow.
See [OSM airport-pack provenance and boundaries](../data/osm-airport-packs.md).

## Loading fairness, explicit global-only mode and readiness

The earlier 2,561-live/4,094-resident polar plateau was a long scheduling tail,
not proven permanent starvation. One outstanding fallback could hold an entire
subtree behind its covering root while nearby primary-miss retries consumed reads.
The integrated fair scheduler alternates eligible primary/fallback attempts,
retains its turn across updates, and preserves primary-ancestor precedence,
late-primary retries/replacement, cooldowns, all existing budgets and atomic cuts.

When no `--tiles` source is supplied, the app now explicitly uses immutable
`EmptyTileSource`. Its false `primary_reads_possible()` capability means primary
reads can never succeed for that instance. Such impossible reads and their miss
history are skipped. Existing source implementations default to primary-capable;
an empty mutable source or configured but absent disk directory remains capable
so later real tiles are still discovered. Cached/resident real ancestors keep
priority, and neither mesh/cache limits nor physical sampling/interpolation,
climate, replay bytes or runway geometry changed.

The paired actual-selector probe uses synthetic fallback DEMs and a fixed
4,094-ID polar cut, with read/preparation budgets eight. One-based calls to exact
ID convergence were:

| Scenario | Earlier distance arbitration | Fair arbitration | Explicit global-only |
| --- | ---: | ---: | ---: |
| Cold selection | 1,878 | 1,195 | 512 |
| Fixed camera move | 639 | 428 | 221 |

Cold source attempts fell from 15,024 to 9,560 to 4,096; moved-camera attempts
from 5,112 to 3,424 to 1,710. Cold fallback generations and preparations stayed
4,096. Not all intermediate work falls: fair arbitration increased moved-camera
preparations from 1,924 to 1,967; explicit global-only mode used 1,761. These are
deterministic work/call counts, **not** measured frame times, `cargo bench`
throughput, GPU speedups or a smooth polar-flight guarantee.

Permanent regressions verify coverage/non-overlap throughout, budgets including
zero/one/eight, cooldown recovery, late real-parent replacement, exact desired
sets, and default-true source behavior. The optimized physical path retains exact
f64 heights across 162 repeated samples; six-location paired flight tests compare
state, ground, atmosphere and interpolation at each of 240 fixed steps. See
[loading fairness](terrain-fallback-fairness-2026-10-02.md) and
[explicit global-only verification](terrain-global-only-2026-10-02.md).

Final terrain diagnostics distinguish displayed, live and last-desired IDs.
`displayed_match true` requires exact desired-ID equality and completed seam
commitment, not matching counts alone. It refers to the last sampled desired cut;
a budget-truncated cut can still be coarser than the original screen-space-error
request. Readiness does not increase source resolution.

## Surface seams and measured limits

Actual-edge bridge meshes, ordinary corner patches and polar caps replace an
arbitrary fixed-depth skirt assumption. Old/new complete surface-and-bridge cuts
switch atomically. Source-generation, reset, cache-pressure, floating-origin,
arbitrary LOD difference and reference/planner identity regressions pass.
Attachment has documented finite-f32 error bounds: vertices are not claimed to
be bit-exact welded, and normals are not claimed smooth at every transition.

The planner performs at most 1,024 defined topology work units per update.
Retained quiet-window measurements reached 3.328 ms per real renderer planning
call; a separate release stress probe retained a 6.409 ms outlier. Default polar
planning needed 87 calls and the 8,192-tile uniform stress case 81. Total planner
CPU is slightly higher than the synchronous reference. Explicit whole-scene
cancellation was 63–85 ms; full stress commit was 19.056 ms. These measurements
precede the later source-arbitration/global-only refinements and are not new
end-to-end timings of the frozen build. This is bounded planning, not a 60 fps,
hitch-free or GPU performance guarantee. See [the measured planner report](terrain-seam-planning-2026-10-02.md).

## Native scenes: accepted evidence and limits

January/July climate contrast, Alpine summer, Sahara, Amazon and Sydney images
exercise the actual app, with both built-in aircraft used across the session.
Static captures establish visible behavior at their captured instant, not
continuous flight quality. Early exact-pole/date-line images showing initial
coarse roots establish finite startup/fallback only. Retained layout checks cover
1280×720, and the final north-pole native capture is 1180×812. Smaller windows and
high-DPI scaling have not been verified.

The later native date-line run started at 0°, 179.9998°, maximum level 13,
prewarmed in the map and crossed to −179.9924°. Its captured flight had 116
displayed/live tiles, 260 visible bridges, no queued bridges or pending topology,
finite HUD and continuous visible water. Screenshot save and process exit both
succeeded. The frozen aggregate's numerical date-line tests provide separate
repeatable coverage; this earlier interactive run is not relabeled as a new
frozen-build screenshot.

### Final north pole: reviewed actual 3D capture

`captures/frozen/north-pole-native-ready-c66eb98.png` was saved at 08:09:54 UTC,
with batch status/process exit **0**. It was independently reviewed and shows
actual 3D terrain, sky, aircraft and finite HUD, not the modal map. At capture the
log records **4,094 displayed = live = exact last-desired IDs**, both match flags
true, **8,185 visible bridges**, no queued bridges, no pending planning and
`stitching false`. The capture SHA-256 is
`cddaaf15e3488d5094eef99a8e1039563d277a7da43f638c6494e72a66ef2c37`.

The camera/aircraft pose was held with a local replay-v2 fixture containing one
zero-duration input frame and its exact frame-zero state, 1,200 m above sampled
ground at 90°N, 0°, with a fixed June-noon clock. Public writer/reader/fingerprint
validation passed; replay finishes immediately and advances zero physics steps.
This isolates renderer convergence at a static pose. It is **not** a sustained
polar flight, stable-cruise, performance or complete native replay-duration test.
The displayed 97 kt is initial state data, not distance flown during the capture.

Sparse bright pixels remain visible in the polar capture; their cause is not
established by a still image. Together with the finite-f32 attachment bound,
this prevents calling the result
pixel-perfect, crack-free at all scales or universally seamless. The exact-cut
readiness result and these visual limitations must both be retained.

### Final south pole: actual 3D capture

`captures/frozen/south-pole-native-3d-c66eb98.png` was saved at 08:26:49 UTC,
with process exit **0**. The original **1180×812** image was independently inspected: it is the
3D Swift scene with finite HUD over broad continuous Antarctic ground and horizon,
not the modal map. The final pre-capture diagnostic at 08:26:38 records **4,094
exact desired IDs displayed**, both match flags true, **8,185 visible bridges**,
zero queued bridges, no pending planning and `stitching false`. SHA-256:
`d01f9715e60124264c4dd487173a0eb32ea524c66366eba1609b2e96ccf7037c`.

The corresponding local static replay holds 90°S, 0°, 1,200 m above sampled
ground, with a fixed December-noon clock and zero physics steps. HUD ALT is
13,124 ft ellipsoidal and AGL 3,937 ft. Conspicuous radial striping and faceted
surface shading remain visual-quality defects; this is not evidence of smooth terrain normals, surveyed surface detail,
or convincing snow/ice materials. It establishes complete-cut rendering at the
fixed pose, not sustained flight quality. The earlier
`south-pole-native-ready-c66eb98.png` is a map-only capture and is not used as
south-pole 3D proof.

The native map was used to prewarm each extreme polar cut before returning to
3D. Screenshot delay uses Bevy virtual time, so rendering under llvmpipe made
the wall-clock capture interval substantially longer than the numeric delay.
This workflow is explicitly not a real-time rendering benchmark.

Both final polar fixtures are local-only controls. Raw fixtures/helpers were not
published. All rendering here uses software Vulkan/llvmpipe; no physical GPU,
frame-rate target or Windows rendering result is inferred.

## Exact final commercial candidate

Optional `commercial-staging` leaves ordinary developer defaults and legacy
profile/replay fingerprints intact, but defaults its local candidate to the
original Swift Sport and accepts assets only beside the executable. Missing
selected models fail cleanly. The Meshy Light Single model is excluded; its
historical source is preserved, and the old paid-plan note is not rights evidence.
The original Swift geometry does not clear unrelated dependency or embedded-asset
obligations. See [staging instructions](../release/commercial-candidate-staging.md)
and [the commercial-rights audit](../release/commercial-distribution-audit.md).

The exact frozen commercial executable above was staged from a clean detached
`c66eb98` source snapshot as `commercial-20261002/candidate-c66eb98`:

- **694 indexed files / 155,617,290 bytes**; all sizes and SHA-256 values checked,
  with no extra payload files or symlinks. The manifest excludes its own bytes
- Explicit Swift-only external asset allowlist plus exact world provenance/terms,
  font/LUT notices and target-specific dependency notice inventory
- Direct launch from an unrelated working directory, without an aircraft override,
  loaded Swift Sport's adjacent 7.12 m model, reached the exact 38-surface/95-bridge
  desired cut, saved a complete 1280×720 PNG and exited **0** at 07:44:48 UTC
- The actual PNG was inspected. Its SHA-256 is
  `b1199f2d8e3c3f8f19d0801d066d8ebc731866ad32de2e4b919bf05df599a11c`
- Final gate: **zero integrity errors and five explicit review blockers**. Stager
  exit **1** and checker exit **2** correctly preserve **READINESS BLOCKED**

The five remaining review gates are dependency license/nested/platform review,
`constgebra` 0.1.4 primary notice, `hexf-parse` 0.2.1 primary notice, the exact
embedded AgX LUT grant, and Blender Filmic LUT version/license provenance.
No positive review record was fabricated. Renamed excluded Meshy bytes and
unreferenced/binary notice payloads are rejected; earlier real-payload negative
checks and permanent regressions cover this boundary.

Companion evidence is `candidate-c66eb98-evidence.json`,
`candidate-c66eb98-final-gate.json`, and `candidate-c66eb98-chase.log/.png` under
`commercial-20261002/`. Earlier candidate/runtime checkpoints are historical and
do not replace this exact frozen-build proof. No executable or raw replay fixture
was uploaded and no publication was performed.

## Unresolved acceptance and release boundaries

- Coarse terrain can obscure runway pavement/lights because rendered triangles
  differ from the bilinear physical sampler. Adding a lift would conceal that
  mismatch; this work does not claim the runway-occlusion issue fixed
- Coarse coastlines, islands, ridge lines and airport grading remain unresolved;
  some static lake elevations are flagged low confidence. This is not surveyed
  geography, live weather or a navigation/certification database
- The static pole captures do not establish smooth moving-flight quality; radial
  surface shading, sparse bright pixels and finite-f32 attachment limits remain
- Layout was tested at 1280×720 and captured native sizes (including 1180×812);
  smaller, high-DPI and resized windows remain unqualified
- No physical GPU/controller test, speaker listening or subjective human handling
  assessment was performed. Audio logs with no available device are not audio QA
- Windows GNU checks are cross-target type/compilation checks, not an MSVC release
  link, Windows executable run, redistributable validation or installed-game smoke
- The Linux candidate is filesystem-portable only; its ABI/native prerequisites,
  Steam Linux Runtime and Steam Deck compatibility are not qualified
- Steam account/SDK setup, agreements, store submission, actual release packaging,
  rights sign-off and publishing approval remain separate. All five commercial
  review blockers remain open; no Steam or blanket rights clearance is claimed

The production source remains frozen at `c66eb98`; this document update changes
only the verification record. Independent review covered the exact aggregate
evidence, commercial candidate, both final polar originals and their diagnostic
logs, and the final documentation. Static coverage/readiness was accepted with the
visual, performance, runway, platform and rights limitations above retained.
This review does not authorize publication or establish commercial clearance.
