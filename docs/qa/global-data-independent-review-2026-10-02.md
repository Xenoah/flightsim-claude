# Independent global terrain / climate data review

Review began 2026-10-02 01:30 UTC on the uncommitted `agent/global-map-climate`
working tree. Scope: `flightsim-world::{global,climate,terrain}`, the global
tile encoder, preparation/bake scripts, bundled source data and provenance.
This is a source/data review, not an application-rendering or flight test.
Production files are being edited by their respective owners during the review.

## Current result: corrected FSGT v2 re-audit, 02:29 UTC

The original data-handling findings below are resolved in the current source and
bundled bytes. This is a **data/source-review pass**, conditional on the still
unrun Rust compilation, Rust tests and actual application validation.

Current terrain identity:

- Dataset `ETOPO2022-COP90-NE-2048x1024-surface-v2`
- 2048 × 1024 canonical cell centres; 8,912,960 bytes
- SHA-256 `57bc03c12caa772ecbd52f1d24cd12704ccc4ace5123e9d8c26b6e454907f13e`
- FNV-1a `500e8db32bde7019`
- Correction ledger SHA-256
  `0c6a217df2f0987cbdbbfe0e8c0347eea77ea5fc259516a0f7246d955c294cab`

Independent rechecks:

1. Rebuilt source arrays offline from pinned NOAA and Natural Earth inputs plus
   the correction ledger, then packed them. The result matches the current
   bundled binary byte-for-byte. The independent source worker separately
   replayed every cached Copernicus range, pixel registration and lake statistic;
   that full provider-range replay is not claimed as the reviewer's own run
2. Independently reconstructed **all 2,097,152 canonical nodes**, using corrected
   physical H, separate N, both masks and the specified feature-pooling conflict
   order. Every quantized H/N value and mask bit agrees. Masks are disjoint;
   stored ocean H is zero. There are 688,434 dry-land and 6,770 inland-water nodes
3. Checked **2,653,849 sample positions**: every canonical node, 524,288 seeded
   random locations, and explicit poles/caps/seam cases. The largest sampled
   direct-atlas versus bilinear DEM difference was **0.000234375 m**, at levels
   6, 8 and 13. This excludes triangular-mesh interpolation and Earth-chord error
4. Lake Baikal at 53.34166666666667°N, 108.175°E now gives H=455 m; Caspian at
   42.00833333333333°N, 50.00833333333333°E gives H=−28 m; the tested Lake Superior
   interior gives H=181 m after quantization. Central Pacific H remains zero
5. Feature pooling preserves a Death Valley canonical node at H=−84 m
   (36.123046875°N, −116.806640625°E) and a Dead Sea inland-water node at H=−427 m
   (31.728515625°N, 35.595703125°E). This deliberately preserves coarse features
   at nearby lattice nodes; arbitrary geographic queries still mix surrounding
   terrain. It is not a surveyed-position accuracy claim
6. Reran all **5 terrain packer tests** and **9 climate baker tests** successfully,
   including injected final-marker failure, path collisions, registration and
   preparation-schema compatibility. Climate rebaked from the new v2 preparation
   still matches its unchanged bundled SHA-256 exactly

Two additional bake-boundary findings were reproduced and fixed during this pass:

- `scripts/pack-global-terrain.py` previously accepted an unchanged prepared
  raster with its manifest longitude origin shifted by 180°. The current packer
  pins the reviewed origin and orientation; the same hostile fixture now fails
  before output. Generic explicit geometry remains a separate Rust library API
- `scripts/bake_climate.py` previously rejected the new v2 terrain-preparation
  schema despite identical geoid data. It now accepts reviewed v1/v2 schemas while
  retaining the exact geoid hash and registration checks; v0/v3 are rejected

The revised source combines public-domain inputs with licensed Copernicus GLO-90
corrections. The reviewer checked the applicable GLO-90-F section (pages 19–21)
in the [official combined license PDF](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf),
the archived excerpt hash, derivative/liability notices, and release-workflow
inclusion. The aggregate atlas must not be called entirely public domain.

