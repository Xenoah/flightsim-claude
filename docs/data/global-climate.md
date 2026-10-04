# Bundled regional and seasonal climate

The simulator includes a **derived monthly climatology**, not live weather or a
forecast. Its numerical temperature, precipitation and cloud fields come from
[NOAA PSL NCEP/NCAR Reanalysis 1](https://psl.noaa.gov/data/gridded/data.ncep.reanalysis.html),
using the **1991–2020 long-term monthly means**. This is a reanalysis (a numerical
model constrained by observations), not a collection of airport observations.

NOAA classifies the cloud and precipitation variables as **Category C**:
the reanalysis model generates them from its atmospheric state; cloud observations
are not directly assimilated into these variables. They are not measured airport
cloud fractions. See [NOAA's variable classification](https://www.cpc.ncep.noaa.gov/products/precip/atlas_2/cont_data.html).

## Data that are real, and behavior that is modeled

The baked atlas retains all 12 months, all 192 source longitudes, and the exact
94 nonuniform Gaussian source latitudes. Nominal spacing is about 1.9 degrees;
the source cannot resolve an individual airport, small island, local mountain
valley, thunderstorm or sea breeze. The 1,180,456-byte runtime asset is bundled
into the binary. No account, API key, network or NetCDF decoder is needed at
runtime.

| Field | Source / interpretation |
|---|---|
| Near-surface air temperature | Monthly reanalysis `air.2m`, in kelvin, 2 m above the model's own surface |
| Precipitation | Monthly reanalysis `prate`, water-equivalent mean flux; converted from kg/m²/s to metres of water per second using 1000 kg/m³ |
| Cloud cover | Monthly reanalysis `tcdc`, total cloud cover for the entire atmosphere; percent converted to fraction |
| Model ground reference | Static reanalysis `hgt.sfc.gauss`, geopotential metres |
| Vertical offset | NOAA ETOPO 2022's EGM2008 undulation, interpolated from the same coarse geoid preparation used for global terrain |
| Altitude correction | **Modeled:** a 6.5 K/km lapse rate; not a sounding or local weather station measurement |
| Climate zone | **Derived broad label:** tropical, dry, temperate, continental/boreal, polar or highland; not an authoritative Köppen classification |
| Snow fraction | **Derived cold-phase visual indicator:** smoothly transitions from 1 at −3 °C to 0 at +3 °C; not measured snow cover, depth or current snowfall |

The monthly precipitation rate is the time average of wet and dry weather. It
must not be described as the instantaneous intensity of a rain shower. Cloud
coverage uses real reanalysis means, but the renderer's individual clouds,
layer heights, visibility and terrain color are simulator representations.
There is no implicit climate-derived wind override; explicit wind/turbulence
settings continue to control those physical inputs.

The [cloud quality settings](../cloud-quality.md) retain a single explicitly
modeled layer. Its height uses a fixed departure reference, so the whole layer
does not follow hills under the aircraft. Light keeps its periodic 2D placement
with calibrated covered area; High/Ultra share an Earth-space procedural field.
Their individual shapes can differ, while their nominal total-cloud fraction
and layer settings remain the same. Neither path supplies missing humidity,
dew point, vertical stability or observed cloud-type information.

## Vertical coordinates and atmosphere integration

`Geodetic.altitude` is WGS84 **ellipsoidal** height `h`. The climate atlas samples
EGM2008 geoid undulation `N` and uses `H ≈ h − N` before applying its lapse model.
The NCEP surface geopotential height is treated as approximate geometric MSL
height at this coarse scale. This is an explicit approximation, not a rigorous
conversion of the reanalysis vertical coordinate into EGM2008.

For source 2 m temperature `T`, source surface height `z`, and lapse `L=0.0065 K/m`:

```text
T_MSL = T + L * (z + 2 m)
T_query = T_MSL - L * (h - N)
ISA_temperature_offset = T_MSL + L * N - 288.15 K
```

Normalizing the source height before applying the destination height avoids
cooling mountainous grid cells twice. The readout/visual lapse is capped above
11 km MSL and its temperature is bounded to 150–350 K. This is not an upper-air
weather model. The FDM instead applies the equivalent, datum-adjusted offset
once to its existing ISA layers. Thus its exact temperature follows the ISA
geopotential-height profile; its pressure remains ISA, and its density and
speed of sound respond to the changed temperature. No live QNH, humidity-density
correction, icing forces or precipitation aerodynamics are implemented.

`flightsim-sim` owns this connection. `flightsim-fdm` has no dependency on
`flightsim-world`. `Simulation::atmosphere_sample()` reports the exact physical
air used for dynamics; `Simulation::climate_sample()` includes the broader
source/visual climate cues. Disabled climate preserves the previous ISA path.

## Dates, interpolation and replay

Monthly means are anchored at the midpoint of their **calendar month**, rather
than being switched abruptly on the first day. Space is interpolated bilinearly
using the actual Gaussian latitude array; longitude wraps at 360 degrees.
Two derived rows at ±90 degrees use the nearest Gaussian row's zonal mean.
Consequently the same pole is independent of longitude and the dateline has
no climate seam. December and January are interpolated cyclically.

`ClimateDate::from_month(1..=12)` selects an exact monthly midpoint. Validated
Gregorian dates, including February 29 in leap years, map each actual month
onto a non-leap reference month. The year selects no historical weather record.
The date is fixed for one flight: the simulation does not read wall time or
advance seasons according to accelerated visual time. Changing a date while
recording requires a new flight/session recording.

Replay format 2 adds a fixed 32-byte extension: flags, terrain identity,
climate annual phase and climate identity. It rejects undefined flags,
nonfinite/out-of-range dates and inconsistent presence/identity fields.
Reproduction checks reject changed bundled dataset identities. Format 1
continues to read as legacy terrain plus ISA. When both new settings are
disabled, the writer retains the **identical format-1 byte layout**.

## Reproducible offline bake

Sources, exact downloads, licenses, SHA-256 values and the shared geoid
preparation are listed in [global-sources.md](global-sources.md).
`scripts/bake_climate.py` requires Python 3, NumPy and h5py; it never installs
packages or performs network requests. NOAA source SHA-256 values are pinned
and checked. A changed input requires review, not a silently updated bake.

First prepare the shared terrain/geoid arrays using
`scripts/prepare-global-terrain.py` as documented in that source record. Put
the four original NOAA climate files in a source directory, then run:

```bash
python scripts/bake_climate.py \
  --input-dir /path/to/noaa-climate-files \
  --geoid /path/to/prepared/etopo_geoid_north_to_south_10min.npy \
  --geoid-metadata /path/to/prepared/terrain-preparation.json
```

Output consists of:

- `crates/flightsim-world/data/ncep-ncar-1991-2020.fsclim`
- its JSON provenance record with the complete input/output hashes
- generated `climate_metadata.rs`, keeping replay identity tied to the payload

For an isolated reproduction, add `--output /tmp/repro/climate.fsclim`. Its
Rust metadata and provenance are written beside that alternate file; the
checkout's compiled identity is left untouched. `--metadata-output` is available
for explicit build integration. The three resolved output paths must differ
from one another and from every input, including through symlinks. Every file
is replaced atomically; the provenance is marked `INCOMPLETE` before replacing
outputs and `COMPLETE` only after both atlas and identity writes finish. This
is fail-closed file replacement, not an atomic multi-file transaction.

The offline path and geoid-registration regression tests require Python and
NumPy, but no source downloads or h5py/HDF5 installation:

```bash
python -m unittest discover -s scripts/tests -p test_bake_climate.py
```

Temperature quantization is 0.01 K; precipitation is 1e-10 m/s (0.00864 mm/day);
cloud fraction is 1/255; model height is 1 m; geoid undulation is 0.1 m.
Maximum round-to-nearest errors are half these steps. These small encoding
errors do not imply equally precise underlying meteorology.

The binary header fixes version, shape, record sizes and payload length. The
runtime checks all of them, the FNV-1a payload identity, latitude ordering,
physical ranges and pole invariants before publishing one immutable `Arc`
snapshot through `OnceLock`. FNV-1a is a corruption/replay identity check, not a
cryptographic authentication mechanism. The provenance uses SHA-256.

## Verification scope

Regression tests cover an independently read NOAA grid value; regional
longitude contrasts; opposing hemisphere seasons; monsoon rainfall; altitude
lapse; finite bounds; dateline/pole/year continuity; malformed baked inputs;
ISA preservation; physical density/force changes; cadence independence; exact
replay/restart; and hostile version-2 extension bytes. These are software and
data-processing checks, not a validation of flight-local meteorological truth.

Credit NOAA Physical Sciences Laboratory, Boulder, Colorado, USA, and NCEP/NCAR
Reanalysis 1. NOAA's [data-use guidance](https://www.psl.noaa.gov/data/help/)
permits use of its federal public-domain data with its stated caveats. This
derived simulator atlas is not an official NOAA product or an endorsement.
