# Local analytical publication consistency checker

Status: **preparation only; publication remains blocked**. The reduced variant
has not been chosen by the user. No completed native qualification, whole target
review, actual user-choice record or publication decision is supplied here.
No new workflow, permission, signing key, approval infrastructure, tag, release
or artifact upload is installed by this change.

`check-analytical-publication.py` consumes the existing exact-source qualification,
whole-review stager, final-bundle runtime and Windows UI validators. It never
changes their acceptance rules. It separately compares the archive's actual
uncompressed member bytes to the extracted bundle that was executed, and binds
all reviewed inputs. This catches a valid ZIP with unexpected, renamed, duplicate,
case-colliding, linked or changed members as well as ordinary checksum changes.
It also rejects hidden bytes after a DEFLATE end-of-stream and requires every
compressed byte to be accounted for. It neither executes the application nor rebuilds, stages or uploads anything.
The existing validators may perform their existing read-only source/dependency
collection checks and create temporary inspection files.

## Honest authorization boundary

There is deliberately **no successful publication exit code**:

- Exit 1: missing/invalid arguments or missing, invalid, stale or changed evidence;
  no public text is prepared before consistency succeeds. Parse failures use the
  same sanitized blocked report and never echo private argument values
- Exit 2: all local consistency checks passed, but actual authorization and the
  independently reviewed publication process are still required

Every report and prepared manifest has `release_authorized: false` and
`publication_blocked: true`. The seven original acceptance-condition identifiers
remain required. The `publication_receipt` condition must remain
`external_authorization_required`; it cannot be filled with `reviewed` to bypass
that boundary. There is no `--authorize`, receipt-generation or network mode.

Three independently obtained digest arguments pin the exact source review,
user-choice record and publication recommendation. These are **consistency
anchors, not credentials**. A digest supplied by the author of its own JSON does
not prove a review happened or that the user chose this variant. Nor do the
`reviewed_by`, `user_message_reference`, CI IDs or passing factual statuses prove
authority. Even fabricated, mutually consistent local fixtures can never obtain
publication authorization from this command.

The eventual publication owner must verify the actual conversation contains the
explicit reduced Swift-only/Reinhard choice and instruction to publish, and
verify actual CI/native/rights/visual reviews through their original sources.
The owner must independently pin the reviewed records after that verification.
A generic previous request for the fuller release, silence, elapsed time, source
CI success, two narrow package assessments or a boolean supplied by a builder
cannot substitute. The future publication workflow and its action-time authority
boundary require separate implementation and review. This checker must not be
wired into an upload condition which treats exit 2 or prepared text as approval.

## Required local inputs

Run the checker from the final clean canonical checkout. Supply the complete
40-character commit and tree identities and the positive variant revision.
All input JSON uses ASCII JSON, sorted keys, two-space indentation, a final LF,
no duplicate keys and no unexpected fields. All paths are absolute, single-link,
regular files or ordinary directories; parent-segment aliases, symbolic links and
reparse points fail.
The input map and three small review/choice documents each have a 256-KiB limit.
They remain private and must not be copied into public metadata.

The `--inputs` JSON object has exactly these path keys:

- `regressions_private`, `regressions_export`: actual complete native regression
  evidence for the exact final source, including both app/render modes, Clippy,
  four exact identity/legacy tests and all compile guards
- `runtime_private`, `runtime_export`: actual final reviewed-bundle evidence,
  its original frozen native dual-build audit, complete assembled notices,
  unchanged inventory, completed whole dependency/platform review and final
  archive/extracted copy plan, four strict outcomes and eight fixed captures
- `live_private`, `live_export`, `legacy_private`, `legacy_export`: the two actual
  final-same-bundle Windows UI observations, including complete ordered captures,
  private OCR/log witnesses and normal process exit
- `source_review`, `user_choice`, `publication_decision`: the three private,
  externally reviewed and pinned documents described below

The original `validate_reviewed_assembly` contract remains authoritative for the
whole dependency review: exact source SHA/tree/native inventory, completed
reviewer/date/scope, assembly hash, complete source provenance/native runtime
coverage, every supplemental notice reference, and unchanged readiness/stager
checks. Its affirmative coverage fields may only be supplied by a real reviewer.
They are not inferred from the collector, an assembler, two-package assessments
or a source-only review. The final native projection is re-derived and compared
before binding. Its final-runtime `runtime-facts/runtime-facts.json` and
`ui-capabilities` projection are separately hash-bound. The exact build/original
private paths and observed file origins are revalidated through
`validate_runtime_facts` and the native projector. The collector retains false
coverage/approval fields and any unknown link/SDK/static-membership facts; the
independent reviewer must reconcile them rather than copying a passing boolean. Any observed runtime/toolset unknown must be settled or explicitly
handled by the genuine whole-target reviewer under the unchanged review rules.

