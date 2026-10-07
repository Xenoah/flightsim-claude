# Local terrain packages, schema v1

## Reproducible local Balzers sample

The [Balzers sample](examples/terrain-packages/balzers/README.md) contains 170
prepared GLO-90-derived DEM tiles: two complete L10 families through L13,
approximately 26.7 × 19.5 km. It is a rebuilt, explicitly versioned local sample;
it does not recover the identity of a later lost draft. The original license,
attribution and source/conversion records remain byte-exact, with a separate
reconstruction record. The nominal 90 m source resolution is not a surveyed
accuracy or runtime mesh-spacing guarantee. Original datum-conversion limitations
remain applicable.

The content-only example validates or imports the ZIP without building Bevy:

```text
cargo run --locked -p flightsim-content --example validate_region_package -- \
  validate docs/examples/terrain-packages/balzers/Balzers_Terrain_Package_v1.zip
cargo run --locked -p flightsim-content --example validate_region_package -- \
  install docs/examples/terrain-packages/balzers/Balzers_Terrain_Package_v1.zip LOCAL_REGION_STORE
```

`validate` discards temporary staging. `install` uses the unchanged immutable
store operation and requires an explicit local destination. Pass that same
directory as the app's `--region-store`; select the package from Installed and
explicitly Start a new flight. Import does not activate terrain, add runways or
buildings, enable regional replay, or qualify native Windows execution.
The example also accepts `inspect INSTALLED_DIRECTORY` and `cancel ZIP`.

No published download URL or real-area catalog is asserted by this sample.
The offline generator can produce catalog metadata only when an explicit valid
GitHub source URL is supplied; it does not publish or verify remote availability.
Rights/release gates are unchanged.

## Format and ownership

`flightsim-content` is the pure Rust boundary for **prepared, data-only regional
terrain packages**. It validates local ZIP files, stages them, and installs
immutable versions. An optional [download foundation](content-downloads.md) can acquire
explicit hash-pinned prepared GitHub ZIPs. The app keeps regional content offline by
default; `--features region-downloads` enables explicit acquisition through a
user-supplied local catalog and the map's Installed / Downloads views.
The content layer does not download/convert arbitrary GitHub repositories, convert raw DEMs,
execute MOD scripts, or activate a region. A GitHub release ZIP is compatible only
if its author deliberately produced this schema. Ordinary repository archives,
GeoTIFF, HGT, OSM PBF, airport databases, scenery databases, nested archives and
executable content are not accepted by v1.

## Ownership and activation

- `content` owns package schema, portable paths, resource budgets, integrity,
  local installation and validated descriptors. Its dependencies point to
  `world`/`core`; it has no Bevy, UI, FDM, sim or offline tilegen dependency.
- `world::dem::io::read_tile` remains the DEM decoder. The importer first checks
  dimensions and exact encoded length before invoking it. The runtime package
  source permits only declared tiles and rechecks their size and SHA-256 on reads.
- `app` owns import workers/progress/cancellation and the selected pending package.
  **At most one regional package is active, and activation is a new-flight
  transaction.** Import, selection, validation failure and cancellation must not
  mutate a live flight. A validated descriptor is the activation input.
- Compose `InstalledPackage::tile_source()` with the existing
  `world::global::GlobalTileSource` so a regional miss preserves global fallback.
  Renderer/FDM policy and sparse-tile behavior are unchanged. Missing/invalid data
  are errors or misses, never evidence that a modified package retains identity.

`terrain_directory()` is provided for display/integration diagnostics. Prefer the
package tile source to a bare DiskTileSource: only the package source restricts
reads to its declared allowlist and rechecks hashes. Inspect installed data before
starting a flight; runtime load errors must be surfaced through the existing
terrain diagnostics. SHA-256 detects changes; it is not publisher authentication.

## Archive layout

The ZIP root contains `manifest.json`, declared terrain files and declared text
records. One extra repository-root folder is not silently stripped.

```text
manifest.json
terrain/9/900/180.fsdem
docs/LICENSE.txt
docs/PROVENANCE.txt        # optional, but declared and hashed when present
```

v1 accepts only a single-disk ZIP32 envelope with no prepended data, archive
comment or ZIP64 central-directory fields. The resource limits fit ZIP32; ZIP64 is unnecessary.
Central extra fields and file comments are capped at 4 KiB each; local
header metadata uses a fixed parser buffer. A fixed
envelope check runs before the ZIP dependency interprets any offsets.

