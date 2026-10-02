# Regional surface data QA — 2026-10-02

## Scope

This record covers the pure-Rust FSSC database, bounded offline PBF importer,
source/provenance checks and an optional Liechtenstein OSM + GLO-90 runtime
sample. It does not establish application rendering, graphics-driver behavior,
flight-control quality or commercial release approval. Runtime application QA is
owned by the integration task.

Source changes are deliberately separate:

- `e2a0238`: immutable scenery API and FSSC v1 codec
- `695be78`: OSM way importer and reason-specific omission counts
- `fea0dfc`: simple-polygon/triangulation coverage checks, aggregate work bound,
  exact local-geometry query ranking and adversarial regressions
- `287cebf`: aggregate decompressed-byte and relation-reference budgets

## Checks passed

Commands run with the supplied toolchain, separate target directory,
`CARGO_INCREMENTAL=0`, development/test debug symbols disabled, and `-j2`:

```sh
cargo test -j2 -p flightsim-core -p flightsim-world -p flightsim-tilegen
cargo test -j2 -p osmpbf --lib blob::tests
cargo test --release -j2 -p flightsim-tilegen \
  --test pbf_hostile_probe --test scenery_pipeline
cargo clippy -j2 -p flightsim-world -p flightsim-tilegen --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -j2 \
  -p flightsim-world -p flightsim-tilegen --no-deps
bash scripts/check-architecture.sh
```

- Core/world/tilegen: 498 tests passed, including doctests
- Vendor raw/zlib size metadata: 2 tests passed
- Release hostile PBF: 17 tests passed; original valid airport bytes remain stable
- Release scenery importer: 8 tests passed
- All-target clippy, strict rustdoc and architecture checks passed
- Scenery runtime tests include byte-exact roundtrip, corrupted counts/flags/classes,
  checksum-correct invalid values, duplicate IDs, finite datum coordinates,
  regional bounds, seam/poles, duplicate triangles, self-crossing rings, invalid
  concave coverage, consistent reverse winding, and aggregate work rejection
- Query tests include a U-shaped road whose bounding sphere encloses the observer,
  and 4,100 misleading road bounds ahead of a nearby building at the 4,096 result cap

The initial codec only checked triangle indices/counts/nondegeneracy. Review
identified that this could admit duplicate or outside triangles. The final
implementation validates ring simplicity, triangle orientation/uniqueness,
coverage area, manifold edge pairing, boundary coverage and interior diagonals.
The 50-million-unit aggregate preflight prevents the 512-point individual limit
from multiplying into uncontrolled quadratic startup work.

The input's 64 MiB compressed bound likewise did not alone bound decompression
work. The final importer charges a validated 256 MiB cumulative uncompressed
budget per pass before decoding each known blob. The vendored decoder still
checks exact output length, compressed input consumption and zlib StreamEnd.
This is bounded data/algorithm work, not an OS memory sandbox or timing guarantee.

## Real fixture

