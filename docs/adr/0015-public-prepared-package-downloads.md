# ADR-0015: Opt-in public, hash-pinned prepared-package acquisition

Status: implemented foundation; app/map download integration and real-area catalog pending
Date: 2026-10-04

## Context

The local package reader already enforces a bounded DEM-only schema and immutable
install lifecycle. Users need on-demand regional downloads without credentials,
paid services, runtime raw-GIS decoders, or changes to flight/replay semantics.
An arbitrary GitHub repository ZIP is not a prepared package. Public availability
and HTTPS are not evidence of a dataset's terms or redistribution rights.

## Decision

Add an optional `downloads` feature/module to `flightsim-content`, beside its
existing pure Rust validation/staging layer. The default feature set and current
app remain offline. Reuse the already resolved ureq 3.3.0 + rustls implementation;
pin ureq exactly because its resolver/transport extension API is unversioned.
Reuse locked httparse for a bounded plaintext-header guard above TLS: ureq strips
encoding/length headers before returning when gzip is feature-unified, so the
response must be rejected before that boundary. No new registry versions are resolved. `flightsim-net` remains the unrelated
bounded traffic/session layer; world/FDM/sim never fetch data.

Accept explicit public release ZIP URLs or raw ZIP URLs pinned to full Git commit
IDs, together with a required whole-archive SHA-256. Manually validate each HTTPS
redirect, reject unapproved destinations and nonpublic resolved IPs, disable
proxies, pass validated addresses directly to TCP, and retain certificate checks.
Bound headers, actual body bytes, waits, absolute network time, cache totals and
outstanding DNS work. Never execute or convert downloaded content.

Stage only after archive hash verification, through the unchanged strict ZIP
reader. Bind URL/archive hash to manifest identity in an immutable cache receipt;
reverify snapshots and schema even offline. Cache locking is separate from atomic
package commit locking. The API returns a ready stage and source identity; the
caller chooses whether to commit. Only app may later activate a package at a new
flight, with existing global fallback and replay gates unchanged.

The full accepted URL grammar, lifecycle, budgets, timeout/cancellation caveats
and source references are in [content downloads](../content-downloads.md).

## Alternatives rejected

- Put HTTP inside world, sim or runtime tile reads: compromises offline/physical
  boundaries and makes read timing affect flight availability
- Reuse `flightsim-net`: terrain acquisition is unrelated to traffic observations
  and would introduce a forbidden package/network dependency
- New general network crate/provider framework: no second acquisition provider
  exists; a narrow opt-in content module suffices
- Add reqwest/Tokio: the workspace already has a synchronous HTTPS implementation;
  the application has a background worker integration pattern
- Generic URL fetcher, arbitrary CDN/redirect suffixes or environment proxies:
  materially broadens local-network/credential exposure for no supported data need
- API search/latest aliases/automatic checksum trust: adds mutable discovery and
  publisher trust decisions before an authenticated catalog contract exists
- Raw repository/GeoTIFF adapters: require distinct bounded formats, conversion,
  datum and source-rights contracts; the prepared ZIP reader must not guess
- Transparent retries/resume/eviction: adds overlapping jobs, mutable partial state
  and deletion policy; explicit clean retries and bounded immutable cache suffice

## Consequences

The usable core and command-line example remain GUI-independent and fully testable
with synthetic hostile transport/filesystem fixtures. Exact source bytes and offline
reuse are inspectable without claiming publisher identity or legal clearance.
Existing local packages keep the same API and default dependency graph.

The tradeoffs are deliberate: a caller must obtain a trusted hash, permitted source
names are narrower than GitHub's complete syntax, proxy-only networks fail, slow
transfers time out, cancellation is cooperative, and cache repair/cleanup is manual.
No catalog, user-friendly map download UI, raw-data adapter, multi-package activation
or regional replay support is delivered here. A real-area package and its actual
terms still need review before distribution; synthetic fixture results do not
qualify real terrain, Windows networking, performance or Steam readiness.
