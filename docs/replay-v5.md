# Full-state turboprop replay v5

`flightsim_sim::replay_v5` is an explicit additive codec, recorder and player for
the running-only turboprop physical family. Version 5 and its law allocations
are fixed by the accepted pure-layer integration. Old readers/writers, identity
gates and numerical laws are unchanged; no app dispatch is added.

## Using the pure host

`TurbopropSimulation::from_supported_state(config, complete_state, environment,
controls)` evaluates a live start at zero duration. It advances no clock and
changes no state bits. `from_state` instead admits structural full states so a
recording can prove an authentic domain rejection at frame zero. `parked` uses an
explicit validated profile `RunningTurbopropStart` and solves only three-wheel
geometry against the sampled plane. It never spins up an engine or solves a
hidden equilibrium. `restart_at` is the structural replacement equivalent of
`from_state`; `restart_parked_at` requires actual parked environmental support.
Both construct a replacement before changing the current host.

The environment supports flat terrain or the exact bundled global source and
optional bundled climate, complete fixed wind/turbulence parameters, start
Julian epoch, visual time rate and authored weather. The physical weather clock
is the elapsed time of accepted 120 Hz steps. Wind is sampled at each prospective
step's time. Terrain is frozen during each FDM attempt. Authored weather retains
its existing interpretation; its visual effects do not add hidden FDM forcing.

`advance_with_controller` prepares effective inputs on a cloned controller.
A successful entire fixed step commits both controller and complete host state;
a rejection latches its typed event, discards unused frame budget, and preserves
both physical endpoints, ground/clearances, elapsed time, contact/log histories
and controller. Clone must isolate mutable controller state; shared interior
mutability or closure side effects are outside that transactional contract.

Start `TurbopropRecorder` on a pristine simulation. Record each returned
`TurbopropAdvance` once, including terminal reports. Continuity compares all 16
scalars, including the before-state of rejected attempts. Each report also binds
exact immutable canonical physical bytes and the complete environment/terrain/
weather bytes to the recorder; a foreign source cannot append even if state and
cursor match. Prospective cumulative visual time is checked before any append. Invalid/capped/noncontiguous reports close the recorder at its authentic
prior prefix without partial appends. `export` creates an independently owned
validated-shape snapshot without closing ongoing recording. `finish` consumes it.

## Wire boundary

All fields are little endian. Header is 8-byte `FSREPLAY`, u16 version 5, u32
conditions length. Conditions contain:

1. u32 UTF-8 name length and name bytes (at most 256)
2. model identity: u16 algorithm 1, schema 3, kind 3; u32 law 1; u64 physical hash
3. u32 turboprop simulation law 1
4. unchanged 112-byte environment
5. u8 terrain kind 0 flat/1 bundled, f64 elevation (bundled requires positive 0)
6. u32 weather byte length and unchanged weather payload (at most 120)
7. sixteen state f64 values

The 16-state order is ECEF position XYZ, ECEF velocity XYZ, body-to-ECEF
quaternion XYZW, body angular rate XYZ, turbine fraction, relative shaft rad/s,
blade pitch rad. Initial conditions occupy 279+name+weather bytes (cap 4096).
No accepted float is normalized or repaired. The rigid 13-scalar validation is
unchanged from the jet structural boundary; additionally fraction[0,1], shaft
[20,1000] and pitch[0,pi/2] must be finite. Actual profile support is distinct.

Then: u32 successful count N (at most 1,000,000), six effective control f64s per
step; u32 checkpoint count (at most 8,334); u32 cursor and 128-byte state per
checkpoint; u32 terminal length and optional terminal. Checkpoints are 120,240,...
belowN plus mandatory finalN, including 0. No state-only checkpoint shortcut can
restore contact or controller history.

The closed terminal reasons, details, stages and diagnostic-mask semantics are
in [ADR-0019](adr/0019-turboprop-full-state-replay-v5.md). Its fixed block is 62
bytes plus 8 per present diagnostic, maximum 134; unused bits and inconsistent
lengths reject. A missing diagnostic means it was not computed, rather than zero.
The maximum encoded length is 49,100,903 bytes; this is neither a memory nor
latency guarantee. `read_from` consumes one bounded record and leaves outer
trailing bytes available to its caller, matching existing codec composition.

## Exact playback and limits

`TurbopropReplayPlayer::new` requires complete matching physical identity and
known exact dataset fingerprints. Structurally valid unknown nonzero dataset
fingerprints can be inspected and re-exported but cannot be reproduced. The
actual profile/environment is checked without warmup. Nonempty records use a
zero-duration scratch query at the first attempt's prospective +dt wind clock,
with its actual recorded controls. Empty nonterminal records use time zero; an
explicit terminal-at-zero is proved by its recorded attempted controls.

Each playback/seek call performs at most 240 execution attempts. `seek_to` and
`continue_seek` rebuild from frame zero, restoring environment, physical time,
controls, interpolation endpoints, logs and contact history. Successful outputs
must match every 128-byte checkpoint exactly. A checkpoint mismatch rolls back
the attempted full host step and cursor and drops the remaining frame budget.

A terminal probe uses cloned full FDM state with the host's prospective ground
and clock. It must reproduce the exact reason/detail/stage/substep/diagnostic
bits while preserving the full committed snapshot. Unexpected success never
commits. The terminal is latched only after matching evidence. A terminal at
cursor 240 is deferred when 240 successful attempts already consumed this call.

Bit equality is a same-build/configuration guarantee; no cross-platform libm
claim is made. Fixtures and tests use original numerical configurations, including
an axial ballistic fixture to isolate replay behavior. They do not qualify a
flyable aircraft, engine start/shutdown, engine damage or a real propeller map.
There is no app/picker/presentation/preset integration in this change.

## Independent and runtime checks

`python docs/qa/replay_v5_reference.py` reconstructs 21 golden byte files from an
independent Python encoder, including all closed reason categories, every
current weather shape, and a structural 121-control witness with distinguishable
periodic/final full states. It also checks the total wire bound arithmetically.
Existing v3/v4 encoders and their files are unchanged. Rust tests test all
truncation points, malformed counts/tags/masks/numeric fields, real multistep
record/playback, engine drift and recorder continuity, terminal failures,
rollback and frame-zero reconstruction. See the dated QA report for executed
commands and remaining limits.
