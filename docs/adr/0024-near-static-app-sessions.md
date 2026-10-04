# ADR-0024: Admit near-static app sessions through explicit profile 4

- Status: locally automated-qualified; independent review and native acceptance are separate gates
- Date: 2026-10-04

## Decision

The app's selected profile and flight session each add a distinct near-static
variant. Original bounded JSON bytes dispatch version 4 to `AircraftProfileV4`.
Live owns one `NearStaticTurbopropSimulation` and v6 recorder; replay owns one
`NearStaticTurbopropReplayPlayer` and borrows its simulation. The old typed
profile-v3/law-1/v5 path remains separate. A presentation category or model name
never selects or converts the physical law.

All sixteen physical scalars remain authoritative through startup, fixed-step
control proposals, restart, candidate preparation, recording, playback and seek.
Live startup takes the profile's exact running engine state and performs the
existing zero-duration supported-state check. Replay uses every recorded engine
value. No settling run, RPM floor, inferred inflow, stabilizer, velocity repair,
coefficient change or widened law-2 operating bound is introduced.

The existing staged new-flight transaction snapshots the exact typed profile,
source, weather/wind, controls, model, camera and audio. A prepared law-2 session
and recorder commit only after scene readiness and generation validation.
Canceled, superseded and failed candidates cannot replace any active owner.
Lateral trims reset only when a new live flight/restart succeeds; failed attempts
and temporary input suspension retain the settings.

## Admission and presentation

Profile 4 requires an explicit external JSON selection in development builds.
The launch choice retains that exact admission; no Cedar or generic law-2 preset
is registered. Commercial staging rejects profile-4 live/replay before source
reads or preparation. Existing distribution allowlists and rights gates remain.
Known wrong-family codec rejections are formatted at the app boundary with the
matching profile version and `--aircraft FILE` hint. The selected reader and its
file-read boundary remain unchanged; no profile is inferred or substituted.
Bounded terrain support is bundled global or explicitly flat-zero; raw/regional
sources remain rejected. Recorded weather, wind and world identity are
mandatory during replay; manual live clouds disable recording as before.

Read-only presentation is implemented beside the near-static host, using its
committed clock, held contact plane and law-2 derivative support check. UI reuses
the existing telemetry, help, control and synthetic-audio plumbing. Labels state
law 2, modeled turbine fraction, relative shaft speed, blade pitch, static rotor
and synthetic audio. Unsupported aero data remains unavailable. The app adds no
landing grade or stationary-parking claim.

F9 exports an authentic full-state v6 prefix without consuming the recorder.
Recorder failure or live visual-rate change closes recording while live flight
can continue. F5–F8 retain the v6 player's bounded frame-zero reconstruction,
terminal probe and exact checkpoint rules. Terminal rejection keeps physical and
controller state and mutes sound; full-state terminal recordings remain exportable.

## Alternatives and costs

Silently upgrading profile 3 or replay 5 would change their accepted meaning and
invalidate historical evidence. Collapsing both laws into an untyped body-only
session could discard engine state or admit a mismatched recording. Both are
rejected. Separate typed ownership requires explicit dispatch arms and a small
read-only presentation module; this duplication preserves closed old contracts.

This app boundary establishes software ownership, not aircraft qualification.
Law 2 remains the bounded J[-0.01,0) approximation with adverse and transverse
induced-speed ratios at most 0.10 and the conservative static power floor. No
reverse-flow, vortex-ring-state or stationary parking fidelity is asserted.
See [app usage and limits](../near-static-turboprop-native-app.md).

Local source and boundary evidence: [automated QA](../qa/near-static-app-integration-2026-10-04.md).
