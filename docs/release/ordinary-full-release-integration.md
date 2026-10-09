# Ordinary full-release integration: local preparation

This integrates the existing ordinary two-aircraft release copy plan with one
fresh dual MSVC capture. The ordinary captured executable, default features,
Rust 1.93.0, Windows x64 MSVC target, Light Single/Swift profiles and Tony/Filmic
source recipe are unchanged. No production review, applicability record or
publication receipt is supplied. Positive tests are synthetic fixtures only.

The ordinary payload adapter implementation was imported unchanged from reviewed
`b5de958` onto source `55094fa`. Its fixture setup, and the new native fixture,
canonicalize the temporary directory before invoking inherited setup, so Windows
short-name TEMP aliases do not violate production path checks. No inherited capture, runtime collector, source-admission,
analytical projector, ordinary authorization checker or runtime source is
relaxed. The full qualification policy remains a separate historical/current
qualification task; its diagnostic archive is not this ordinary release archive.

## One executable, one final archive

The updated `release.yml` preserves its successful exact-main-push CI gate,
read-only build job, inventory-bound source authorization, explicit artifact
paths and no-checkout publisher. Its Windows job now:

1. Rechecks the existing source/rights/publication gate before building
2. Retrieves the same checksum-pinned original Bevy crate privately
3. Invokes the unchanged dual capture once on the exact final source
4. Passes that capture to `prepare-ordinary-full-release.py`
5. Revalidates retained originals immediately before uploading the final files

The capture's `target-ordinary/x86_64-pc-windows-msvc/release/flightsim-app.exe`
is the only executable copied into the ordinary plan. There is no second compile.
The original offline distribution handshake precedes staging. The retained
ordinary `inspect()` and `verify_bundle()` functions remain authoritative for
rights, authorization and shipped payload. The job uses the production
`windows-latest` runner and a 360-minute job ceiling to accommodate
the existing up-to-300-minute dual capture plus bounded final processing. This
is a ceiling, not measured native duration or a changed runtime deadline.

`verify-ordinary-release-archive.py` creates a ZIP containing exactly the
ordinary top-level directory and copy-plan files plus the audited executable.
It checks local/central ZIP headers, complete compressed-stream consumption,
membership, names, lengths and hashes, then extracts once into a fresh directory
with per-file limits. Prefixes/trailers, duplicates, alias collisions, extra
fields/members, links, hidden compressed bytes and changed content fail.

Only the existing Light Single cockpit and Swift Sport chase selections run,
with synthetic traffic, five-second screenshot delay, software D3D12/WARP and
the existing 180-second deadline each. Both require complete decoded PNGs, all
existing fitted/load/completion markers, no fatal log and exit zero. Commands,
private streams, PNGs and the exact extracted member set are revalidated.
No broader UI/weather/map/switching sequence is introduced. A screenshot and
zero exit do not decide subjective appearance or physical-hardware acceptance.

## Actual source and native review applicability

`project-ordinary-native-evidence.py` composes the unchanged ordinary payload
adapter and existing low-level native/runtime parsers. It projects complete
ordinary package/source/features/graph/header/embedded/PE/platform facts from
the same validated capture, with both raw inventory identities. Its required
runtime collector is bound to that capture's compiler, trace and executable.
Unknown selected SDK, static membership, process execution or coverage remains
unknown. Missing constructed-link-command evidence is not changed into proof.
The existing capture recipe does not enable additional `RUSTC_LOG` output; a
command can therefore remain unknown. A justified conservative coverage review
may cover that state; matching unknown strings alone is not a coverage review.

The projector also exposes a versioned `content_view` with eleven complete
sections: source, packages, graph, headers, embedded_assets,
build_script_link_requests, platform, runtime_rust, runtime_microsoft,
runtime_final_link and runtime_query_outcomes. See its separate contract for
the exact observation-local binding exclusions. Original raw facts remain
unchanged. File identities, installed candidate identities and unknowns are
preserved. Link rlib names, trace counters and output-association observations
currently compare exactly. An actual difference stops the final job for a
bounded applicability review; this implementation does not infer that a change
is harmless or caused by host paths.

`check-ordinary-release-applicability.py` consumes a **genuine completed**
committed `docs/release/full-review-applicability.json`. It has a distinct
`ordinary_full_content_applicability_review_v1` identity. It is not the
analytical exact-source external-review schema. Required fields are:

- schema_version 1; kind; status reviewed; truthful reviewer, timezone-bearing
  date and substantive scope
