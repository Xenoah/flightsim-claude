# Authored modeled-weather contract

Status: the pure, validated parameter API, additive replay-v3 codec, bounded
renderer and application bridge are integrated. New flights can explicitly select
an authored preset. Recorded v3 parameters and executed simulation time drive
replay presentation without reconstruction from current UI defaults. Legacy
remains the default; v1/v2 bytes, physics and manual-cloud optics are unchanged.
See [app verification](qa/weather-app-controls-2026-10-04.md) and the
[native integration record](qa/modeled-weather-integration-2026-10-04.md) for the
tested scope, measured resource counts and remaining visual/qualification limits.

## Meaning and scope

`WeatherSelection::Legacy` is the default. An absent weather block means the
existing environmental semantics, including explicit wind/turbulence, ISA or
enabled monthly-climate atmosphere, and the existing climate/manual cloud path.
It does not mean Clear, and must never select weather using current UI defaults.
`WeatherSelection::Modeled(WeatherScenario)` explicitly selects authored weather.

The six named presets and `Custom` all use `WeatherSource::AuthoredModel`.
Their morphology, coverage, visibility, intensity and layer heights are authored
design choices, not observed meteorology. `Layered`, `Puffy` and `Towering` name
visual shapes rather than diagnosed WMO cloud classes. `Storm` specifies a
towering-cloud/rain scene: it does not imply lightning or convective dynamics.

The bundled NOAA monthly climate remains a separate input with its existing
provenance and limitations ([climate data](data/global-climate.md),
[cloud model](cloud-quality.md)). This module does not consume that climate or
invent a humidity/temperature profile. Rain/snow is selected explicitly; it is
not inferred from surface normals. There is no freezing rain, icing, snow/ice
accumulation, wet-contact/friction modification, downburst or precipitation force.
No atmosphere temperature, pressure, wind or turbulence field is duplicated or
overridden here. A Snow preset is therefore a visual scenario, not a validated
thermodynamic state. Existing user-selected wind remains authoritative.

`WeatherParameters` is a copyable, editable DTO with unit-bearing physical
quantities; it is not itself trusted. `WeatherScenario::try_from(parameters)`
checks every scalar, revision and cross-field invariant without clamping or
rewriting the values. A scenario's fields are private; `parameters()` returns a
copy. `WeatherScenario::from_preset(preset, departure_reference, seed)` resolves
the six fixed presets, while `Custom` requires explicit parameters. A named
preset must match its complete resolved parameter set. Any override must change
the preset tag to `Custom`. `TryFrom<u16>` for every tag rejects unknown values.

The departure reference is captured once, with latitude/longitude in radians
and altitude in WGS84 ellipsoidal metres. Its altitude is the resolved departure
ground/reference height, not the aircraft's altitude above ground. The caller
must supply a consistent datum; this module performs no terrain/geoid lookup.
Named preset offsets are resolved to absolute ellipsoidal layer heights once.
Layers do not follow the aircraft over terrain, and may intersect mountains.
Changing a reference later does not silently move resolved layers. A custom layer
may use independent, explicitly authored absolute bounds.

There is at most one cloud layer and one horizontally uniform fog layer. Absence
is an `Option::None`, with no hidden placeholder values. All seeds, including 0
and `u64::MAX`, are valid. The seed is explicit, never sampled from a wall clock
or random source. No time or particle-history state is stored here. Future
effects must be deterministic functions of these resolved parameters, position
and **executed simulation time**; pause/seek/replay must preserve that time.
Solar time, time-of-day acceleration, render frame count and wall time cannot
drive precipitation or weather evolution.

## Visibility and renderer integration

Each visibility is a separate nonnegative extinction contribution. For this
authored model, a visibility distance `V` specifies a uniform contribution
`sigma = -ln(0.02) / V` in inverse metres: that contribution alone transmits 2%
over distance `V` under `exp(-sigma * distance)`. This is an explicit optical
convention, not a claim that a real visibility observation supplied the value.
Cloud occupation/density weights its own contribution; fog applies within its
fixed bounds. Ambient extinction stays active throughout both layers. Where
components overlap, add their extinction coefficients/optical depths. Never
replace ambient visibility with a larger fog/cloud visibility or use `max(V)`.
Thus an additional fog/cloud contribution cannot improve ambient visibility.
Precipitation rate does not add another implicit visibility reduction; its
authored obscuration is already represented by the explicit visibility fields.

