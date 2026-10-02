# Global terrain data and boundary QA — 2026-10-02

## Scope and verification status

This record covers the compact terrain asset, offline preparation/packing, and
pure data boundaries. The authorized Rust toolchain is now available. The
headless test run recorded in
the local `headless-initial-0318` test log (not distributed) passed all
885 tests, including the world terrain, offline encoder and global-generator CLI
regressions below. This establishes compilation and execution of those Rust
paths; native rendering and flight presentation require separate app verification.

The reported tilegen `nonminimal_bool` clippy finding was fixed and the file
formatted at 03:26 UTC; workspace lint status belongs to the lead's verification
record. At 03:32 UTC, independent Python and Rust canonical resampling from the
rebuilt corrected inputs produced identical bytes, also identical to the bundled
atlas. Native app verification is outside this record. An earlier architecture
check could not run because cargo was then absent; its later result belongs to
the lead's workspace verification record.

The separate [independent review](global-data-independent-review-2026-10-02.md)
reconstructed every final data node and audited sources, geometry and provenance.

## Final reviewed artifact

- Format: FSGT v2, 2048×1024 canonical geographic samples
- Bytes: 8,912,960
- Dataset: `ETOPO2022-COP90-NE-2048x1024-surface-v2`
- FNV-1a content fingerprint: `500e8db32bde7019`
- SHA-256: `57bc03c12caa772ecbd52f1d24cd12704ccc4ace5123e9d8c26b6e454907f13e`
- Dry-land nodes: 688,434
- Inland-water nodes: 6,770
- Negative dry-land nodes: 1,462
- Overlapping land/water bits: zero
- Minimum dry-land H: −409m; minimum inland-water H: −427m

The prepared source corrections retain 1,617 negative dry-land nodes and identify
7,269 inland-water nodes on the original 2160×1080 source lattice. The canonical
lattice differs, so these counts are deliberately not claimed identical.

Source correction ledger SHA-256:
`0c6a217df2f0987cbdbbfe0e8c0347eea77ea5fc259516a0f7246d955c294cab`.
All cached Copernicus range/pixel selections and lake estimators were replayed
independently; corrected source outputs and original geoid hashes matched exactly.
Of 942 represented lakes, 160 have explicitly low-confidence estimates affecting
363 source nodes. They use recorded in-polygon statistics where no clear flat
surface was identified. This is a coarse static model, not measured current water.

## Executed checks

- Offline pinned-source preparation and repeat: passed
- Python packer repeat and exact byte comparison: passed
- Rebuilt source inputs using five hash-pinned downloads and the committed
  correction ledger: every raw-array and `.npy` hash matches original preparation
- Independent Python packer versus Rust `flightsim-globalgen --canonical` on
  those corrected source inputs: every byte matches the bundled atlas
- Separate canonical asset decode/re-encode through the existing Rust CLI:
  every byte matches (this narrower roundtrip does not itself test resampling)
- Five executable tests in `scripts/test_pack_global_terrain.py`: passed
  - Lake height retained, negative dry land retained, ocean seabed excluded
  - FSGT v2 layout, separate masks, checksum and generated Rust metadata
  - Failed final provenance write leaves INCOMPLETE, never stale COMPLETE
  - Input overwrite protection
  - Wrong source longitude/latitude registration or orientation rejects before output
- `git diff --check` on terrain-owned changes: passed
- Rust headless suite: 885 passed, zero failed
  - `flightsim-world` library: 204 passed, including all global terrain regressions
    and bounded diagnostics during long-distance travel
  - `flightsim-tilegen` library: 91 passed, including global atlas encoding,
    canonical resampling, lake masks and deterministic feature pooling
  - `flightsim-globalgen` CLI: three passed, covering external provenance,
    protected source files and failed-bake INCOMPLETE status
- Independent reconstruction of all 2,097,152 H/N samples and both masks: exact
- Independent 2,653,849-point scan (all nodes, random points, poles and seam):
  maximum direct-atlas/bilinear-DEM difference **0.000234375m** at levels6,8,13

The last result tests bilinear height resampling. It does not test triangle saddle
interpolation, curved-Earth chord error, runtime LOD transitions or landing contact.
Those remain app/render verification responsibilities.

The 03:32 source rebuild did not repeat the large Copernicus overview acquisition.
It replayed the committed, previously audited correction ledger. The earlier
source-range audit remains the evidence for those Copernicus samples. This run
used isolated build-time pyshp 3.1.6 and Shapely 2.1.2 with NumPy 2.3.5; no system
Python changes were made. Source and prepared-array caches were retained locally
after an environment replacement. Those build-time caches are not part of the source distribution.