Explicit directory entries are optional. If present, they must be empty ancestors
of declared files. Store or Deflate compression is accepted. Encryption, unknown
flags/methods, links, devices and other special file types are rejected. ZIP file
attributes are not restored; imports never create links or executable permissions.

Paths are relative `/`-separated ASCII components of letters, digits, `_`, `-` and
`.`. Empty components, dot/dot-dot, trailing dots/spaces, Windows device names,
drive paths, UNC, backslashes, alternate data streams, Unicode aliases, file vs
directory collisions, duplicates and case collisions at every directory level
are rejected on every platform. Maximum path length is 180 bytes and depth is 8.

## Manifest contract

All fields below are required; unknown fields, duplicate object fields, kinds and
schema versions fail closed. Each file's hash is the lowercase SHA-256 of its
exact original bytes. A manifest does not list/hash itself; its exact bytes are
hashed separately for `RegionalIdentity`.

```json
{
  "schema_version": 1,
  "id": "org.example.region",
  "version": "1.0.0",
  "title": "Example regional terrain",
  "content_kinds": ["terrain_dem"],
  "terrain": {
    "bounds_degrees": {"west": 135.0, "south": 30.0, "east": 145.0, "north": 40.0},
    "nominal_resolution_m": 90.0,
    "datum": "EPSG:4979"
  },
  "sources": [{
    "id": "source-name",
    "url": "https://example.org/dataset",
    "revision": "exact release/revision or dated dataset",
    "provenance": "Input data, source datum, geoid conversion, tools and assumptions",
    "credits": "Original provider's exact required attribution",
    "license": {
      "name": "Original license identifier or title",
      "url": "https://example.org/license",
      "text_path": "docs/LICENSE.txt"
    }
  }],
  "files": [{
    "path": "terrain/9/900/180.fsdem",
    "kind": "terrain_dem",
    "size_bytes": 74,
    "sha256": "REPLACE_WITH_ACTUAL_LOWERCASE_SHA256",
    "source": "source-name"
  }, {
    "path": "docs/LICENSE.txt",
    "kind": "documentation",
    "size_bytes": 123,
    "sha256": "REPLACE_WITH_ACTUAL_LOWERCASE_SHA256",
    "source": "source-name"
  }]
}
```

This is a structural example; placeholder hashes, sizes, coverage and rights text
must be replaced with evidence from the actual files. Metadata strings, URLs,
credits and license text are untrusted inert text. URLs are not fetched, and
records do not prove a provider's identity, accuracy, redistribution rights or
user acceptance of legal terms. Preserve original source/license text and byte
hashes; do not invent missing permission or silently fix a datum.

- IDs are stable lowercase ASCII names, beginning with a letter, at most 80 bytes.
  Versions are canonical numeric `MAJOR.MINOR.PATCH` (no leading zero aliases,
  prerelease syntax or automatic ordering/upgrade policy).
- `content_kinds` is exactly `["terrain_dem"]`. Files have `terrain_dem` or
  `documentation` kind; unknown future airports/scenery/MOD kinds are rejected.
  Extending kinds needs a reviewed schema/reader contract, not a generic file copy.
- Tile files must use the exact `terrain/<level>/<x>/<y>.fsdem` path derived from
  their runtime tile ID. No leading-zero tile path aliases are accepted.
- Coverage uses degree-valued geographic bounds and must contain every entire
  tile. West greater than east describes antimeridian-crossing coverage; equal
  longitudes are invalid. This is a bounding region, not a promise of full tile
  coverage or complete child groups at every LOD.
- Nominal resolution is finite and 0.01–100,000 metres; it describes source data,
  not runtime mesh density. Tile dimensions/format are verified independently.
- Datum is exactly `EPSG:4979`, WGS84 ellipsoidal heights in metres, matching FSDM.
  Raw orthometric data must be normalized by the offline pipeline before packaging.
- Every source needs provenance, revision, credits and a declared local UTF-8
  license-text file. Source and license URLs must begin HTTP(S), but are inert.

## Fixed v1 budgets and validation

| Resource | Limit |
|---|---:|
| Input ZIP file / actual cumulative uncompressed output | 512 MiB each |
| Manifest bytes | 1 MiB |
| Manifest payload files | 4,096 |
| ZIP/installed file and directory entries | 8,192 |
| Source records | 64 |
| DEM encoded bytes | 56 + 2 × 1,024 × 1,024 |
| DEM samples | 1,024 × 1,024; each edge 2–4,096 |
| Documentation file bytes | 256 KiB |
| Per-entry compression ratio | 1,024:1 |
| Listed installed versions / top-level entries | 256 / 1,024 |

