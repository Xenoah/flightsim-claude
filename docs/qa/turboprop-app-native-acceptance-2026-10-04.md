# Turboprop app, lateral trim and flight-help native acceptance

Date: 2026-10-04 UTC. Scoped native acceptance passed against reviewed source
`a416af1112ee5765f453d157263013366c2d5639`. Final runtime binary is built from
`0b1e973ebbbee36a29ba26b2710e60a7dbecfdac`; the intervening binding commit changes
only source verification/tests/documentation. Native observations used a Linux
cloud desktop with CPU llvmpipe. This is not a frame-rate or hardware-GPU result.

## Actual app behavior

Matched Swift ground cases 32 and 34 used the same location, heading, date/time,
Light quality and short distance. The accepted help sits to the right of the
central aircraft view. At 640x480 the compact guidance stays readable; the opaque
paused reference scrolls to the end, leaves attribution visible, and resumes
without retaining its scroll offset. This verifies the flight HUD/reference,
not every small-window modal: the world map still overflows at 640x480 and has a
separate bounded layout fix pending.

Actual held L/O inputs changed roll/yaw trim, Shift+L produced the fine rate,
restart cleared trims, and paused help/map-cancel gestures preserved recording
bytes. Case 34 exposed a missed brief K tap. The final sampler correction passed
107 input tests in a separately retained run; native case 36 then verified a
brief K resets both trims while Ctrl+K preserves the existing setting. Source
review documents the conservative same-frame modifier-edge behavior and the
intentional absence of queued reset commands across no-step frames.

Cedar case 35 loaded the original explicitly selected profile v3 and original
model. A 270-degree/12-kt wind, modeled turbine fraction, shaft speed and blade
pitch remained in the full state and recording. Manual lateral trim was applied;
this flight is app-path acceptance, not a steady hands-off handling claim. The
record contains 19,419 successful 1/120-s steps and 162 complete 128-byte
checkpoints, including the final state. Restart preserved all 16 authored initial
state words and reset explicit trims. Two paused exports were byte-identical.

Case 38 loaded that actual v5 record with the same profile, paused at displayed
0:57, sought back to 0:47 in bounded reconstruction chunks, and stayed paused.
Live roll/power/reset controls did not alter displayed state. The map showed
Cedar locked, recorded weather, and disabled new-flight start. Resuming and
selecting 2x transport reached REPLAY COMPLETE at 2:41/2:41 with the recorded
engine state. All five credited runs exited normally with exit code 0.

The intermediate help layout in case 33 was corrected before acceptance. Case
37 used an incorrect operator CLI flag and stopped before scene creation;
it receives no verification credit. The documented selector is `--aircraft FILE`.

## Independent numerical reproduction

Six authentic native Swift v3 files contain 178,120 total frames and 1,487 stored
104-byte checkpoints, including intentionally duplicated pause/cancel snapshots.
A separate public-API probe compared every stored checkpoint exactly, then
rewound and compared all 178,126 reconstructed states including frame zero.
Byte export, wrong-profile/family rejection, pause state and no paused-time
backlog checks passed. These v3 files do not contain mandatory final checkpoints:
the final 101/101/82/37/37/42-step tails establish repeatable reconstruction,
not comparison with an independently stored native final state.

The three actual Cedar v5 files contain 20,617 frames and 172 complete stored
checkpoints in total, including the duplicate paused pair. Independent production
reader/player reproduction and rewind passed. The probe's newer additive sim
library was source-bound against the accepted unchanged old host/codecs; it did
not replace or regenerate the native files. Full native artifacts, probe code,
source binding and raw results are retained in the accompanying QA evidence.
The [compact receipt](turboprop-app-native-acceptance-receipt.json) pins their
sizes and SHA-256 values, plus each native capture and launcher receipt.

## Remaining limits

Cedar remains an original experimental approximation selected by an external
profile. It is not registered as a public picker preset or added to the Swift
commercial bundle. The original propeller is static and audio is a generic
synthesizer; no measured propulsion, acoustic, stationary-parking or real-aircraft
performance qualification follows from this milestone. Law 2/profile 4/replay 6
are separate work and are not included here. Exact new-source Windows
verification, physical controllers and the existing binary rights/dependency
release gates remain separate. No binary release is authorized by this report.
