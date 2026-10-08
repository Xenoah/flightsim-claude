# Independent review of ordinary-build two-package evidence

Review date: 2026-10-08 (UTC). Reviewer: independent OpenAI engineering source/license-evidence reviewer. This is an evidence assessment, not a lawyer's opinion or publisher authorization.

## Result

Accept both narrow package assessments and their ordinary-build rebinding with the stated distribution conditions. All 42 checks of the updated candidate and portable assembly inputs passed. Neither this report nor the original assessment constitutes a whole-inventory `reviewed` record.

The original accepted assessment is byte-identical to its archived copy. All 13 referenced evidence inputs, all 13 files across the two exact registry archives, and all 9 upstream-authored Git blobs were independently verified. The recovered registry source copies also match every archive member. Reproducible checks and their results are in `verify_original.py` and `original-verification.json`.

The ordinary inventory is bound to SHA-256 `02797865cac5ecee5f05c33ea1a682b2bd772c8f6de8cdfea6ba7d3d91ef11d8`, with source commit `d918943a70d01644b5a6bebb8a903a5bbc89139f`. It is a conservative normal/build dependency closure collected for `x86_64-pc-windows-msvc`, not proof of final linkage or exhaustive nested-source licensing. Its recorded status remains `not_reviewed`; its unresolved records still name both packages and AgX.

## constgebra 0.1.4

Accept the narrow Apache-2.0 route for archive SHA-256 `e1aaf9b65849a68662ac6c0810c8893a765c960b907dd7cfab9c4a50bf764fbc`, subject to the stated distribution conditions.

The package's original manifest and the independently fetched [pinned upstream manifest](https://github.com/knickish/constgebra/blob/eae8e094e5779f42f1db2a7adf9036aa33744bc8/Cargo.toml) declare `MIT OR Apache-2.0` for constgebra itself. This is a package-specific upstream declaration tied to actual archive bytes, rather than an unrelated SPDX label or an unattached generic license. The complete [pinned tree](https://github.com/knickish/constgebra/tree/eae8e094e5779f42f1db2a7adf9036aa33744bc8) has no LICENSE or NOTICE file, consistently with the nine-file archive.