Public [Geofabrik Liechtenstein](https://download.geofabrik.de/europe/liechtenstein.html)
PBF, snapshot **2026-09-30T20:22:42Z**, retrieved during the 2026-10-02 session:

- Source bytes: 3,458,908
- Source SHA-256: `c6f495bd86bdc52ba48557733f765508be0098eb95ad32dbb60cfb683b8b4ae7`
- 54 PBF blocks; 7,958,633 uncompressed bytes per pass
- Source scan: 404,761 elements and 37,190 selected ways
- Output: 13,932 road ways, 17,906 building footprints, 4,067 landcover polygons
- Output: 8,381,178 bytes; SHA-256 `ef6c802053df442a1db8a871e4591476bb5f845bb4ec394a1bf6df9cf81a867e`
- All **336,453 retained coordinates** independently decoded from the FSSC and
  compared with independently parsed original PBF nodes/way references: exact
  radian agreement in this fixture (maximum observed difference 0)
- Aggregate polygon-validation charge: 28,280,713 of 50,000,000 permitted units
- Source extent approximately 47.03494–47.29603 N, 9.47089–9.65620 E

Heights are explicitly distinguished: 15 direct OSM height tags, 861 levels-based
estimates, and 17,030 default 9 m extrusions. Widths are inferred for 13,678 roads.
Those source gaps are visible modelling limitations, not evidence of real measured
heights/widths. The importer discarded 475 non-ground road ways, 13 area roads,
7 oversized ways, 780 multipolygon members and 10 invalid geometries; 238
multipolygon relations were identified. No missing nodes or invalid coordinates
were found in selected retained candidates. Counts reconcile to all selected ways.

## Colocated terrain fixture

Two inventory-verified public Copernicus GLO-90 COGs, N47E009 and N46E009,
supplied 3 arcsecond native posts (1,200×1,200 each). Source XML identifies
WGS84-G1150 / EPSG:4326 horizontal
coordinates and EGM2008 / EPSG:3855 orthometric metres. The COG's missing vertical
GeoKey was not treated as ellipsoidal.

The previously retained NOAA ETOPO2022 EGM2008 geoid was hash-verified and reused.
It is point-decimated to 10 arcminutes; bilinear matching-model N was added at
every native source post as **h=H+N**, before float32 storage or resampling.
The derived GeoTIFF declares EPSG:4979 and PixelIsPoint; production tilegen baked
it without `--assume-ellipsoidal` and required `--min-coverage 1`.

Actual-scene QA identified that the original 47.11–47.30 N sample let the Vaduz
view's 4.5 km scenery radius reach its southern regional/global transition.
The correction extends the requested corridor to 47.03–47.31 N. Its L10 ancestors
reach south of 47 N, so the official adjacent N46E009 COG is required, rather than
padding missing samples. The provider omits the shared south/east rows: N47 ends
at 47 + 1/1200 degrees N and N46 starts at 47 degrees N. The native mosaic preserves
that spacing with no duplicate row, clamping, NoData fill or sea-level substitute.
The OSM-derived FSSC bytes are unchanged.

- Additional N46E009 COG: 5,225,729 bytes; SHA-256
  `6860c0b93a271f3b702a4a16aa885ba19c4907e808c56f94a620c6a657aa5f18`
- Additional source XML: 44,690 bytes; SHA-256
  `090dea9689d3986d5d6ea526ad598b0246686a5889786bd4cf729e49683234be`
- Source XML again confirms EGM2008 / EPSG:3855, with the same GLO-90 licence

- Focus bounds after southern-edge scene QA: 9.40–9.68 E, 47.03–47.31 N
- 765 tiles: 9 level 10, 36 level 11, 144 level 12, 576 level 13; 65×65 samples
- Complete primary hierarchy extent at every included level: 9.31640625–9.84375 E, 46.93359375–47.4609375 N
- All 409,600 retained native posts independently match the original two COGs;
  all matching geoid corrections were independently re-evaluated from source DODS
  coordinates, including the 47 N mosaic join
- Exact tile-ID/path equality, every L10–L13 ancestor chain, and all 189 complete
  four-child families were checked
- All 281 previous runtime DEM tiles remain byte-identical; 484 tiles were added
- Independent comparisons covered all 3,232,125 tile samples against native-post
  sampling, with a maximum 0.017579 m difference, within per-tile half-quantization
  scale plus 0.001 m floating-point allowance
- All 1,440 same-level neighbor edge pairs passed the combined quantization tolerance
- Schaan vicinity 47.165 N, 9.51 E: level-13 ground 506.803963 m ellipsoidal
- Vaduz vicinity 47.141 N, 9.522 E: level-13 ground 509.497175 m ellipsoidal
- Actual QA view 47.127 N, 9.529 E: level-13 ground 643.375724 m ellipsoidal
- Balzers 47.068 N, 9.501 E: level-13 ground 522.675037 m ellipsoidal
- Southern scenery 47.035 N, 9.55 E: level-13 ground 1,149.418387 m ellipsoidal
- After this data-only extension, all 175 tilegen unit/integration tests passed

A second actual-scene probe showed that merely including the focus corridor and
its ancestors was insufficient: at 47.0959673741 N, 9.5665614863 E, the renderer
preserved real parent `10/1078/244` because siblings `11/2156/489` and
`11/2157/489` were absent, despite the requested fine tile being present. The
renderer correctly requires complete primary child families before replacing a
real parent. The final pack therefore contains every descendant through L13 of
L10 roots x=1077–1079, y=242–244. Its source envelope is unchanged; no new source
or fill is needed. Production tilegen uses inward bounds
`9.316406251,46.933593751,9.843749999,47.460937499` to avoid including border
parents. Independent checks enumerate integer descendants, require exactly nine
roots and all 189 four-child families, and compare all 281 previous tile bytes.
The previously pinned point has validated L13 height 1,952.073940 m ellipsoidal;
actual rendered-mesh accuracy remains a separate scene acceptance check.

These numerical residuals describe conversion/resampling checks, **not** source
survey accuracy. GLO-90 is a reflective DSM, not bare-earth DTM, and the coarse
geoid adds unquantified finer-scale vertical uncertainty. The corrected regional
DEM includes the southern OSM
scenery near Balzers and all L10–L13 descendants of the nine focus-area roots. Default
global terrain remains relevant beyond the supplied tiles.

## Measured runtime data costs

```sh
FLIGHTSIM_SCENERY_BENCH_FILE=/path/to/liechtenstein.fsscenery \
  cargo bench -j2 -p flightsim-world --bench scenery -- --quick
```

Criterion quick-run release measurements in the shared Linux execution container,
on the exact 35,905-feature, 8,381,178-byte fixture:

- Decode and complete validation: estimate 175.42 ms; interval 173.78–181.98 ms
- Exact 4 km local query, cap 4,096: estimate 5.0314 ms; interval 5.0000–5.1571 ms

These are data-layer measurements, not native-GPU FPS or frame-time guarantees.
Loading is a synchronous startup operation; queries should not run every frame.
Renderer selection, build and residency budgets are separately required.

## Rights/package boundaries

Current primary OSM/OSMF/ODbL, Geofabrik and Copernicus terms were reviewed.
ODbL permits commercial use with notice, derivative database share-alike and
machine-readable access obligations. The optional local sample contains the full
OSM-derived database, ODbL text, Copernicus licence bundle, modification/no-liability
notices, SHA-256 inventory and source/transformation metadata. No raw PBF, raw
GeoTIFF, executable or credentials are in its 772-file ZIP. ZIP CRC/path checks and
every listed SHA-256 were verified.

The sample remains outside Git and outside the commercial-staging allowlist.
The optional data ZIP was saved to the user’s Library; this is not a public
release. No GitHub writes, account setup or release publication was performed by
the data task. Keep licence obligations and application visual QA as separate
release gates; successful source conversion is not Steam clearance.
