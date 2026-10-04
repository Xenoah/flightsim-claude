# Authored-weather app bridge, 2026-10-04

Scope: isolated `b03efc8` app-v3 base plus accepted renderer `9c73f2a` (cherry-pick
`ffac3e4`, documentation status conflict only). No FDM/wind/contact changes,
source/candidate pin changes, new dependencies or regional-download edits.

## Behavior

`--weather legacy|clear|cloud|fog|rain|snow|storm` selects the initial flight
presentation; Legacy is unchanged by default. Optional `--weather-seed U64`
requires an authored preset, defaults to 0, and is preserved exactly. Invalid or
duplicate options, manual-weather mixtures, and explicit replay overrides fail
before startup condition mutation. Manual cloud flags keep their existing
optics and visible recording/F9 block. Mapping them to Custom would change the
old 5% optical convention and accepted bounds, so it is deliberately deferred.

The plain offline map offers F12 for pending weather only, with an explicit
apply-on-Start/not-live label. LAN contexts keep F12 Leave LAN; credits, region
selection and coordinate editing cannot cycle. Cancel/Close/reopen restores the
actual flight choice. Explicit Start creates a new resolved scenario and exact
initial recording block, using actual departure ground/surface ellipsoidal
height. The layers never follow aircraft AGL or moving terrain.

Supported modeled v3 (including Custom) now reaches the complete renderer. The
recorded block wins over UI defaults; strict legacy partial-identity policy and
regional-package replay rejection remain. `RenderWeather` receives executed
`Simulation::elapsed()` after step, bounded seek, restart and map-start and before
weather preparation. Paused frames cannot advance precipitation via solar or
wall time. The map suspends the 3D camera before weather preparation; inactive
camera handling releases precipitation meshes/materials/diagnostics and restores
an identical field at the same elapsed time when the camera returns.

HUD and attribution identify authored modeled weather. Statistics use its actual
resolved cloud/fog/ambient/rate/seed/time fields, explicitly representing missing
layers, while Legacy logs keep their original values.

## Verification

Validation used Rust 1.93, the shared build environment, `-j 2` and warnings as
errors. The sim/render/UI/app crate roots were made fresh before the first build;
compiler output identified this isolated checkout for those crates.

- App all-targets after the modal-ownership correction: 242 passed, one existing
  ignored candidate-pin test
- UI all-targets: 196 passed, including real Taffy/Cosmic Text map containment
  with the longest pending-weather disclosure; examples contain no tests
- Renderer all-targets in the aggregate run: 342 unit tests plus all integration
  targets passed; existing ignored native/candidate gates were not changed
- After the final marked-camera ownership refinement, all six
  `modeled_weather_systems` integration tests passed again, including an inactive
  marked flight view alongside an active unrelated 3D camera
- Affected app/render/UI all-target clippy with `-D warnings`: passed
- App/render/UI private-item rustdoc with `RUSTDOCFLAGS='-D warnings'`: passed
- Render/UI doctest commands: passed (zero doctests)
- Changed-file rustfmt check, `git diff --check`, architecture check: passed

The real map-layout check initially found the new weather disclosure 13 pixels
outside the bounded body. The fix reserves 35 vertical pixels by reducing the
2:1 map column maximum from 900 to 830 pixels. Existing text, glyph, control and
body containment bounds were retained and passed. A ground-reference regression
also distinguishes the exact requested pre-flight sample from the simulation's
subsequent ECEF round-trip resample (a 4.5e-11 m difference in one alpine case):
weather is compared exactly with the original sample and separately checked
against the physical ground within 1e-8 m.

No native visual/performance or publication acceptance is claimed here.

Added/updated regressions cover all six presets, bounds/poles/dateline/negative
references, actual ocean surface datum, CLI conflict rejection, map cancel and
LAN F12 overlap, map-start recording identity, exact v3 serialization and executed
weather time across live/pause/seek/restart, cloud quality preserving identity,
effective HUD labels and inactive-camera precipitation cleanup. Existing strict
legacy replay and manual recording-block tests remain in place.

