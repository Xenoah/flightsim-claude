# World / offline data boundary audit (2026-10-01)

Base: `main` commit `2d2295b` (`0.6.0-alpha.18`).
Scope: `flightsim-tilegen`, compatibility with `flightsim-world` formats.

## Prioritized findings

1. **Issue #23, malformed OSM PBF**: `osmpbf 0.3.7` performed unchecked dense
   ID/coordinate/metadata delta accumulation, coordinate/time scaling, way and
   relation deltas, and unwrapped unknown relation member enums. Invalid string
   table entries could silently terminate tag iteration. Existing hostile tests
   covered the outer framing, not these inner protobuf cases. Latest upstream
   0.3.8 was inspected and still had the unchecked arithmetic, so a version-only
   update was insufficient.
2. **Issue #22, height datum**: a CLI-only identity/rejection gate existed, but no
   geoid conversion. The public library generation entry point could bypass the
   CLI gate and write unacknowledged orthometric heights.
3. **GeoTIFF identity and units**: absent/malformed GeoKeys, non-WGS84 geographic
   CRSs, non-degree angular units and non-metre vertical units could be treated
   as WGS84 degrees/metres. Derived pixel centres could overflow or leave the
   valid latitude range.
4. **Sampling and planning bounds**: a float-to-i64 saturation followed by
   `column + 1` could overflow for extreme pixel sizes; a huge footprint iterated
   enormous out-of-raster ranges; invalid levels reached shifts/panics; huge
   output grids reached allocation before the writer's limit; planning allocated
   all tile IDs even for an infeasible dry run.

## Parser decision and regression evidence

A local safety patch to the pinned `osmpbf 0.3.7` lives under
`vendor/osmpbf`. Upstream licenses and source attribution
are retained. Details, upstream archive hash and patch boundaries are in
`FLIGHTSIM-PATCHES.md` there. The workspace excludes the vendored crate from its
own tests/lints; it remains an ordinary offline dependency.

The immutable raw `PrimitiveBlock` is validated once before its infallible
iterators are exposed. Validation checks all arithmetic, parallel arrays,
UTF-8/string indices and enum members, including metadata not used by airportgen.
Both discovery and indexed dependency extraction use the same validated decoder.
Blob framing, size, partial EOF and zlib expanded-size checks were tightened.
No panic-catching or process-wide panic hook is used.

`tests/pbf_hostile_probe.rs` independently encodes protobuf messages for hostile
inner fields, verifies ordinary errors, and verifies that input bytes and an
existing output DB survive failure. Its valid synthetic airport includes valid
runway/taxiway plus area, closed, missing-node and invalid-coordinate skip cases.
The golden FSAP was captured from the unmodified baseline executable, before the
fork, using those same synthetic PBF bytes:

- Size: 400 bytes
- SHA-256: `199260d950dcab5d0cd24e845806e4c9dee6e65f4c222ead02d1b35d231ee135`

This preserves byte output and reason-specific reports for the regression fixture.
It does not claim that all real-world OSM inputs were tested or that resource
exhaustion is recoverable. Individual PBF blobs and selected application collections are bounded; the whole-file index, node dependency set and aggregate apron data are not a total-memory sandbox. Allocator/process limits still apply.

## Vertical normalization decision

Use a **local, user-supplied GeographicLib 16-bit PGM geoid grid**, identified
explicitly with `--geoid-grid <PGM> --geoid-model egm2008|egm96`. Do not bundle a
model, auto-download data, infer a model from a filename, or substitute EGM96 for
EGM2008. The file's Description must identify the selected WGS84 model.

The source DEM datum must match the model. Ellipsoidal inputs are unchanged;
unknown, unsupported and unspecified-MSL sources fail. The existing explicit
`--assume-ellipsoidal` override still preserves values without correction and is
mutually exclusive with geoid conversion. Public `generate_tiles` now checks the
same contract; library callers must normalize or explicitly acknowledge an
assumption. Synthetic numerical fixtures declare their analytic heights as
ellipsoidal rather than relying on an absent datum.

For each original valid DEM pixel centre, apply **h = H + N** in metres, then
resample the corrected raster. Nodata remains missing and cannot collide with a
corrected value equal to the old nodata sentinel. The geoid interpolation is
bilinear with periodic longitude and longitude-independent pole rows. Applying
N after coarse resampling was rejected because it would apply a different
correction to the represented source footprint.

`.fsdem` remains byte-compatible and continues to mean WGS84 ellipsoidal metres;
there is no geoid dependency at runtime. `terrain-provenance.txt` records the
latest CLI invocation's source paths, input datums, content fingerprints,
model/description/dimensions, interpolation, options and explicit override. It is
atomically marked INCOMPLETE before writing tiles, changed to COMPLETE after successful generation, and never written by a dry run. It explicitly does **not** attest pre-existing tiles outside that invocation.
FNV-1a fingerprints detect accidental changes and are not authentication hashes.

