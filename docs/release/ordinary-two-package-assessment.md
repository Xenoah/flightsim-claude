# Ordinary-tone native candidate: two package license assessments

Prepared 2026-10-08 against source
`d918943a70d01644b5a6bebb8a903a5bbc89139f` and actual Windows MSVC
[run 37830779995](https://github.com/Xenoah/flightsim-claude/actions/runs/37830779995),
artifact `11575502730`. The [assessment](ordinary-two-package-assessment.json)
is package-scoped. The [independent review](licenses/review-evidence/two-package-ordinary/independent-review/REVIEW.md)
accepted both routes and this exact ordinary-native rebind with their stated
distribution conditions. All 42 packet checks passed.
The existing gate remains blocked and no source, collector, review schema,
license declaration, release recipe, variant choice or authorization is changed.

Here, ordinary means the existing tonemapping LUT feature is enabled. This
particular native candidate uses `commercial-staging` and contains Swift only.
It is neither approval of that reduced deliverable nor clearance for Light
Single or the ordinary two-aircraft release.

## Reuse of the substantive prior assessment

The [preserved prior record](licenses/review-evidence/two-package-ordinary/prior-package-assessment.json)
already includes independent acceptance of both narrow package assessments,
their notice plan and 13 hashed evidence inputs. Its analytical inventory and
source identity are historical and have not been relabeled as native evidence.
The new [reconciliation](licenses/review-evidence/two-package-ordinary/reconciliation.json)
binds those unchanged package bytes to the fresh native inventory:

- `constgebra 0.1.4`: archive SHA-256
  `e1aaf9b65849a68662ac6c0810c8893a765c960b907dd7cfab9c4a50bf764fbc`,
  revision `eae8e094e5779f42f1db2a7adf9036aa33744bc8`
- `hexf-parse 0.2.1`: archive SHA-256
  `dfa686283ad6dd069f105e5ab091b04c62850d3e4cf5d67debad1933f55023df`,
  revision `4225763d744183d720f575ae96d04161b4d08ea0`

Both complete native package records, including empty feature sets, equal
those previously assessed. All nine constgebra archive files and all four hexf
archive files match the preserved snapshots. The 13 prior evidence-file hashes
also match. Fresh primary-source reads reproduce both pinned manifests and the
Creative Commons text exactly. The native inventory's original bytes and all
359 package records are preserved; its SHA-256 remains
`02797865cac5ecee5f05c33ea1a682b2bd772c8f6de8cdfea6ba7d3d91ef11d8`.

## constgebra: the declared Apache alternative

The exact archive's manifest and matching
[pinned upstream manifest](https://github.com/knickish/constgebra/blob/eae8e094e5779f42f1db2a7adf9036aa33744bc8/Cargo.toml)
associate `MIT OR Apache-2.0` with this package. Cargo's
[manifest specification](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields)
identifies this field as the package's license and defines OR as a choice.
The package association is primary upstream evidence, not a bare SPDX label
from a third-party database. Exact archive, source and history checks supply
the byte/provenance boundary; the stated limitations remain below.

The assessed distribution route selects Apache-2.0 and supplies the complete,
unchanged [ASF legal instrument](https://www.apache.org/licenses/LICENSE-2.0.txt).
This text is distributor-supplied and is not described as an upstream-packaged
license. Preserve relevant source notices and the original README attribution,
identify future modifications and retain applicable NOTICE content if present.
No root license or NOTICE file exists in this exact package or pinned tree;
no holder/year is invented. The README's floating-point origin through
`const_soft_float` and its compiler-builtins/libm lineage still requires its
own substantive dependency review. This decision does not clear that scope.

## hexf: the original CC0 route

The checksum-bound manifest and matching
[upstream manifest](https://github.com/lifthrasiir/hexf/blob/4225763d744183d720f575ae96d04161b4d08ea0/parse/Cargo.toml)
associate CC0-1.0 with the exact published parser. Preserve that declaration.
The complete [Creative Commons instrument](https://creativecommons.org/publicdomain/zero/1.0/legalcode.en)
is supplied from Creative Commons' own pinned
[COPYING file](https://github.com/creativecommons/cc-legal-tools-data/blob/19d5489b40a726e6c5bfb9cab15d03e7853ba435/COPYING),
with package/source identification. It is distributor-supplied; it was not
present in the historical crate. Creative Commons confirms that
[CC0 can be applied to software](https://wiki.creativecommons.org/wiki/CC0_FAQ).

The copyright/related-rights waiver and fallback license cover commercial use
within their terms. The fallback is non-transferable and non-sublicensable:
each recipient relies on the affirmer's direct fallback, not a new grant made
by this distributor. CC0 imposes no attribution or license-copy condition;
shipping the complete text is this project's documentation practice. Patent
and trademark rights are expressly unaffected, and CC0 does not warrant title
or clear other persons' rights. This package assessment makes no such claim.

The existing [four-author 0BSD dossier](hexf-parse-0.2.1-additional-grant.md)
remains corroborating context. This selected original-CC0 route does not depend
on deciding the reach of that later grant. The later parser is not identical
to 0.2.1: its exponent expression changed from `powf` to `libm::exp2`.

## Assembly boundary and remaining gate

The assessment's `evidence_copy_plan` is a complete, portable list of the 13
required source paths, destination paths and hashes. It retains the existing
resolution/evidence shape for eventual incorporation into a genuine completed
whole-target review. Copy each original file, with matching hash, beneath the
final inventory's `licenses/` subtree and reference every copied file. Preserve
the inventory byte-for-byte; do not erase either original unresolved record.
Reconcile any future target inventory and rebind that final review's exact hash.

The ordinary inventory still correctly records missing primary package texts.
The revision-pinned collector supplement mechanism is specifically for texts
published upstream at those revisions. Inserting distributor-supplied standard
texts there would misstate their origin. The readiness checker consumes package
resolutions only within a complete inventory-bound `status: reviewed` record.
This two-package assessment must never be renamed or relabeled to meet that
condition. Integrity checks verify bytes; they do not supply the missing review.

Filmic uses a separate, already-existing embedded-asset state and notice path.
Its scoped asset decision can be expressed there without asserting a completed
dependency review. That mechanism does not provide a package-level acceptance
path for constgebra or hexf. No new path is introduced by this evidence change.

AgX, complete dependency/source/runtime obligations, final native/bundle and
appearance acceptance, source archives, deliverable choice and publication
authorization remain separate. Existing four candidate readiness blockers
remain: whole dependency review, AgX, constgebra and hexf. These package
assessments are concrete inputs to completing that review, not a gate bypass,
human legal opinion, title warranty, or executable/runtime attestation.
