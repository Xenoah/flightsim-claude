# Ordinary release payload projection (preparation only)

`scripts/project-ordinary-release-payload.py` is a read-only factual adapter for
the ordinary two-aircraft copy plan. It does not activate a workflow or replace
the ordinary authorization gate, analytical external-review contract, full
native projector, substantive review, or final extracted-archive smoke.

The implementation is prepared against source commit
`549b53a4359b2a7d1ffee951ed2249285c29a485`, tree
`0d5d79165ec3b018ad8cc9fdd9a29ebf851c6788`. No genuine native execution or
completed whole-target review is supplied by these changes. All new positive
test inputs are temporary, explicitly synthetic fixtures.

## API and actual authorities

`compare_inventories(original, captured)` takes two absolute paths to original
collector JSON files. It preserves their byte-level SHA-256/length identities
and both `metadata_sha256` values. It rejects every non-metadata semantic field
difference, including unknown or missing top-level fields. It compares complete
values, including array ordering and JSON types, rather than package counts.
Duplicate keys, non-finite numbers, changed collector identity, oversized JSON,
symlinks and hard links fail. It does not write or normalize either inventory.

Its comparison has three factual outcomes:

- `raw_bytes_equal`: both original byte identities match
- `metadata_digest_changed_requires_applicability_review`: every other field
  matches, but the raw metadata digests differ; the cause is `not_established`
- `encoding_changed_requires_applicability_review`: parsed fields match but
  original bytes differ; this also cannot be treated as byte equality

All outcomes retain `review_applicability_approved: false`. Even equal collector
inventories do not prove equivalence of audited native source, linkage, runtime
origins, notice completeness, or review applicability. A metadata difference
does not establish that only host paths changed.

`project_payload(repo, expected, build_private, build_text, bundle)` requires
canonical absolute, pairwise disjoint input roots and an exact source SHA:

1. The unchanged `check-release-authorization.inspect(repo)` must authorize the
   current committed source, completed existing review and ordinary copy plan.
   No caller-supplied plan, fixture receipt, or bypass parameter is accepted.
2. The unchanged `capture-analytical-swift-msvc.validate_export(build_text,
   repo=repo, expected=expected, private=build_private)` must validate the actual
   completed current native capture. Its source recipe must be the admitted
   two-LUT recipe, and its ordinary side must contain exactly two LUT payloads.
   The exact raw summary is hashed before validation, must equal the canonical
   validated result, and must remain unchanged afterward. The validator retains
   authority over command journals, captured source,
   canonical source files, audit, compiler/build evidence and frozen trees.
3. The executable comes only from
   `build_private/target-ordinary/x86_64-pc-windows-msvc/release/flightsim-app.exe`.
   Its hash/length and the actual captured inventory/metadata must equal the
   native validator's records. The fresh inventory's metadata digest must equal
   the exact captured metadata digest.
4. The committed original inventory and fresh capture are compared without
   rewriting either. The existing notice collector/stager contract validates
   both closed notice trees, and every captured original notice is compared
   byte-for-byte with the shipped original tree. Review supplements remain
   explicitly referenced by the ordinary review.
5. The unchanged `verify_bundle(bundle, plan, executable)` verifies the ordinary
   payload. Each payload snapshot must independently equal the plan's member
   records plus the audited executable, not only match an earlier snapshot.
   Matched notice records must also equal the plan's original notice records.
   The adapter additionally rejects Windows path aliases, case
   collisions, symlinks/reparse points, hard links, special files, unexplained
   directories and unbounded trees. The payload includes the original committed
   inventory and review. Diagnostic files required by P's different candidate
   bundle are not added to the ordinary plan.
6. Source, ordinary gate/plan, shipped files, exact capture inputs and captured
   notices are checked again before returning. The adapter runs no compiler,
   linker, executable, upload or other external write.

The CLI accepts `--repo`, `--expected-sha`, `--build-private`, `--build-text`,
and `--bundle`, and writes the factual JSON to stdout. Exit zero means only that
the described projection checks completed. It must never be used as the final
publication condition. Failed input validation exits nonzero.

## Sidecar schema

Schema 1 has kind `ordinary_release_payload_projection_not_acceptance` and
status `facts_projected`. It records actual source SHA/tree/recipe, version and
build recipe, ordinary source/copy-plan digests, authorization receipt digest,
build-summary/executable/metadata/review bindings, both inventory identities,
individual matched original notices and the exact shipped member hashes/lengths.
Hashes of metadata, logs and engineering inputs are ordinary opaque evidence;
this adapter does not redact or reinterpret their meaning.

`release_authorized`, `dependency_review_approved`,
`review_applicability_approved` and `runtime_accepted` are always false: the
adapter grants none of them. `archive_and_extracted_smoke_binding` and
`whole_target_native_review_applicability` remain `not_evaluated`. A pre-existing
ordinary receipt's digest is recorded as an input binding, not reissued by this
sidecar. All output data belongs outside the shipped copy-plan payload.

## Remaining integration boundary

After genuine original full native facts and a substantive content review exist,
an independently reviewed final adapter must enforce that review's explicit
source/native/runtime applicability conditions, including any permitted raw
metadata difference. It must project full fresh native/runtime facts from the
same ordinary build and bind the final archive and observations from its fresh
extraction. The workflow must preserve both original inventory identities and
use the captured ordinary executable without rebuilding it. This preparation
does not define those missing review conditions or claim that P's diagnostic
archive establishes final ordinary release facts.

Tests run the unchanged ordinary gate and copy-plan verifier against temporary
synthetic repositories; only the native capture/source-evidence boundary is
mocked. They establish local adapter behavior, not native Windows acceptance.
