# Replay candidate source-pin migration audit, 2026-10-04

This is a bounded static audit for the owner of the Windows qualification runner.
It does not refresh a pin, authorize distribution, change candidate acceptance,
or qualify a Windows binary. The runner and all five existing legacy pins remain
untouched by the app replay migration.

## Exact frozen source

`scripts/check-swift-windows-candidate.py` names the following source:

- Commit: `5c5b2a3057549c7429236b93aa0cdc99e2de38d1`
- `crates/flightsim-sim/src/replay.rs` Git blob:
  `ad38d0514603d21a20fe05ce78cc6271ac6fb56a`
- SHA-256:
  `0b783ceed247b984729021ae57c74b061936d627c04275a850e59079266a18c1`

Reading that Git object reproduces the pinned SHA-256 exactly. The four other
pinned sources remain byte-identical to that baseline: Light Single profile,
FDM `aircraft.rs`, FDM `lib.rs`, and app `aircraft_profile.rs`. The legacy
`record_takeoff.rs` example and app `distribution.rs` also remain unchanged.

The identity foundation was introduced by
`62d954fa07dfc8738879c75b24b4035e6b2b27f0`; explicit v3 codec dispatch was added by
`86de934f9441eeccbb990fc6fde288f07c849a40`. This audit additionally inspected the
application/player migration associated with this document.

## Preserved legacy behavior and intentional changes

The full `aircraft_fingerprint` function body (2,696 bytes) and `Recorder`
implementation (2,208 bytes) are byte-identical to the frozen source. The former
still omits `yaw_rate_p`; no corrected hash is placed in the old untagged slot.

Legacy codec and numeric-validation code was extracted into shared helpers.
Inspection found unchanged numeric limits, validation rules, field ordering,
frame/keyframe encoding and count limits. Historical `Recording::read_from`
accepts only v1/v2; the historical `FORMAT_VERSION` remains 2.
`Recording::write_to` retains automatic v1 selection without enabled world/climate
and v2 otherwise. New explicit encoders and `ReplayFile` additionally preserve a
disabled-world v2. They do not change the original writer's selection rule.

`check_reproducible_with` now delegates through the classified API, retaining its
historical acceptance and mismatch error behavior. The classifier exposes the
matching old fingerprint as **partial** evidence. Its historical Result-only API
still cannot establish complete aircraft identity.

Historical `Player` timing state moved to a private `PlaybackCursor`, shared with
the additive `ReplayFilePlayer`. Public method signatures and inspected timing,
pause, overflow, seek and end behavior remain unchanged. Regression evidence is
in [app migration QA](replay-app-v3-2026-10-04.md) and the retained sim tests.

The deliberate acceptance change is at app startup: explicit format dispatch,
default legacy rejection, restricted complete-baseline opt-in, persistent missing
yaw disclosure, and unsupported-weather gates. New app flights record v3;
existing files are never rewritten as complete identity.

The new app mismatch diagnostic originally used Debug formatting, which printed
legacy fingerprints in decimal. The audit caught its incompatibility with the
runner's existing hexadecimal fingerprint assertion. The final app diagnostic
explicitly labels legacy partial and complete evidence and prints 16-digit hex
fingerprints. An independently pinned Light/Swift mismatch regression checks that
wording. The runner's negative-case fingerprint assertion can remain intact.

## Expanded source review boundary

The relevant replay contract now spans at least these source files:

- `crates/flightsim-sim/src/replay.rs`: frozen legacy hash, shared validation,
  legacy codecs and historical public APIs
- `crates/flightsim-sim/src/replay/identity.rs`: complete identity coverage,
  metadata and partial/complete classification
- `crates/flightsim-sim/src/replay/current.rs`: explicit dispatch, source-version
  retention, environment copying and bounded v3 codec
- `crates/flightsim-sim/src/replay/player.rs`: shared timing cursor and
  version-preserving playback
- `crates/flightsim-sim/src/lib.rs`: module/export wiring
- `crates/flightsim-sim/src/weather.rs`: also required when binding the full v3
  weather-validation contract
- `crates/flightsim-app/src/main.rs`, `replay_policy.rs`, `replay_runtime.rs`:
  options, acceptance, initialization, playback and status publication
- `crates/flightsim-ui/src/replay.rs`: actual persistent notice behavior

The runner's existing `source_inputs` manifest already hashes **every committed
file** and verifies a clean exact commit before and after qualification. Thus
these files are bound to a run. That does not compare them to an independently
reviewed legacy baseline: the special frozen comparison currently covers only
five named sources, including the formerly monolithic `replay.rs`. Refreshing
only that file's SHA would conflate run binding with reviewed compatibility.

The qualification owner must choose and review the updated boundary explicitly.
Any new pins should describe the reviewed migration, preserve the independent
legacy identity vectors, and be followed by qualification of the exact integrated
source. This document supplies no replacement hash list or acceptance receipt.

## Meaning of the existing Light/no-model case

Adding `--legacy-replay-compatibility` only to the runner's explicit
`--aircraft light-single --no-model --replay` positive case, and requiring its full
partial-identity startup warning, preserves that case's original inspection
scope: selected Light dynamics, no Light model, unchanged legacy v1 fixture, and
startup/screenshot smoke. It does not add an aircraft model to the candidate or
expand the candidate into a Light-model distribution.

Default Swift must still reject that actual Light fingerprint. The explicit
positive case assumes the frozen Light baseline; it does not prove which
`yaw_rate_p` the historical recording actually used. The runner's limitation text
currently says that no-model replay "proves original identity"; an eventual
reviewed migration should qualify that as the original **partial fingerprint**.

The candidate's existing identity unit test uses the historical compatibility
API, so that check alone does not cover app admission. Asserting a logged warning
also does not prove persistent on-screen readability. The app migration adds
separate status/layout evidence; the final integrated native/Windows run remains
the qualification owner's responsibility.

No Cargo run, Windows execution, candidate edit, pin refresh or publication was
performed for this static audit. The separate app migration checks are reported
with their actual scope and results in the linked QA document.
