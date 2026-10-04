# Prepared GitHub package downloads (opt-in app and catalog)

`flightsim-content` has an optional `downloads` feature for fetching a prepared
[data-only schema-v1 ZIP](content-packages.md). The application's separate opt-in
`region-downloads` feature connects that same strict downloader/cache to the map's
**Regions → Installed / Downloads** workflow using an explicitly supplied local
catalog. Default app builds retain offline local import/selection. Neither the
default nor the existing `commercial-staging` feature enables HTTPS acquisition;
an opt-in download build needs dependency/release review for its own feature set.

No curated real-area catalog or real downloadable terrain is shipped by this
feature. There is no repository discovery, automatic network access, update,
retry, activation or live terrain streaming. The synchronous content worker API
and command-line example remain available separately.
[Optional Windows dependency inventory and limits](release/region-download-dependencies.md)
are recorded separately from the default application and Swift-only candidate.
[ADR-0015](adr/0015-public-prepared-package-downloads.md) records the acquisition
foundation's boundary and tradeoffs.

## Application workflow

```text
cargo run -p flightsim-app --features region-downloads -- \
  --region-catalog FILE.json [--region-cache DIR] [--region-offline]
```

Brackets denote optional arguments, not literal shell syntax. `--region-catalog`
reads a local file and opens the map's Regions panel; it does not fetch a remote
index. These flags are rejected in builds without `region-downloads`.
`--region-cache` and `--region-offline` require `--region-catalog`. Catalog mode
requires global terrain for fallback and cannot be combined with `--import-region`,
`--list-regions` or `--replay`; run those operations separately.
`--region-cache` overrides
the download cache, separately from `--region-store`, which controls installed
packages. The default cache is `flightsim-claude/region-download-cache` under the
same per-user application-data base used for the default region store. An explicit
store override does not relocate that default cache. Cache and store must be
separate, non-nested directories owned by the user.

1. Open the world map and **Regions** (G). **Installed / Downloads** (D) switches
   views. In Downloads, select a row (1–5 on the current page) to center the map on
   its declared bounds and inspect its source ZIP URL, exact archive SHA-256 and
   publisher-declared provenance. Previewing does not acquire or select terrain
   for a flight. PageUp/PageDown change rows; Left/Right page the metadata.
2. **Download / Retry** (F) explicitly starts one attempt. It revalidates a matching
   cache entry first and contacts GitHub only on a clean cache miss. **Cached only**
   (C) uses verified cache without DNS or network access. `--region-offline` forces
   both buttons to use this cache-only policy; a miss is an error. Failures never
   retry automatically, repair corrupted cache online or fall back to another URL.
3. The worker hash-checks and stages the ZIP through the existing strict importer.
   Before installation, the app compares the staged manifest's **ID, version,
   title and geographic bounds** with the selected catalog record. Any disagreement
   rejects installation. Download integrity alone is insufficient to pass this
   comparison. A verified archive may already be in cache at this point; that is
   not an installed or activated package.
4. Successful installation updates the Installed list without selecting the new
   package. Explicitly choose its **Installed** row, inspect the original source
   and license notices, return to the map and use **Start new flight**. Start fully
   reinspects the exact installed manifest identity before activation. Preview,
   download, install and cancellation never change the current flight.

Refresh (R) rereads the local catalog and installed list; there is no catalog
watcher or background update. If the selected record is removed or any of its
source, hash, identity, title, bounds or provenance changes on refresh, its preview
selection is cleared. Select the revised row again before another attempt. A job
uses the selected record snapshot, not a live reference to an externally edited
file. A changed catalog never updates or replaces an installed immutable version.
Correct a catalog/manifest mismatch and explicitly Refresh before retrying.

Import, refresh, acquisition and installed inspection share one background worker
with one bounded progress snapshot and no queue. X, closing Regions, or dismissing
the map requests cancellation and invalidates late UI results. The worker retains
its slot until it finishes; another action is not queued behind it. Cancellation
is cooperative and cannot interrupt an in-progress OS filesystem/DNS operation.
It also cannot roll back a cache publication or atomic installation that has
already happened. Refresh after the worker finishes to see any completed install;
it still needs explicit Installed selection and Start. The lower-level timeout,
cleanup and cache rules below apply unchanged.

