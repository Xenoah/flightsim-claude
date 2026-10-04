# ADR-0013: Calibrated cloud amount and bounded cloud rendering tiers

- Status: implemented; validation and release status are recorded separately
- Date: 2026-10-03

## Problem

The original renderer used two alpha-masked planes and camera-local fog. Its
`1-cover` noise threshold did not represent the requested covered area, and its
climate-driven plane height followed terrain underneath the aircraft. Increasing
visual quality must not invent meteorological observations or add upper-tier
vertex/texture work to the default Light path.

## Decision

Cloud quality is independent of GraphicsQuality: Off, Light (default), High,
Ultra. F3 cycles and Shift+F3 restores Light. Input runs before weather preparation
and respects the modal map. The simulation, wind, aircraft identity and replay
physics are unchanged.

Light retains the original two planes, StandardMaterial and 256² RGBA image.
Image creation calibrates each seed's threshold with a 4096-bin histogram, using
16 KiB temporary CPU storage and reusing the final image buffer. Image and
camera-fog thresholds match. This intentionally changes the old coverage bias;
it is not pixel parity with the erroneous mask. Equal climate assignments no
longer cause unnecessary regeneration.

High/Ultra use the same procedural Earth-space field and one modeled curved
layer. A 64³ RG8 texture contains the deterministic morphology. Threshold-aware
horizontal-density mips smooth the distant field; they do not preserve nonlinear
extinction or individual silhouettes exactly. One additional near view-density
sample sculpts height-dependent local shape within occupied columns. Sun-path
sampling remains coarse. Detail fades with spatial footprint and distance.

A render-world gate resolves exact compiled pipelines, view eligibility, uniforms,
targets and noise before suppressing Light for that frame. It filters only the
marked flight view's cloud-plane submissions, corresponding shadow lists, and
weather-owned fog contribution. An inert fog component preserves the PBR specialization
key in Off and active volume modes; removing the component caused a cold shader
transition that temporarily hid the scene in native testing. Every extraction restores the fallback. A pending/error path
keeps Light; cancellation and Off/Light release upper-owned targets/noise/uniforms.
Small shader assets/control resources and Bevy's pipeline caches are separate.
Only the current single marked flight-camera arrangement is supported.

The volume is composed into linear HDR after the main scene pass and before
postprocessing. Current opaque cockpit/aircraft depth is supported. The depth
upsampler retains nearer valid cloud prefixes and rejects samples integrating
behind a foreground surface, avoiding both sky slits and cockpit overpainting.
Arbitrary future transparent/transmissive objects need separate integration.

## Explicit budgets and approximations

| Tier | Maximum target | View steps | Sun steps per occupied view step |
|---|---:|---:|---:|
| High |640×360|40|4|
| Ultra |960×540|64|6|

The target follows viewport aspect ratio without exceeding either dimension.
Two ray targets use 12 logical bytes/pixel. The full density mip chain is 599,186
bytes; retained CPU source is 524,288 bytes; the view uniform is 336 bytes. The
worst-case 3D texture-lookup counts with near detail are 55,296,000 High and
265,420,800 Ultra per frame, before empty/opacity/detail early exits. These counts
are not hardware GPU timing or total driver memory. Old in-flight resources and
engine caches can outlive the owner's replacement. The 336-byte uniform is updated
each active frame; density-upload counters do not count those writes.

A seed change regenerates the source texture. Cover uses a nearest 1/255 cache
key (maximum 0.5/255 difference) for density mips; small interpolation changes do
not recreate them every frame. The f64-derived local spherical shell is a visual
approximation to WGS84; canonical coordinates and coordinate conversion remain
in core. Integration is capped at 700 km with a 650–700 km fade. No temporal
history or per-cloud entities are created. Ground cloud shadows are not modeled
by the volume renderer.

## Scientific boundary

The available NOAA 1991–2020 climatology supplies Category-C reanalysis total-cloud
means. The current aircraft's fraction is applied to one surrounding layer;
distant ray samples do not each fetch their own climatological value. Individual
positions, shape, drift, heights and optical density are modeled. The fixed height
reference is departure's coarse NCEP surface plus geoid plus 1,500 m, with 1,200 m
thickness. It fixes the moving-hill artifact without claiming diagnosed cloud base.

No humidity, dew point, vertical stability, cloud water, layer winds or current
observations are fabricated. WMO cloud classifications are explanatory context.
Light keeps its old 2D position approximation; only High/Ultra share exact macro
field inputs, and their different sampling need not produce identical pixels.
Coverage validation is for statistical mip0 horizontal support, not observed
local sky amount or far rendered opacity. See [user guide](../cloud-quality.md)
and [climate provenance](../data/global-climate.md).

## Alternatives rejected

- Increasing the old noise threshold or duplicating total cover into three
  independent layers would distort cloud amount
- Giant built-in Bevy FogVolume boxes use scene-shadow coverage and bounding-radius
  attenuation that do not suit long cloud horizons in this scene
- A new material/noise pipeline in Light would add default per-pixel work
- An analytic multi-octave hash at every view/sun sample would add billions of
  hash operations at the proposed limits; a bounded lookup texture is used
- Unfiltered long ray steps or a 220 km cutoff produce poor high-altitude horizons
- Meteorological layer diagnosis from monthly 2D temperature/rain would be an
  unsupported inference; real profile inputs remain a future adapter
