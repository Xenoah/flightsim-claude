# Public profiles and prepared downloads: integrated checks

This source milestone combines the public aircraft-profile v1 schema and an
opt-in command-line GitHub package downloader. The regular application's region
panel continues to import local prepared ZIPs; URL entry and a real-area catalog
are later integration work. Existing profiles, forces, controls, renderer and
replay behavior are unchanged. Weather and complete replay-identity contracts
are being developed separately and are not included in this tree.

## Profile contract

The [schema and guide](../aircraft-profiles.md) describe the existing strict
runtime loader, with 89 shared acceptance/rejection recipes. The Rust tests call
the actual file loader for the corpus and both fictional examples. The Python
checker uses explicit UTF-8 decoding, including on Windows; its Draft 2020-12
validator is pinned to the locally tested jsonschema 4.26.0 in source-boundary CI.
The guide distinguishes schema checks from byte limits, integer token spelling,
duplicate members, finite-machine-number conversion, inertia and model checks.

The test-module declaration lives under `cfg(test)` in `main.rs`.
`aircraft_profile.rs` remains byte-identical to the protected baseline, including
SHA-256 `59c6deb0db1822178b30a0ba4e2fcbcac9e2177e1f0f0851f54c74510f3c6da0`.
No candidate source pin, parser, built-in profile or model adapter was changed.

## Download boundary and independent review

[Download scope](../content-downloads.md) is explicit public GitHub release ZIPs
or commit-pinned raw ZIPs, with a caller-provided SHA-256. Hashes pin content;
they do not establish publisher trust or redistribution rights. The cache and
installer retain separate locks and stages; a completed download does not start
a flight. Arbitrary repository archives, raw DEM conversion, credentials,
proxies and unconstrained URL hosts are unsupported.

Review found a real feature-unification hazard: enabling ureq gzip through the
existing offline asset tool made ureq strip encoding and length headers before
the original response checks. The fix places a bounded raw-header guard after
TLS and before ureq, so unsupported encodings cannot reach its decoder. Tests
exercise the combined gzip-enabled feature graph, fragmented headers, exact
header caps, byte framing, DNS concurrency/timeouts and offline cache validation.
The independent follow-up review found no blocking boundary issue. Hostile local
writers and power-loss durability are outside the documented cache guarantee.

The production CLI's public synthetic fixture attempt on the local executor
failed at direct DNS resolution (`EAI_AGAIN`). No live download success was
claimed from that attempt. Separate bounded Windows/Linux CI jobs run the same
CLI against the pinned original 1,362-byte synthetic package, then verify offline
cache reuse into another store. Their terminal results are required before
calling the public transport qualified; ordinary unit tests remain offline.

## Integrated local verification

Using Rust 1.93.0, two Cargo jobs and warnings denied, the integrated source passed:

- 24 download tests, 23 package-boundary tests and 36 asset-tool tests under the
  combined feature graph: 83 passed, no failures
- Complete app all-target tests: 221 passed, one existing optional external-data
  fixture ignored; the shared profile corpus is included
- 173 Python boundary tests, five global-pack tests and two public-schema tests
- Workspace all-target Clippy with the download feature enabled, formatting,
  architecture checks and strict private-item docs with that feature enabled

No new native flight or image comparison is claimed for this data-only milestone.
The existing default app smoke remains in CI. The canonical default MSVC notice
inventory was mechanically refreshed: 359 packages, unchanged notice bytes and
four unresolved entries, still `not_reviewed`. Downloads remain disabled in the
default app; that inventory is not a license review for an enabled download build.
Distribution rights and release-authorization gates remain active.