- basis: original observed source SHA/tree, native-projection hash/length,
  original inventory hash/length and original metadata digest
- dependency_review: exact hash/length of the genuine committed ordinary review
- source_content_files: complete current tracked file path/hash/length list,
  excluding only this applicability file, ordinary dependency review,
  authorization receipt and the closed dependency-evidence notice root
- native_content_sha256: exact canonical digest for each of the eleven sections
- inventory_policy: separate boolean allow_metadata_digest_change and
  allow_encoding_change, substantive rationale and nonempty evidence references
- coverage: completed source/native coverage decisions with reasons/evidence
  and no unresolved conditions

The applicability-specific source boundary is **not** an exclusion from the
ordinary authorization gate. That gate still binds every tracked file/mode/blob
except its own receipt, including this record, review, notices and workflow.
The excluded notice/review bytes are independently bound and compared by the
ordinary gate and payload adapter. This keeps the committed content review
finite without a future self-SHA or a blanket ignore-documents rule.

The basis source identities and native-projection hash are trusted
maintainer-reviewed provenance references. This checker validates their shape
and binds them to the completed committed review; it does not independently
download/reopen historical evidence or authenticate a reviewer's judgment.
They must refer to genuinely inspected originals. Filling fields, hashes or
coverage booleans cannot establish that substantive review occurred.

Every non-metadata inventory semantic field remains exactly equal. Both original
inventories stay unmodified, with `review_status: not_reviewed` and their original
unresolved observations. Any raw byte difference requires the explicit
allow_encoding_change condition; a metadata digest difference additionally
requires allow_metadata_digest_change. These names cover permission for raw
serialization differences, including when metadata also changes; neither
establishes why the bytes changed. All native-content conditions must still
match. No helper generates a policy permitting differences or marks the cause
as only host paths. The real review decides whether those precise conditions
are substantively justified.

All projectors/checks return factual comparisons with review, runtime,
appearance and release approval flags false. The completed committed reviews
and real inventory-bound publication decision supply their own authority.
False factual flags do not imply that applicable agreements were never accepted.

## Retained and exported evidence

All Cargo metadata, raw build/runtime logs, commands and private runtime
observations stay on the original Windows host. Retained originals are required
by `--validate-only`; downloaded hashes cannot replace them. The final evidence
sidecar binds exact source/tree, ordinary inventory/receipt, both inventory
identities, native/platform projection, actual archive, extracted member set and
both original smoke observations. It contains hashes and bounded facts, not
private absolute paths or raw log text.

If a final check fails after capture, a failure-only step reruns the unchanged
same-build native projector against the retained originals. Only a successfully
revalidated bounded facts JSON is retained in a separate engineering artifact.
It contains no raw streams or executable and is never a publication input.
Missing or changed originals prevent that export; no gate is bypassed.

The separate Actions native artifact contains only the native projection and
final evidence sidecar. The publication artifact contains exactly the ZIP,
checksum, release metadata, final evidence sidecar, source-bound release notes
and two fixed PNGs. The
no-checkout publisher checks the sidecar and ZIP bindings, both PNG hashes and
lengths, and exact artifact membership before any release write. The release
assets are the ZIP, checksum, two PNGs and bounded evidence sidecar. The
source-bound notes provide the release body's component-terms pre-disclosure.
The legacy publisher's existing tag/retry/overwrite behavior is otherwise
unchanged by this narrow integration. External source/tag/archive and remote
asset verification remains required before declaring publication complete.

## Remaining real inputs

1. Successful exact-source CI and the pending real full native qualification
2. Genuine original ordinary inventory and notice bytes from that qualification
3. Content/source/native reconciliation and complete substantive dependency
   review, including only actually applicable platform distribution conditions
4. An exact applicability record justified by those facts and the final reviewed
   integration source; any actual native identity differences need a bounded
   resolution rather than an assumed normalization
5. Committed notices/review/applicability and actual ordinary publication
   authority, rebound through the unchanged two inventory digests
6. Successful final-source CI and a real Windows run of this final same-build,
   same-archive chain, followed by remote source/tag/release verification

The preparation adds no publisher registration, paid tool subscription, reduced
variant choice, provider release permission or general legal-approval step.
Only a concrete applicable new agreement/action or materially new recipient
terms/flow needs its actual authorization. Synthetic tests and a user approval
cannot substitute for unsupported coverage or protective-equivalence findings.
