# Optional region-download application dependency evidence

The `flightsim-app/region-downloads` feature has a separate Windows MSVC inventory
and exact notice pool under
[`licenses/region-download-dependency-evidence/`](licenses/region-download-dependency-evidence/README.txt).
The existing default application inventory and Swift-only candidate feature/asset
policy remain unchanged. Enabling downloads is not implicit in a normal build.
The executable reports `region_downloads: true` in `--distribution-info` when
this feature is compiled in, including with `commercial-staging`; offline
builds report `false`. Current staging, readiness, Swift candidate and trusted
release-build checks require the literal boolean `false`, and existing binary
recipes reject optional-network dependency inventories. This evidence creates
no new staging or publication recipe.

Collection used locked, offline Cargo metadata for `x86_64-pc-windows-msvc` with
`--features flightsim-app/region-downloads`, followed by the existing
`scripts/collect-dependency-notices.py` with root package `flightsim-app`.
It records 408 normal/build packages and 786 exact notice files. The default
inventory has 359 packages. This is a conservative workspace-feature-unified
metadata closure, not proof that every recorded feature or byte enters the final
executable. In particular, other workspace consumers can contribute ureq features;
the transport is tested with gzip enabled and guards encoded responses below its
decoder. No new dependency version was added to Cargo.lock by the app feature.

The inventory SHA-256 is
`a39ef9b7a4d2cba6fb3c8338fe32612bd1a50707ae56dfc3470879a3f3997e46`.
Its notices are stored separately so neither feature set supplies an incomplete
or mixed notice directory to the strict stager. A source-only readiness check
finds zero integrity blockers and six existing review blockers. `review_status`
remains `not_reviewed`; the same four unresolved entries remain: constgebra,
hexf-parse, Bevy AgX and Blender Filmic LUTs. Collection does not resolve legal
terms, select license alternatives, inspect native linkage or authorize publishing.

Before distributing an executable with this feature, recollect and review its
exact target/features, preserve the original notice bytes, qualify its real
platform behavior and obtain inventory-bound publication authorization. This
inventory does not expand the current Swift-only Windows acceptance recipe or
binary packaging allowlists. The separate GitHub acquisition QA is technical
transport evidence, not a publisher-trust or dataset redistribution grant.

Recollection after the exact profile-v2 loader changed only the Cargo.lock and
metadata bindings. All 408 package records, features, notice bytes, embedded
assets and unresolved records remained identical. `serde_json/raw_value` was
already present in this feature closure; no `float_roundtrip` feature is enabled.
