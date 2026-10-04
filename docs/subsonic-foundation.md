# Stateless subsonic component foundation

This is an additive, opt-in `flightsim_fdm::subsonic` API. Its components now also
compose the separate [bounded jet runtime](subsonic-runtime.md). They remain
disconnected from legacy `FlightDynamics`, a selectable app aircraft and an
accepted release candidate. Existing
`AircraftConfig`, `EngineConfig`, propeller forces, FDM revision 2, profile v1,
replay identities and old goldens remain unchanged. Adding the module export
changes a frozen candidate source pin; that pin is deliberately not refreshed.

## Scope and physical interpretation

`DryJetTable` stores installed aggregate **net** thrust in newtons, indexed by
ambient static pressure ratio `p / 101325 Pa`, ambient static absolute temperature
ratio `T / 288.15 K`, and flight Mach `V / a`. The current atmosphere constants and
its local sound-speed sample provide those quantities. These pressure and
temperature ratios are not engine compressor ratios or stagnation/static ratios.

NASA's [turbofan thrust explanation](https://www.grc.nasa.gov/www/k-12/airplane/turbfan.html)
accounts for outgoing and incoming momentum in net thrust. This motivates a
separate net-thrust table, including signed values, rather than the existing
propeller power-over-speed law. It does not supply or validate any of our tables.
No generic density exponent, added ram correction, engine cycle calculation,
afterburner or manufactured real-engine performance claim is introduced.

Each cell contains idle and maximum-dry net thrust. Command 0 means authored idle,
not an engine-off switch; command 1 means authored maximum dry. The bounded
linear command interpolation is an approximation, not a shaft-speed, fuel-flow,
spool or throttle-lever model. Both thrust channels may be negative; maximum-dry
must be at least idle, an explicit monotone-command limitation of this component.
Zero ambient pressure rows must have zero thrust, an explicit mathematical vacuum
boundary. This boundary does not establish physical validity near vacuum or model
flameout. Static thrust at zero speed is an ordinary table endpoint.

`MachAeroSchedule` linearly interpolates all 25 existing aerodynamic coefficients
at ordered Mach knots. Coefficients and stall parameters are continuous; slopes
may change at a knot. No `1 / sqrt(1 - M²)` singularity is used. Numerical validation
does not establish static/dynamic stability, trimability, handling or flight safety.
A later aircraft profile must supply and validate its own operating envelope.