Fresh path histories for Cargo.toml, README.md, and all three Rust source files reach the initial commit and identify knickish and elpiel. The [initial manifest](https://github.com/knickish/constgebra/blob/9a3971e03bd56b386212eb4f0bfd2f585213152b/Cargo.toml) already declares the same alternatives. The manifest at [elpiel's included contribution](https://github.com/knickish/constgebra/commit/72bfaa010a29bc5039f0590aec618b45a8e24faa) still declares them; the contribution changes comptime.rs and contains no conflicting license notice. This supports the narrow declaration-based assessment, while not certifying contributor title or undisclosed third-party rights. Fresh fact records and primary text are retained in this report directory.

The distribution must contain the complete unchanged [Apache 2.0 text](https://www.apache.org/licenses/LICENSE-2.0.txt), identified as distributor-supplied standard text, plus the package identity and primary declaration. Preserve applicable source notices and the README's floating-point origin statement. Modified files need change notices. No NOTICE, copyright name, or year should be invented. Apache's limited contributor patent grant, patent-litigation termination, trademark exclusions, and warranty/liability terms remain applicable.

The `const_soft_float` dependency is separately present as version 0.1.4 in this ordinary inventory. Its compiler_builtins/libm lineage and any nested origin obligations remain separate review work; constgebra's Apache route does not discharge those obligations or Rust/native-runtime obligations.

## hexf-parse 0.2.1

Accept the original CC0-1.0 route for archive SHA-256 `dfa686283ad6dd069f105e5ab091b04c62850d3e4cf5d67debad1933f55023df`, with the stated limits.

The exact original and normalized package manifests and independently fetched [pinned parse manifest](https://github.com/lifthrasiir/hexf/blob/4225763d744183d720f575ae96d04161b4d08ea0/parse/Cargo.toml) associate CC0-1.0 with this package. The parser and manifest Git blobs match the archive. The pinned parse subtree and four-file archive contain no primary license file. Fresh source/manifest histories identify lifthrasiir, pchickey, sunfishcode, and youknowone. The original manifest and the inspected manifests at each contributor's source commit retain CC0. This supports the original CC0 association without asserting blanket contributor authority or non-infringement.

The [official pinned CC0 text](https://github.com/creativecommons/cc-legal-tools-data/blob/19d5489b40a726e6c5bfb9cab15d03e7853ba435/COPYING) freshly fetched for this review is byte-identical to the proposed `CC0-1.0.txt` (SHA-256 `a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499`). Its [current official legal-code presentation](https://creativecommons.org/publicdomain/zero/1.0/legalcode.en) corroborates the terms.

Keep the entire text and original CC0 metadata. CC0 provides a waiver, subject to applicable law, and a fallback for each affected recipient directly from the affirmer. The fallback is non-transferable and non-sublicensable. It applies to the affirmer's copyright and related rights; no affirmer patent or trademark rights are granted or waived. The work is provided as-is without title, merchantability, fitness, accuracy, defect, or non-infringement warranties; responsibility for clearing others' rights and obtaining permissions is disclaimed. Creative Commons is not a party and assumes no duty. Supplying attribution and a license copy is this project's traceability practice, not a CC0 attribution condition.

The later 0BSD dossier is corroborating context only. Its consent summary remains labeled as generated, and the selected route does not depend on interpreting or back-applying it. The later parser has a powf-to-libm::exp2 change; its source must not replace or be represented as the exact 0.2.1 artifact.

## Gate and scope boundaries

The current readiness gate consumes resolution records only when a genuine complete review is bound to the inventory hash with `status: reviewed`. Package-scope acceptance is not that record. Do not change the original inventory, erase its unresolved entries, change gate logic, fabricate whole-target review status, or relabel the hexf archive. Attach these resolutions and their exact evidence only as part of eventual complete review.

Remaining whole-target matters include AgX and other package, embedded-asset, platform, runtime, and distribution obligations. This reviewer made no repository, branch, gate, external-state, release, or legal-agreement changes. Only the assigned independent-review directory was written.

## Final candidate and portable inputs

Reviewed assessment SHA-256: `281b45591a9f3aea2f0508be42c73ce425a149a4815c51110e4b6944c98d66c8`. At review time its status honestly remained `package_scope_assessed_pending_independent_review`. The parent may finalize that field as `package_scope_reviewed`, record this bounded acceptance and report references, and refresh the corresponding explanatory prose and portable copy. This does not permit changing the selected routes, obligations, evidence hashes, target binding, or scope boundaries.

`candidate-verification.json` records all 42 checks. The candidate's base is the expected source commit and its tracked files are unchanged; only the new documentary/evidence inputs are present. All original obligations and routes are preserved. The native inventory is byte-identical to its exported source; all 359 package records and unresolved entries remain intact. Every one of the 13 copied evidence files is unchanged, exists under its specified portable `source_file`, and is referenced by a resolution. All 13 complete-archive reconciliation entries and the three main metadata references match their hashes and sizes. The portable assessment matches the source assessment.

The native scope wording is accurate: ordinary tonemapping LUTs are enabled, but the tested candidate uses `commercial-staging` and Swift only. This review does not select that deliverable or authorize a two-aircraft release. The portable directory explicitly contains assembly inputs rather than a complete notice tree and cannot be staged alone.

The unmodified readiness and staging code confirms that a package-scope record cannot activate these resolutions or bypass the genuine complete review requirement. The parent's saved preflight likewise remains blocked, including AgX and both original package unresolved records. Its reconstructed notice directory is labeled as reconstruction, not a captured native notice directory. That notice-integrity preflight is not runtime or whole-target acceptance.

No substantive issue remains in the two-package evidence packet. The small distributor README and pending-review wording corrections identified during review were fixed before this acceptance. Source evidence is in `fresh-primary-records.json` and `fresh-per-file-histories.json`; neither contains full third-party discussions.