Input is copied through bounded cancellable reads into a private temporary
snapshot before preflight/decoding, so an original ZIP changed during import cannot
bypass the checked envelope. Peak disk staging includes this ≤512 MiB snapshot
plus ≤512 MiB of extracted data. Metadata is streamed through a fixed rawzip buffer; the central directory is not
bulk-allocated from its claimed count. Local/central names, flags and methods,
entry count, overlapping ranges, special modes, archive/manifest file sets,
individual/cumulative sizes, CRC and actual inflated byte counts are checked.
Both compressed input reads and inflated output reads are capped at 32 KiB and
check cancellation. Empty nonfinal Deflate blocks cannot keep cancellation outside
a long zero-output decoder loop. Output never exceeds remaining declared bytes
by more than one rejection byte. Files are never recursively unpacked.

Each payload's SHA-256/size and UTF-8 text or FSDM semantics are checked. FSDM
header dimensions and exact length are checked before runtime decoding/allocation.
Decoded heights must be finite and within −12,000 to +100,000 metres. Declared
geometric error must not exceed their elevation range plus 10⁻⁶ m of roundoff
tolerance: the standard writer derives error from a bilinearly interpolated coarse
version of that same decoded grid. This also bounds automatic render skirt depth. These checks
bound resource use and reject corrupt data; they do not certify terrain accuracy.
`world::terrain::DiskTileSource` also bounds disk-file size and streams through the
existing DEM reader, rejecting trailing data instead of allocating the whole file.

## API and lifecycle

```rust,ignore
let staged = flightsim_content::stage_zip_with_progress(&zip, &store, |progress| {
    report(progress);          // bounded callback; do not run this on a render thread
    !cancelled()
})?;
let installed = staged.commit()?; // explicit, atomic publication; no activation
let summaries = flightsim_content::list_installed(&store)?;
let selected = flightsim_content::inspect_installed(&summaries[0].directory)?;
// The application passes selected into a later new-flight transaction.
```

`ImportProgress` reports `Inspecting`, `Extracting`, `Validating` and `Ready`;
file totals and extracted bytes are reported. Returning false cancels, including
at `Ready`. Dropping a ready `StagedPackage` also cancels. Only that operation's
private `.import-*` directory is cleaned; installed versions and other operations'
staging directories are preserved. `install_zip` is a no-callback convenience.

Publication takes a nonblocking OS advisory lock on the store's `.install.lock`,
checks the destination under that lock and renames the complete staging directory
on the same filesystem to `<store>/<id>/<version>`. Lock contention returns
`StoreBusy`; an existing version returns `AlreadyInstalled`, including an empty or
malformed directory. No replacement, downgrade, uninstall or updater runs.
A crash before rename leaves an ignored staging directory, with no version
reservation; another import can proceed. A crash after rename leaves the complete
version. Power-loss durability of directory metadata is not promised.

The caller owns a trusted local store root. Symlinked roots/ancestors are rejected;
cooperating importers use the store lock. This is not a filesystem sandbox against
an attacker who can concurrently replace the application's own store/lock files.
Do not use an untrusted shared-writable installation directory.

`list_installed` parses only metadata and returns `InstalledSummary`; it is not an
activation token. `inspect_installed` rejects extra/missing files, links and any
payload mismatch, returning private-field `InstalledPackage`. The loader verifies
declared bytes again when loading a tile. External owner edits invalidate a
package; an old descriptor does not authenticate newly modified bytes.

## Replay and future MODs

`RegionalIdentity` is `(id, version, SHA256(exact manifest bytes))`; because the
manifest binds every file hash, different content or provenance changes identity.
Archive timestamps/compression are irrelevant; manifest whitespace is significant.
This identity does **not** get squeezed into an existing global atlas fingerprint.

Existing replay bytes remain untouched by package handling.
`InstalledPackage::require_replay_support()` returns `ReplayUnsupported`: the app
must block saving and playback with package-backed terrain. A future explicitly versioned replay schema
must record regional identity, validate installed payloads against it and fail
clearly on missing or mismatched data, with dedicated byte-compatibility tests.
No package-backed deterministic replay support is claimed by this foundation.

Future FSAP/FSSC content needs explicit bounded runtime reader integration,
precedence, credits and replay identity. Executable MODs are outside this data-only
format and are never launched merely because a file appears in a ZIP.


## Dependency evidence

[New package dependency records](release/content-package-dependencies.json) list
newly resolved registry versions, archive checksums and declared licenses, plus
existing dependencies newly referenced by this crate. Regenerate the complete
application dependency/notice inventory for its exact target and feature set after
integration. These records are not completed legal review or distribution approval.

