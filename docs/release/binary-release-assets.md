# Binary release asset policy

The Windows two-aircraft release and the local Swift-only commercial candidate
have separate asset lists in [asset-rights-manifest.json](asset-rights-manifest.json).
Both refer to the same exact SHA-256 and reviewed-state records. List membership
does not create a grant or authorize publication.

- `commercial_external_assets` remains the required Swift Sport GLB/JSON pair.
  The commercial checker requires both files and rejects other external assets;
  the stager's explicit source list is unchanged.
- `release_external_assets` covers the already-recorded original Light Single
  JSON and Swift Sport GLB, JSON and editable Blender source. The release gate
  verifies each shipped asset's record, reviewed state and exact bytes. The list
  controls eligibility, not the release's required payload.

The Light Single JSON is project-authored configuration, distinct from the
unresolved Meshy model it references. Its dynamics contract is checked by
`crates/flightsim-fdm/tests/bundled_definitions.rs`. Swift's original procedural
source, generation process and exact model/scene hashes are recorded in
[aircraft asset QA](../qa/aircraft-assets-2026-10-01.md) and
[ATTRIBUTION](../../ATTRIBUTION.md). Its GLB, Blender scene and generator are
unchanged from source commit `8e16bf7514252b695d1210cd8b56045bf302eb96`.
These existing records support the original assets' release-list coverage;
their grant states and recorded hashes are unchanged.

The release recipe still requires **both aircraft and Swift's Blender source**,
and the extracted Windows smoke still exercises Light Single cockpit and Swift
Sport chase. `light_single.glb` remains unresolved and absent from both lists.
Its original or renamed bytes remain blocked anywhere in the release copy plan,
including under another approved asset or a dependency-notice path. A new
authorization receipt cannot override these rights checks. See
[the rights audit](commercial-distribution-audit.md#1-legacy-meshy-model-exclude-or-establish-its-actual-grant)
for the missing generation-specific evidence.

The release preflight also inspects a committed dependency inventory while its
review is missing. This reports actual unresolved package/embedded-asset evidence
instead of describing a present inventory as missing. Target, features, notice
integrity and source-manifest bindings are still checked. Untracked inventory or
review files cannot supply evidence; missing review and publication authorization
remain blockers, and no copy plan is written while any blocker remains.

The existing default and Swift-only binary recipes remain offline. Distribution
metadata reports a separate compile-time `region_downloads` boolean; it is not
implied by the aircraft profile. Immediately after the trusted release build,
`check-release-authorization.py --verify-built-executable PATH` rechecks release
authorization, executes the bounded `--distribution-info` handshake, requires
literal `region_downloads: false` and the exact default Windows MSVC identity and
version, and reports the binary SHA-256 with that metadata. Use this explicit
execution mode only for the trusted freshly built application. Generic
`--bundle` verification remains non-executing and compares staged/extracted bytes
against the build output and exact allowlisted copy plan. Source/feature-bound
authorization and unresolved rights checks still apply; these checks do not
authorize a region-download binary recipe.

Asset-list changes alter the manifest digest. Collect dependency evidence against
the final manifest, and obtain inventory-bound review/publication decisions only
after the real outstanding conditions are met. This policy correction is not
current Windows runtime proof or commercial clearance.