Remaining intentional limitations: 160 of 942 represented lake features have
explicitly weak estimates affecting 363 original source nodes; the coarse
overviews cannot establish dependable surfaces in every narrow steep lake.
The source-to-nearest-canonical-centre assignment is within half a cell per
axis, but that is not a precise shoreline/feature-footprint accuracy bound.
The 19.5 km grid, generalized masks, feature pooling and bilinear shoreline
mixing remain unsuitable for local aviation/navigation or surveyed contacts.

## Findings and disposition

The following records the initial review and subsequent fixes, including
superseded v1/intermediate-v2 data. Use the current result above for final status.

### 1. High: water bathymetry is exposed as land collision terrain

- Location: `crates/flightsim-world/src/global.rs:322`, consuming the mask made
  by `scripts/prepare-global-terrain.py:183-185` in the original reviewed version
- The independent Natural Earth coastline mask is necessary to retain genuine
  negative land, but it is not a sufficient surface-water classification for
  ETOPO bathymetry. The initial atlas has 65 land-classified nodes below −500 m,
  including 13 below −1,000 m. At 53.34166666666667°N, 108.175°E, the Lake Baikal
  sample is −1,177 m orthometric and −1,212.4 m ellipsoidal. At
  −15.491666666666674°, 168.175°, the raw relief is −1,393.5 m and becomes
  −1,394 m in the atlas despite the land bit. These are not ordinary terrestrial
  depressions. The former is lake-bottom elevation; the latter is a
  coastline/source disagreement requiring classification evidence
- Minimal safe remedy: independently classify inland water and resolve its
  surface elevation; reconcile documented coastal mismatches using source
  evidence. Preserve true negative land. A blanket `max(H, 0)` is not acceptable
- Status: resolved by independent lake/Copernicus correction, separate inland-
  water classification and feature-preserving canonical regridding; see current
  v2 re-audit above. Weak lake estimates remain explicitly modeled limitations

### 2. Medium: rebake output/provenance and input identity boundaries

- Initial `scripts/bake_climate.py:144-165` always changed the checkout's
  `climate_metadata.rs`, even for an alternate `--output`. Fixed by owner;
  independently rebaked to a temporary alternate path and confirmed identical
  binary bytes and unchanged canonical metadata
- Initial climate inputs lacked pinned SHA-256 and month-axis checks. Fixed by
  owner. An independently corrupted source is now rejected before output;
  source month coordinates and field metadata are checked in addition to hashes
- Initial `scripts/pack-global-terrain.py:97-112` replaced the atlas while an old
  COMPLETE manifest could remain after a later write failure. Fixed by owner
  to mark INCOMPLETE before replacement. Reviewer independently injected a
  final-marker write failure and confirmed the exact replacement atlas remained
  paired with an INCOMPLETE marker, not stale COMPLETE provenance
- Follow-on climate path collision reproduced: `--output collision.json`
  returned success and described a 1,180,456-byte atlas, but wrote a 6,975-byte
  COMPLETE provenance JSON over the binary. At the then-current
  `scripts/bake_climate.py:188`, only the metadata destination was compared with
  the other two paths. Require three distinct resolved destinations, and reject
  any destination that aliases an input. Status: fixed and independently
  rechecked; `.json` binary output now fails before writing
- Follow-on geoid-registration corruption reproduced: replacing only the
  preparation manifest's latitude origin with −89.84166666666667° was accepted
  by `scripts/bake_climate.py:93-113`, despite the pinned geoid bytes. It produced
  a new COMPLETE climate asset using the wrong polar geoid across most of Earth.
  Require the exact documented finite shape/origin/spacing and latitude coverage.
  Status: fixed and independently rechecked; the same altered-axis manifest
  now fails before writing an atlas

### 3. Medium: calendar month boundary round-trip