NASA's [Mach explanation](https://www.grc.nasa.gov/www/k-12/airplane/mach.html)
and [drag coefficient discussion](https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/drag-coefficient/)
explain why coefficients from a different Mach regime do not establish high-speed
accuracy. [Isentropic relations](https://www.grc.nasa.gov/www/k-12/airplane/isentrop.html)
distinguish static and total quantities; this API uses ambient static inputs and
performs no isentropic inlet conversion. The Mach ceiling below is an authored
numerical scope choice, not evidence of transonic accuracy. Reynolds effects,
buffet, inlet behavior, transonic/supersonic flight and real aircraft certification
are outside this foundation. Test tables are original numerical fixtures only.

## Bounds and unsupported inputs

| Quantity | Accepted table data |
| --- | --- |
| Knots per axis / aero schedule | 2–32 |
| Pressure × temperature × Mach cells | at most 4096, exact product required |
| Ordered-knot separation | at least `1e-9` |
| Pressure ratio | 0–2, first knot zero |
| Temperature ratio | 0.25–2 |
| Mach | 0–0.95, first knot zero |
| Idle and maximum-dry net thrust | −1,000,000 to +1,000,000 N each |
| General aerodynamic coefficient magnitude | at most 100 |
| Stall angle / blend rate | 0.05–0.7 rad / 0.1–100 |
| Minimum drag / Oswald efficiency | 0.0001–1 / 0.05–1 |
| Flap lift / drag increment | 0–100 |

These are resource/numerical policies, not universal atmospheric or aircraft
limits. Individual tables may cover narrower intervals. Exact endpoints are
inside the domain. Finite pressure and speed must be nonnegative; temperature
and sound speed must be positive. NaN/Inf, invalid signs, invalid command and
nonfinite derived ratios return `InvalidInput`. The atmospheric conversion reads
only pressure, temperature and sound speed: it does not validate unused density.

Finite otherwise-valid queries outside any actual table axis return
`OutsideJetDomain` with **all** axes marked Below/Within/Above, or
`OutsideMachDomain`. There is no returned fallback force/coefficient. No aircraft
velocity, height, atmosphere, throttle or query is silently clamped or continued.
The future simulator must explicitly choose and expose its unsupported-flight
handling, including RK4 intermediate stages. Reusing the previous force, freezing
an aircraft invisibly, or silently clipping its speed is not an implied policy.

Runtime types are immutable and cannot be deserialized directly. External DTOs
are untrusted until `from_definition` succeeds. Constructors check shape bounds
before deriving products or copying schedule arrays. Owners must additionally
limit serialized bytes before decoding the raw DTO vectors; post-decode axis
bounds do not prevent an oversized JSON allocation.

## Deterministic arithmetic and identity

Flattened jet cells are `[pressure][temperature][Mach]`, Mach fastest. Interpolate
Mach first, then temperature, then pressure, independently for the idle/dry
channels; interpolate commanded idle-to-dry thrust last. Each lerp is
`a + (b-a)*weight`, with exact endpoint branches. Bounded axes and values prevent
nonfinite intermediate results for valid supported inputs. Aero interpolation
uses the same Mach weight for every coefficient. There is no wall clock, random
source, mutable global state or hidden engine state.

`canonical_bytes()` exposes complete component bytes, **not** a compatibility
hash, recording format, or aircraft identity. All numbers preserve their actual
validated IEEE-754 bits, including signed zero. Each component starts with its
ASCII tag including the terminating zero, then local schema `u16` little endian:

- Jet: `flightsim/subsonic-dry-jet\0`; for pressure, temperature and Mach in order,
  `u32` little-endian length then each `f64` little-endian bit pattern; then `u32`
  cell count and each `idle_n, maximum_dry_n` pair in flattened order
- Aero: `flightsim/subsonic-mach-aero\0`; `u32` knot count and all Mach bits; then
  each knot's 25 scalar bits in `AerodynamicDefinition` declaration order
  (`lift_zero` through `yaw_rudder`), with the stall angle in radians

Local schema 1 commits to the units, bounds, ordering, arithmetic, idle meaning
and reject-outside policy. A future semantic change needs an explicit component
revision and host model identity decision. The host must also identify aircraft
model kind/revision, mass, geometry, gear and every other force-affecting field.
Never put these bytes into the frozen legacy profile-v1/replay identity path or
infer propulsion type from an aircraft name.

**JSON precision caveat:** existing workspace `serde_json` does not enable
`float_roundtrip`. In tests, a computed f64 `-30.400000000000002` serialized to that
shortest decimal but decoded as `-30.4`, a one-ULP change. The component API does
not claim that arbitrary raw-DTO JSON export/reparse retains original f64 bits.
Identity must describe the actual validated decoded values. Profile/model-v2
integration must deliberately choose an exact numeric decoder/encoding and test
it. Enabling a global parsing feature here would also change legacy parsing, so
this foundation leaves dependencies untouched. Exact-binary fixture values do
round-trip, and exact component bits are pinned independently of JSON formatting.

## Verification and remaining integration

`tests/subsonic_tables.rs` compares interpolation with an independent multilinear
polynomial and an eight-corner tensor-product sum; tests exact knots, both sides
of internal boundaries, atmosphere SI conversion, vacuum/static/signed thrust,
malformed shapes, count limits, invalid/nonfinite inputs, outside-domain errors,
all 25 aerodynamic field effects, every thrust-cell identity effect and signed
zero. `docs/qa/subsonic_reference.py` independently encodes the written byte
contract and checks the committed golden JSON fixture; it does not read Rust.

Verified in this isolated checkout on 2026-10-04: 244 core/FDM tests including
doctests (13 new foundation tests), strict core/FDM all-target Clippy, format,
architecture and the independent Python fixture check passed. Independent
physics/API review approved the additive foundation with no blocking findings
and separately reran the Python fixture check. An initial broad
JSON round-trip assertion failed and exposed the documented decoder limitation;
the passing round-trip coverage is explicitly limited to exactly representable
fixture numbers. No sim/app, GPU or native acceptance is claimed.

Run with the repository's serialized build lane:

```sh
cargo test -j2 -p flightsim-core -p flightsim-fdm
cargo clippy -j2 -p flightsim-core -p flightsim-fdm --all-targets -- -D warnings
python3 docs/qa/subsonic_reference.py
bash scripts/check-architecture.sh
```

Before any flyable jet can be admitted: implement explicit model/profile-v2
dispatch and exact numeric parsing; version and verify complete model-specific
replay identity; integrate the bounded runtime's explicit transactional rejection policy into the host;
author and validate an aircraft's tables and envelope; test trim, trajectories,
controls and replay; migrate the candidate source contract through its separate
review. No spool, fuel or variable gear state is added until its complete state
and replay contract is versioned. Native/GPU performance and handling acceptance
remain separate; this work makes no measured performance claim.