Machine-readable commands, output hashes and comparison results were retained
in local source-rebuild and canonical Rust roundtrip records. They are not
distributed as raw QA telemetry; portable source provenance and the correction
ledger are committed under `docs/data/`.

## Named-place numerical checks and remaining bias

H is EGM2008 orthometric metres; runtime ellipsoidal surface is h=H+N.

| Probe | Result | Interpretation |
|---|---|---|
| Himalaya 28N,86.8E | H5397.008m | Broad high-relief region survives |
| Andes22S,67W | H4621.065m | Broad mountain terrain survives |
| Greenland72N,42W | H2969.490m | Ice-surface height, not subglacial bed |
| Central Pacific0N,140W | H0m, N+0.79472m | Ocean surface h=N |
| Baikal53.3416667N,108.175E | H455m, inland water | Former bathymetry removed |
| Caspian42N,51E | H−28m, inland water | Negative water level retained |
| Superior47.7N,87.5W | H181m, inland water | Quantized static water model |
| Former Vanuatu anomaly15.4916667S,168.175E | H+77.445m (blended coast) | No longer exposed−1394m seabed |
| Death Valley pooled node36.123046875N,116.806640625W | H−84m, dry land | Source depression preserved on coarse lattice |
| Dead Sea pooled node31.728515625N,35.595703125E | H−427m, inland water | Narrow water feature retained |

Pooling is an explicit tradeoff. It changes 1468 negative-ground nodes and264
inland-water nodes relative to ordinary canonical resampling; the largest node
height change is837.582m. Corrected inland-water source points are retained first,
with nearest-point/source-row-major tie breaking; otherwise genuine negative
dry-land minima are retained. Masks stay disjoint.

The proven coordinate bound is only the source point's assignment to a canonical
node: at most0.087890625° per axis. Interpolated shorelines/height footprints can
extend farther. The exact Badwater-area query36.25N,116.83W still interpolates to
H+169.030m between coarse nodes; the nearby retained negative representative must
not be described as a surveyed prediction at that exact coordinate. The query
31.5N,35.5E near the Dead Sea yields H−223.531m and an inland-water fraction0.319,
so majority classification there remains dry land. These are honest limitations
of approximately19.5km global data, not evidence of precise local terrain.

Regional high-resolution `.fsdem` tiles remain the preferred accuracy path.

## Rust regressions executed successfully

- Datum sign, ocean versus seabed, below-sea-level dry land and nonzero lakes
- Disjoint water/land masks, physical-height guard, malformed headers/lengths,
  reserved bits, padding, checksum and finite coordinates
- Shared tile edges, antimeridian, poles and aligned source breakpoints
- Real primary ancestors take priority over generated fine fallback
- Fixed33×33 generation and max level13; no nested primary IO
- Failure diagnostics capped at4096 with a saturating dropped counter
- Offline encoder raw arrays, deterministic resampling/pooling, atomic output,
  protected sources and COMPLETE/INCOMPLETE provenance

These regressions passed in the headless log cited above. They do not by
themselves establish native visual quality, frame timing or release readiness.

## Dateline mesh-sampling correction, 03:53 UTC

Final integration review found a boundary bug outside the atlas bytes:
`GeoBounds::normalise` globally wrapped the eastern tile's inclusive +180°
sampling endpoint to −180°. `DemTile::elevation_at` then clamped that point to
the tile's west column. Raw grid-edge comparisons had not exercised this path,
so they did not establish correct edge elevations in `build_mesh`.

Normalization now retains longitudes inside the tile's inclusive interval and
otherwise selects the equivalent longitude nearest its centre. Ordinary nearby
out-of-bounds coordinates still clamp to their correct west/east side. Full-width
bounds preserve both explicitly supplied endpoints. Tile containment keeps its
existing half-open convention.

The added tests use distinct west/east DEM heights and check wrapped longitude
equivalents, ordinary clamping, actual mesh heights, recovered ECEF vertex heights
and the shared dateline edge. All world tests pass after this correction: 206
unit tests, 10 integration tests and one doctest, with zero failures. Targeted
world clippy with all targets and warnings denied also passes. Logs:

- Local `world-dateline-0352` test log (not distributed)
- Local `world-dateline-clippy-0353` lint log (not distributed)

The independent rebuilt-library probe compares the actual bundled mesh elevation
arrays at +180° and −180° across every latitude tile and all 33 edge vertices
at levels 0, 3, 6, 8 and 13. Maximum disagreement is **0.0m** at every level.
The local independent dateline probe record supplies this result. This checks
mesh elevation sampling; rendered triangle curvature, stored `f32` ECEF precision
and visual LOD transitions remain separate checks. The atlas bytes and fingerprint
are unchanged.
