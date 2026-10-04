# ADR-0019: Preserve complete turboprop state in an additive replay format

- Status: accepted pure host/replay integration; app and preset qualification remain separate
- Date: 2026-10-04

## Decision

Keep `replay_v5` and `turboprop_simulation` separate from all existing codecs and
simulation laws. Format 5 requires identity algorithm 1/schema 3/kind 3/FDM-law 1
and separately named turboprop simulation law 1. The v4 writer still writes 4;
existing model support gates remain jet-only. The new explicit reader is not
added to an old dispatch enum.

A complete state is the existing 13 rigid f64s then turbine fraction, relative
shaft rad/s and blade pitch radians:128 bytes. Conditions are 279+UTF-8 name bytes
+weather bytes, at most 4096. Environment 112 bytes, terrain tag/elevation and
weather encoding are unchanged from the integrated v4 boundary. Controls are six
f64 values per successful 1/120s step. Each checkpoint is u32 cursor+128-byte state.
Periodic cursors 120,240,... belowN plus mandatory finalN include final 0.

Limits stay 1,000,000 successful steps,8,334 checkpoints and 240 execution attempts
per public playback/seek call. One terminal scratch attempt counts once without
advancing time. With 256 name bytes and 120 weather bytes the maximum serialized
size is 49,100,903 bytes. Counts are checked before allocation; serialized bounds
are not process-memory or wall-time measurements.

Structural inspection/export validates finite state, existing rigid-body unit
quaternion and geometric bounds, turbine fraction[0,1], relative shaft[20,1000]
rad/s and pitch[0,pi/2]. It preserves all accepted bits, including signed zero.
Unknown nonzero bundled dataset fingerprints remain inspectable/exportable;
reproduction requires exact supported data plus complete model identity. Actual
profile pitch stops, shaft envelope and environmental support remain runtime
requirements. No default engine state is substituted into a recording.

## Closed terminal wire table

A u32 length is zero or `62+8*popcount(mask)`, at most 134. The nonempty block is:
u32 cursor; six f64 effective controls; u8 reason; u16 detail; u8 stage; u32
substep; u16 diagnostic mask; selected f64 diagnostics in increasing bit order.
All fields are explicitly little endian; no Rust enum memory is serialized.

Reason tags are fixed for this format and do not extend v4:

1. Invalid input: details 1..16 are timestep, rigid state, quaternion, position,
   atmosphere offset, ground, wind, relative velocity, temperature, density,
   turbine fraction, shaft rate, blade pitch, derived/component query,
   derivative, intermediate state
2. Outside atmospheric altitude: one axis code
3. Outside envelope: seven 2-bit axes pressure, temperature, Mach, relative
   shaft, absolute spin, tip Mach, crossflow; bits 14..15 reserved zero
4. Outside turbine power map: pressure and temperature 2-bit axes
5. Outside propeller map: J and pitch 2-bit axes
6. Outside aero schedule: one Mach axis
7. Substep budget: detail 0
8. Propeller power bound: detail 1 nonpositive CP with positive CT;2 below ideal
   disk bound;3 nonfinite or underflowed positive derived bound

Axis codes Below 0/Within 1/Above 2 are closed; an outside reason requires at least
one outside axis. Tip/crossflow cannot be Below their inclusive-zero lower bound.
Stages Initial 0/K 1=1/K 2=2/K 3=3/K 4=4/Endpoint 5 and substeps 0..7 are closed; Initial
requires substep 0.

Mask bits 0..8 are pressure ratio, temperature ratio, Mach, J, pitch, relative
shaft, absolute spin, tip Mach, crossflow. Higher bits reject. Law 1 establishes
only groups 0,0x30,0x77,0x1f7,0x1ff in evaluation order. Present values must be
finite; pressure/Mach/tip/crossflow nonnegative; temperature/relative shaft
positive; pitch[0,pi/2]. J and absolute spin may be signed at a domain exit.
Absence means not yet computed, never a fabricated zero.

Reason/mask consistency is part of admission: atmospheric altitude and early raw
state failures use 0; environmental/relative-air/atmospheric failures use 0x30;
base-envelope exits use 0x77 with Within tip/crossflow; tip/crossflow exits and
power-map exits use 0x1f7; propeller/aero/power-bound failures use 0x1ff. Budget
uses 0x1ff or Initial+0 for an outer-duration failure. Derivative uses 0x1ff,
intermediate state uses 0; ground permits 0x30 or endpoint 0x1ff. Derived-query
failures may occur at any established group. Runtime reproduction still verifies
all exact failure bits; structural admission is not proof that an error occurs.

## Host transactions and reconstruction

The committed host snapshot carries both full interpolation endpoints, ground,
all gear clearances, executed time, log, airborne/contact history, touchdown and
step count. Controls are proposed on an isolated clone and committed only after
an accepted full FDM step. Rejections preserve that snapshot and controller;
only terminal latch changes and unused frame budget is discarded. The recorder
accepts whole contiguous reports and compares all 16 state scalars; a report error
closes an authentic prior prefix atomically. The prospective cumulative
visual-time product is validated before any report is appended.

Replay seeks always reconstruct from frame zero using recorded environment,
wind/turbulence seed and prospective executed time. Physical checkpoints are
comparison evidence, not restorable host snapshots. Corrupt checkpoint failures
roll back the attempted step and discard the remaining replay frame budget.
Terminal checks use a scratch full FDM state with the actual prospective host
clock/ground, require an exact error and unchanged committed snapshot, then
latch the verified terminal. Unexpected success never commits a step.

`from_state` permits authentic frame-zero domain-stop evidence.
`from_supported_state` evaluates the actual start at zero duration;
nonempty playback admission uses the first recorded attempt's prospective +dt
wind clock on scratch state (empty nonterminal playback uses time zero); parked
initialization solves only geometry and uses explicit authored engine values.
No hidden warmup, steady-state claim, engine RPM floor or body velocity clamp is
introduced. A restart constructs a complete replacement before committing.

## Compatibility and scope

Exact reproduction means same build/configuration, not cross-platform libm
identity. Independent Python bytes pin the layout; numerical runtime tests pin
real playback, rollback and reconstruction. Existing goldens remain unchanged.
This adds no aircraft preset, app/picker family, model, audio, renderer, native
acceptance, measured propulsion data, or real-aircraft performance qualification.
