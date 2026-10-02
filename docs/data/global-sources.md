# Global terrain and climate source record

Verified **2026-10-02 UTC**. This document describes the sources and offline
preparation, not an aviation/navigation-quality database. Runtime use is offline;
neither the simulator nor its FDM downloads data or needs an API key.

## Selected stack

- **Terrain:** NOAA ETOPO 2022 v1 ice-surface elevation, taking every tenth point
  from the global 60 arc-second product. The resulting 10 arc-minute grid is
  approximately 18.5 km apart at the equator. This is a real, coarse elevation
  fallback; peaks, small islands, and coastline details can disappear
- **Geoid:** the co-located NOAA ETOPO 2022 EGM2008 geoid product, with the identical
  stride and node registration. Keep orthometric height `H` and geoid undulation
  `N` separate until the consumer computes WGS84 ellipsoidal height `h = H + N`
- **Land/ocean:** Natural Earth 1:10 million land polygons, v5.1.1, tested at the
  exact terrain sample nodes. “10m” in this source name means map scale, not
  ten-metre spatial resolution. Natural Earth v5.0.0 lakes and the enclosed
  Caspian Sea ring supply a separate inland-water mask. Classification remains
  independent of elevation sign
- **Physical-surface corrections:** selected Copernicus GLO-90 hydro-edited,
  EGM2008 surface samples replace ETOPO lake bottoms and independently check
  negative land nodes. This makes the terrain asset a mixed-license derivative;
  it is no longer solely a NOAA/Natural Earth public-domain derivative
- **Climate:** NOAA PSL NCEP/NCAR Reanalysis 1 monthly 1991–2020 climatologies,
  retaining all 12 months and the 94 × 192 Gaussian grid. These are reanalysis
  long-term means, not station observations, a forecast, or current weather

Regional high-resolution `.fsdem` data remain the appropriate source for local
terrain detail and retain their existing WGS84 ellipsoidal-height contract.
The fallback does not turn coarse source points into 30 m observations.

## Rights and attribution

### NOAA ETOPO 2022 and EGM2008 grid

