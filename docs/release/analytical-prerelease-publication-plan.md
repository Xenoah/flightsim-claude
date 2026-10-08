# Proposed Swift/Reinhard prerelease publication path

Status: **bounded design for separate review, not implemented publication and
not authorization**. The user has not yet chosen the reduced Swift-only variant.
No positive receipt, tag, release, artifact upload or source-only substitute is
created here. The ordinary Release workflow remains the two-aircraft/default-LUT
recipe and cannot publish this variant by changing a flag or borrowing a receipt.

## Exact variant and version identity

Use a separate identity, `swift-reinhard-windows-prerelease-v1`. Its app recipe is
the exact analytical/commercial recipe already audited: Rust 1.93.0, native x64
MSVC, release, jobs 2, locked/offline, warnings denied, incremental off, defaults
disabled, exactly `analytic-tonemapping,commercial-staging`, region downloads off.
Its contents are original Swift Sport only, Reinhard, embedded coarse global
terrain/monthly climate, and the complete reviewed source/runtime notices.

The title must say “Swift-only / Reinhard Windows prerelease”. Release notes must
explicitly state the excluded Light Single model and other aircraft, darker
changed highlights, coarse/global and monthly-climate limits, offline behavior,
and any remaining physical GPU/controller/audio limitations. No two-aircraft,
Steam, certified flight or hardware qualification claim is made.

Keep the program's existing exact `package_version` from Cargo metadata and the
extracted binary handshake. A distinct tag identifies this feature variant,
without silently changing Cargo's version or taking the ordinary version tag:

- If package version already has a prerelease suffix, proposed tag rule is
  `v{package_version}-swift-reinhard.{variant_revision}`
- Otherwise it is `v{package_version}-swift-reinhard.{variant_revision}` as well,
  which introduces the required prerelease suffix
- `variant_revision` is a separately reviewed positive integer, initially 1
- With the current package version, the illustrative tag is
  `v0.6.0-alpha.21-swift-reinhard.1`; it is not selected or created

The public variant manifest must expose `package_version` and `release_tag` as
different fields. The application does not pretend its package version contains
the extra variant suffix. Set GitHub `prerelease: true`; never promote this
variant by changing that flag in an idempotent retry. Reject an existing conflicting
tag/release/asset rather than overwrite it. The ordinary `v{package_version}` tag
remains available for its separately eligible fuller release.

## Preconditions for a publication receipt

A new, separate checker/receipt schema must be independently implemented and
reviewed. It must require every following fact for the same final source and
archive. Engineering scripts always retain false authorization fields and cannot
emit this receipt:

1. The user's explicit reduced-variant choice and publication instruction are
   recorded with their exact bounded scope. Silence or the previous fuller-release
   instruction cannot supply the missing choice.
2. Exact final source commit/tree, clean canonical checkout, accepted source
   contracts/migrations, Cargo.lock, source CI and review identity all match.
   Any new path-patched dependency has the completed original-source assessment,
   exact modified-file/tree/patch identity and newly bound notice spans.
3. Actual native dual-build artifact audit proves analytical LUT exclusion with
   the ordinary positive control. Combined regressions/Clippy, four exact legacy
   tests and all compile guards passed on the same final source.
4. A genuine completed whole-inventory dependency/platform/runtime record binds
   the exact native inventory and source, all final supplemental notices, Rust
   runtime coverage, applicable Microsoft toolset/SDK provenance and conditions,
   fonts, shaders, terrain/climate and remaining origin/mark duties. Nothing
   substitutes two narrow package assessments for this review.
5. The sole unchanged stager has produced the final reviewed bundle. Its exact
   archive/extracted copy plan and packaged reviewer record passed readiness,
   all four strict runtime outcomes and eight fixed appearance captures. The
   verified archive hash and bytes are frozen after this recheck.
6. Actual final-bundle Windows interactive/appearance evidence has been reviewed,
   including the Reinhard tradeoff, map/camera/lifecycle behavior, HUD, narrow
   viewport/legacy disclosure and normal exit. Any accepted limitation must be
   explicit; missing evidence cannot become a passing boolean. Hardware and
   two-aircraft issues remain open where their own requirements are unmet.

The receipt must bind hashes of those real records, exact tag/title/asset names,
source, package version, feature variant, inventory, review, bundle manifest,
archive, executable, notices and user choice. It must identify its actual reviewer
and decision date. Do not generate a positive template and fill in invented
success later. A false/unset field keeps the publication path blocked.

## Exact public payload and separation of jobs

Proposed public assets are exactly:

- `flightsim-v{package_version}-swift-reinhard.{variant_revision}-windows-msvc.zip`
- `SHA256SUMS.txt`, binding the ZIP and the separate variant manifest
- `release-variant.json`, a bounded canonical metadata document without private
  paths, raw logs, metadata, conversations or credentials

The ZIP must be the exact byte-for-byte archive from the final reviewed-bundle
recheck, preserving the sole stager's `swift-candidate/` membership. It contains
the application, only the Swift GLB/profile external assets, unchanged required
project/data notices, original dependency inventory, every reviewed supplemental
notice, the completed dependency-review copy, distribution/readiness metadata
and bundle manifest. No Light GLB, renamed excluded payload, LUT, debug/runtime
installer, model-generator source, replay or unreviewed DLL can be added.

The static `commercial-staging` metadata and LOCAL-CANDIDATE marker do not grant
publication and must not be rewritten after qualification. Release notes and
the separate variant manifest explain that engineering staging markers describe
the production route; the separate genuine publication receipt supplies the
specific prerelease decision. A change to these generated bundle bytes would
need its own stager review and a new final archive/runtime recheck.

Use a separately reviewed `Analytical Swift prerelease` workflow, with no
`workflow_run` linkage that can accidentally reuse the ordinary release. The
qualification side has read-only repository permissions. Only a final publish
job receives `contents: write`; it does not check out or execute repository code.
It independently verifies the receipt and hashes of the exact frozen payload.

The authorization gate must run **before any upload of executable/archive bytes**,
including an intermediate Actions artifact. In a public repository such an
artifact may itself expose the binary. Until every precondition is satisfied,
only the separately reviewed bounded engineering text/Swift-PNG exports are
permitted. Do not cache or upload private build trees, raw metadata or notices.

At publication, verify the remote tag is absent or already points to the exact
source, create the specifically authorized prerelease and upload only the three
named assets, then verify the remote tag/source, prerelease flag, downloadable
asset membership/bytes/checksums and release notes. A retry may reuse identical
results; a mismatch is a blocker. Do not overwrite an existing conflicting tag
or silently replace released bytes.

## Remaining work

The final-notice assembler, same-build final runtime recheck, modified-source
projection and bounded Win32 observer are preparation tools. Native capability
and all actual final acceptance must still be established. This publication
checker/receipt/workflow is a design deliverable requiring separate implementation
and review, followed by the user's pending variant decision. No currently running
ordinary/build-only workflow can substitute for these steps.
