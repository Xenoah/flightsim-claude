# Prepared GitHub package downloads (opt-in foundation)

`flightsim-content` has an optional `downloads` feature for fetching a prepared
[data-only schema-v1 ZIP](content-packages.md). The current application does not
enable it or expose download controls. Its map still imports local ZIPs only.
This foundation provides a synchronous worker API and a command-line example;
there is no real-area catalog, repository discovery, implicit update or live
terrain streaming. [ADR-0015](adr/0015-public-prepared-package-downloads.md) records
the boundary and tradeoffs.

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
bytes restart from zero. The app still owns any future single-worker UI integration.

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
are recorded in [download QA](qa/content-downloads-2026-10-04.md).

HTTPS and checksums do not grant data use, modification, redistribution, commercial
use or legal-term acceptance. The schema retains the provider's inert provenance,
credits and original license text; acquiring or installing bytes does not accept
new terms. Real geographic packages still require actual source, datum/conversion,
coverage, attribution and rights evidence. This change supplies none by inference.
The existing Steam/commercial release gates remain authoritative.