The High/Ultra render-world readiness gate now suppresses only the cloud-owned
camera fog, copying a separately extracted ambient/fog remainder. Off, Light,
pending/error fallback and ready upper tiers preserve the same ambient/fog
extinction. The camera retains a `DistanceFog` component throughout, preserving
Bevy's material specialization key. Legacy still uses its existing inert fog
replacement when its volume becomes ready.

## Renderer presentation and integration

`RenderWeather { selection: WeatherSelection, elapsed: Seconds }` is initialized
to Legacy/zero. The host supplies a validated selection and **executed simulation
time**, after simulation or replay seek/restart and before `RenderSet::Weather`.
It must never integrate another wall clock or derive this time from accelerated
solar time. The renderer does not advance this resource. Its default Legacy
branch retains the existing manual/climate cloud settings, optical convention,
textures, planes and solar-driven drift; no precipitation meshes/materials or
upper-tier resources are allocated by the new weather path in Legacy.

The first system in `RenderSet::Weather` resolves a separate internal cloud deck.
It reads the existing `CloudLayer` for Legacy and derives the authored deck only
for Modeled. App climate/manual cloud updates must run before this set; they never
write the authored resource and cannot overwrite the authored deck. Changing back
to Legacy restores the current legacy deck. Explicit Clear removes that deck,
but retains its authored 100 km ambient visibility. No scenario mutates the
legacy `CloudLayer`, sun clock, FDM, wind, turbulence or contact/friction.

App HUD/log provenance must branch on `RenderWeather.selection`, not only on
`clouds_were_given` or `climate_enabled`. For Modeled, read
`scenario.parameters()` and label the source **AuthoredModel / preset** (including
Custom); do not label the scene NOAA/monthly climate or live/observed weather.
Report the authored ambient visibility, optional cloud morphology/coverage/
absolute base/top/cloud visibility, optional fog absolute bottom/top/visibility,
and explicit precipitation phase/water-equivalent rate. Absence is “none,” not
an inherited climate deck. Preset heights are already resolved ellipsoidal
values, so do not add the departure reference again. Include seed, parameter
schema/model revision and fixed departure reference where diagnostic identity
is needed. Nominal component visibility is separate from combined local
visibility; do not present any single component as the total. Graphics/cloud
quality and precipitation sample cap are presentation diagnostics, not weather
source or meteorological state. The existing climate/manual label logic remains
appropriate only for Legacy. `PrecipitationDiagnostics` supplies owned mesh
counts/bytes/draws without reading the private resolved deck resource.

Negative authored ellipsoidal layers retain their original heights. They arrive
only through validated `WeatherScenario`, without relaxing the legacy public
`CloudLayer` constructor. For a modeled negative cloud base, the synthetic
ellipsoid blocker lies one metre below the layer's base (or below zero for
nonnegative bases); actual opaque terrain depth still clips the cloud. This
avoids incorrectly erasing below-sea-level clouds with the old zero-height shell.

Authored local cloud density uses the same radial ECEF field, seed, elapsed time,
coverage and morphology across Off/Light/High/Ultra. Layered clouds have a flat
interior with smooth 100 m (or quarter-thickness) edges. Puffy clouds retain the
existing noise-shaped vertical envelope; Towering clouds use a taller authored
envelope. High/Ultra use the same authored profiles; quality changes ray/target
sampling, not nominal weather. Light retains its existing calibrated 256² mask
and two PBR planes, including its different 2D geometric approximation. The
fallback silhouette/holes therefore need not match the volumetric image, and a
quality/readiness transition is not pixel-identical. This is a known geometry
approximation, not a change in the authored camera extinction parameters.

Ambient, fog and locally occupied cloud coefficients are summed before creating
camera fog. Unlike the legacy density-as-alpha approximation, authored density
weights the extinction coefficient itself. Fog has a smooth 2 m entry/exit feather
(or quarter-thickness for thinner custom layers), allowing full fog at a normal
2 m eye height above the preset's ground reference. This is **camera-local,
homogeneous fog**: it attenuates scene geometry as if the local medium continued
along the view ray, and distant fog banks are not rendered from outside. The
atmospheric sky is not replaced by a full atmospheric/fog transport solution.
The ready upper tier integrates its cloud along rays rather than duplicating the
camera-local cloud fog; sampling and spatial-integration differences remain.