## Local catalog schema v1

The catalog must be a regular, non-symlink JSON file of at most **256 KiB**, with
`schema_version: 1` and a `regions` array of at most **64** entries. An empty array
is valid. Unknown fields/schema versions, duplicate fields, duplicate ID/version
selectors and duplicate source URLs are rejected. Each entry requires
`id`, `version`, `title`, `bounds_degrees`, `url`, `archive_sha256` and `provenance`.
IDs/versions must be canonical portable package selectors; titles and provenance
are nonempty text of at most 160 and 8,192 UTF-8 bytes respectively. Control
characters are rejected except newlines in provenance. The current UI simplifies
unsupported glyphs and points to the original UTF-8 catalog or installed files;
source URLs and hashes retain their full ASCII text through pagination.

Bounds require finite `west`/`east` longitudes within −180°…180° and `south`/`north`
latitudes within −90°…90°, with south less than north. West greater than east
describes antimeridian crossing. Equal longitudes and the zero-width 180°/−180°
alias are rejected. These are catalog coverage claims, not proof that every tile
or LOD in that area exists. The package must independently pass its own bounds
and payload checks. Catalog provenance is displayed as an inert claim; it is not
a substitute for the installed manifest's sources and original license text.

The following is **only a structural example**. Its URL, identity, bounds and text
are placeholders, and its deliberately invalid hash prevents use. Replace them
only with a real prepared package's exact metadata, independently obtained archive
hash and actual provenance/rights evidence; no downloadable dataset is asserted.

```json
{
  "schema_version": 1,
  "regions": [{
    "id": "org.example.region",
    "version": "1.0.0",
    "title": "Placeholder package",
    "bounds_degrees": {"west": 135.0, "south": 30.0, "east": 145.0, "north": 40.0},
    "url": "https://github.com/OWNER/REPO/releases/download/TAG/ASSET.zip",
    "archive_sha256": "REPLACE_WITH_ACTUAL_64_LOWERCASE_HEX_DIGITS",
    "provenance": "REPLACE with the actual package source, datum/conversion and rights evidence"
  }]
}
```

Every record's URL and archive hash must pass the same source contract below.
A local catalog does not authenticate a publisher. SHA-256 verifies requested
bytes, not terrain accuracy, source trust, license acceptance or redistribution
rights. Source/license URLs in the inspected package remain inert metadata.

## Source contract

The caller must supply the SHA-256 of the **entire exact ZIP**, as 64 lowercase
hexadecimal digits, alongside one explicit unauthenticated public URL:

- `https://github.com/OWNER/REPO/releases/download/TAG/ASSET.zip`
- `https://raw.githubusercontent.com/OWNER/REPO/COMMIT/PATH/ASSET.zip`, where
  `COMMIT` is a complete 40-character lowercase hexadecimal commit ID

URL path components use ASCII letters, digits, `.`, `_`, `-` only; each is at most
255 bytes, at most 16 components, and the whole source URL at most 2,048 bytes.
Empty/dot segments, percent encodings, credentials, query strings, fragments,
explicit ports, backslashes and normalization aliases are rejected. The suffix is
exactly `.zip`. This intentionally excludes some valid GitHub tag/asset names.
Mutable raw branches, release `latest` aliases, API URLs, short repository names,
Git LFS pointers, repository archive ZIPs and private repositories are unsupported.
GeoTIFF, HGT, PBF, FSAP/FSSC, executable MODs and arbitrary ZIP adaptation remain
outside schema v1. No raw-data conversion occurs.

