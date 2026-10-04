# ADR-0023: Isolate complete near-static turboprop replay in version 6

- Status: implemented and locally qualified; independent pure-host/codec review pending
- Date: 2026-10-04

## Explicit allocation and compatibility

Add `near_static_turboprop_simulation` and `replay_v6`. Their public host and
recording types are explicitly prefixed `NearStaticTurboprop`. Runtime accepts
only `flightsim_fdm::turboprop::near_static::TurbopropAircraftConfig`; no old
config is silently converted. Reproduction requires identity algorithm 1,
schema 4, kind 3, FDM law 2 and near-static host revision 1. Host revision 1 is
this separate host's prospective-clock/contact contract, not the FDM law number.
Version 6 and ADR-0023 are centrally allocated for this change.

Existing profiles, identities, hosts, codecs, dispatchers and v1-v5 fixtures
remain unchanged. In particular profile 3, `supported_turboprop` and replay 5
continue to select law 1. Profile 4 and schema 4 are the explicit law-2 boundary.
No app selection, physical-model change, aircraft preset or publication follows.

The accepted v5 host mechanics are copied into a separate typed host and parked
initializer. The new codec has a separate `terminal` module for its closed wire
contract. No old helper visibility or numerical semantics need change. Shared
existing environment/weather encoders and physical state/unit types retain their
existing meaning. The two new module exports are the only edits to existing
production source.

## Exact envelope and structural admission

All integers and binary64 values are little endian. The header is `FSREPLAY`,
u16 version 6, and u32 conditions byte count. Conditions are:

1. u32 UTF-8 aircraft-name length followed by at most 256 name bytes
2. u16 algorithm, u16 schema, u16 kind, u32 FDM law and u64 full physical hash
3. u32 near-static host revision
4. the unchanged 112-byte environment
5. u8 terrain kind (0 flat, 1 bundled), f64 elevation (bundled requires +0)
6. u32 weather byte length followed by unchanged weather bytes (at most 120)
7. sixteen f64 physical-state scalars

The state order is ECEF position XYZ, ECEF velocity XYZ, body-to-ECEF quaternion
XYZW, body angular rate XYZ, turbine fraction, relative shaft rad/s, blade pitch
radians. State length is 128 bytes; conditions length is 279 + name + weather,
bounded to 4096. Structural checks retain accepted signed-zero bits and all
existing finite/quaternion/geometric bounds, fraction [0,1], shaft [20,1000] and
pitch [0,pi/2]. Profile-specific runtime support is a separate check.

Next come u32 successful count N, N six-f64 controls, u32 checkpoint count,
checkpoints (u32 cursor + 128-byte state), and u32 terminal byte length followed
by an optional terminal. N is at most 1,000,000, checkpoints at most 8,334.
Required cursors are 120,240,... below N plus exactly one final N, including 0.
The full serialized maximum is 49,100,927 bytes. Counts are bounded before
allocation. These are wire/work bounds, not process-memory or timing claims.

Unknown nonzero identity algorithm/schema/kind/law allocations and unknown
nonzero dataset fingerprints remain structurally inspectable and exportable.
They cannot reproduce: the player requires the exact supported tuple and full
configuration hash plus exact supported world data. Zero identity allocation
numbers reject. Every accepted identity bit is preserved; inspection never
relabels old identities as the new physical law. Host revision remains exactly 1.

## Closed terminal wire table

A terminal is 62 + 8 * popcount(mask) bytes, at most 158: u32 cursor, six control
f64s, u8 reason, u16 detail, u8 stage, u32 substep, u16 mask, then diagnostic f64s
in increasing bit order. Zero length denotes no terminal. Stages are Initial 0,
K1 1, K2 2, K3 3, K4 4, Endpoint 5; substeps are 0..7, Initial requires 0.

Tags 1..8 retain the following meanings in the new v6 table. They do not extend
or reinterpret the v5 table:

1. Invalid input, details 1..16: timestep, rigid state, quaternion, position,
   atmosphere offset, ground, wind, relative velocity, temperature, density,
   turbine fraction, shaft rate, pitch, component query, derivative, intermediate
   state
2. Outside atmosphere altitude: one 2-bit axis
3. Outside envelope: pressure, temperature, Mach, relative shaft, absolute spin,
   tip Mach, crossflow, each 2 bits; bits 14..15 must be zero
4. Outside turbine map: pressure, temperature, each 2 bits
5. Outside propeller map: J, pitch, each 2 bits
6. Outside Mach-aero schedule: one 2-bit axis
7. Substep budget: detail 0
8. Forward propeller power bound: details 1 nonpositive CP with positive CT,
   2 below ideal disk bound, 3 invalid derived bound

New v6-only allocations are:

9. Near-static power bound: details 1 nonpositive CT, 2 nonpositive CP,
   3 below static floor, 4 invalid derived floor
10. Outside near-static domain: adverse and transverse axes, each 2 bits
11. Invalid near-static scale: detail 0

