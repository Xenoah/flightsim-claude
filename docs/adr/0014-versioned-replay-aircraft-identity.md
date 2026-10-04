# ADR-0014: Separate complete aircraft identity from legacy replay evidence

- Status: complete identity, additive v3 codec and explicit-policy app migration implemented
- Date: 2026-10-04

## Context

Replay v1/v2's frozen fingerprint omits `aero.yaw_rate_p`, although the FDM uses
that coefficient. Those files store no aircraft profile. Their display names,
matching partial hashes and positional drift checks cannot prove the missing
input. Correcting the old algorithm in place would confuse file compatibility,
physical identity and legacy evidence, especially while adding new aircraft
propulsion and explicit weather scenarios.

## Decision

Add a pure `flightsim-sim::replay::identity` API with a domain-separated,
versioned, complete parameter identity and explicit FDM model-law revision.
Classify evidence as complete match, legacy partial match, or mismatch. Keep the
legacy calculation, Conditions representation and v1/v2 readers/writers unchanged.
The old Result-only playback gate is documented as partial compatibility, not proof of reproduction.

New callers must make their legacy acceptance policy explicit. No name, current
profile, successful playback or drift tolerance upgrades the original evidence.
Pre-revision physics stays incompatible. The non-cryptographic digest is a
change detector and makes no security claim.

The additive v3 codec now uses explicit version dispatch and bounded conditions
and weather subreaders. `CurrentConditions`/`CurrentRecorder`/`CurrentRecording`
hold a complete identity without a fabricated legacy slot; `ReplayFile` retains
V1/V2/V3 on export, including all-zero-world V2. Legacy APIs keep their previous
semantics. Exact bytes and migration boundaries are specified in
[the replay identity contract](../replay-identity.md). App compatibility and
weather presentation remain separate gates. Regional-package playback remains
blocked until its separate package identity contract exists.

## Alternatives rejected

- Adding the omitted scalar to the legacy hash: changes the meaning of the old
  u64 without recording which algorithm produced it
- Guessing old versus new algorithm by trying both: untagged data cannot establish
  its provenance, and future variants make the ambiguity worse
- Treating a matching builtin name/hash as complete: custom coefficients can
  preserve both values
- Using a position-drift threshold as an identity check: sparse positions neither
  prove the coefficient nor exact attitude/trajectory reproduction
- Bumping the maximum format constant alone: the old reader's equality branch
  would stop reading the v2 world/climate block correctly
- Combining this with physics, profile-v2, weather, app or rendering changes:
  loses a small independent compatibility foundation and enlarges regression risk

## Costs and verification

Maintaining a frozen partial algorithm beside a complete schema is intentional
compatibility cost. The app records v3 and requires an explicit, persistently
labeled legacy assumption for supported frozen baselines. That opt-in never restores missing
historical yaw evidence. The later authored-weather bridge admits supported v3
weather without altering this identity policy; weather CLI and manual replay
cloud overrides are blocked before startup conditions change. Live manual cloud overrides retain
presentation and disable recording/F9 with a persistent notice.
Future field/model changes must update the versioned identity contract. Public
field destructuring catches additions, while private mass/gear changes require
owner review. A hash can collide and does not replace input validation.

Tests mutate all 64 independently configurable physical scalars, check gear
ordering/model revision/metadata, pin independent Python-encoded complete and
legacy hashes, and classify independently encoded v1/v2 fixtures without changing
their bytes. Existing byte-boundary, hostile-input, fidelity and numerical tests
remain authoritative. The later authored-weather bridge supports modeled v3
parameters and the executed clock; native visual acceptance remains separate. App evidence is recorded in
[migration QA](../qa/replay-app-v3-2026-10-04.md).