The [official dataset metadata](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ngdc.mgg.dem:etopo_2022)
explicitly states **CC0-1.0 worldwide** and includes a navigation-use warning.
Use the provider's citation: NOAA National Centers for Environmental Information
(2022), ETOPO 2022 15 Arc-Second Global Relief Model,
[doi:10.25921/fd45-gt74](https://doi.org/10.25921/fd45-gt74), accessed 2026-10-02.
Identify our files as derived/decimated simulator assets, not official NOAA products.

The [geoid's source attributes](https://www.ngdc.noaa.gov/thredds/dodsC/global/ETOPO2022/60s/60s_geoid_netcdf/ETOPO_2022_v1_60s_N90W180_geoid.nc.das)
separately mark its NGA-derived data as public domain. Its description identifies
the WGS84-to-EGM2008 vertical-offset grid, derived from `egm08_25.gtx`.
The [ETOPO user guide](https://www.ngdc.noaa.gov/mgg/global/relief/ETOPO2022/docs/1.2%20ETOPO%202022%20User%20Guide.pdf)
describes product resolution and source integration.

### Natural Earth

[Natural Earth terms](https://www.naturalearthdata.com/about/terms-of-use/) put its
raster and vector data in the public domain, including modification and commercial
reuse. Attribution is encouraged rather than required. Credit: “Made with Natural
Earth.” The [land product page](https://www.naturalearthdata.com/downloads/10m-physical-vectors/10m-land/)
identifies v5.1.1 and warns of coastline accuracy limits in some regions.

### NOAA PSL climate

[NOAA PSL data-use guidance](https://www.psl.noaa.gov/data/help/) identifies federal
data as public domain unless otherwise annotated, and asks users to acknowledge
PSL. It disallows misleading ownership, endorsement, and official-product claims.
Credit NOAA Physical Sciences Laboratory, Boulder, Colorado, USA, and NCEP/NCAR
Reanalysis 1; identify the derived climatology period as 1991–2020. See the
[dataset page](https://psl.noaa.gov/data/gridded/data.ncep.reanalysis.html) and
[monthly catalog](https://www.psl.noaa.gov/thredds/catalog/Datasets/ncep.reanalysis/Monthlies/surface_gauss/catalog.html).

### Copernicus GLO-90 surface corrections

The [official GLO-90 license](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf)
is a specific free-and-open license permitting reproduction, distribution and
adaptation with mandatory notices. It is not CC0 or CC-BY. The applicable section
is pages **19–21** of this 24-page PDF, headed COP-DEM-GLO-90-F; earlier pages
describe different restricted products. An unchanged-page excerpt is archived at
[copernicus-glo90-license.pdf](copernicus-glo90-license.pdf). Source PDF SHA-256:
`bb4a01dcd7f61acefa81c9ccd76af975e12096158169acf0d7b5c44c26c8701f`;
excerpt SHA-256:
`817048e228139aab85724a5c2a47c803c77b5dd3f3de74e827e1a8c5b55d9263`.

The derivative notice required by Article 6(b) is:

> produced using Copernicus WorldDEM™-90 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved

Article 6(c) also requires this disclaimer:

> The organisations in charge of the Copernicus programme by law or by delegation do not incur any liability for any use of the Copernicus WorldDEM™-90

Preserve these notices and the applicable terms when redistributing the derived
atlas. Do not imply provider endorsement. See ATTRIBUTION.md for bundled credits.
These corrections use GLO-90, so a WorldDEM-30 notice alone is insufficient.

## Terrain downloads, registration and checksums

`scripts/prepare-global-terrain.py` uses the following exact, no-authentication
downloads. DAP2 stride selection avoids downloading the full-resolution grids.

| Source | Bytes downloaded | SHA-256 |
|---|---:|---|
| [ETOPO ice-surface, stride 10](https://www.ngdc.noaa.gov/thredds/dodsC/global/ETOPO2022/60s/60s_surface_elev_netcdf/ETOPO_2022_v1_60s_N90W180_surface.nc.dods?z%5B0:10:10799%5D%5B0:10:21599%5D) | 9,357,398 | `575c7a0c84f18b41430a723f26475fa41b1d787c94fa641f0d0705291e3991fd` |
| [ETOPO geoid, stride 10](https://www.ngdc.noaa.gov/thredds/dodsC/global/ETOPO2022/60s/60s_geoid_netcdf/ETOPO_2022_v1_60s_N90W180_geoid.nc.dods?z%5B0:10:10799%5D%5B0:10:21599%5D) | 9,357,389 | `0bdb4e88ed0d65c08052d2ae62bd981353333859c24a80dfd57c088efe45a600` |
| [Natural Earth land v5.1.1](https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_land.zip) | 3,269,070 | `e547d749445eaa0964aba76738090ec88f5e63c4585122170f98c67a7ea922dc` |
| [Natural Earth lakes v5.0.0](https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_lakes.zip) | 2,349,685 | `0803a06f9c3cb4671d89b68c48b142aad9366ba40f665245e12a913fbc61722a` |
| [Copernicus public GLO-90 tile inventory](https://copernicus-dem-90m.s3.amazonaws.com/tileList.txt) | 1,111,950 | `e5a5efe088e70506bc1007d22006bdcb09b0ec03177b62f9652363c13f49ed97` |

The mutable Natural Earth download name is pinned by its complete checksum; a
changed source must be reviewed rather than accepted silently. NOAA source and
derived-array checksums are also enforced. The ETOPO response is limited before
reading to prevent accidental full-grid downloads.

Both selected source arrays are 1080 rows × 2160 columns, float32 metres, with:

- source latitude ascending from −89.99166666666666° to +89.84166666666667°
- source longitude ascending from −179.99166666666667° to +179.84166666666667°
- spacing exactly 1/6° in each direction, inherited from stride 10 of 60″ cells
- original ETOPO pixel centres, not newly averaged 10′ cell centres

Preparation reverses rows and reorders longitude without interpolating values:

- output latitude origin +89.84166666666667°, step −1/6°
- output longitude origin +0.008333333333325754°, step +1/6°, periodic at 360°
- longitude never contains a duplicated seam column
- exact poles are absent; the runtime must close polar caps continuously

The **uncorrected ETOPO** range at these sampled nodes is −10,511.002 to +6,734.173 m;
the geoid range is −106.87641 to +85.655365 m. These sampled extrema are not the
Earth's extrema. The mask contains 772,407 land nodes, including 3,375 nodes with
negative orthometric height. For example, a Death Valley node remains land at
approximately −83.78 m. A height-sign mask would incorrectly flood such terrain.

Natural Earth and ETOPO coastlines can disagree. Point classification also misses
islands smaller than the spacing. ETOPO explicitly includes lake **bottoms**, even
in its “ice surface” product. Neither a negative-only lake correction nor a blanket
zero clamp is valid: Great Lakes bottoms may be positive, whereas dry depressions
and the Dead Sea legitimately have negative surface heights.

The v2 preparation therefore emits **disjoint dry-land and inland-water masks**.
Its separate, checksum-pinned correction ledger records selected Copernicus
surface samples and the derived lake levels. The consumer replaces remaining
ocean bathymetry with `H = 0` before physical-surface interpolation. An EGM2008
sea-level approximation has ellipsoidal height `N`, not globally zero.
Oceanography's actual mean sea surface is more complex.

### Lake and coastal correction policy

The [Copernicus product description](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM)
documents edited coastlines, flattened water bodies and EGM2008 heights. The
[public COG documentation](https://copernicus-dem-90m.s3.amazonaws.com/readme.html)
documents provider-created **average-downsampled** overviews and geographic
registration. This keeps the correction in the same vertical datum as ETOPO;
HydroLAKES' approximate MSL elevations are not silently substituted as EGM2008.

Correction candidates are all source nodes within the independently rasterized
Natural Earth lakes, the sole enclosed Natural Earth land ring (Caspian Sea), and
all remaining Natural Earth land nodes with negative ETOPO height. Candidate
selection does not classify negative heights as ocean. Independent Copernicus
heights can remain negative, preserving below-sea-level dry land.

Source acquisition reads only each relevant COG's smallest averaged overview,
using bounded HTTP Range requests. The range must be honored with HTTP 206.
Each overview is capped at 1 MiB; aggregate acquisition is capped at 512 MiB.
The ledger records provider URLs, exact byte ranges, hashes, overview dimensions,
georeferencing, selected pixels, raw values and the correction method. These
samples have coarser support than the native 90 m DEM, typically about 360 m; they
must not be represented as native 90 m observations.

Lake levels are static simulator approximations derived from the hydro-edited
surface samples, not current gauges or surveyed lake levels. Pixel centres inside
each lake polygon, inset by half the overview-pixel diagonal, supply the estimate.
The repeated flat value is used only when it has at least eight pixels, at least
10% of the selected pixels and twice the runner-up support. Tied modes use the
lower value deterministically. Where no such plateau is isolated, the in-polygon
pixel median is explicitly marked **uncertain**, with counts, quartiles and full
range retained. These thresholds are quality flags, not assertions of survey
accuracy. The reviewed data contain 160 uncertain estimates among 942 represented
lake features. Narrow Ozero Dyupkun is an example where the overview mixes steep
shore terrain and cannot establish a dependable lake level.

A prior coarse-node median was rejected after it mixed high shoreline pixels
into narrow lakes. The accepted dense-pixel method isolates 253 m for Lago San
Martín and 178.5 m for Lago Argentino in this provider's EGM2008 data. Raw coarse
samples and their old reference spread are retained as diagnostics, not presented
as the support for the chosen modal level. Individual Natural Earth features,
including separately drawn basins/ponds sharing a name, get independent estimates.
Two fully overlapped anonymous features have no final grid nodes and are recorded
as inactive. The enclosed Caspian polygon explicitly excludes mapped dry islands.

Averaged shoreline pixels can mix water and land, and Natural Earth geometry is
generalized. Failures, unexplained deep non-ocean heights or incomplete candidate
coverage prevent output rather than silently producing a partially corrected globe.

Lake surfaces and dry land remain distinct in the atlas. No bathymetric depth is
used as a flight collision surface for a classified inland-water node. The
unchanged geoid array is still used for `h = H + N`, including negative-height
lakes; no ocean-level override is applied to them.

### Reproduction

Build-time dependencies are Python 3, NumPy, pyshp and Shapely. The preparation
script does **not** install packages; use an appropriately provisioned environment.
Verified with NumPy 2.3.5, pyshp 3.1.6 and Shapely 2.1.2. No GIS decoder becomes a
simulator runtime dependency. No source file or software install is fetched unless
the operator explicitly selects the relevant step.

```bash
# Explicit core-source network acquisition, about 26 MB; no credentials needed.
python scripts/prepare-global-terrain.py \
  --source-dir /path/to/source-cache --output-dir /path/to/prepared --download

# Fully offline repeat using exactly the same reviewed source bytes.
python scripts/prepare-global-terrain.py \
  --source-dir /path/to/source-cache --output-dir /path/to/prepared
```

Prepared files include `elevation.f32le`, `geoid.f32le`, `land.u8`, `inland_water.u8`, matching `.npy`
arrays, exact coordinate arrays, and `terrain-preparation.json`. Raw arrays are
row-major and headerless; dimensions, origin, units, methods and checksums are in
the JSON, whose schema is `flightsim-global-terrain-inputs-v2`. The small reviewed
correction ledger is `docs/data/global-surface-corrections.json`; its checksum is
pinned by the preparation script. Pass these to the separate global-atlas packer;
they are not `.fsdem`
files and must not be interpreted as ellipsoidal heights yet.

The reviewed correction ledger is 8,159,675 bytes, SHA-256
`0c6a217df2f0987cbdbbfe0e8c0347eea77ea5fc259516a0f7246d955c294cab`.
It corrects 10,229 source nodes and distinguishes 766,682 dry-land nodes from
7,269 inland-water nodes. The unchanged geoid and final v2 raw arrays are:

| Corrected v2 output | SHA-256 |
|---|---|
| `elevation.f32le` | `56edec09212c2f73b9674dfb689226f1ab8e9436283fd6b0e139e87773bfee82` |
| `geoid.f32le` | `d7dc76e9a96a42e0de9668665f4b7c9ce66196131c594139d32266b8c64ef193` |
| `land.u8` | `fbf3c2a64c7207d920c7ac2bb42437f4c1bbe5dd7f4b1825eb2c8aca3a301f94` |
| `inland_water.u8` | `7c39c16f20649234c0bfd0e53a0162a59da078555067bdbc4f472e7c4e94d71d` |

There are 1,617 negative dry-land nodes, with minimum −408.99048 m; the lowest
non-ocean modeled surface is −427 m. The 160 uncertain lake features cover 363
source nodes. These are source-grid results; subsequent canonical packing and
resampling are a separate, explicitly documented operation.

The following are the **uncorrected baseline hashes**, still checked before
applying the correction ledger. Final corrected hashes and both masks are in the
ledger and generated preparation manifest.

| Uncorrected baseline raw output | Bytes | SHA-256 |
|---|---:|---|
| `elevation.f32le` | 9,331,200 | `1d651c98645ce9a0604f912754a5720c3b5a4d406706172554be98903dd98b65` |
| `geoid.f32le` | 9,331,200 | `d7dc76e9a96a42e0de9668665f4b7c9ce66196131c594139d32266b8c64ef193` |
| `land.u8` | 2,332,800 | `b70a947473ab876287d6d237f3b0714904929af61f0483d54e5d6f732628399d` |

For a full source audit, supply the cached, bounded Copernicus overview ranges.
Pillow (verified version 12.3.0) is an additional build-time dependency for this audit only. Adding
`--download` explicitly permits fetching missing pinned ranges, subject to the
same per-file and aggregate limits; there is no automatic package installation.

```bash
python scripts/prepare-global-terrain.py \
  --source-dir /path/to/source-cache --output-dir /path/to/prepared \
  --verify-copernicus-sources /path/to/copernicus-overview-cache
```

The complete cached-source replay passed on 2026-10-02: range hashes, TIFF
registration, coordinate-to-pixel lookups, sampled values, dense lake statistics
and final output hashes all reproduced. The replayed four raw arrays were also
compared byte-for-byte with the delivered v2 arrays. Focused checks rejected
missing correction nodes, wrong geographic coordinates, mismatched source
heights/lake identities and a tampered ledger; the Caspian island remained dry.
This is a pipeline/source-replay check, not independent geophysical validation or
a claim of surveyed lake-level accuracy. Canonical packing and runtime tests are
separate from this source-preparation check.

Development verification also compared the selected surface values against an
independent NOAA OceanWatch ERDDAP delivery of ETOPO 2022 after its longitude
reordering; all float32 samples matched. This checks delivery/registration, not
independent geophysical accuracy.

**Avoid the WCS `resx`/`resy` route for this job.** During verification the NOAA
WCS endpoint accepted the request but returned complete ~933 MB NetCDF grids.
For comparison only, HEAD-verified full 60″ GeoTIFF sizes are 465,969,062 bytes for
[surface elevation](https://www.ngdc.noaa.gov/mgg/global/relief/ETOPO2022/data/60s/60s_surface_elev_gtif/ETOPO_2022_v1_60s_N90W180_surface.tif)
and 426,406,081 bytes for the
[geoid](https://www.ngdc.noaa.gov/mgg/global/relief/ETOPO2022/data/60s/60s_geoid_gtif/ETOPO_2022_v1_60s_N90W180_geoid.tif).

## Climate downloads and interpretation

The climate preparation script is separately owned and documented in
`scripts/bake_climate.py`. All four files below were actually downloaded and
inspected; their coordinate axes match exactly. Temperature, precipitation and
cloud source attributes explicitly specify `1991/01/01 - 2020/12/31`.

| Variable and official no-key URL | Bytes | SHA-256 |
|---|---:|---|
| [2 m air temperature](https://downloads.psl.noaa.gov/Datasets/ncep.reanalysis/Monthlies/surface_gauss/air.2m.mon.ltm.1991-2020.nc) | 964,084 | `c86a3c575010ca2c9414b24022361c43be6966dcdc62c821c1918e9c44ca9be9` |
| [Precipitation rate](https://downloads.psl.noaa.gov/Datasets/ncep.reanalysis/Monthlies/surface_gauss/prate.sfc.mon.ltm.1991-2020.nc) | 1,155,046 | `35c390d9b82128925f960b8189992f3f617d131d8f4af4051dcd216d40825798` |
| [Total cloud cover](https://downloads.psl.noaa.gov/Datasets/ncep.reanalysis/Monthlies/other_gauss/tcdc.eatm.mon.ltm.1991-2020.nc) | 1,102,408 | `61c7021d96fdf65d06ca63c8b1106b2e0dd94b93412a41b80463f3b306f7ef7e` |
| [Static model surface geopotential height](https://downloads.psl.noaa.gov/Datasets/ncep.reanalysis/Monthlies/surface_gauss/hgt.sfc.gauss.nc) | 55,502 | `0862a41820743c04e94c89b0475d734232f517431951dd9d413658e87d6f5d88` |

Source axes are 94 nonuniform, descending Gaussian latitudes, approximately
+88.542° to −88.542°, and 192 regular longitudes 0° to 358.125° at 1.875° spacing.
Do not replace the latitude coordinates with a uniform spacing. Latitude caps,
periodic longitude, December/January interpolation and month anchoring require
explicit treatment in the derived atlas/runtime.

Source units and actual ranges:

- `air`: K, 199.707855–312.074982; 12 × 94 × 192 samples
- `prate`: kg m⁻² s⁻¹, 5.98918 × 10⁻⁸–3.11199 × 10⁻⁴; 12 × 94 × 192 samples
- `tcdc`: percent, 0.062364–89.596603; 12 × 94 × 192 samples
- `hgt`: geopotential metres, −539–5760; 1 × 94 × 192 samples

Monthly mean precipitation is a time-averaged flux, not the intensity of an
individual rain shower. Cloud fraction is an atmospheric mean, not a map of
current cloud objects. Snow cover, biome classes and altitude-lapse corrections
derived from these inputs are additional simulator models, not observed NOAA
fields. A fixed temperature lapse rate is not a meteorological sounding.

If correcting temperature for altitude, first relate aircraft ellipsoidal height
to an approximate MSL height using the EGM2008 geoid, then compare with the
source model surface elevation and the 2 m reference height. NCEP geopotential
height is not rigorously identical to EGM2008 orthometric/geometric height; using
it as such is an explicit coarse-model approximation. Using raw ECEF/ellipsoidal
altitude directly as height above the source terrain would add a datum error and
could count mountain elevation twice.

## Alternatives considered

- **Copernicus GLO-30/GLO-90:** retain for selected high-resolution local DEMs;
  bounded GLO-90 overview samples are additionally used by the correction policy above.
  [Public AWS access](https://registry.opendata.aws/copernicus-dem/) needs no AWS
  account. GLO-30 Public has some unreleased tiles; neither product supplies
  ocean tiles. It is a surface model including vegetation/buildings, with
  EGM2008 vertical reference. Its [specific license](https://documentation.dataspace.copernicus.eu/APIs/SentinelHub/Data/DEM/resources/license/License-COPDEM-30.pdf)
  permits redistribution/adaptation but requires notices and downstream terms;
  it is not simply public-domain data. Full-world high-resolution processing is
  inappropriate for this compact baseline
- **Natural Earth II:** a public-domain visual basemap, not numerical elevations
  or climate normals. The [small shaded-relief download](https://www.naturalearthdata.com/downloads/50m-raster-data/50m-natural-earth-2/)
  is advertised as 39.69 MB; optional visual texture work should keep its idealized
  land-cover nature distinct from physical source data
- **NASA Blue Marble Next Generation:** [monthly composites](https://science.nasa.gov/earth/earth-observatory/blue-marble-next-generation/base-map/)
  can support seasonal appearance, but do not supply temperature or precipitation
  normals. [NASA's own product description](https://science.nasa.gov/earth/earth-observatory/blue-marble-next-generation/)
  identifies compositing, cloud/snow and coastal-water limits and asks for NASA
  Earth Observatory credit. No NASA imagery is bundled by this preparation
- **SRTM alone:** the [survey coverage](https://pubs.usgs.gov/fs/2009/3087/)
  excludes the polar regions, so it cannot independently satisfy full-Earth
  fallback coverage. Existing SRTM inputs require their own EGM96 conversion
- **WorldClim:** excluded from redistributable assets. Its [current terms](https://www.worldclim.org/about.html)
  prohibit redistribution/commercial use without prior permission, despite free
  download availability
