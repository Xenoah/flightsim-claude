# Authored visibility and cloud base: native acceptance

The new-flight map now edits background visibility and a cloud-bearing preset's
base above departure ground. Apply changes the draft; Start prepares and commits
the complete flight. Untouched fields retain their exact recorded values.
These are authored conditions, not observed or live meteorology.

Fresh acceptance used runtime source `357156bf383d8282e57c21fee56092630ddd9bd2`
and executable SHA-256
`cd9be6580e049a8cad80cd25f1c8f8904c14cfbd783d8f1f7fec8704bcdb8efd`.
The integration adds documentation, reviewed source bindings and an isolated
experimental jet example; the tested app/render/physics runtime is unchanged.
See [automated verification](authored-weather-editor-recovery-2026-10-05.md)
and [the user guide](../modeled-weather.md).

## Native interaction and actual recordings

Eight completed native Linux cases exited successfully and saved PNGs. The
adapter was CPU llvmpipe, Mesa 25.0.7 / LLVM 19.1.7. The first launch attempt
stopped because the reset environment lacked a Vulkan driver; the official
Debian software driver was restored locally before these accepted cases.
There is no physical-GPU, speaker, Windows or binary-release qualification.

- Untouched Apply retained Rain. Visibility 9 m plus base 200 m was rejected
  atomically; Cancel/reopen restored 10,000 m / 600 m
- A valid 2,000 m / 1,200 m draft became Custom without starting a flight.
  Closing the map retained the paused active flight byte for byte
- Explicit Start committed base-only and visibility-only edits, keeping the
  authored layer thickness, precipitation, seed, wind and turbulence unchanged
- Actual 640×480 and 1280×720 windows showed all editor fields and buttons.
  Scrolling the small map and reopening retained the committed values
- Legacy disabled the editor. Clear and Fog could not gain a cloud layer.
  Replay disabled weather editing and Start, including pointer and key attempts
- Pending Legacy/Clear/Fog exploration followed by closing the map left the
  active Custom session byte-identical

Seven actual F9 files contain four unique byte sequences. Independent decoding
and replay verified Swift's complete identity, every environment/weather field,
exact serialization and all 13 rigid-body scalar words at every checkpoint.
Unique coverage is 8,421 steps and 72 checkpoints; counting duplicate isolation
saves gives 18,295 steps and 156 checkpoints. Files 001/002/003 match exactly;
006/007 also match exactly. Legacy v3 has no stored partial-tail final state, so
tail replay is deterministic reconstruction rather than comparison to a stored
final value. The unchanged v1–v6 golden suites separately remain green.

Custom records 004–006 have identical initial pose and environment. They retain
wind from 233.75 degrees at 8.125 knots, the existing Light turbulence amplitude
and seed, weather seed 4242, and a 2,000 m layer thickness. The map's existing
June-15/climate-enabled new-flight semantics are explicit; it does not promise
to retain the CLI's June-21/climate-disabled startup date.

## Matched native rendering

Six small replay fixtures were derived from those actual records, with one
zero-duration frame and a common initial pose. Daytime retains the recorded
epoch; nighttime adds exactly 8/24 Julian days, moving local 12:30 to 20:30.
The fixtures are controlled rendering evidence, not additional flown maneuvers.

| Case | Background visibility | Base above departure ground | Start |
|---|---:|---:|---|
| Below-cloud reference | 10,000 m | 1,200 m | 1,000 m AGL |
| Reduced visibility | 2,000 m | 1,200 m | same pose |
| Inside cloud | 2,000 m | 200 m | same pose |

All six saved images are 1180×812. The daylight images show reduced distant
detail at 2 km and additional aircraft obscuration inside the cloud. At night,
the inside-cloud case removes the visible runway lights from this view. A fixed
aircraft/background pixel comparison supports increasing daytime obscuration;
it is an image check, not a calibrated meteorological measurement or runway
visual-range qualification. The app still uses camera-local homogeneous fog,
approximate cloud shapes and sparse precipitation geometry.

## Cloud-quality lifecycle and measured resources

The same Custom conditions were switched through Light → High → Ultra → Off →
Light, then quickly requested High and restored Light. Every logged weather
field remained unchanged. High visibly used Light while preparing, then reached
Ready. The second High request was already ready before the rapid return; this
does not claim cancellation during shader compilation.

| Ready tier | Internal resolution | View / sun steps | Owned target bytes |
|---|---:|---:|---:|
| High | 523×360 | 40 / 4 | 2,259,360 |
| Ultra | 784×540 | 64 / 6 | 5,080,320 |

Both upper tiers reported 599,186 noise bytes, 336 uniform bytes and 524,288
source bytes. Initial Light allocated zero additional volume bytes. Returning
to Off/Light retired the owned target/noise/uniform/source resources to zero;
this excludes retained renderer/driver caches and is not total GPU memory.
The first High request reached Ready in 0.852 s; Ultra reached Ready in 0.138 s
after High. These preparation intervals are not frame performance or GPU timing.
No runtime speedup, physical-GPU FPS or new realism level is claimed.

The native QA bundle contains exact configurations, logs, source/binary hashes,
seven actual recordings, independent verifier and bindings, six derived fixtures
with provenance, eight unedited PNGs, image analysis and resource-state receipts.
The editor changes no rendering budget, weather schema/model revision, physical
force law, icing, convection, live data service or distribution gate.

## Source and distribution checks

The source contract now binds 127 complete source/witness files plus 102 frozen
independent anchors. Nine reviewed hashes changed and five weather files were
added; the other 113 pins remain exact. All existing validator bodies and
acceptance commands remain intact. The one obsolete hidden-modal mutation
target was updated to the new weather-first dispatch. All 237 Python tests
passed, including new semantic-drift and omitted/rehashed-evidence rejection.

Final remote CI is tracked against the published commit. The preceding
`23a0a23` Windows candidate compiled, then failed its ordinary 180-second Swift
screenshot timeout. Dependency/rights review and ordinary release gates remain
independent. The latest published binary is still alpha.20 until a release
passes those gates.
