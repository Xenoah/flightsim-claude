# Complete near-static turboprop replay v6

`near_static_turboprop_simulation::NearStaticTurbopropSimulation` and
`replay_v6::{NearStaticTurbopropRecorder, NearStaticTurbopropRecording,
NearStaticTurbopropReplayPlayer}` form an explicit pure host/codec for near-static
FDM law 2. They require the concrete law-2 configuration from profile 4 and use
complete identity schema 4. The old law-1 host and replay v5 remain unchanged.
No app dispatcher or selectable aircraft is added.

Construct a host with an explicit complete state and environment. `from_state`
admits structural states, including authentic frame-zero domain stops.
`from_supported_state` additionally checks zero-time physical support without
warmup. `parked` uses `RunningTurbopropStart` values and geometric gear placement;
it does not establish a stationary equilibrium. Restart constructs the complete
replacement before swapping the current host.

Start the recorder on a pristine host. Pass each returned `NearStaticTurbopropAdvance`
to `record` exactly once, including a terminal report. Each report carries complete
configuration/environment provenance and exact state/cursor continuity. An invalid
or overflowing whole report closes recording while preserving the previously
accepted exportable prefix. It does not freeze or mutate the live host. `export`
returns an independent valid snapshot; `finish` consumes the recorder.

`advance_with_controller` prepares controls on a cloned controller and commits
only accepted entire fixed steps. Its clone must isolate mutable state. All
six effective controls and all sixteen physical scalars are recorded exactly.
Terrain is held per attempt; wind/turbulence use prospective accepted tick time.
Modeled weather keeps its existing authored semantics and adds no hidden forces.

The player requires an exact supported model tuple/hash and exact world data.
Unknown nonzero identities and dataset fingerprints may be inspected and
exported but cannot reproduce. No identity is upgraded. On nonempty admission,
the first support query uses the first recorded prospective +dt weather instant.
Empty nonterminal records use time zero; frame-zero terminals reproduce the
recorded attempted controls and exact failure.

Playback, seek and rewind rebuild the full host from frame zero, restoring
interpolation endpoints, clock, terrain/clearances, controls, flight log and
contact history. Each call performs at most 240 attempts, including scratch
terminal work. A physical checkpoint never substitutes for a host snapshot.
Checkpoint mismatch rolls back the attempted host step. A terminal probe must
fail exactly and leave the full snapshot unchanged; unexpected success commits
nothing.

The little-endian wire envelope retains the 128-byte complete state and
48-byte control records, with a final full checkpoint even at zero steps.
V6 allocates separate near-static terminal tags 9..11 and diagnostic bits 9..11.
The maximum terminal is 158 bytes; the bounded record maximum is 49,100,927
bytes. The complete byte table, masks, evaluation-order constraints and limits
are in [ADR-0023](adr/0023-near-static-full-state-replay-v6.md).

Independent fixtures pin bytes, including distinct 121-control/checkpoint/tail
content and all new terminal cases. Numerical reconstruction tests separately
prove multi-step behavior. Same-build bit equality does not claim portable
libm trajectory identity. The accepted near-static approximation, persistent
brake creep and tailwind/domain limits remain unchanged; this is not app/native
acceptance, measured aircraft data or a production release.