### Bounded precipitation

Rain and snow use one lit, opaque geometric mesh: thin streaks versus small
diamond flakes, respectively. These are simplified visual proxies, not refractive
or translucent raindrop optics. Opaque depth writes let the existing upper-cloud
compositor keep a cloud behind a nearby particle from overpainting it. No shadow
casting/receiving, emissive value, textures, collision, accumulation, particle
entities, spawn history, physics feedback or network input is introduced. The
material uses the existing scene light/ambient path rather than self-lit white.
Rain's image-plane axes remain nondegenerate at 90-degree bank and when looking
vertically. Particle geometry fades out near the camera (1.5–3.5 m) and at its
bounded field edge; opaque cockpit depth occludes it normally. Small opaque
particle depths can affect low-resolution cloud upsampling; native acceptance
must check these edges as well as cockpit/near-plane behavior.

The field is a repeated 64 m cell in a **fixed departure-local frame**, not the
floating render origin. A bounded nearest copy of each seeded sample is selected
around the camera; particles fade to zero before a copy changes. Positions come
directly from seed, particle index and executed elapsed time. Camera rotation
only orients geometry. LocalFrame/ECEF conversions remain in core and all
positions stay f64 until final conversion to the current RenderOrigin. This
fixed-frame approximation does not diagnose local winds or precipitation aloft.
The phase/rate come only from the contract. Snow's rate is water-equivalent depth;
no snowfall-depth or snow-density inference is made. Visual fall speeds are
explicitly authored (rain 8–11 m/s; snow 0.8–1.4 m/s).

When a scenario contains a cloud layer, its resolved absolute ellipsoidal top
also bounds precipitation. Each particle's ECEF position is converted by core
to WGS84 height, including after long flights away from the fixed departure.
Its area feathers over 2 m below a 0.75 m geometric safety margin, so the whole
streak/flake remains below the top, including at steep bank. This leaves ordinary
below-cloud samples unchanged; it does not infer cloud water, a vertical humidity
profile, local cloud occupation, evaporation or a precipitation floor. A Custom
scenario with precipitation but no cloud has no known top and retains its explicit
precipitation throughout the bounded camera field; no ceiling is invented.

| Requested cloud tier | Precipitation cap | Maximum logical CPU mesh bytes | Mesh draws |
|---|---:|---:|---:|
| Off or Light | 128 | 18,432 | 1 |
| High | 256 | 36,864 | 1 |
| Ultra | 384 | 55,296 | 1 |

Cloud Off disables cloud geometry only; it does not remove authored visibility
or precipitation. This first slice reuses the cloud tier as a bounded visual
sampling budget, not a separate weather identity. Requested quality drives the
cap even while volume shaders are pending. A rate-based square-root density
proxy selects a bounded prefix of these samples (5 mm/h Rain: 58/115/172; 1 mm/h
water-equivalent Snow: 26/52/77; 25 mm/h Storm: 128/256/384). Higher tiers scale
particle area by 128/cap, approximately preserving nominal projected coverage
instead of tripling apparent rate. Sample rounding is at most one particle.

Each particle has six position/normal vertices and no index/UV/color buffers:
144 bytes per particle. The mesh buffers are allocated only on count/phase changes
and mutated in place thereafter. These are logical owned source bytes, not GPU
allocation or performance measurements. `PrecipitationDiagnostics` reports count,
cap, source bytes and owned draw count. Clear/Fog/Cloud (no precipitation) and
Legacy report zero, own no precipitation assets and allocate no particle buffers.
Switching to them removes the previous mesh/material/entity. Mesh extraction and
GPU driver caches may retain in-flight copies beyond the owner lifetime.
The same zero-allocation/cleanup policy applies when the entire 32 m-radius
particle-center field is above an authored cloud top. Reentry reconstructs the
same seed/time field without spawn history. Diagnostics count allocated samples,
including faded samples near the top, and return zero after this full-field exit.

