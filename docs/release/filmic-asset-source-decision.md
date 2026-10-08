# Filmic-only source-rights integration candidate

This change records one exact embedded asset's source-notice route. It does not
create `dependency-review.json`, change executable or LUT bytes, or authorize
publication. The [scoped decision](filmic-asset-source-decision.json) identifies
the original ordinary Windows inventory and the separately preserved,
independently checked reproduction evidence.

The existing `dependency_assets` schema can express this scope: only
`bevy-blender-filmic-lut` becomes `licensed_with_notices`, with the same Bevy
package, feature, path, size and SHA-256. The recorded BSD-3-Clause route applies
the pinned Blender configuration's license reference to its associated numeric
tables. That scope inference is explicit; individual table license headers and
Bevy's historical Blender version are not asserted. The exact decoded content
has been reproduced from those inputs without tolerance.

The full Sony BSD notice and Filmic attribution are listed in `notice_files`.
The unchanged collector copies both only when the LUT feature is selected. Its
inventory remains `review_status: not_reviewed`. The unchanged stager verifies
the hash of every referenced notice and copies it into the bundle. Global
`required_bundle_files` is unchanged, so an analytical build with the LUT
feature disabled does not acquire a new unused Filmic notice requirement.

The collector records observed notice hashes; it does not independently decide
whether the notices are legally sufficient. The bounded decision, independent
review, exact source identity and focused integrity tests provide the review
context. Final whole-target review must still bind the newly collected complete
inventory and actual notices. This source decision cannot substitute for it.

The preserved ordinary `fe7373e` inventory is immutable historical evidence,
SHA-256 `64635e2d9bd322de6a6282daf7a83f7de1e69bfcc11f1c7c92265021557a8087`.
It contains 359 packages and four embedded assets, including Filmic's original
unresolved record. The new manifest intentionally makes it stale for staging.
Neither its summary nor its manifest digest has been edited to manufacture a
new native inventory. Its exported directory also lacks the referenced notice
tree and cannot serve as a complete staged-bundle check.

After source adoption, collect a fresh inventory against the final candidate
source and real target/features. Review the exact resulting notice tree and
whole dependency/platform obligations before building a final frozen bundle.
AgX, Light Single, Microsoft/platform coverage, runtime/UI acceptance, user
variant choice and publication authorization remain independently required.

The supporting [evidence tree](licenses/review-evidence/bevy-blender-filmic-lut/)
contains the original 28-file text-only reproduction packet unchanged, plus
the original native inventory. Its original research reports correctly state
that they did not themselves clear an asset or release. The separate integration
review evaluates the present bounded manifest/notice decision. These are
different scopes; no historical receipt was rewritten.

## Admission and report integration still required

This rights-only source candidate is not admitted by the unchanged analytical
build contract: `scripts/analytical-swift-source-contract.json` still pins the
prior asset-manifest hash. Its own digest is fixed by the MSVC capture script
and capture contract. The actual admission check rejects this candidate with
`analytical source pin changed: docs/release/asset-rights-manifest.json`.
A separately reviewed refresh of those exact bindings is required before an
analytical run on a source containing this decision. No fallback or bypass is
introduced here, and passing general source/archive checks is not analytical
admission.

The unchanged ordinary candidate report also contains a conservative display
sentence describing both AgX and Filmic as unresolved. Correct that wording
before producing a fresh report from a source adopting this decision, keeping
its actual rights and publication checks unchanged. Historical reports remain
unaltered.

Validation uses `test_filmic_asset_integration.py` plus the existing commercial
readiness, release-authorization, stager and analytical-recipe tests. Synthetic
collector fixtures exercise conditional copying and corruption rejection; they
are not native execution or authorization evidence.
