# Cloud settings and scientific limits

Cloud rendering is independent of the existing surface/lighting preset. Use
`--cloud-quality off|light|high|ultra`, press **F3** to cycle, or **Shift+F3**
to return to Light. Light remains the default. F4 still controls the separate
graphics preset. The modal world map captures these keys; pause permits them.
Cloud controls do not change aircraft forces, wind, turbulence or replay physics.

The HUD identifies `MONTHLY MODEL`, `USER MODEL`, or `CLEAR`. An upper setting
shown with `(LIGHT)` is using the lightweight fallback while its renderer is
unavailable. A selected setting alone does not prove that a GPU pipeline is ready.

## What determines the clouds

With climate enabled and no manual cloud arguments, the nominal cloud fraction
comes from NOAA PSL NCEP/NCAR Reanalysis 1 **1991–2020 monthly total-cloud means**.
This is a coarse reanalysis climatology, not today's weather. Cloud and
precipitation fields are model-generated Category C variables, rather than
direct assimilated cloud observations. See [NOAA's classification](https://www.cpc.ncep.noaa.gov/products/precip/atlas_2/cont_data.html)
and the [bundled climate documentation](data/global-climate.md).

The current aircraft location supplies that fraction to the surrounding single
layer, refreshed every two seconds. Distant ray samples do not each fetch their
own regional climatology. Crossing a broad climate gradient changes the layer's
threshold; Earth anchoring describes its procedural coordinates, not a worldwide
observed cloud-amount field.

Individual cloud positions, shapes, drift, base, thickness and optical density
are simulator models. The atlas has no dew point, humidity profile, stability,
cloud water or layer winds. It therefore cannot diagnose cumulus, thunderstorms,
fronts, orographic uplift, or a lifting-condensation level. Monthly mean rain is
not the current intensity of a shower. The [WMO cloud classifications](https://cloudatlas.wmo.int/en/clouds-definitions.html)
provide explanatory context; the simulator does not claim to observe those types.

The modeled single layer uses the departure location's fixed coarse NCEP surface
height plus its geoid undulation plus 1,500 m; thickness is 1,200 m. The result is
an ellipsoidal height, consistent with the rest of the application. This fixed
reference prevents the entire cloud layer following hills under the moving
aircraft. It does not model local uplift, inversions or a sounding. The coarse
reference can differ substantially from the regional DEM.

Manual `--cloud-cover`, `--cloud-base`, `--cloud-top` and `--cloud-visibility`
remain explicit overrides. Base/top are WGS84 ellipsoidal metres, not AGL or MSL.
These visual parameters do not create cloud collisions or aerodynamic forces.

## Rendering tiers

- Off removes weather-owned cloud geometry, disables its fog and releases cloud-owned
  volume resources. A transparent, zero-strength fog placeholder keeps the scene
  shader ready during toggles; the small camera uniform/shader branch remains
- Light retains two alpha-masked PBR planes, the existing 256×256 mask, and
  camera-local fog. Its finite periodic mask is now calibrated per seed to
  match nominal covered area; this deliberately corrects the old coverage bias
- High and Ultra share one Earth-anchored procedural density field and the same
  weather inputs. Ultra raises resolution and sampling budgets, not cloud amount
  or weather severity

Light's repeated 2D placement is a geometric approximation and differs from the
Earth-space High/Ultra footprint. Switching Light to an upper tier can change
individual cloud positions. Their nominal monthly cloud fraction, layer bounds,
seed and visibility remain the same; this is not a change in real meteorology.
A local procedural cloud patch can differ from its long-term nominal fraction.
Validation distinguishes local snapshots from an ensemble over seeds and drift
phases; the noise repeat distance is not the validation patch size.
Ensemble calibration does not establish the actual cloud distribution at an airport.

The upper renderer uses one curved layer, bounded ray integration, approximate
single scattering and ambient illumination. Distant density is spatially averaged;
it does not preserve every distant cloud silhouette. It respects opaque scene
and aircraft depth. Arbitrary future transparent/transmissive objects need further
integration. Terrain cloud shadows, multiple weather layers, precipitation and
live-weather downloads are outside this implementation.

## Optional future meteorological inputs

A weather profile adapter should preserve source, issue/valid time, region,
vertical datum, missing values and uncertainty. Cloud-layer fractions require
an explicit overlap model; total-cloud fraction cannot simply be copied to three
independent layers. Surface temperature plus a contemporaneous dew point can
support a parcel LCL approximation only under the stated parcel assumptions;
it cannot determine a detached upper cloud layer or cloud top. No such missing
inputs are fabricated here, and no network service or credentials are configured.

All procedural cloud textures are generated by the project. No new third-party
texture assets are downloaded or redistributed. Existing NOAA notices and the
separate binary-distribution review remain applicable.
