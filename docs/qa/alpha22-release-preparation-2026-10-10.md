# alpha.22 source and notice preparation — 2026-10-10

This is release preparation, not publication or new Windows qualification.
The terrain implementation is the reviewed public commit
`3574bf175c66851df51ae89903e04a13f4a172e6`, tree
`7ac0cedd0963ac4133fa8c5251f57f26037324d7`, equivalent to the historical local
candidate `0bc4a4961c677a5173350630bf2641a2c1b40b94`.
Its [full bounded QA report](terrain-night-moving-2026-10-09.md) is copied
byte-for-byte, including ordinary-tile color residuals, one existing skyline
subsample and numerical-reference limitations. Issue #6 remains open.

## Prepared changes and actual evidence

- Workspace version advances to alpha.22 through a two-file exact source
  migration; the prior runtime map, migration manifests and whole source
  contracts are preserved as historical witnesses
- The [collector ordering correction](dependency-notice-ordering-2026-10-10.md)
  gives source-notice discovery explicit case-sensitive POSIX-relative order.
  It changes neither comparison gates nor previously collected evidence
- Actual Rust 1.93.0 metadata/tree commands ran on Linux for the ordinary default
  `x86_64-pc-windows-msvc` target. This is target-filtered source evidence, not
  cross-compilation or native Windows execution
- Both old and changed collectors produced the same 737-file original output
  tree on that host. The original and regenerated inventories are each 407,610
  bytes, SHA-256 `9028d70de8e818f2e72666a2686b198424cf4f78986b47c5d5683091f1949730`
- The 359-package closure preserves every registry identity, declared licence,
  checksum, feature set and enabled embedded asset. Eleven used project packages
  receive alpha.22 identities; the other two workspace packages are outside this
  application's conservative normal/build closure
- The 1,264 full normal/build edge records match the genuine historical native
  baseline exactly after only those project-version substitutions. All 510
  source-header records were revalidated against the actual current package
  sources and remain identical
- The updated project-authored bevy_pbr modification notice is collected as its
  actual 1,295 bytes. Its current vendor tree, replacement patch and notice
  resolution are explicitly reviewed. zune-jpeg's source stays unchanged; its
  binding to the shared provenance manifest updates
- The previous 557 reviewed supplemental notice files are copied unchanged into
  a fresh 1,294-file assembly. The raw collector remains `not_reviewed` with both
  original constgebra/hexf missing-primary-notice observations; their existing
  exact-package resolutions remain separate substantive review inputs

The committed dependency review binds this actual inventory and assembly. Its
`source_sha`, `source_tree`, `native_evidence`, `environment_evidence`,
`component_conditions` and `runtime_observation` retain their historical values.
The new `source_content_amendment` labels current source/metadata evidence
separately. In particular, source550's historical failed runtime is not changed
to success or reassigned to alpha.22. Coverage means the bounded source/notice
licensing assessment; future platform facts still require actual matching review.

The approved English/Japanese Microsoft terms, component notice, main/startup,
component-terms Rust module and PowerShell dialog are byte-for-byte unchanged.
The existing complete component disclosure remains an exact suffix of the
release-note template. The new preface does not assert recipient assent.

## Deliberately open release gates

`full-review-applicability.json` and `release-authorization.json` are unchanged
historical alpha.21 records. They are intentionally stale for this preparation.
No matching alpha.22 source/native condition map or publication receipt has been
invented. A successful source/notice preflight still reports BUNDLE_NOT_CHECKED.

The publication owner must first bind genuine current authorization if using the
ordinary workflow to collect the changed-source native packet. The stale
applicability record stops archive publication; the existing failure-only native
export may retain only successfully revalidated bounded facts. Review that actual
packet, reconcile any observed platform/content changes and bind a truthful new
applicability basis. Rebind final source authorization last, then perform a fresh
exact-final-source run through the unchanged same-build native, archive,
extracted Light Single/Swift smoke and remote source/tag/asset checks.

The earlier alpha.21 executable, captures and native packets remain their own
historical evidence. The candidate's Linux moving-terrain captures are not
repeated or relabeled as alpha.22 Windows captures. No release is complete until
its own source-bound artifacts and public assets are verified.