## Application local import and selection

The application supports these separate local operations:

```text
flightsim-app --import-region prepared-region.zip [--region-store DIR]
flightsim-app --list-regions [--region-store DIR]
flightsim-app --region org.example.region@1.0.0 [--region-store DIR]
```

Import and list complete before starting the renderer and exit. Import does not
activate terrain. `--region` opens the world map with a pending selection. Close
Regions, select a departure point, and use **Start new flight** to apply it.
`--region` rejects combinations with `--tiles`, `--replay`, and global terrain off.
The import/list/select operations are mutually exclusive; run them separately.
Existing raw `--tiles` and legacy replay paths keep their original semantics.

The default store is `flightsim-claude/regions` below Windows `LOCALAPPDATA`, macOS
`~/Library/Application Support`, or Linux `XDG_DATA_HOME` (falling back to
`~/.local/share`). An explicit `--region-store DIR` overrides this. Missing user
application-data settings produce an error instead of silently writing beside
executables, into the source tree, or into a shared temporary directory.

While the map is open, **Regions** (G) lists at most 256 installed versions, five per
page. Keyboard controls are 0 for base terrain, 1–5 for the visible rows, PageUp /
PageDown for package pages, Left / Right for credit pages, R to refresh, X to
cancel, and Esc to return to the map. Selection stays pending until a later
explicit Start. The shortcuts preserve coordinate-editor input and consume their
opening/closing frames so no stale Enter can start a flight. Arrow and page
navigation also accept logical keys from NumLock-off keypads/remapped layouts;
numeric package selection remains on the top-row 0–5 keys. A prepared local `.zip` can be dropped onto the visible map; drops outside
the map do nothing. Default builds perform no regional network downloads, and
repository ZIP conversion is never supported.
Import, refresh and full installed-package inspection share a single background
worker. Progress replaces one bounded snapshot, and there is no queued backlog.
Cancel or map dismissal invalidates any late result; a worker keeps its slot until
it finishes. Cancellation after the atomic install has already happened cannot
undo publication: Refresh shows any such installed version. Neither outcome
changes the running flight.

With the opt-in `region-downloads` feature, launch with `--region-catalog FILE.json`
and optional `--region-cache DIR` / `--region-offline`. Regions adds **Installed /
Downloads** (D), **Download / Retry** (F) and **Cached only** (C). Catalog rows only
preview the declared location, source/hash and provenance. An explicit attempt
reuses the strict downloader/cache on the same worker; the staged manifest's ID,
version, title and bounds must match the selected catalog record before immutable
installation. Completion never selects or activates the downloaded package:
choose it in Installed, then explicitly Start. Refresh clears a preview whose
catalog record changed; cancellation cannot undo publication already completed.
See [catalog, controls and cancellation](content-downloads.md) for the complete
contract. No real-area catalog is shipped, and catalog bounds do not promise full
tile coverage. Default and existing commercial-candidate feature sets retain the
offline regional-content dependency graph.

Only an explicit Start begins a full off-thread inspection of the selected exact
manifest identity. Changed departure/month, cancellation, dismissal, invalid bytes
or changed manifest discard that result. Successful activation shares the same
immutable inspected descriptor between the physical sampler and renderer; each
uses the package allowlist and hash-checking tile source with the existing global
fallback. Existing LOD, level-selection and sparse-tile policies remain in effect;
`--max-level N` still controls the highest source level searched.

Changing package identity creates a free flight: the old airport surfaces, lights,
signs, scenery and runway-specific landing evaluation are removed, because they
were built against another terrain source. Terrain contact and ordinary flight
logging still work. Returning to the baseline creates another explicit free flight;
it does not resurrect stale airport geometry. Restarting the application restores
an explicitly configured airport/scenery session.

For package-backed flights, replay recording, F9 export and playback are
blocked. The map and flight attribution explain this limitation. No regional
identity is inserted into legacy replay bytes. Region source/provenance/credit and
license metadata appear as bounded inert paginated text; full original license
files remain under the displayed installed-package store. URLs are not opened or
fetched, and importing is not license acceptance or redistribution clearance.

`inspect_installed_with_progress` adds cancellation checkpoints before metadata,
at every installed tree entry, between payload validations and at Ready. Each
payload checkpoint covers at most one bounded 2 MiB DEM / 1,048,576 samples; it does
not claim to interrupt a blocking filesystem read. `inspect_installed` remains the
no-callback convenience API.