Native acceptance is lead-owned: review actual Rain/Snow/Fog/Storm scenes,
all cloud tiers and fallback, day/night, paused/replay camera behavior, map label
layout and cancel/reopen, native LAN overlap, memory/resource bounds and frame
cost before accepting visual quality or performance.


## Reviewed modal-transition ownership correction

Independent source review found that closing Credits/Regions with Escape or a
Close click plus F12 could return to the plain map before the weather handler
ran, allowing an unintended pending-weather cycle. A coordinate Enter commit
had the same problem. The correction publishes current-frame shortcut ownership
from the UI input handler instead of inferring it only from the final map state.
It requires a plain map at input entry and after all modal/editor handling;
child-modal close, coordinate commit, map toggle and focus-loss frames keep it
false. Opening and committing an editor in one input batch is also captured.

Actual UI input scheduling regressions cover Credits and Regions with Escape or
Close click, plus existing or same-frame-open coordinate editors with Enter or
Start click. Each combination is exercised offline and with configured LAN:
pending weather stays unchanged on the transition frame, offline ownership can
return on the next fresh plain-map frame, and LAN retains F12 Leave. The original
pending/cancel/seed, replay-clock, geography and CLI cases use the actual UI input
handler now as well. No UI bounds, source pins or replay policy were relaxed.

Correction validation: all nine weather-runtime tests passed; complete app and
UI all-target suites passed (242 and 196 tests, respectively; the one existing
app candidate-pin test remains ignored). App/UI all-target strict clippy,
private-item strict rustdoc, changed-file formatting, whitespace and architecture
checks passed. Compiler/check paths identified this worktree. No native run was
performed for this correction.


## Combined regional-modal scheduling correction

The combined `48bf859` integration has a second modal owner: region-runtime
update can open Regions after UI input when processing a dropped package or
initial catalog setup. Pending-weather selection now explicitly runs after that
update, before any new-flight Start. This makes the final region-modal state
authoritative and removes the previously unordered F12/open transition.

The combined-schedule regression registers the actual UI, region and weather
handlers in both module registration orders. It covers eligible local ZIP drops
and initial local catalog setup with simultaneous F12, offline and with configured
LAN. The current flight state, modeled Rain selection/seed and a nonempty v3
recording stay unchanged, no Start is queued, no region activates automatically,
and LAN retains F12. Asynchronous workers are allowed to finish before fixture
cleanup; completion still cannot restart or replace the flight.

The existing installed-package→Base scenario now uses Storm and a nondefault
64-bit seed. Preview and Cancel retain the current weather and recording;
explicit package Start resolves its fixed reference to the fixture's exact 350 m
ellipsoidal surface. The initial CurrentRecorder keeps that identity while
package frames and F9 remain blocked. Previewing/canceling Base is inert; explicit
Base Start resolves the global reference, preserves preset/seed, and resumes v3
frame recording whose serialized weather matches the new global reference.

README and the application replay-policy paragraph now describe supported
modeled v3 playback and exact selected-weather recording, with Legacy remaining
the default. No weather/FDM law, cloud resource behavior, UI bounds or candidate
source pin changed in this correction.

Validation on the isolated combined-source checkout, with fresh sim/render/UI/app
crate roots and logged checkout paths: `cargo test -j 2 -p flightsim-app
--features region-downloads --all-targets` passed 257 tests with the unchanged
one ignored candidate-pin test. Default-feature `region_runtime::tests` passed
all 16 tests. Region-enabled app all-target clippy with `-D warnings`, private-item
rustdoc with `RUSTDOCFLAGS='-D warnings'`, changed-file rustfmt, whitespace and
architecture checks passed. No native process was launched by this correction;
combined native visual acceptance remains a separate lead-owned gate.