The [GitHub release-asset documentation](https://docs.github.com/en/rest/releases/assets)
identifies `browser_download_url` as the binary download location. The
[repository contents documentation](https://docs.github.com/en/rest/repos/contents)
identifies `raw.githubusercontent.com` downloads. This API does not call the
GitHub REST API, search repositories, retrieve checksums or infer trusted authors.
A release tag or asset can change; the mandatory archive hash pins the requested
bytes regardless. Obtain that hash through a source you trust. A checksum fetched
from the same compromised publisher does not independently authenticate it.

## Network and resource policy

- HTTPS uses normal rustls certificate/hostname verification and bundled web PKI
  roots. There is no insecure fallback, custom CA, credential, cookie persistence
  or authorization header. Environment proxies are explicitly disabled. Networks
  that require a proxy may therefore fail; this API does not weaken the policy.
- Redirects are manual, with at most three hops. A release URL may redirect only
  to the exact hosts `release-assets.githubusercontent.com`,
  `github-releases.githubusercontent.com` or `objects.githubusercontent.com`.
  Raw URLs cannot redirect to another source. These hosts are not accepted as
  caller entry points. Unknown new GitHub CDN hosts fail closed pending review.
  GitHub documents its content/CDN domain family in
  [automatic dependency submission](https://docs.github.com/en/code-security/reference/supply-chain-security/automatic-dependency-submission).
- Every redirect is checked before issuing its request. Redirect URL length is
  capped at 8,192 bytes; credentials, non-HTTPS schemes, fragments and nonstandard
  ports are rejected. Signed redirect URLs are transient and never persisted in
  receipts, progress or errors. They are not a reusable identity.
- Resolution is restricted to those exact hosts. All returned addresses are
  checked before the connection; mixed public/private answers fail closed.
  TCP receives the already validated addresses, with no second DNS resolution.
  Loopback, private, link-local, shared, documentation, multicast, reserved and
  mapped/tunnelling ranges are excluded by a conservative unicast policy.
- Only one system DNS lookup can be outstanding per process. A timed-out lookup
  keeps its slot until the actual OS call exits, so repeated retries cannot spawn
  unbounded resolver threads. Another request during that interval fails and can
  be retried explicitly. OS DNS itself is not interruptible through this API.
- DNS/connect/request-header/response-header waits are capped at 5 seconds. Socket
  waits are capped at 5 seconds beneath TLS as well. A 180-second absolute deadline
  covers the redirect chain and body reads, including repeated TLS transport
  reads. Header bytes are capped at 16 KiB and 128 fields; informational 1xx responses
  are unsupported and rejected. Slow transfers may need a later retry.
- A bounded plaintext-header guard above TLS rejects HTTP content encodings
  before ureq sees the response or constructs a decoder. This is necessary because
  ureq can remove encoding/length headers before returning a response when another
  workspace dependency enables gzip. Text responses are also rejected before
  application body reads, preventing charset conversion. No encoded body is
  passed to a transparent decompressor; transport buffers can contain prefetched
  bytes. The guard uses the existing locked httparse parser. Unknown-length/chunked bodies are read
  through 32 KiB buffers. At most one byte beyond the remaining archive/cache
  budget is requested from the body reader for rejection; transport buffers may
  already contain prefetched bytes; excess bytes are never written.
- Declared and actual **framed HTTP body** length are checked. Duplicate lengths,
  Content-Length with Transfer-Encoding, oversized claims, truncation and hash
  mismatches fail. A server that understates Content-Length cannot expand disk
  usage; bytes outside its HTTP message framing are not downloaded. The remaining
  prefix must still match the requested whole-archive hash and strict ZIP reader.
- Archives retain the existing 512 MiB limit and every local ZIP/path/CRC/hash,
  inflated-output and DEM-reader check. Completed cached archive data is capped at
  2 GiB and 32 entries; cache-root scanning stops after 128 entries. Small receipts
  are capped at 8 KiB. Temporary download or cache snapshot, importer ZIP snapshot
  and extracted staging can add up to 1.5 GiB while one operation is in progress.
  Incomplete crash directories are counted but never automatically removed.

## API, cache and cancellation

```rust,ignore
use flightsim_content::download::{CacheMode, DownloadSource, stage_github_with_progress};
let source = DownloadSource::github(&url, &archive_sha256)?;
let result = stage_github_with_progress(&source, &cache, &store,
    CacheMode::PreferCache, |progress| {
        report(progress); // bounded callback on an application worker
        !cancelled()
    })?;
let source = result.source;       // exact source URL + archive SHA-256
let installed = result.staged.commit()?; // explicit immutable install, no activation
```

`CheckingCache`, `Connecting`, `Receiving`, `VerifyingCache`, `Importing` and
`Ready` progress contain no server text or signed URLs. Returning false cancels,
including the final `Ready` checkpoint. A callback is not invoked inside blocking
network/filesystem calls: cancellation is cooperative, with the stated network
wait/deadline bounds, not an immediate interrupt or filesystem latency guarantee.
No queue/backlog, background thread manager, automatic retry or partial-range
resume is created. Reinvoke explicitly after a recoverable failure or cancellation;
bytes restart from zero. The opt-in app integration uses the single worker
described above and performs the catalog/manifest comparison before commit.

The cache key hashes the canonical source URL plus requested archive hash.
A complete cache entry contains `package.zip` and a strict source receipt binding
those fields to the package ID, version and exact manifest SHA-256. Only a hash-
and schema-validated package is atomically published into cache. This identity is
separate from `RegionalIdentity`; changing ZIP timestamps/compression can change
archive identity while retaining the same regional manifest identity.

`PreferCache` reuses a matching entry only after copying it through cancellable
bounded hashing into a private snapshot and repeating all ZIP validations. It
contacts GitHub only for a clean miss. `Offline` never resolves or contacts a host;
a miss is an explicit error. Corruption, extra files, links, changed receipts or
payloads fail closed without a silent network repair. Cache removal/repair is an
explicit owner operation; there is no automatic eviction.

A nonblocking OS `.download.lock` serializes jobs using the same trusted cache
root. This is distinct from the installation `.install.lock`; a network stall
cannot block a local ZIP install. Different cache roots are independent, with the
single process DNS slot noted above. Normal failures/cancellation remove only the
operation's own temporary directories. An existing valid cache, installed version
or another job's crash directory survives. Cancellation after this API returns
cannot undo a completed cache publication or a later explicit install.

Cache and store roots must be separate and non-nested; overlapping canonical paths
are rejected before downloading or creating a job lock. Cache/store ancestors and
existing files are checked for links. As with the local
importer, the caller must own these directories: this is not a sandbox against a
local attacker concurrently replacing the application's own files or lock. Atomic
renames prevent partial normal publication; power-loss durability is not promised.

## Command-line example

```text
cargo run -p flightsim-content --features downloads --example download_region -- \
  GITHUB_ZIP_URL ARCHIVE_SHA256 CACHE_DIR REGION_STORE_DIR online
```

Replace the last word with `offline` to use a verified cached package. This example
explicitly installs the prepared package but never starts or changes a flight.
An existing installed version is not overwritten. The application can later select
it through its existing local Regions workflow; replay restrictions remain unchanged.

## Evidence and rights boundary

The checked-in `tests/data/download-fixture.zip` is a tiny constant-height test
fixture generated independently with Python's standard ZIP writer and the FSDM
binary specification. Its source and license strings are test metadata, not a real
terrain license review. Adversarial tests and any public synthetic-fixture smoke
are recorded in [download QA](qa/content-downloads-2026-10-04.md). The opt-in app,
catalog and lifecycle checks are recorded separately in
[application download QA](qa/app-region-downloads-2026-10-04.md).

HTTPS and checksums do not grant data use, modification, redistribution, commercial
use or legal-term acceptance. The schema retains the provider's inert provenance,
credits and original license text; acquiring or installing bytes does not accept
new terms. Real geographic packages still require actual source, datum/conversion,
coverage, attribution and rights evidence. This change supplies none by inference.
The existing Steam/commercial release gates remain authoritative. Adding a local
catalog and map controls does not turn synthetic fixtures into reviewed real-area
content or approve a download-enabled commercial build.
