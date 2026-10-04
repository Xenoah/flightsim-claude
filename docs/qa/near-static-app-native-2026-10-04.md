# Profile 4 / law 2 / replay 6: actual app verification

## Tested runtime

The native executable was built from `b24aa55fd0983f1750a4aad57cb097e27db6ae09`
with `region-downloads`. Its SHA-256 is
`211d46a653ca801bb1cf0fff9d324508555d5022ef65552d1a9af435960b3a46`
(155,225,168 bytes). The subsequent `47b8a2f` commit changes source qualification
bindings only. These observations use the cloud Linux CPU renderer; they do not
qualify a hardware GPU, sound output, Windows distribution or measured aircraft.

The external Cedar profile is an original experimental approximation, with
profile schema 4, propulsion kind 3, law revision 2 and canonical fingerprint
`3b9ca42bba4c6eaa`. Its original forward-flow configuration is unchanged. The
authored initial engine state is turbine response 0, shaft speed 180 rad/s and
blade pitch 0.08 rad. Cedar remains an explicit development profile, not a
registered or commercially bundled preset.

## Ground session and cancellation

Native case 41 launched the actual profile at the authored ground start, in
calm wind. The live turbine, shaft speed and blade pitch evolved. Manual roll
and yaw trim changed with L/O. The documented alternate throttle key `=` raised
power to 33%; the cloud input route's Page_Up gesture did not register and is
not credited as a successful throttle test.

The flight was paused and saved with F9. Selecting Swift in the map changed
only the draft. Closing it and saving again produced byte-identical files 016
and 017: 352,110 bytes, SHA-256
`101780cbf504082648807eafa48c36d1d31c38ecdc3f5991baec7b775add16ba`.
After the throttle change, file 018 retained the earlier control prefix and
added the evolved engine state. Restart restored the complete authored initial
state and zero power/roll/yaw trim in file 019. All fifteen earlier recordings
were preserved unchanged.

An independent same-build verifier reproduced 25,265 steps and 212 stored
128-byte checkpoints across files 016–019. All sixteen scalar state fields,
including the mandatory final checkpoint, matched exactly. Per-step full-host
reconstruction, bounded seek, rewind and pause checks passed. File 018's final
engine state was approximately `(0.32994105536, 180.04677510, 0.29953560998)`.
These are deterministic reconstruction checks, not a cross-platform libm or
physical flight-envelope claim.

## Native replay

Case 42 opened file 018 with the matching profile. F5 paused playback; live
throttle, trim and restart inputs did not change the recorded session. F8
performed bounded reconstruction from 0:43 to the requested 0:33 and remained
paused. Aircraft selection, wind editing and Start were disabled in the map;
clicking them and returning retained the same state. Playback then reached
the actual 1:22 final checkpoint, showing 33% power, turbine response 0.330,
shaft speed 180.05 rad/s and blade pitch 17.16 degrees. The app exited normally.

This run identified the older shared replay HUD behavior that displayed trim
as zero. Recordings contain combined effective elevator input, so independent
trim cannot be recovered from that value. The following presentation correction
marks replay trim unavailable instead of inventing a separate trim setting;
it does not change recorded controls, identity or physics.

The corrected executable was then built from
`53d5b3abffc28cf9d089ae5145d3e06b572c31ce`, SHA-256
`ab6ca568de134e968417246546e64966724b6e8d79bd2999dcd81ef26cb4f982`
(155,227,408 bytes). Native case 44 replayed the actual airborne file 023 to
completion and saved a PNG showing `TRM N/A`, the correct Cedar model and the
recorded engine state. Case 45 launched the same live profile and saved a PNG
showing `TRM 0.04 nose up`. Both normal screenshot-and-exit runs finished with
exit code zero. The correction passed 363 app and 246 UI tests, plus ordinary
and combined commercial/region strict Clippy; one pre-existing optional
regional-fixture app test was not run. The source/codec/input paths governing
flight physics and replay identity were unchanged.

## Cross-family transactions

Case 43 used global terrain, calm wind and the same original external profile.
Actual map Start transactions changed Cedar to Swift Sport, then Kestrel Jet
Trainer, then the original Cedar launch profile. Draft selection retained the
current aircraft until preparation and commit. Each accepted transaction
changed the visible model, control guide and propulsion HUD together and began
a fresh recording. The resulting wire versions were 6, 3, 4 and 6 in files
020–023. Returning to Cedar restored the authored engine initial state instead
of retaining jet state. Native cases 41, 42 and 43 all exited with code zero.

Independent reconstruction of these four files passed 16,672 steps and 142
stored checkpoints. The canonical fingerprints were Cedar
`3b9ca42bba4c6eaa`, Swift `172167174cf90012`, and Kestrel
`e58eb47f1e6952d7`. The v4/v6 mandatory final checkpoints matched all state
bytes. Swift's last 54 steps reconstruct and rewind deterministically, but v3
has no independently stored final checkpoint for that tail.

The map creates a new flight using its selected monthly preview. Initial
file 020 has climate disabled and June 21, while map-created 021–023 have
climate enabled, the June 15 representative date and climate fingerprint
`33f6a12038b9f6b8`. Wind remains calm. The recordings reproduce these actual
conditions exactly; this test does not claim that a new map flight preserves
the initial command-line climate switch or exact day of month.

## Qualification limits

The earlier Cedar physical qualification remains unchanged: braked idle creep
and the narrow adverse-flow envelope still prevent ordinary preset admission.
This work adds usable app ownership, restart and replay for the separately
versioned numerical model. It does not assert stationary parking, takeoff,
landing, crosswind or high-speed qualification. The rotor model is static and
audio is synthesized; this cloud machine had no audio output device.

Exact-source CI and Windows candidate results are separate. Responsive-map
main `53cdbb548632157d1fe8bd2b626a9431082428aa` passed all ten CI jobs in run
`37239023290`. The earlier `9f47945` Windows candidate `37238283190` built and
loaded Swift on the Microsoft Basic Render Driver, but its ordinary screenshot
timed out at the unchanged 180-second watchdog. No PNG or candidate acceptance
credit resulted; subsequent cases were not reached. The five commercial
dependency/rights gates remain and no new binary release is claimed here.
