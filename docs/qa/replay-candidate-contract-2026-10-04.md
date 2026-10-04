# Reviewed Windows replay candidate contract, 2026-10-04

This is the candidate-only migration associated with the independently reviewed
app replay-v3 change at `b03efc868b1bcd4672ad5655e2dc3e5839e063b2` and the
[frozen-source audit](replay-candidate-pin-audit-2026-10-04.md). It changes no app,
weather, FDM or renderer implementation and grants no distribution permission.
The final coherent source is `b68b0ff1ce63ff1fc441a72d9bbd997f9b39a014`, the reviewed
region/weather scheduling fix atop `48bf8595dc5383b77f3f452710e0c1fe3a0342ee`, combining
region milestone `509424e7bc9f2ed57d5004421ef5e83ba2ab2cfd` and authored-weather
modal-ownership fix `6097bc952a43e559acbbe98c5bb983222844a286`. Its exact full-file
digests are recorded in `scripts/replay-candidate-contract.json`.

Compared with reviewed app replay source b03efc8, only four original contract
entries changed: app `main.rs`, `replay_policy.rs`, `replay_migration_tests.rs`
and `distribution.rs`. The main merge retains both additive weather and region
CLI arms, all CurrentRecorder paths, explicit legacy opt-in and recorded-weather
authority before startup mutation. The reviewed weather bridge deliberately
admits modeled v3 weather while retaining the complete identity gate, manual
replay override rejection, and unchanged partial-identity policy/notice. The
distribution delta adds the literal compiled download feature identity.

Five further full files enter the boundary: app `weather_runtime.rs` (recorded
selection/executed-clock authority and pending choices), `world_runtime.rs`
(new-flight/restart recording setup), `cloud_runtime.rs` (effective-source
presentation), `region_runtime.rs` (package-backed replay prohibition), and UI
`world_map.rs` (new-flight input ownership). The latter includes the reviewed
same-frame modal/editor ownership fix; it cannot be skipped while pinning its
consumer. The final b68b0ff delta changes only two of those 26 source pins:
`weather_runtime.rs` explicitly orders pending weather after region modal updates,
and `region_runtime.rs` exposes that system plus real combined-schedule and
package-to-Base replay/weather regressions. It leaves the main/codec/policy bytes
unchanged from the reviewed 48bf859 integration. No sim/FDM source, independent
encoder or golden bytes changed in this integration. Renderer behavior and native/Windows appearance remain separate
qualification, bound to the complete run source inventory but not represented as
legacy identity evidence by this contract.

## Frozen contract design

The old five historical hash values and baseline commit remain unchanged in the
checker. The old replay.rs hash remains labeled historical; it cannot attest the
newly extracted codec or policy. The Light profile and FDM aircraft/lib pins remain
direct requirements. Profile selection/wiring is reviewed with the migration.

The separate versioned manifest covers the entire replay.rs, identity/current/
player helpers, sim exports/weather, the unchanged fixture producer, both aircraft
profiles, app distribution/profile/main/policy/runtime, persistent replay UI,
the new-flight/weather/region integration helpers and map ownership UI,
and the sim/app regression tests used to substantiate the contract. An exact
required path set rejects row removal or substitution. Every listed source and
the loaded manifest must match both its raw canonical Git blob and the exact
checkout bytes. No source normalization or function-fragment hashing is used.
The source-input report advances to schema 3 and contains the manifest, its raw
UTF-8 text and hash, independent anchors, historical provenance and per-file
canonical/checkout data.
The complete clean tree/HEAD inventory still runs before and after qualification.

Both independent Python encoders and all existing v1/v2/v3 golden bytes are
unchanged from their independent foundations (`62d954fa07dfc8738879c75b24b4035e6b2b27f0`
for identity and `86de934f9441eeccbb990fc6fde288f07c849a40` for codec fixtures), separately pinned in the checker, and verified without `--write`.
Changing current implementation hashes cannot refresh these anchors. These are
compatibility evidence, not authentication or a recovery of the missing legacy
yaw coefficient. The existing `record_takeoff` still writes legacy v1 and the
observed Light fingerprint remains `0505e6644bb29a53`.

## Acceptance boundaries retained and strengthened

- Default Swift still rejects the actual legacy Light fingerprint in hexadecimal,
  without a compatibility flag
- Only the explicit Light/no-model replay smoke gains
  `--legacy-replay-compatibility`; it requires the full missing-yaw diagnostic
- The report states partial identity, explicit assumption and unverified historical
  yaw. Evidence validation rejects complete-identity claims, a missing warning,
  a changed/rehashed warning-less log, or missing persistence/layout test evidence
- The original release/MSVC/commercial-staging fingerprint test stays. Three exact
  tests in that same app test binary additionally check restricted opt-in, actual
  playback/status/banner persistence and real-font layout. Each must execute one
  passing test; zero matched tests fail
- The reviewed `029c1b7` literal `region_downloads=false` guard is retained in the
  candidate handshake; the feature recipe remains offline and full default LUTs
- Staging/copy recipes, inventory rules, upload allowlists, PNG validation,
  original timeout/diagnostic acceptance, rights/dependency blockers and release
  authorization gates stay intact. No readback probe is introduced

The private legacy PNG is still a startup screenshot. A warning in a log does not
prove that a native screenshot displayed a readable persistent banner. System and
font-layout checks substantiate the implementation; actual integrated Windows
execution and visual inspection remain separate evidence.

## Verification and remaining qualification

The candidate/stager focused Python suite passed all 53 tests after final source
binding (32 candidate, 21 stager). Another 93 readiness, release authorization,
release-workflow and source-CI workflow tests passed, for 146 focused Python
checks. The independent reviewer approved the contract
implementation and source-binding fix; final integration review is recorded with
the local commit handoff. `git diff --check` passed.

Focused Python tests exercise canonical semantic changes, every expanded source
and independent golden anchor, missing contract rows, unchanged legacy inputs,
CRLF checkout with clean Git status, raw upstream notice preservation, literal
boolfalse identity, opt-in/hex diagnostics and exported partial-identity evidence.
Independent reference encoders verify the original complete/partial hashes and
all existing golden bytes without regeneration.

No Cargo command, Windows executable, native GPU run, binary distribution,
rights receipt, dependency approval or publication is performed by this migration.
The exact integrated source still needs the full Windows candidate run, including
all required application tests, four startup/capture cases, final source/bundle
verification and human screenshot review. Earlier candidate outcomes cannot be
reused as qualification of this change. Physical GPU/controller/audio, performance,
and store/release approval remain separate.
