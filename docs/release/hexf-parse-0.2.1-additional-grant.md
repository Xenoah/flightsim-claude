# hexf-parse 0.2.1 additional-grant evidence

Reconstructed and re-fetched 2026-10-08 UTC against application source
`dca2dc86b317766096af01cc00e334efa8c8db0a` after local executor loss.
Status: **evidence collected; substantive review pending**. This is not a
completed dependency review, legal opinion, binary publication receipt, or
permission to distribute the application. No release gate or license declaration
is changed. This reconstruction does not claim the previous local patch identity.

## Exact published package

The official crates.io archive was downloaded again and verified as SHA-256
`dfa686283ad6dd069f105e5ab091b04c62850d3e4cf5d67debad1933f55023df`,
matching the committed inventory's package checksum. Its four files are preserved
without modification in the [package snapshot](licenses/review-evidence/hexf-parse-0.2.1/package/):
`.cargo_vcs_info.json`, `Cargo.toml`, `Cargo.toml.orig`, and `src/lib.rs`.
There is no packaged primary license text. Both manifests retain **CC0-1.0**.

The VCS record pins source revision
[`4225763d744183d720f575ae96d04161b4d08ea0`](https://github.com/lifthrasiir/hexf/commit/4225763d744183d720f575ae96d04161b4d08ea0).
The 19,527-byte parser has SHA-256
`511b164642efe470e88323b8afe0274e797805ff2441f0173b5b005ad4b4d1b1`
and Git blob SHA-1 `60439bb5beadd6823330d0293cc3d74e2790171f`.
These identities are verified from the newly readable bytes, not assumed from
the prior report. `Cargo.toml.orig` matches upstream `parse/Cargo.toml`, blob
`2833f557b25322ab20cc78254e057f132c6e6a0e`.

## Additional-grant evidence

The [path history at the package revision](licenses/review-evidence/hexf-parse-0.2.1/upstream/source-history.json)
contains six commits by four GitHub-linked authors. Each author gave affirmative
consent to 0BSD relicensing of their contributions in
[upstream issue 26](https://github.com/lifthrasiir/hexf/issues/26).

| Original-source author | Original-source commits | Consent source |
|---|---|---|
| lifthrasiir | `1208bc2`, `aca384a` | [1829549155](https://github.com/lifthrasiir/hexf/issues/26#issuecomment-1829549155) |
| pchickey | `fd81897` | [1830149852](https://github.com/lifthrasiir/hexf/issues/26#issuecomment-1830149852) |
| sunfishcode | `1461bff` | [1830227783](https://github.com/lifthrasiir/hexf/issues/26#issuecomment-1830227783) |
| youknowone | `e39c009`, `ee1ad2d` | [1829687101](https://github.com/lifthrasiir/hexf/issues/26#issuecomment-1829687101) |

The [generated consent summary](licenses/review-evidence/hexf-parse-0.2.1/upstream/consent-summary.json)
contains author identities, comment IDs, timestamps, source URLs and paraphrased
observations. It quotes zero discussion words and is labeled as a summary, not
a raw API snapshot. Full discussion remains only in local validation evidence;
the public summary records retrieval fingerprints. These are the authors in
the fetched history of the packaged Rust file, not an independent certification
of all legal rightsholders or of contributor authority.

The maintainer later [changed both crate manifests to 0BSD](https://github.com/lifthrasiir/hexf/commit/8a14eb63c3823b0ed5a04a4a200a03ccde648a0d),
referencing issue 26. The complete [0BSD license text](https://github.com/lifthrasiir/hexf/blob/41f0018229c1ee3d6fd813b6808d1ad1f506554c/LICENSE)
is preserved from revision `41f0018229c1ee3d6fd813b6808d1ad1f506554c`:
607 bytes, Git blob `5aec25813c410539140a5c36d15b6c5178a865d3`.
Its tree records `parse/LICENSE` as a symlink to `../LICENSE`; the dossier
contains no filesystem symlink.

**The later licensed parser is not identical to 0.2.1.** Its blob is
`242acaf93b20f511b1383b2595584cc859371414`, 19,528 bytes. One expression changes
from `mantissa * (2.0 as $f).powf(exponent as $f)` to
`mantissa * libm::exp2(exponent as f64) as $f`. Both complete source files are
preserved. The proposed historical-byte rationale rests on the original
authors' contribution-wide consents, not on treating the later licensed tree
as the published 0.2.1 archive.

## Existing gate and remaining review

The unchanged collector's missing-primary-text record remains correct. Its
supplement mechanism pins the exact Cargo source revision. A later license
cannot truthfully be recorded as present in the earlier package revision,
and the archive's CC0 SPDX declaration is not changed to 0BSD.

The existing readiness check supports evidence-backed resolutions within a
genuine inventory-bound dependency review. A qualified reviewer must assess
whether the grants cover these historical contributions, whether provenance
and authority are sufficient, and which route and notice obligations apply.
If accepted, that review can identify `hexf-parse@0.2.1`, give its reasoning and
reference these hashed sources. The checker validates integrity, not judgment.
Do not create a `status: reviewed` record solely for this package.

Whole-target dependency, embedded-asset and native/Rust runtime review remains
separate. constgebra, AgX, Filmic, Light Single and publication authorization
remain unresolved. Generic Apache text alone is not treated as a constgebra
upstream grant.

## Packaging and verification

The [evidence index](licenses/review-evidence/hexf-parse-0.2.1/evidence-index.json)
records 15 preserved inputs, source URLs, byte counts, SHA-256, applicable Git
blob identities and the unchanged target-inventory digest. The discussion
record is a generated factual summary. Other GitHub API JSON is preserved as
returned by the connected tool; no signed transport-format claim is made.

Keep pending evidence outside the live `dependency-evidence` directory, whose
stager rejects unreferenced extra files. At genuine review time, copy only
explicitly referenced evidence into the inventory's `licenses/` subtree, with
relative `path`, `sha256` and authoritative `source` for every copied file.
If copying the index, calculate and reference its hash too. Rebind the review
to the final exact inventory. Do not copy the archive wholesale or invent an
approver. Existing inventory, supplement, SPDX, feature and payload records
are unchanged; all nine release blockers remain applicable.

Relevant tests are `test_commercial_readiness.py`,
`test_release_authorization.py`, `test_stage_commercial_candidate.py` and
`test_release_workflow.py`; run each with
`python3 -m unittest discover -s scripts/tests -p NAME -v`.
Inspect `python3 scripts/check-release-authorization.py --allow-blocked`:
exit 0 in that mode is not authorization. This evidence collection does not
attest an executable build, Windows runtime or completed dependency review.
