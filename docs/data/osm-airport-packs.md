# Offline OSM airport data: provenance and future packaging

Status: optional local preparation only. No real OSM database is bundled, and
this guide does not approve one for a release. Checked against the official
sources linked below on 2026-10-02. This is an engineering checklist, not legal
advice or a finding that a particular Steam package is cleared.

## What is implemented

`flightsim-airportgen` converts a user-provided regional `.osm.pbf` into an
FSAP v3 `.fsairports` database. It extracts runway/taxiway centre lines, aprons,
holding positions, reference-derived signs, and ground lights. Runtime supports
FSAP v1/v2/v3, selects one nearest runway, and draws nearby ground equipment
within 15 km. It reads its elevations from the active terrain source. There is
no global road, river, city-building or land-use importer in this pipeline.

OSM supplies editable geographic features; it does not replace a consistent DEM
or the monthly climate source. The independent terrain and climate databases
remain separate. See [ADR-0008](../adr/0008-osm-airport-data.md) for the exact
format, safety limits, existing no-bundling decision and regional rendering
limits.

## Record the source of a local conversion

First generate and validate the DB using the existing converter and application.
Keep its actual converter revision and source snapshot date. Do not substitute
today's date, the current checkout's revision, or a guessed provider URL.

```sh
cargo run -p flightsim-tilegen --bin flightsim-airportgen -- \
  --input data/region.osm.pbf --output data/region.fsairports

python scripts/record-osm-airport-provenance.py record \
  --input-pbf data/region.osm.pbf \
  --database data/region.fsairports \
  --snapshot-date YYYY-MM-DD \
  --converter-id flightsim-airportgen@ACTUAL_CONVERTER_COMMIT \
  --source-url https://PROVIDER/ACTUAL-SNAPSHOT-OR-SOURCE-PAGE
```

The date, revision and URL above are placeholders. Use known values; if the
source URL is unknown, omit `--source-url`. The manifest records `null` instead
of inventing an origin. If the snapshot date or converter identity is unknown,
recover that evidence before recording a companion.

The standard-library-only helper writes one human-readable
`data/region.fsairports.provenance.json`. It includes SHA-256 and byte count for
both local files, FSAP version, user-supplied snapshot/converter identity,
optional source URL, OSM attribution, ODbL link, and an explicit
`distribution.status = not_reviewed`. It makes no network requests, runs no
converter, and does not copy, modify, package or upload the input files.

Publication is one-file, atomic and no-clobber: a complete same-directory
temporary file is flushed and linked into its new name. An existing file,
symlink or concurrent publisher wins; the helper refuses to overwrite it. If
hard links are unavailable on that filesystem, publication fails safely rather
than switching to an overwrite operation. A process crash may leave an unlinked
temporary-name file; the final companion is either absent or complete. Power-loss
durability of the directory entry is not guaranteed. To record a new snapshot,
use new DB/companion names, or explicitly choose a new `--output` path.

## Check integrity later

```sh
python scripts/record-osm-airport-provenance.py verify \
  --manifest data/region.fsairports.provenance.json \
  --database data/region.fsairports \
  --input-pbf data/region.osm.pbf
```

Verification is read-only. File relocation/renaming is supported. Omitting
`--input-pbf` checks the DB and manifest only; the command explicitly reports
that the source PBF digest was not rechecked. A later DB edit or rebake changes
its digest even if the FSAP checksum has been recomputed.

The helper checks exact supported headers, framing, v3 directory bounds and
FNV-1a checksum. It does **not** duplicate the Rust reader's geometry, coordinate,
reference or UTF-8 validation, parse the PBF, inspect its actual snapshot date,
verify that the DB was derived from that PBF, or authenticate the provider.
The source/conversion relationship remains user-attested. Hashes are integrity
records, not signatures, provenance proof or rights clearance. The helper always
keeps distribution unreviewed; modifying that status invalidates its verification.
Hashing the entire source PBF is intentional offline work and may be expensive.

## Before a future data release

Keep code/assets, independent DEM/climate data and OSM-derived data identifiable
and separately documented. Separation helps audit the boundary; it does not
remove obligations for a derived database. ADR-0008 still requires a new approved
distribution decision before adding regional OSM packs to a release.

1. Establish the real source, snapshot, input/output hashes and reproducible
   converter version. Resolve missing evidence; a local manifest is not enough
2. Ship suitable OSM attribution and the ODbL license or URI with each database
   and its documentation. Preserve upstream notices. Maintain visible game
   attribution and accessible detailed credits when OSM content is used
3. Determine and satisfy the applicable derivative-database share-alike and
   machine-readable access/offer obligations, including when distributing a
   produced work made from a derivative database. Configure an actual recipient
   access method and verify it before release
4. Review any storefront DRM or additional data-use terms; recipients' ODbL
   rights must remain exercisable, with an unrestricted parallel copy when
   required. Review the actual package rather than assuming Steam does or does
   not impose a particular restriction

Commercial use is permitted by ODbL; the license expressly excludes the computer
programs making or operating a database. This is not an automatic requirement to
relicense the game code. The database notices, share-alike, access and restriction
rules are in ODbL §§4.2–4.7. [Official ODbL text](https://opendatacommons.org/licenses/odbl/1-0/)

For games and simulations, OSMF permits suitable startup, gameplay, menu or
credits attribution with detailed information accessible. Database attribution
also belongs with the data. The existing app conditionally credits loaded OSM
data in its HUD and map credits; `ATTRIBUTION.md` is part of the release bundle.
[Official OSMF attribution guidance](https://osmfoundation.org/wiki/Licence/Attribution_Guidelines)

Do not build offline packs by scraping the public raster tile server: bulk
prefetch and offline archives are prohibited there. Use independently licensed
local extracts, a suitable provider or your own infrastructure.
[Official tile policy](https://operations.osmfoundation.org/policies/tiles/)

The editing API is for editing rather than a read-only game backend. Large data
users should use planet/extract downloads or appropriate services.
[Official API policy](https://operations.osmfoundation.org/policies/api/)
Public Overpass instances likewise identify a consumer app backend or stitched
world scraping as problematic; offline preparation is the bounded path here.
[Overpass operator guidance](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html)

## Tests

```sh
python -m unittest discover -s scripts/tests -p test_osm_airport_provenance.py -v
```

Tests use the existing 400-byte, independently generated synthetic FSAP v3
fixture and constructed legacy frames. They cover malformed headers/directories,
checksums, digest changes, dates/URLs, aliases, existing outputs, publication
races/failures and partial source verification. No real OSM data is downloaded or
added. Full simulator/native Windows validation is outside this helper's scope.