### Accepted costs / rejected alternatives

- Users obtain and retain their own official grid/provenance and applicable data
  license. File Description is a consistency check, not proof of provenance.
- Bilinear interpolation has model/resolution-dependent approximation error. It
  is not spherical-harmonic evaluation or GeographicLib's default cubic method.
- A global PGM grid is held in offline memory, capped at 512 MiB; runtime is unaffected.
- A native GDAL/PROJ dependency was not added, preserving the pure-Rust Windows
  deployment path. Inputs with unsupported CRS/units must be converted externally.
- Catching osmpbf panics was rejected because optimized overflow can wrap without
  a panic and because invalid indices could silently discard tags.

## Independent geoid verification

Primary specifications and data:

- [GeographicLib geoid grid format and test-set description](https://geographiclib.sourceforge.io/C++/doc/geoid.html)
- [GeoidEval sign convention and bilinear/cubic error estimates](https://geographiclib.sourceforge.io/C++/doc/GeoidEval.1.html)
- [Official egm2008-5 grid archive](https://sourceforge.net/projects/geographiclib/files/geoids-distrib/egm2008-5.tar.bz2)
- [Official GeoidHeights reference set](https://sourceforge.net/projects/geographiclib/files/testdata/GeoidHeights.dat.gz)
- [OGC GeoTIFF 1.1 CRS and VerticalGeoKey specification](https://docs.ogc.org/is/19-008r4/19-008r4.html)

The reference set has 500,000 points with EGM84/96/2008 heights produced by NGA
spherical-harmonic synthesis programs, independently of this grid reader. It was
downloaded to scratch, not added to the repository or release. Source hashes:

| File | SHA-256 |
|---|---|
| egm2008-5.tar.bz2 | `9a57c14330ac609132d324906822a9da9de265ad9b9087779793eb7080852970` |
| egm2008-5.pgm | `96d55e88db186ddae892b00c5f8cc42a37cdac8ddb69b6de00af8c75dffb34a3` |
| GeoidHeights.dat.gz | `e0b9804694b3eac78ffd0b3df9cffd30a134d72007276e732ff72f9651bc9995` |

Initial independent check of the new reader on the 4320 x 2161 EGM2008 5-minute
grid, all 500,000 points:

- Maximum absolute error: **0.274256968 m**
- RMS error: **0.011791141 m**
- Worst observed point: latitude 3.261894, longitude 283.949972; independent
  reference 32.866017 m, bilinear result 32.59176003174451 m

Reproduce after decompressing the reference set:

```sh
cargo run -p flightsim-tilegen --example check_geoid -- \
  /path/to/egm2008-5.pgm egm2008 /path/to/GeoidHeights.dat 0.5
```

This verifies the geoid reader/interpolator numerically. It is **not** physical,
real-DEM, visual, GPU, or flight-validation evidence. Real Copernicus DEM baking
and night/high-altitude rendering are tracked separately under #6.

## Test status

Verified locally with Rust 1.93.0 (Linux), isolated data target directory:

- `cargo test -p flightsim-tilegen --all-targets`: **147 passed**
- `cargo test -p flightsim-world --all-targets`: **184 passed**, and all benchmark smoke cases succeeded (these are not performance measurements)
- `cargo clippy -p flightsim-tilegen -p flightsim-world --all-targets -- -D warnings`: passed
- `cargo test -p flightsim-tilegen -p flightsim-world --doc`: passed (one world doctest)
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flightsim-tilegen -p flightsim-world --no-deps --document-private-items`: passed
- Changed-file rustfmt and `git diff --check`: passed
- Normal dependency tree: no Bevy and no native zlib backend
- `cargo test --release -p flightsim-tilegen --test pbf_hostile_probe`: **17 passed**, including golden-byte output equality and ordinary errors for overflow/indices

No Windows, physical-controller, GPU or real-flight pass is inferred from these results.

## Real Copernicus follow-up for #6 (data preparation, not visual approval)

The [AWS Copernicus DEM registry](https://registry.opendata.aws/copernicus-dem/)
links to the [provider's COG documentation](https://copernicus-dem-30m.s3.amazonaws.com/readme.html).
The [Copernicus product page](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM)
identifies GLO-30 DGED as WGS84 geographic / EGM2008 (EPSG:3855), metres,
PixelIsPoint. It also distinguishes a DSM (buildings/vegetation included) from a
bare-earth DTM and specifies attribution obligations for modified products.

A public N35 E139 COG was retrieved directly from the registry's documented
bucket, retained only in scratch:

```sh
curl -fL 'https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N35_00_E139_00_DEM/Copernicus_DSM_COG_10_N35_00_E139_00_DEM.tif' \
  -o copernicus-N35-E139.tif
```

Input SHA-256:
`a4a8dd95b4d3a9f06d7bd09d5125e3196efd7e142fc5b4c6a85431a46f1905fc`.
The downloaded file is about 36 MiB. Its actual GeoKeys identify EPSG:4326,
degrees and PixelIsPoint but **omit VerticalGeoKey**. Therefore conversion is
rejected without an explicit declaration. The new
`--source-vertical-datum egm2008` option fills absent metadata only; it never
replaces conflicting metadata, and the declaration is included in provenance.
This flag is justified here by the provider's independent product documentation.

Reproducible bake:

```sh
cargo run -p flightsim-tilegen -- \
  --input copernicus-N35-E139.tif --output copernicus-haneda-tiles \
  --bounds 139.15,35.15,139.95,35.85 --min-level 8 --max-level 12 \
  --min-coverage 0.9 --source-vertical-datum egm2008 \
  --geoid-grid egm2008-5.pgm --geoid-model egm2008
```

Observed result: 461 candidate tiles, 429 written (3.5 MiB), 32 skipped for low
coverage, no filled grid points reported. Repeating the bake produced identical
SHA-256 hashes for **all 429** tile files. The sorted `sha256  relative/path`
manifest's SHA-256 is
`7cfa7d0e8f157bc13dc5ab470b0faca0542b456305e70207d7626bb113d53a48`.

Readback checks (metres):

| Latitude, longitude | Raw H | Geoid N | Normalized source h | L12 runtime tile h |
|---|---:|---:|---:|---:|
| 35.55, 139.78 | 4.000000 | 35.618208 | 39.618210 | 39.407389 |
| 35.60, 139.70 | 37.822472 | 36.177840 | 74.000313 | 73.866301 |
| 35.70, 139.40 | 94.370117 | 38.579760 | 132.949875 | 131.580926 |

These checks verify the conversion sign and ingestion of the actual compressed
COG. Runtime values additionally include spatial resampling and quantization;
they are not supposed to equal a point sample of the original DSM exactly.
The bake is suitable for the lead's night/high-altitude screenshot checks at
`--start 35.55,139.78`; those images require separate inspection.

No raw or derived real DEM is included in this commit/release. If distributing a
modified Copernicus GLO-30 product, retain the exact modified-product attribution
notice from the linked Copernicus licensing/citation section and the dataset's
applicable license. Do not infer permission for an unrelated dataset.

### Additional regression findings

The normalization tests uncovered an old **synthetic GeoTIFF builder** defect:
GDAL_NODATA ASCII strings of four bytes or fewer must be stored inline in the TIFF
IFD, but the builder always wrote an offset. Corrected the helper and retained a
regression where a valid corrected elevation equals the previous nodata sentinel.
This was a test-fixture encoding bug; the production TIFF decoder was not blamed.

Independent EGM96 5-minute validation also passed the same 500,000 NGA-harmonic
reference points: maximum error **0.115708552 m**, RMS **0.004570351 m**.
Worst observed EGM96 point: latitude 28.371018, longitude 343.381243;
reference 50.255009 m, bilinear result 50.13930044784942 m.

Additional source hashes:

| File | SHA-256 |
|---|---|
| egm96-5.tar.bz2 | `c46224f8f723dc915d97179f4e1580a98d6c742fe2b82cd8fef0ecaaad13e614` |
| egm96-5.pgm | `c4b25a03ec5845cec4778a54b580aeda676363f2205a89137e8677b2337af3ec` |
| GeoidHeights.dat (decompressed) | `0110b5c52df1c4a9614913c82b8ddffc42e9eb7bbeaf8f74a4eb97adde9ff943` |

```sh
cargo run -p flightsim-tilegen --example check_geoid -- \
  /path/to/egm96-5.pgm egm96 /path/to/GeoidHeights.dat 0.15
```

## Follow-on independent review and output safety

The independent review added zlib end-of-stream and complete-input validation,
including all four missing Adler32 trailer lengths, trailing junk and concatenated
streams. Unsupported/nonfinite GeoTIFF Z scales and tie heights now fail explicitly;
conventional flat-height tags (`ScaleZ` 0 or 1, zero raster/model tie Z) remain accepted.

Every tile is encoded first, written and synced to a same-directory temporary file,
then atomically replaced. Before the first tile mutation, the CLI atomically writes
an INCOMPLETE provenance marker. Only a successful full invocation changes it to
COMPLETE. A failed partial batch stays explicitly INCOMPLETE. Existing tiles outside
the invocation are still not attested. This is per-file atomicity, not a whole-batch
transaction or a power-loss durability guarantee; concurrent writers are unsupported.

Automatic raster coverage now normalizes 0–360 longitude extents and correctly joins
arcs across the dateline, including full-world/polar extents. The CLI regression bakes
both edge columns without `--bounds`. A separate exhaustive probe sampled 6,561 arc
unions without losing covered longitudes.

Final tilegen gates after these additions: 155 tests across all targets; clippy,
rustdoc with warnings denied, doctest gate, formatting and diff checks passed. This
includes valid-output byte compatibility and 17 hostile PBF regression cases; the
hostile cases also passed in optimized/release mode before the output-only additions.
