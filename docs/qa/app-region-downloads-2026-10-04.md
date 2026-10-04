# Opt-in application region downloads — 2026-10-04

## Scope and boundaries

The app's `region-downloads` feature connects the existing prepared GitHub ZIP
acquisition API to Regions → Installed / Downloads. An explicitly supplied local
catalog identifies candidates; selecting a row only previews its declared bounds,
source URL, archive SHA-256 and provenance. Download/Retry and Cached only are
separate explicit commands. Default and existing commercial-staging builds keep
regional content acquisition disabled.

The catalog has a 256 KiB file budget and 64-record limit. Schema/field/duplicate,
portable identity, finite geographic bounds, text-control/size and download-source
checks precede use. The verified staged manifest must match the selected catalog's
ID, version, title and bounds before commit. Catalog provenance remains a claim;
checksums are integrity checks, not publisher authentication or rights clearance.
No real-area catalog or actual terrain redistribution qualification is included.

Import, refresh, acquisition and installed inspection retain the existing one-worker
slot, progress snapshot and cancellation generation. Installation never selects or
activates a package. A user must choose its Installed row and explicitly Start;
the existing inspection, global fallback and package-backed replay restrictions
continue to apply. Cancellation cannot roll back an already completed cache
publication or atomic install. See [the user workflow](../content-downloads.md).

## Automated evidence

Linux, Rust 1.93.0, two Cargo build jobs, `RUSTFLAGS="-D warnings"`; all tests ran
offline. No production network transfer was needed for these application tests.

| Check | Result |
|---|---|
| `cargo test -j 2 -p flightsim-ui -p flightsim-app --locked --offline` | 222 app and 197 UI tests passed; one existing optional app fixture test ignored |
| `cargo test -j 2 -p flightsim-ui -p flightsim-app --features flightsim-app/region-downloads --locked --offline` | 232 app and 197 UI tests passed before the final refresh-error regression was added |
| `cargo test -j 2 -p flightsim-app -p flightsim-content -p flightsim-assetgen --features flightsim-app/region-downloads --all-targets --locked --offline` | Final enabled app: 233 passed; content: 24 download + 23 package tests passed; assetgen: 36 passed; one existing optional app fixture test ignored |
| Enabled `cargo clippy` for app/UI/content/assetgen with the same feature unification and `--all-targets -- -D warnings` | Passed |
| Default `cargo clippy` for app/UI with `--all-targets -- -D warnings` | Passed |
| Default executable `--help`, then `--region-catalog must-not-be-read.json` | Help includes the opt-in flags; unsupported feature exits 1 with the explicit build-feature error before renderer initialization |
| `cargo fmt --all -- --check`, `bash scripts/check-architecture.sh`, and diff whitespace | Passed |
| `RUSTDOCFLAGS="-D warnings" cargo doc -j 2 -p flightsim-app -p flightsim-ui -p flightsim-content --features flightsim-app/region-downloads --no-deps --document-private-items --locked --offline` | Passed |

Assetgen's ureq defaults unify gzip with the download implementation, so the final
joint test/lint run also exercises the existing bounded transport guard under that
feature combination. The ignored test is
`optional_real_region_build_reports_cpu_work_and_respects_scene_caps`, which needs
an explicitly supplied external scenery/tile fixture.

New and retained application coverage includes:

- Missing/unknown/duplicate JSON fields, duplicate identities and source URLs,
  exact 64-row acceptance and over-limit rejection, oversized/nonregular/symlink
  catalog files, unsafe IDs/versions, malformed URLs/hashes, text controls and
  nonfinite, degenerate, polar and antimeridian bounds
- Preview without acquisition or changes to pending/active terrain; unchanged
  catalog refresh retains the preview, while changed or invalid claims require
  explicit reselection
- A failed installed-store listing still rereads and reconciles a changed/invalid
  catalog; neither failure can leave the old candidate downloadable
- Forced offline behavior even for Download/Retry, clean cache-miss failure,
  no automatic retry when cache bytes appear, and explicit verified-cache retry
- Production offline-cache reader → strict staging → install → separate Installed
  selection → explicit Start; pre-install ID/version/title/bounds disagreements
  leave no installed package or operation-owned staging directories
- Cancel/back/close generation handling, repeated requests while a worker is
  pending, discard of late completion, reopening and explicit retry
- Button/keyboard guards, source/hash/notice pagination and existing real Bevy
  layout checks for region rows, credits and controls
- Feature-disabled CLI validation/help, percentage calculation for zero/unknown
  totals and `u64` limits, and existing local import/activation/replay regressions

The cache fixture uses the existing independently generated Python ZIP/FSDM test
archive. Its source URL is synthetic and is never contacted; its manifest is not
a real dataset or a source-rights review. A truncation-notice test initially failed
because its wording wrapped between two words. The assertion now inspects the
visible text across line breaks while retaining the existing line/page bounds.
Clippy also caught and corrected one redundant borrow in a test helper.

## Review and limits

Subsequent native application, feature-identity and packaging checks are recorded
in [the integration record](region-download-integration-2026-10-04.md).

Independent runtime review found the refresh/store-list short-circuit described
above; the final separate results and regression resolve it. No remaining blocking
runtime finding was identified in that review.

This record does not claim a native graphical interaction pass, Windows app-feature
qualification, target-head remote CI, default/optional dependency-inventory release
approval or a commercial release. Those remain integration-owner checks. The
existing [download foundation QA](content-downloads-2026-10-04.md) is separate
network/transport evidence, not a substitute for the app/map checks. FDM, replay
formats, render settings and release gates are unchanged by this feature; the
replay-block wording is general enough to keep the package identity restriction
clear across supported formats.
