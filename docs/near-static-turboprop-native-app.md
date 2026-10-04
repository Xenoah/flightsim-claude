# Explicit near-static turboprop app sessions

Development builds accept external profile version 4, selecting near-static law
2 and full-state replay version 6. Profile 3 continues to select law 1 and replay
5. Neither route converts the other. The retained forward physical definition
and all old replay fixtures are unchanged.

A wrong-family replay error identifies its required profile version and asks for
`--aircraft FILE`. A selected legacy or jet reader rejecting v5/v6 does not mean
that the whole development app lacks those formats. Commercial-staging errors
state that the experimental turboprop families require a development build.
No format sniffing, additional file reads or automatic profile substitution is used.

```sh
cargo run -j2 -p flightsim-app -- \
  --aircraft path/to/exact-profile-v4.json --global-terrain off \
  --climate off --wind 0/0 --turbulence calm --no-model

cargo run -j2 -p flightsim-app -- \
  --aircraft path/to/exact-profile-v4.json --global-terrain off \
  --replay flight-001.fsreplay --no-model
```

These are route examples, not a promise that arbitrary profile hints and weather
are supported. Live startup validates the actual body and authored engine state
at zero duration. There is no warmup, automatic trim, settling phase or state
repair. The numerical example under `docs/examples/aircraft-profiles-v4/` tests
software boundaries; it is not an aircraft preset or a performance specification.

The explicit launch choice can be retained through the existing new-flight map
transaction, including its model/no-model, fit, camera and synthetic sound choice.
Cancel, changed destination/weather/forces and late scene completion retain the
current flight. Only successful new flight or live restart resets lateral trims.
J/L roll trim, U/O yaw trim, Shift fine and K reset retain their existing controls.

Live and replay each own all sixteen physical scalars: rigid body, turbine
fraction, relative shaft speed and blade pitch. UI interpolates body pose only;
it never replaces full simulation state. The notice identifies experimental law
2 and labels turbine fraction as modeled x rather than N1, static model rotor,
and synthetic audio. No landing grading or acoustic fidelity is added.

F9 saves v6. F5 pauses, F6/F7 change replay speed, and F8 rewinds through bounded
frame-zero reconstruction. Replay keeps recorded engine, wind, turbulence,
weather, source and visual clock; profile running-start defaults do not replace
recorded values. Unsupported terminal evidence remains reproducible. Manual
cloud overrides disable live recording and cannot override replay. Recording
failure and live sun-rate changes close an authentic exportable prefix.

Only bundled global or explicitly flat-zero terrain is supported. Raw tiles and
regional packages cannot replace these sources. Commercial-staging rejects this
new family's live/replay startup. No Cedar picker entry, distribution allowance,
asset/rights approval or source-pin relaxation is introduced.

Near-static support is limited to fixed J[-0.01,0), adverse/transverse induced-
speed ratios <=0.10 and the conservative static power floor. The separate Cedar
braking qualification preserves forward coefficients and demonstrates only its
reported conditions. Approximately 0.20062 m of braked idle creep over ten
seconds remains, so stationary parking is not qualified. Flight and wind
qualification, native v6 interaction/visual checks, speaker listening and release
acceptance remain separate gates.