- Original location: `crates/flightsim-world/src/climate.rs:147-150`
- August 1 midnight has phase `212 / 365`; multiplying it by 365 yields
  `211.99999999999997`, so `month()` returned July. This affects month reporting
  and selectors, though the interpolated climate stays continuous
- Fix: compare phase against each `month_start / 365`, avoiding the lossy
  round-trip. Owner implemented this and added every month-boundary plus
  immediately-preceding-float regressions. Source reviewed; Rust tests not run

### 4. Medium: terrain contact discrepancy is not always "tiny"

- Location: original `docs/global-terrain.md:105-106`; sampler path at
  `crates/flightsim-world/src/global.rs:290-313`
- Physics samples the atlas directly; each 33 × 33 generated DEM resamples it.
  An independent deterministic probe of 100,001 source nodes found these
  absolute bilinear height differences, before triangle curvature error:

  | Generated tile level | Largest sampled difference |
  |---|---:|
  | 7 | 482.96 m |
  | 8 | 226.12 m |
  | 10 | 54.72 m |
  | 13 | 8.18 m |

- Level-13 example: −8.825°, 147.50833333333333°; direct atlas surface 3,382.87 m,
  reconstructed DEM surface 3,374.69202033 m. This is a sampled discrepancy,
  not a proven global maximum or source accuracy estimate
- Minimal remedy: remove "tiny", document the render/physics mismatch and
  global-only contact limitation, and add a deterministic regression. Aligning
  mesh breakpoints with source-cell boundaries would be a larger geometry change
- Status: resolved for bilinear DEM sampling by canonical source-grid alignment,
  independently checked above. Rendered triangular-mesh/chord error remains a
  separate documented limitation requiring application-level checks

## Independent checks completed

- Read `CLAUDE.md`, `ARCHITECTURE.md`, reviewer instructions and ADRs 0002,
  0003, 0005 and 0006
- Read-only review found bounded FSGT dimensions, protected header/checksum,
  reserved-bit and exact-length checks, bounded physical values, independent
  sign-safe land classification, correct `h = H + N` sign, periodic longitude,
  continuous longitude-independent poles, and separately budgeted fallback APIs
- All **2,332,800** original terrain nodes match independently computed
  halfway-away-from-zero quantization of the prepared H and N arrays, and all
  land bits match the prepared mask
- All three monthly climate fields match independently quantized NOAA source
  values at every source grid point and both added polar rows. The 94 actual
  Gaussian latitude coordinates are preserved. Precipitation uses the correct
  water-depth conversion from kg/m²/s to m/s
- Alternate climate bake reproduces SHA-256
  `b1841fe206dd0b1154868335f7ff09ea9a0e5f17c87baa70029240871fd3bf1a`
- Reran all 8 `scripts/tests/test_bake_climate.py` tests successfully after the
  fixes: alternate metadata output, distinct output family, input collision,
  symlink alias, atomic replacement and geoid registration. Reran full alternate
  bake and exact binary comparison after those changes
- Initial terrain atlas SHA-256 is
  `50a7cb9dd237af7ae2028519a9d4be4fbe8563abfdb61457317786a209207da8`;
  corrections to water surfaces must change its documented identity
- Live primary-source rights check: [NOAA ETOPO metadata](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ngdc.mgg.dem:etopo_2022)
  expressly states CC0-1.0 worldwide and warns against navigation use;
  [Natural Earth terms](https://www.naturalearthdata.com/about/terms-of-use/)
  state public domain; [NOAA PSL guidance](https://psl.noaa.gov/data/help/)
  permits federal public-domain data reuse with no false endorsement or official
  modified-product representation. Geoid source metadata identifies NGA-derived
  public-domain data. The reviewed attribution descriptions are consistent

## Verification limits

`cargo` and `rustc` were absent at review time. No Rust tests, compilation,
clippy, app rendering, FPS measurement or collision flight were performed by
this review. Python used pre-existing task-local packages; no software was
installed. Numerical checks validate data processing, not local aviation,
meteorological, coastline or lake-surface accuracy. The findings above require
final-state recheck after concurrent owner edits.
