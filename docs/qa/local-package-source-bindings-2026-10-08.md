# Local dependency and capture-admission source boundary, 2026-10-08

Status: bound to the independently verified coherent runtime commit
[`7d8c8770a5f3aaa69f36303ac11390ff0755a037`](https://github.com/Xenoah/flightsim-claude/commit/7d8c8770a5f3aaa69f36303ac11390ff0755a037),
tree `4abfd4528b80e005c8c0a5dd9aa17848614cfb45`. Full-suite and clean source-chain
checks pass as recorded below. This record grants no Windows/GPU, licence,
reduced-deliverable or publication approval.

## Why the boundary expands

The reviewed original JPEG transpose and parallax replacements are supplied by
local `vendor/zune-jpeg` and `vendor/bevy_pbr` packages. Their names and versions
remain the same, but Cargo.lock no longer supplies registry checksums for those
local bytes. The modified tree must therefore be authenticated directly rather
than treating the old registry identity as its current source attestation.

The combined whole-file boundary contains 402 paths:

- 159 existing replay source hashes remain unchanged
- The existing root Cargo.toml and Cargo.lock hashes advance for the independently
  reviewed dependency-remediation component
- The existing app main.rs, screen_capture.rs and capture_backpressure.rs hashes
  advance for the separately reviewed batch-preparation admission component
- Two new complete capture-admission modules bind the engine hooks and pure
  scene-generation state, including their integrated Rust tests
- 207 complete vendor files protect all Rust, WGSL, package/build declarations,
  the retained embedded KTX2, integrated tests, licences and provenance/notices
- 20 validation-tool files protect the independent JPEG, mathematical and
  genuine imported-shader witnesses and their complete declared build inputs
- Nine provenance/archive-policy files protect both manifests, addition-only
  vendor patches, root export rule, source-policy records, checker and tests

Every accepted path is a literal reviewed member. No acceptance-time directory
scan, mutable provenance row set or package name/version can grow that list.
A new exact-set validator checks both live canonical source collection and
exported source evidence. Missing or additional members under either vendor root
or any of the three executable witness roots fail admission. Case-aliased roots
are also rejected. A live-checkout walk additionally rejects ignored extra files,
non-regular inputs, symlinked parents/members and Windows reparse points inside these five boundaries.
This walk only rejects; it cannot grow the literal accepted set. This closes the
Git-clean ignored build.rs/local-config gap. Keep generated witness targets and
logs/reports outside the reviewed roots. CARGO_TARGET_DIR moves Cargo targets,
but not the mathematical witness's validation.log or the integration witness's
output directory; select external output paths or use a disposable witness copy.
Unrelated source paths retain the existing run-inventory rules.

The source-header and modified-source manifests must match the actual protected
file bytes and complete file sets. Original registry checksums and revisions are
retained as provenance, not relabeled as hashes of the modified packages. The
unresolved documentation PNG and pristine-registry VCS marker remain omitted.
The source policy independently excludes the unresolved Light Single model from
commit/tag archives; development history and the old binary release gates remain.

## Retained controls

All 102 historical independent anchors and previous mutation attacks are retained.
All 39 existing candidate checker functions keep their prior logic, with only three
additive calls in collection/export validation. Removing those calls, the two new
helpers and literal path constants recovers the prior checker AST.
Of 103 existing candidate test/helper functions, 102 are unchanged. The CRLF
checkout test moves only its disposable LF-contract attribute override into
.git/info/attributes, retaining the existing HEAD. This preserves newly pinned
.gitattributes bytes while every original CRLF/raw-notice/hash/diagnostic and LF
recovery assertion remains unchanged. The analytical
checker executable AST is unchanged; its count comment and existing test count
assertion advance from 164 to 402.

No codec, legacy fingerprint, partial-identity disclosure, aircraft admission,
normal feature choice, screenshot readiness, timeout, stager allowlist, right or
positive publication decision is weakened by this source migration. The separately reviewed batch preparation fix is explicitly reconciled in this
combined boundary. It preserves the existing stability mutation and backpressure
lifecycle anchors byte-for-byte.

## Additive adversarial checks

Fourteen new test methods cover:

- Complete literal package/witness/policy closure, patch targets, path-package
  identities, integrated transpose/shader anchors, 510 source-header bindings,
  provenance byte hashes and declared functional features
- Missing, additional and case-aliased source members at all five exact boundaries
- 36 specific committed semantic mutations and four source/provenance/binary
  mutations, rejected before compilation
- Ten committed exact-set addition/removal cases
- Ten Git-clean ignored build.rs/local-config attacks and 15 mocked parent/member
  symlink guards without requiring Windows symlink-creation privileges
- 20 portable Windows reparse-attribute cases at protected ancestors, roots,
  nested source directories and manifest files, retaining the symlink controls
- Five resealed omission/hash/contract-row attacks for each of the 236 added paths,
  giving 1180 per-file exported attacks
- Fifteen additional exported unknown-member, alias and resealed-contract attacks
- 50 targeted capture-admission mutations and 25 resealed exports across the two
  new modules and three changed app sources
- Literal lifecycle/order witnesses for snapshot extraction, exact view-list
  restoration, same-scene prior Render acknowledgement, scope and one-shot exit
- The retained 15 archive-policy tests, explicitly invoked from ordinary source CI
  because their root-level script is outside scripts/tests discovery
- Both actual committed archive formats retain every protected source/notice and
  independent anchor byte while omitting the unresolved Light model

Altered Rust/WGSL is never compiled by these tests. Source admission is not a
functional, GPU or legal proof. The source-level tests supplement, not replace,
the independently executed transpose/oracle and genuine shader validation.

The archive oracle was strengthened after a disposable probe showed that source
info/global attributes could define both the expected and supplied omission.
A fresh temporary bare repository, empty template and disabled ambient Git
configuration now isolate the expected committed export. An independent eight-case
ZIP/TAR probe rejects info attributes, local/global attribute files and environment
configuration injection while preserving the required vendor notice. Hosted
archive origin and final tag/commit contents still need separate verification.

## Batch-preparation admission semantics

ADR-0030 distinguishes 30 real Main updates from full-scene draws. Only native
batch capture installs the new controller. It retains complete original Main,
Extract and Render preparation/upload/cleanup, the two-credit bounded GPU gate,
configured delay, ReadyScene/stability checks and existing budgets. During loading,
only the original camera driver's view list is temporarily saved/cleared/restored.
Any extracted Screenshot bypasses suppression. Once admitted, views stay open.

A generation-tagged prior same-scene admitted Render opportunity is required
before requesting capture. The token is recorded only after the entire original
Render schedule returns and only for the intended valid prepared flight view. It
is not GPU or shader completion. Changed/unready scenes, cancellation, failure,
zero/overflow and late acknowledgements cannot manufacture a capture grant. The
unwind guard restores saved camera fields, invalidates the opportunity and resumes
the failure. The existing queue callback remains the GPU-credit authority.

The source tests protect these reviewed semantics; they do not establish a native
performance gain, 180-second success, screenshot correctness or physical-GPU
qualification. Those require execution on the final combined source.

## Binding and qualification still required

The actual metadata-to-source projection must prove each patched package resolves
to its expected canonical repo/vendor/.../Cargo.toml and full frozen tree. A
same-name/version external package, escaped/symlinked manifest, omitted source,
extra source or mismatched tree must fail. This belongs to the genuine build and
qualification evidence; it is not inferred from source pins or Linux-hosted
Windows-filtered metadata.

The actual remote runtime commit was verified before either reviewed-source
pointer advanced. Only the five reviewed old source hashes and 238 new explicit
paths were written. The analytical 22-path set is retained; exactly seven affected
build-input, checker, contract and count hashes changed. Capture retains its
historical `75753f5e1dfe8d56d321775cb800b29bf058b3e3` provenance, four-file boundary
and all restrictions; only the two inherited hash values and corresponding runner
pin propagate. Any additional runtime delta needs its own source review and pins.

The corrected full Python suite ran 335 tests: 334 passed and one existing
Windows-only test was skipped, in 378.129 seconds. This includes every retained
source/legacy attack and the new vendor, capture, ignored-input, reparse and archive
cases. Independent review also reran the new semantic/export/archive methods.

Clean candidate source collection and exported-source validation pass, as do the
22-path analytical layer and four-path capture layer, including canonical-byte
verification of all 2660 tracked files. All 402 replay bindings and 102 independent
anchors match. These receipts require independent final migration review.

The unchanged live binary release preflight reports `authorized: false` and ten
blockers: unresolved/unaligned Light Single asset rights (two), missing dependency
review inventory, changed Cargo.lock binding, required dependency review, four
unresolved dependency records, and missing inventory-bound publication approval.
The four unresolved records are the two ordinary Bevy LUT records, constgebra and
hexf-parse. This records the live committed gate rather than inferring clearance
from separate source/provenance research. No inventory or rights record was
refreshed to hide a blocker. The source-archive exclusion does not change that
binary gate. Earlier native evidence belongs to its original source and cannot
qualify these modified packages.