Validation is split: CPU/ECS tests cover extinction, tier/fallback invariants,
negative layers, phase, pause/seek/restart, origin changes, banked geometry and
asset cleanup. The lead's native acceptance must separately compare all six
presets in one scene, verify rain/snow distinction, fog at all cloud tiers,
readiness without a blank frame, cloud/particle/cockpit depth boundaries and
night lighting. This source slice does not claim those native checks passed.

## Schema 1 / model revision 1 bounds

All floating-point values must be finite. Endpoints below are inclusive except
where explicitly marked; the limits are engineering bounds, not climatology.
The schema/model revisions are separately explicit and both currently equal 1.
Unknown revisions must be rejected; never reinterpret them as this model.

| Field | Unit | Accepted bounds / consistency |
|---|---|---|
| Departure latitude | rad | `[-pi/2, pi/2]` |
| Departure longitude | rad | `[-pi, pi]`; both dateline representations valid |
| Departure reference height | m, ellipsoidal | `[-1000, 10000]` |
| Ambient, cloud and fog visibility | m | `[10, 200000]` each |
| Precipitation rate | m/s, liquid-water-equivalent depth | `[0, 0.3 / 3600]`, i.e. 0–300 mm/hour |
| Precipitation kind | tag | None iff rate is zero; Rain/Snow require strictly positive rate |
| Cloud base and top | m, ellipsoidal | `[-1000, 30000]` each; top minus base `[1, 20000]` |
| Cloud coverage | dimensionless fraction | `(0, 1]`; zero coverage is absent cloud |
| Fog bottom and top | m, ellipsoidal | `[-1000, 15000]` each; top minus bottom `[1, 5000]` |
| Seed | unsigned integer | Every `u64` |

Negative ellipsoidal reference, cloud and fog heights are deliberately valid.
All preset constructors resolve across the full allowed departure domain,
including negative heights, poles, dateline and a 10000 m reference. A positive
snow rate is water-equivalent depth, never snowfall depth, snow density or particle
fall speed. No snow/water ratio is supplied. Signed floating-point zero remains
unchanged where zero is permitted; encoding must preserve its bits.

## Exact authored presets

Heights in this table are offsets from the fixed departure reference. API/wire
values store the resolved absolute heights. All rows use schema 1, model revision
1 and source AuthoredModel; the caller provides the departure and seed. Missing
cloud/fog means `None`. All numbers are authored values, not weather observations.

| Preset | Ambient visibility m | Cloud morphology / base..top m offsets / coverage / visibility m | Fog bottom..top m offsets / visibility m | Precipitation |
|---|---:|---|---|---|
| Clear | 100000 | absent | absent | None, 0 |
| Cloud | 40000 | Puffy / 1500..2700 / 0.65 / 500 | absent | None, 0 |
| Fog | 50000 | absent | 0..300 / 250 | None, 0 |
| Rain | 10000 | Layered / 600..2600 / 1 / 250 | absent | Rain, `0.005 / 3600` m/s (5 mm/h) |
| Snow | 3000 | Layered / 300..1800 / 1 / 200 | absent | Snow, `0.001 / 3600` m/s (1 mm/h water equivalent) |
| Storm | 5000 | Towering / 500..8000 / 1 / 150 | absent | Rain, `0.025 / 3600` m/s (25 mm/h) |

The preset-validation contract uses the exact binary64 expressions above and
ordinary binary64 addition of departure height to the listed offsets. External
values with different rounding are valid only as `Custom`; the validator does not
recompute, round or repair them. Numeric equality treats +0 and -0 as equivalent
for preset matching, but the supplied signed-zero bits are retained.

## Length-delimited replay-v3 weather block

This compact, exact contract is implemented by `flightsim_sim::replay::ReplayFile`
and `CurrentRecording`. V1/v2 condition semantics and bytes remain unchanged. The outer
envelope's weather length `W == 0` maps only to `WeatherSelection::Legacy`.
An explicit Clear scenario always has a present, 62-byte block.

All integers are unsigned little endian, floating-point values are IEEE-754
binary64 SI values encoded using their unchanged bits, with no padding or
alignment. The first eight bytes exactly match the central identity proposal.