### Release identity

All three records bind the same release object where applicable. It contains
`repository`, `variant`, `source_sha`, `source_tree`, `package_version`,
`variant_revision`, `release_tag`, `title`, `prerelease`, and `assets`.
The repository and variant are fixed to `Xenoah/flightsim-claude` and
`swift-reinhard-windows-prerelease-v1`. The title is exactly
“Swift-only / Reinhard Windows prerelease” and `prerelease` must be true.

The exact Cargo workspace package version must match the previously validated
binary handshake. The tag is `v{package_version}-swift-reinhard.{revision}`;
it is distinct from the package version and the ordinary release tag. Build
metadata in the package version is currently unsupported and requires separate
review. The three exact proposed public names are:

- `flightsim-v{package_version}-swift-reinhard.{revision}-windows-msvc.zip`
- `SHA256SUMS.txt`
- `release-variant.json`

No tag or release is created by deriving these names.

### Source review

The exact fields are `schema_version` (integer 1), `identity`
(`analytical-final-source-review-v1`), `status` (`reviewed`), `source_sha`,
`source_tree`, `reviewed_by`, `reviewed_at`, `source_ci`, and
`release_authorized` (false).

`source_ci` contains exactly `repository`, `workflow_path`
(`.github/workflows/ci.yml`), `workflow` (the exact file's SHA-256/byte record),
`head_sha`, positive integer `run_id` and `run_attempt`, `status` (`completed`),
and `conclusion` (`success`). A real reviewer must verify the original run and
all required jobs. The checker only validates the supplied binding, not GitHub's
current remote state or the claimed CI outcome's provenance.

### Explicit user-choice record

The exact fields are `schema_version` (integer 1), `identity`
(`analytical-explicit-user-choice-v1`), `release`, `choice`
(`publish_swift_only_reinhard_prerelease`), `publication_requested` (true),
`recorded_by`, `recorded_at`, `user_message_reference`, `accepted_limitations`,
and `release_authorized` (false). The reference points privately to the actual
user instruction; never put conversation text, private paths or that reference
in the public manifest. The recorder must not manufacture this document while
choice remains pending. None is included in this change.

### Independent publication recommendation

The exact fields are `schema_version` (integer 1), `identity`
(`analytical-independent-publication-review-v1`), `status`
(`reviewed_recommendation_external_authorization_required`), `release`,
`bindings`, `reviewed_by`, `reviewed_at`, `acceptance_conditions`,
`substantive_reviews`, `accepted_limitations`, and `release_authorized` (false).
The independent reviewer differs from both the source reviewer and choice
recorder. Its UTC date must not precede the source review or choice; future dates
are rejected. The date records a claimed review time and does not authenticate it.

`bindings` must equal every recomputed final binding, without omissions or extras:
Cargo lock, source workflow, regressions, original dual-build result, supplemental
assembly, whole dependency review, final runtime, live UI, legacy UI, native
projection, runtime facts, UI capability facts, archive, executable, inventory,
bundle manifest, source review and
user choice; full bundle/notice and runtime/live/legacy export tree digests are
also bound. Tree digests hash the canonical sorted file-path/size/SHA-256 map;
no private paths or per-file maps are exported publicly.

`acceptance_conditions` contains all seven original gate IDs. Six have value
`reviewed`; `publication_receipt` remains `external_authorization_required`.
`substantive_reviews` has all nine IDs in the script's fixed `REVIEWS` list, each
`accepted_with_declared_limits`. Those include visual scenes, HUD, map/cameras,
lifecycle, narrow-viewport legacy disclosure, normal exit, darker Reinhard,
whole dependency/platform/runtime and modified-source/notice provenance. These
are actual reviewer judgments to be independently checked, not generated passes.

Both the choice and recommendation must explicitly include the script's full
fixed `LIMITS` list: Swift only; other aircraft excluded; darker highlights;
coarse/global terrain; monthly climate; offline downloads; physical GPU,
controller and audio not qualified; partial legacy identity; cancellation during
admitted preparation not proven; no Steam or certified-flight qualification.
The UI observer's bounded Start/Cancel race does not establish preparation-phase
cancellation. Keeping that limitation explicit does not close its open acceptance
item or authorize changing the existing gates.

## Bounded public text preparation

`--prepare-public-text` may name a fresh directory disjoint from source and every
private input. Only after all checks and final frozen-byte/source rechecks does
the tool exclusively reserve the destination and create `release-variant.json` and `SHA256SUMS.txt`. It never
copies the ZIP or executable. A collision is a blocker; existing output is never
replaced. An I/O failure may leave partial text in that new directory, but it
returns a blocked result and cannot grant authorization. The checksum file binds the exact frozen ZIP and actual manifest bytes.
The manifest is at most 32 KiB and uses only fixed text, release identity and
hash/byte counts. It includes no raw logs, OCR, arbitrary review prose, message
references, reviewer identities or private filesystem paths.

These two files are prepared text, not permission to publish either them or the
archive. The exact ZIP remains unchanged, including the sole stager's original
`commercial-staging` and `LOCAL-CANDIDATE` metadata. The manifest explicitly says
it is prepared metadata, publication is blocked and those markers confer no
permission. All current release gating and permission scopes remain unchanged.

## Frozen-archive handoff: separate implementation still required

A short-lived hosted qualification runner currently retains the only exact
private archive. Its allowed exports contain text/Swift PNGs, not that archive.
When the runner ends, the archive is gone. The current ZIP writer uses
filesystem timestamps (`ZipFile.write`); neither ZIP nor compiler-output
reproducibility is established. A later rebuild, even from the same source, must
not be called the reviewed archive unless **every byte and hash actually matches**
and all original private evidence remains verifiable. Do not upload or cache an
unapproved binary as a shortcut.

A concrete candidate for later independent review is a **bounded, same-run private
hold** after the user has actually chosen the reduced variant:

1. A dedicated, manually requested qualification job has repository read-only
   permissions and exact immutable source/workflow identity. It performs native
   collection and keeps original build trees, the final archive, extracted copy
   and complete private evidence alive on that same runner. Only existing allowed
   bounded text/Swift PNG exports are sent for review. No archive/executable
   upload occurs before the authorization boundary.
2. The publication owner verifies actual user scope, exact source CI, whole
   native/platform rights, final runtime and visual judgments. The owner creates
   a real decision on a dedicated decision ref only after those checks. The
   decision binds repository, qualification run ID/attempt, workflow/source/tree,
   original build, inventory/notices/reviews, final runtime/UI and exact archive,
   manifest, executable and public asset identities. The frozen archive must
   already exist before its decision is made; no predicted hashes or time-based
   approval are allowed.
3. A read-only bounded poll can observe the dedicated ref, but the future trusted
   verifier must establish the authorized publication owner's actual GitHub
   action/identity through a reviewed event/API boundary. Editable `reviewed_by`
   text or Git commit author text alone is insufficient. It must pin the fetched
   decision's immutable commit and file digest, verify the exact run attempt and
   content, reject moves/conflicts, and recheck every frozen input immediately
   before any byte upload. Repository code must not be able to turn local
   consistency into this decision. This verifier is not implemented here.
4. Only after that external boundary succeeds may the same qualification job
   upload the exact approved files. A separate, minimal `contents: write`
   publisher executes no repository code and accepts only the authorized exact
   run, immutable decision and three allowlisted assets. It independently checks
   archive/manifest/checksum identities, absent-or-identical tag/release/asset
   state, prerelease flag and bounded notes. It then verifies downloaded bytes
   and remote tag/source after publication. Conflicts block rather than overwrite.
5. The job must reserve a real finite review window within the platform's actual
   job lifetime. Existing native budgets can consume most of that lifetime;
   fitting build, whole review, final stager/runtime and UI runs cannot be assumed.
   The window and step deadlines require capability/timing validation first.
   Expiry, runner loss, source/ref movement, cancellation or missing decisions
   discard the private candidate and leave publication blocked. Waiting never
   creates authority. A fresh run needs its own final review and decision.

This design introduces no new credentials, persistent machines or approval
settings here. It remains a follow-on requiring source review, actual native
capability and time-budget evidence, the user's pending choice and explicit
review of the remote authority/TOCTOU controls. The available repository connector
has no release/asset-write operation; the independently reviewed Actions path is
therefore a proposed supported route, not a claim that current tooling can publish.

## Validation and current limits

The tests exercise actual small ZIP streams, file/path/JSON boundaries, exact
scope/hash binding, missing choice, independent recommendation rules, byte/source
mutation, safe output and nonzero exit status. Success fixtures are temporary,
explicitly synthetic test data. Mocked native adapters test consumer plumbing
only. They are never presented as native execution, completed rights review,
visual acceptance or a real user authorization record.

Run `python -m unittest discover -s scripts/tests -p test_analytical_publication.py -v`.
The complete existing Python suite must also pass on the final integrated source.
Native Windows build/runtime/UI evidence and all real reviewer/choice records
remain prerequisites; Linux tests cannot establish them. No positive production
receipt or actual publication result accompanies this implementation.