Axis codes are Below 0, Within 1, Above 2. Unused detail bits and code 3 reject;
outside reasons require at least one outside axis. Tip, crossflow, adverse and
transverse axes cannot be Below their inclusive-zero lower bounds. Tag 10's
axes must agree with the encoded ratios against the fixed maximum 0.10.

Diagnostic bits 0..8 are pressure ratio, temperature ratio, Mach, J, pitch,
relative shaft, absolute spin, tip Mach and crossflow. Bits 9..11 add hover
velocity m/s, adverse ratio and transverse ratio. Bits 12..15 reject. The only
established groups are 0, 0x30, 0x77, 0x1f7, 0x1ff and 0xfff. Absent means not
computed. Present values must be finite; pressure/Mach/tip/crossflow/transverse
are nonnegative; temperature/relative shaft/hover/adverse are positive; pitch
is [0,pi/2]. Full 0xfff requires J < 0. For every post-domain reason other than
tag 10, the full negative-flow ratios must be at most 0.10.

Reason/mask admission follows the FDM's precise evaluation order:

- Atmospheric altitude and early raw/intermediate-state failures: 0
- Atmosphere, wind, relative-air and early ground failures: 0x30
- Base envelope exit: 0x77, with tip/crossflow Within; later envelope or power
  map exit: 0x1f7
- Propeller map exit: 0x1ff; forward power-bound: 0x1ff with nonnegative J
- Near-static power-bound: 0x1ff with negative J
- Invalid near-static scale: 0x1f7 before J was established (underflow), or
  0x1ff with negative J before a valid induced-velocity triple was established
- Near-static domain exit: 0xfff, with exact above/within ratio statuses
- Aero, derivative and stage budget: 0x1ff with nonnegative J or 0xfff with
  negative J; outer-duration budget additionally permits Initial + mask 0
- Endpoint ground: the same post-flow group, only at Endpoint; component-query
  failures may use any established group

The shape contract does not assert every structurally admissible terminal is
reachable. Every reproduction requires the exact actual reason, detail, stage,
substep and optional diagnostic bits.

## Transaction, provenance and reconstruction

The committed host snapshot includes both complete physical endpoints, ground,
all gear clearances, elapsed physical time, log, airborne/contact history,
touchdown details/count and committed cursor. Effective controls are prepared
on an isolated clone and commit only after the entire FDM call succeeds. Failure
preserves the snapshot and controller; only the terminal latch changes and
unused frame budget is discarded. A controller clone must isolate mutable
state; external closure side effects are outside the contract.

Every report shares exact immutable origin bytes: NUL-terminated
`flightsim/near-static-turboprop-host`, u32 host revision 1, u32 physical-byte
length and the complete schema-4 canonical physical configuration, followed by canonical initial environment,
terrain, weather and source fingerprints. A reduced hash cannot replace these
bytes. Frame-zero start/heading use the same actual-state metadata as the
recording. Admission checks this origin plus complete before-state and cursor,
including terminal-only reports. Same-state/cursor foreign configuration or
environment cannot append. A whole report is validated before any append,
including prospective sequential duration and visual-time overflow. Recorder
failure closes only recording at its authentic exportable prior prefix; the
live host remains usable.

Seek/rewind reconstructs from frame zero. Checkpoints are comparison evidence,
not restorable host snapshots. A checkpoint mismatch restores the complete
prior host snapshot and cursor and discards the attempted replay budget.
Every public playback/seek call permits at most 240 execution attempts, counting
one scratch terminal probe. A terminal after the 240th success waits for the
next call. An unexpected-success probe never changes committed state.

Nonempty playback's first support query uses the first recorded prospective
elapsed +dt wind/turbulence time on a scratch state. Empty nonterminal records
use time zero; empty terminal records prove the exact recorded rejection.
`from_state` permits structural initial terminal evidence; `from_supported_state`
checks the actual zero-time environment without stepping. Parked initialization
solves geometry using explicitly authored engine state. No hidden warmup, trim,
state repair, velocity clamp, shaft floor or engine equilibrium is introduced.

## Evidence and limits

Independent Python encoding and distinct nonempty 121-control witnesses pin wire
order, periodic checkpoint and final-tail state. Runtime tests separately check
real multi-step, complete-host reconstruction and exact atomic failure stages.
All old codec fixtures and reference encoders remain unchanged. The original
bare-FDM law-1 boundary keeps its exact error pin. The unchanged v5 host's
explicit held-plane environmental context has its own exact pin, checked
against direct old-law FDM evaluation and v5 terminal reproduction. Law 2's
identical host initial attempt is separately recorded with schema 4 and replay
6. The two environmental contexts are not conflated.

Same-build/configuration bit equality does not promise cross-platform libm
trajectories. The scientific and original Cedar braking evidence remain bounded
experimental results, with brake creep, tailwind and authored-domain limits.
This host/codec does not qualify stationary parking, real-aircraft performance,
app/picker/native/GPU behavior or public distribution.