| Block offset | Bytes | Content |
|---:|---:|---|
| 0 | 2 | Parameter schema = 1 |
| 2 | 2 | Source tag = 1 (AuthoredModel) |
| 4 | 4 | Model revision = 1 |
| 8 | 2 | Preset tag: Custom 0, Clear 1, Cloud 2, Fog 3, Rain 4, Snow 5, Storm 6 |
| 10 | 2 | Precipitation tag: None 0, Rain 1, Snow 2 |
| 12 | 8 | Deterministic seed |
| 20 | 8 | Fixed departure latitude, rad |
| 28 | 8 | Fixed departure longitude, rad |
| 36 | 8 | Fixed departure reference altitude, ellipsoidal m |
| 44 | 8 | Ambient visibility, m |
| 52 | 8 | Water-equivalent precipitation rate, m/s |
| 60 | 2 | Layer flags: bit 0 cloud present, bit 1 fog present; all others zero |
| 62 | 34 if cloud | Morphology u16 (Layered 1, Puffy 2, Towering 3); then base, top, coverage, cloud visibility as four f64s |
| 62 + cloud bytes | 24 if fog | Bottom, top, fog visibility as three f64s |

Thus the only present lengths are 62 (neither layer), 96 (cloud), 86 (fog), and
120 (both), with no trailing bytes. The v3 envelope's conservative 512-byte cap
does not authorize opaque or arbitrary payloads. Its total conditions length
must also match its independently bounded name and weather lengths.

The reader rejects unknown schema/source/model/preset/precipitation/
morphology tags, reserved flag bits, inconsistent lengths, truncation, trailing
bytes, nonfinite/out-of-range fields, kind/rate contradictions and mislabeled
preset parameters **before any simulation mutation**. Validate the fixed length
against flags before reading variable sections; the small block needs no
unbounded allocation. Decode into `WeatherParameters`, then construct a validated
`WeatherScenario`. Never regenerate a preset instead of checking its recorded
resolved fields. The writer serializes the validated exact values,
without claiming absent legacy parameters were recorded.

Existing wind/turbulence and environmental/world fields stay in their canonical
v3 conditions slots; this block does not contain them. Graphics/effects quality,
frame cadence and presentation settings are not weather identity. A later
precipitation phase, multilayer model, new source, changed authored preset, or
changed effect law requires an explicit schema and/or model revision decision.
There is no unknown-version fallback to clear sky or legacy behavior.

## Verification scope

The pure tests pin authored values and source/tags, exercise all valid presets at
coordinate/height/seed boundaries, accept negative layer heights, and reject every
numeric field's NaN/infinity/out-of-range values. They cover layer thickness,
explicit layer absence, precipitation consistency, unsupported revisions/tags,
preset/override mismatch, copied-value immutability and preserved signed zero.
They establish the parameter contract, not observed meteorological realism,
weather rendering, frame performance, particle motion or end-to-end replay-v3.

The following historical checks describe the parameter-only foundation before
codec integration. The additive codec has its own [verification record](qa/replay-v3-codec-2026-10-04.md).
On 2026-10-04, Rust 1.93 on the isolated `4177daf`-based weather worktree passed:

- `cargo test -j 2 -p flightsim-sim --all-targets`, including 13 new weather
  contract tests and the existing replay, climate, fixed-step, flight and
  turbulence suites
- `cargo clippy -j 2 -p flightsim-sim --all-targets -- -D warnings`
- `cargo test -j 2 -p flightsim-sim --doc` (2 doctests)
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-sim --no-deps --document-private-items`
- Changed-file rustfmt, `git diff --check` and `bash scripts/check-architecture.sh`

No dependencies, assets or external APIs were added. No application, GPU,
native-runtime or end-to-end weather/replay checks were performed in this
parameter-only change. The existing simulation and replay source files were
left unchanged; the sole existing-code change is the `pub mod weather` export.

### Renderer source validation, 2026-10-04

On the isolated `1f2052e`-based presentation checkout with Rust 1.93, after forcing
this checkout's sim/render crate roots to rebuild against the shared dependency
cache, the following checks passed:

- `cargo test -j 2 -p flightsim-render --all-targets`: 422 passed, including 16 new
  weather unit/ECS tests and both actual single-/multisample WGSL validation paths;
  benchmark smoke targets also completed
- The two existing ignored checks remain ignored: external normalized Haneda
  terrain coverage and manual quiet-window planning latency. No performance
  measurement is inferred from benchmark smoke success
- `cargo clippy -j 2 -p flightsim-render --all-targets -- -D warnings`
- `cargo test -j 2 -p flightsim-render --doc` (no render doctests)
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-render --no-deps --document-private-items`
- Changed-file rustfmt, `git diff --check`, and `bash scripts/check-architecture.sh`

