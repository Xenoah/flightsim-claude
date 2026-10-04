# Opt-in running-turboprop app sessions

The native app admits exact external profile v3 as a third explicit physical
family and reproduces matching complete-state replay v5. This integration is
separate from the qualification of an authored aircraft preset. No Cedar picker
entry, coefficient tuning, commercial distribution allowance, or rights gate is
introduced here. Numerical fixtures exercise software boundaries; they are not
real-aircraft performance or handling data.

Commercial-staging rejects new profile-v3 live and replay startup before source
reads or model/session preparation. Its existing v1/v2 inspection exceptions
and the pure v3/v5 library APIs remain available; the bundled aircraft and picker
remain Swift-only. Normal development builds support the explicit v3 route.

The version probe passes the original bounded JSON bytes to the v1, v2 or v3
decoder without a float-valued intermediary. Selecting a v3 file is the opt-in.
The launch entry retains that validated profile and its explicit model/no-model,
fit and sound selections. The existing Swift, Meadow and Kestrel entries keep
their exact required families; the commercial picker remains Swift-only.

```sh
# Controlled synthetic profile; no provided model and no handling qualification.
cargo run -j2 -p flightsim-app -- \
  --aircraft docs/examples/aircraft-profiles-v3/numerical-turboprop.json \
  --global-terrain off --climate off --wind 0/0 --turbulence calm --no-model

# A v5 recording requires the exact physical profile that produced it.
cargo run -j2 -p flightsim-app -- \
  --aircraft path/to/exact-profile-v3.json --global-terrain off \
  --replay flight-001.fsreplay --no-model
```

These commands describe the route, not a promise that arbitrary profile hints,
weather or starting positions remain inside a model's operating domain. Live
admission evaluates the actual 16-state start at zero duration. It does not run
a hidden settling phase. Unsupported live starts fail before replacing the
current flight. Authentic terminal-at-zero v5 records remain reproducible.

## State and ownership

`TurbopropLive` owns its complete simulation and recorder. `TurbopropReplay`
owns one v5 player and reads that player's simulation. A body-only render bridge
borrows the rigid-body member; the authoritative state always retains turbine
fraction, relative shaft speed and blade pitch. Live init and restart use the
profile's exact running-start values. Replay uses the 16 recorded scalars and
never substitutes profile defaults for engine state.

Live controls, lateral trim and a parking toggle are proposed per fixed step and
commit only when the full physical step succeeds. Rejection preserves body,
engine, controller, clock, contact history and recorder prefix, then displays the
terminal reason. No RPM floor, shaft clamp, automatic compensation or attitude
hold is added. F9 exports a non-consuming complete-state snapshot; a recording
error closes its authentic prefix while live flight can continue. A live sun-rate
change also closes recording because v5 stores one initial visual rate.

Map Start retains the existing staged model transaction. It prepares profile,
full session, recorder, wind, authored turbulence, visual weather, climate,
controls and camera before committing. All raw/regional selection stages reject
for the bounded families. Failed requests retain the active physical terrain
sampler, render source and pending choice. Cancel removes only staged work.

Replay locks new-flight, weather and wind editing. Recorded conditions, source,
climate and prospective wind time are authoritative. F5 pause, F6/F7 speed and
F8 bounded rewind preserve the v5 reconstruction contract. Time of day follows
recorded physical time, including stopped and seeking states. Sound is muted for
paused, seeking, completed and faulted replay. V1–v4 codecs remain unchanged.

## Explicit lateral trim

J/L decreases/increases aileron trim. U/O decreases/increases rudder trim. Hold
Shift for fine adjustment; K resets both lateral trims. Normal rate is 0.01
normalized control units/s and fine rate is 0.002/s, integrated at fixed-step
cadence with bounds ±0.2. The settings hold after key release. Ctrl/Alt/Super
shortcuts cannot change them. Existing elevator trim keys remain unchanged.

Both new settings start at zero for every aircraft, and every explicit new
flight or live restart resets them to zero. Pause, focus loss and map/editor
capture release transient input but preserve the committed trims. The effective
summed surfaces are recorded; replay does not reconstruct a separate trim knob
history. Zero lateral trim bypasses addition and saturation entirely, preserving
old signed-zero/control arithmetic. No profile or input-configuration schema is
extended. Five-decimal settings appear for live turboprops and whenever nonzero
on another live family; the old zero-trim HUD layout stays unchanged.

## Presentation limits

The persistent notice labels power command, modeled turbine fraction x, relative
shaft rad/s and blade pitch. x is not measured N1 and shaft speed is not inferred
from sound or throttle. Read-only atmosphere, wind-relative air data, contact and
supported aero coefficients use the committed physical clock. Unsupported stall
information is unavailable rather than extrapolated. There is no legacy landing
grade for this family.

Original model files use the existing GLB loader, staging, fit and cockpit path.
The accepted Cedar asset has a static `Cedar propeller assembly` root. This app
change does not animate the rotor or claim acoustic realism: audio is the existing
synthetic selected category. Legacy aircraft retain their geometry, materials,
cockpit presentation and F1–F4 rendering controls.

## Source-review findings

The old picker used `!is_jet()` to identify Swift/Meadow dynamics. On the accepted
two-family baseline that meant legacy; after v3 admission it would also accept a
turboprop whose metadata impersonated the preset. The new explicit `is_legacy()`
check prevents that third-family regression. This was not a reachable baseline
v3 route.

The baseline direct `prepare_world_map_flight` helper assigned its candidate
`active_region = package` before validating jet source support. A direct call
with an active package and no replacement could erase the source from the local
candidate before validation. The ordinary UI already guarded all source stages.
Validation now occurs before candidate reassignment and also rejects a supplied
regional package. This is defense of the preparation boundary, not a claim of a
previously demonstrated user-facing transaction failure.

Automated source/lifecycle/transaction coverage and native acceptance must be
reported separately. Native visual, manual handling, speaker listening and
platform acceptance are not established by the GPU-free tests.
