# Public prepared-package acquisition QA (2026-10-04)

Scope: opt-in `flightsim-content/downloads`, its synchronous API, cache and CLI
example. The current app does not enable the feature. No terrain/FDM/render/replay
behavior or release-rights gate was changed. This is a foundation qualification,
not a real-area catalog, UI download acceptance, or shipping approval.

## Fixture evidence

`crates/flightsim-content/tests/data/make_download_fixture.py` independently
constructs FSDM bytes from the binary specification and uses Python's standard ZIP
writer for a stored ZIP32 package. It contains a constant 350 m 2×2 test DEM,
manifest and inert test documentation. Re-running the script reproduced SHA-256
`ef3d47de1b40f09c5f4e377551a1f6fc99d8b599641db384301be2ec2e24c995`.
No real geography, source license clearance or external data is claimed.

## Automated acceptance

The opt-in suite covers:

- Explicit release/raw URL grammar and hash requirements; latest/branch/archive,
  credentials, ports, queries, fragments, normalized/encoded paths and raw-data
  suffixes fail before network
- Exact redirect hosts, suffix attacks, private/localhost targets, raw-source
  redirects, signed URLs, loop and hop limits; unsafe redirects never issue a
  second request and signed URLs never enter the cache receipt
- IPv4/IPv6 nonpublic/special/mapped/tunnelling address rejection; production
  resolver preflight rejects unsafe host/scheme/port and proxy configurations
- A timed-out DNS job keeps its slot until its actual lookup finishes; another
  lookup cannot spawn during that interval
- Socket waits constrained by both idle and absolute deadlines underneath TLS
- Deterministic plaintext transport fragments at every header split, including
  CRLF delimiter boundaries, and exact 16 KiB−1 / 16 KiB / 16 KiB+1 header limits;
  accepted headers and body bytes remain byte-identical
- Real loopback HTTP parser fixtures for truncated Content-Length and chunked
  body byte caps (test-only plaintext route, unavailable to the public API)
- Missing/dishonest/duplicate/ambiguous Content-Length, unknown/infinite bodies,
  oversized wire headers and declared body lengths, encoded/text responses, transport
  errors, HTTP failures, hash mismatches and invalid-but-hash-matching ZIPs
- Successful download then complete cache revalidation and explicit offline
  install with the same source/manifest identity; offline miss performs no request
- Corrupt bytes/receipts/manifest identity, extra files, links, overlapping roots,
  cache byte/count/root budgets, nonblocking cache locks and local-import independence
- Cancellation before connection, during receipt/verification/import, at Ready
  and after receiving bytes; cleanup, clean explicit retry, preservation of valid
  cache/installed versions and other operations' crash directories
- Changed source checksum cannot overwrite an existing cache or installed package

All existing strict local-package adversarial tests remain part of the same run.
The optional feature is enabled for core CI (Windows and Linux); the architecture
checker also includes its dependency graph. Target-platform remote CI and full
application qualification remain integration-owner checks.

## Local verification

Linux, Rust 1.93.0, `-D warnings`, two Cargo build jobs:

- `cargo test -j 2 -p flightsim-content -p flightsim-assetgen --features flightsim-content/downloads --all-targets --offline`: passed **24 new download tests, 23 existing package tests and 36 assetgen tests**
- `cargo clippy -j 2 -p flightsim-content -p flightsim-assetgen --features flightsim-content/downloads --all-targets --locked --offline -- -D warnings`: passed
- `cargo test -j 2 -p flightsim-content --features downloads --doc --locked --offline`: passed (no executable documentation examples)
- `RUSTDOCFLAGS="-D warnings" cargo doc -j 2 -p flightsim-content --features downloads --no-deps --document-private-items --locked --offline`: passed
- `cargo build -j 2 -p flightsim-content --features downloads --example download_region --locked --offline`: passed
- `bash scripts/check-architecture.sh`: passed, including the optional download graph
- Architecture negative control in a disposable worktree: an otherwise-hidden
  optional content→net dependency enabled only by `downloads` was rejected with
  exit 1 and the intended unrelated-runtime-layer error
- Independent fixture regeneration and new local documentation links: passed

The joint assetgen/content run matters: assetgen enables ureq's default gzip
feature. Independent review found that the original post-response header check was
insufficient because ureq stripped Content-Encoding and Content-Length before
returning. That attempt was corrected before integration. The final plaintext
transport guard is tested with real HTTP gzip-wrapped valid ZIPs, oversized encoded
lengths, chunked gzip, duplicate encodings, Brotli, ambiguous framing and an
informational prefix; all reject before decoding. A valid identity-encoded response
retains its exact ZIP bytes and declared length under the same feature unification.

## Public production-path attempt

The built CLI was run against the existing public **synthetic** repository fixture:

`https://raw.githubusercontent.com/Xenoah/flightsim-claude/8b4df0e1b7e744218176b1d19233a1463f304b47/crates/flightsim-app/tests/fixtures/region-synthetic.zip`

Expected SHA-256:
`d3b86a557404bd687baa7549ca274759cde7e68d5718ea94cb414116d72b9e73`.

The attempt exited 1 before receiving an archive. A same-executor direct DNS check
for `raw.githubusercontent.com` returned `EAI_AGAIN` (-3). Proxy variables were
present; their values were not printed, and the downloader intentionally ignores
proxies. No proxy, hostname or TLS policy was weakened. **Live public GitHub
transport has therefore not been qualified in this executor.** Loopback transport
and mocked source/cache success are separate evidence, not a substitute for that
missing end-to-end check. No complete cache entry or installed package was published
by the failed attempt.

A separate `public synthetic download` CI matrix (Windows/Linux) now builds and
runs that same production CLI against exactly this pinned URL/hash, then requests
`offline` reuse into a second store. Each job has a 10-minute cap; the downloader's
own DNS/socket/deadline bounds still apply. These live jobs have not been run by
this local qualification. Their remote outcomes must be checked before claiming
public transport success. The ordinary unit suite performs no external requests.

## Limitations

There is no certified production server, real-area catalog, user-supplied provider
terms acceptance, raw GeoTIFF/repository adapter, range resume, background queue,
cache eviction policy or map UI in this change. Transport fixtures establish
specific boundaries; they are not a penetration test or universal network proof.
Cancellation is cooperative and blocking filesystem reads have no latency bound.
The trusted cache is not a filesystem sandbox against its owner. HTTP/TLS and
checksums do not establish authorship, terrain accuracy or distribution rights.
No application benchmark, physical GPU/controller test, new aircraft qualification
or Steam release approval is inferred.