Focused source review found and corrected banked rain-quad degeneration,
translucent-particle/cloud composition ordering, and the zero-height synthetic
shell hiding negative authored layers. The tests pin the resulting geometry,
opaque-depth material policy, negative-layer blocker, additive extinction,
upper-ready non-cloud remainder, fog through all requested tiers/fallback,
exact pause/seek/restart reconstruction, origin conversion, six-preset lifecycle,
and reported mesh source budgets/cleanup.

This establishes renderer source/CPU/ECS behavior only. No app startup bridge,
end-to-end weather replay, native image, hardware timing, Windows or publication
acceptance is claimed by this slice. In particular, actual fog/rain/snow appearance,
night brightness, cloud/particle/cockpit boundary composition and readiness
transitions remain the lead's native acceptance gate.

### Precipitation ceiling source validation, 2026-10-04

On the isolated `14114ec`-based ceiling checkout with Rust 1.93, after rebuilding
core/sim/render from that checkout, the eight precipitation unit tests and seven
modeled-weather ECS tests passed. Three new unit checks cover the geometric
margin/feather, exact 32 m full-field exit, global/polar/dateline/negative-layer
height bounds, unchanged below-cloud samples, and actual largest-area mesh
vertices through banked views and rebases. The new ECS check covers cloud-top
pause/seek, asset cleanup, deterministic reentry, cloud Off, Custom without a
cloud and return to zero-allocation Legacy for Rain/Snow/Storm.

Strict renderer all-target Clippy, changed-file rustfmt, `git diff --check` and
the architecture check also passed. Independent source review found no blocking
issues. These are focused CPU/ECS checks, not a new full-workspace or native
visual acceptance result; the lead still must compare above/near/below-cloud
Rain on the final application binary before publication.


## App selection and playback

Use `--weather clear|cloud|fog|rain|snow|storm` for an authored new flight.
`--weather legacy` (also the default) retains the existing monthly/manual path.
`--weather-seed U64` is optional with an authored preset; default 0 is stable,
including across restart. All unsigned 64-bit seeds are valid. Duplicate,
missing, unknown, mixed-manual and replay-override arguments fail before startup
mutation. Presets do not set wind, turbulence, atmosphere temperature or friction.

The offline plain world map offers F12 to cycle the pending new-flight weather.
The label explains that it applies on Start and is not live weather. Preview,
Cancel, Close, credits, coordinate editing and region selection never change the
flight's current weather or recording. Close/reopen restores the current choice.
Only explicit Start commits the pending preset with the existing seed, using the
new departure's actual GroundSampler terrain/surface datum. There is no silent
mid-recording mutation. LAN contexts retain F12 Leave LAN and do not offer weather
cycling. Manual-cloud launches retain their exact legacy rendering and visible
recording/F9 block; restart without those flags to choose recorded weather.

A resolved scenario uses the departure terrain height, including negative
ellipsoidal surfaces and ocean surface/geoid height, never aircraft AGL or
bathymetry. Layer heights stay fixed for that flight as the aircraft moves. A
terrain result outside the validated reference domain is an explicit start error,
not a clamped or silently substituted scenario.

The app publishes `RenderWeather` after simulation, replay seek, restart and map
start and before `RenderSet::Weather`. Its elapsed seconds are the actual executed
`Simulation::elapsed()`, including fixed-step remainder and terminal-frame
handling. Solar acceleration, replay wall-time budget and render cadence do not
control weather. Replay supplies the unchanged recorded selection directly;
pause/seek/restart require no weather history. F3 changes cloud quality without
changing the recorded identity. Closing the 3D flight camera for the map releases
owned precipitation geometry; resuming regenerates the same deterministic field.

HUD labels use `MODELED CLEAR`, `MODELED RAIN`, etc. Attribution says authored,
not live. Render-stat logs print the effective authored cloud/fog/ambient and
precipitation values, with absent layers explicitly absent; they do not report a
stale monthly CloudLayer as the active authored weather.
