# ADR-0022: Bind explicit near-static law 2 to profile 4 and complete identity 4

- Status: additive implementation; independent review remains a separate gate
- Date: 2026-10-04

## Version allocation and scope

Allocate concrete `AircraftProfileV4`, physical identity algorithm 1 / schema 4 /
kind 3 / law 2 and propeller wrapper component schema 2. These numbers were
checked against the accepted law-2 base and centrally allocated before changes.
ADR-0021 belongs to UI layout. Replay 6 is reserved for a separate task and is
not implemented here. Every old profile, identity, replay and host gate stays
closed to the new law. No dispatcher, app, picker, preset or runtime law changes.

## Explicit original-token input

The new concrete loader requires version 4 and dynamics
`running_turboprop_table` revision 2. Its retained airframe, turbine, governor,
Mach-aero schedule, envelope, display metadata and running-start DTOs reuse the
existing original-token parser, with only crate-private visibility changes to
the old definitions. No schema-3 input is interpreted as law 2 or converted.

Propeller schema 2 requires a nested `forward` schema-1 definition, two explicit
negative rows, their exact fixed J coordinates [-0.01,-0.005], and a required
`near_static_domain` object. Domain values commit to the accepted law: adverse/vh
and transverse/vh maxima 0.10; 64 binary64 epsilons in the power-floor check;
signed pitch-then-J interpolation, positive static actuator-disk floor, and
current-positive-thrust hover-velocity scale. Constants compare binary64 bits;
semantics compare exact closed strings. Different values reject, never repair,
clamp or change FDM semantics. Equivalent decimals may round to the same bits.

The new configuration retains the whole validated law-1 configuration via
`near_static::TurbopropAircraftConfig::from_forward`, then validates both rows
and the shared J=0 closure. The combined grid is at most 32 by 32 nodes (30
forward J knots plus two negative knots). Row lengths must match the forward
pitch axis. The positive static floor and monotone CP are physical admission
conditions. Actual adverse/transverse ratios and all old envelope constraints
are still checked at each runtime force evaluation, including intermediate
stages; parsing is not evidence of a supported initial condition or trajectory.

Duplicate/unknown keys reject at every nesting level. Input remains bounded to
1 MiB, 128 JSON containers, 128 bytes per physical/metadata numeric token, and
existing decoder-bound arrays. Integer tags retain integer-token grammar.
Correctly rounded conversion consumes the original token exactly once, retaining
signed zero/subnormals where physically admissible, rejecting nonfinite values
and nonzero decimal underflow to zero. Export preserves accepted bits, not source
spelling. The public JSON Schema is explicitly a weaker structural check.

## Complete canonical composition

The outer canonical header is the existing NUL-terminated identity domain plus
u16 algorithm 1, u16 schema 4, u16 kind 3 and u32 law 2. All fields are little
endian. Two u32-byte-length-prefixed components follow:

1. The COMPLETE unchanged `canonical_turboprop_bytes(forward_config)` sequence,
   including its schema-3/law-1 header. This identifies the retained forward
   configuration only; it does not select the runtime. It contains mass, full
   locked inertia, geometry, ordered gear/friction, all envelope bounds,
   turbine map/dynamics, forward propeller map/rotor/sense, governor and every
   Mach-aero coefficient including yaw_rate_p. A digest is never substituted.
2. The schema-2 extension described by the byte table in
   [the format contract](../aircraft-profile-v4.md). Every negative CT/CP pair,
   both fixed negative J knots, all three fixed domain scalars and the three
   closed semantic tags are present. Counts and row order are explicit.

FNV-1a64 hashes the whole sequence. It detects changes, not malicious collisions.
Names, display/model metadata, controls metadata and explicit running-start
values remain excluded. Cached reduced inertia remains derived from the
included complete physical definition. Tuple support is separately named
`supported_near_static_turboprop`; it neither matches fingerprints nor admits
any recording or real runtime state. Old support helpers are unchanged.

## Evidence and boundaries

Independent Python encoding reads the original JSON, reuses the independent
schema-3 specification encoder and implements the new extension separately.
It pins every canonical byte. Tests perturb every independently configurable
physical scalar, including new negative cells and yaw_rate_p. A private encoder
witness changes each fixed scalar/tag to prove its byte and digest participation;
the public loader separately rejects those unsupported changes. LF/CRLF input,
exact decimal ties, signed zero, subnormals, underflow and export are covered.
These deterministic encoding checks are portable; local execution does not
claim cross-platform runtime/libm trajectory identity or a remote CI run.

The example is original synthetic numerical data, derived from the prior
numerical fixture with simple explicit negative rows. It is not Cedar, measured
propeller data, a performance rating, a parked equilibrium or a selectable
preset. Law-2 approximation limits from ADR-0020 remain unchanged. Profile,
identity, replay, host, native operation, aircraft qualification and public
release are separate acceptance gates.
