# Authored visibility / cloud-base editor: recovery and validation status

Fresh automated verification on runtime source `357156b` is recorded in the
[2026-10-05 recovery report](authored-weather-editor-recovery-2026-10-05.md).
The following records the initial reset/recovery state; native acceptance remains separate.

The cloud filesystem reset was verified at 2026-10-04 23:42 UTC. The original
weather source (`edbc53f`, followed by independent binding `f7c499`), local working
archive and raw test logs were lost. The incomplete native46 run is unavailable
and must be repeated. No old test result qualifies the recovered source.

This recovery starts from local baseline
`aa600b64382abd9b02b31de42ba97c933ad13490`, whose tree
`9644c80e90a70fb529e6bd240ca283332981e6ac` matches public `23a0a23`.
App, UI, tests and documentation are being reconstructed from retained complete
write/patch commands. Post-format byte identity to the old source is not yet
established; the original Git objects are unavailable. All recovered source
requires fresh compilation, tests, independent review and native acceptance.

## Recovered implementation boundary

The existing offline map can edit background visibility and, for cloud-bearing
presets, base above the departure ground reference. The app retains an exact
canonical template and optional explicitly edited SI fields. Untouched Apply
preserves the preset and all exact values; visual edits become Custom. Cloudless
presets cannot gain a layer. Legacy/monthly, manual clouds, replay and LAN keep
their existing behavior. The complete exact draft and revision join the existing
Start generation and aircraft/position/month/region/physical-condition snapshot.
Only the existing complete-aircraft transaction publishes resolved weather.

No sim/FDM/render source, codec, weather schema/model revision, canonical preset,
network service or rendering work/allocation budget is changed. Candidate binding
and mutation updates remain the independent reviewer's responsibility.

## Required fresh verification

- App/UI all-target checks and tests, including all original assertions
- App/UI strict Clippy, strict private docs, format and architecture checks
- All-features/all-target app/UI compilation
- Existing sim weather/replay/identity and independent v1-v6 golden suites
- App exact-draft, cancel/stale-load/edit-restore/invalid-height/fresh-retry tests
- All-family Custom v3/v4/v5/v6 actual recording/playback, exact bytes/full state,
  pause/bounded seek/rewind/restart and executed-clock render publication
- UI dirty-field omission, atomic invalid Apply, all modal keyboard/pointer
  ownership and real 640x480/720p scroll/cancel/reopen/Apply layout witnesses
- Independent source binding/review, native day/night/cloud-boundary and
  fixed-pose obscuration/fallback/resource/timing acceptance, final CI/publication

The map keeps its existing 1000 m-above-ground Start. Rain with base200 m and
retained thickness2000 m begins inside the layer. A separate base1200 m case at
the same Start can compare background10000 m and2000 m below cloud. No altitude
control or alternate Start path is added. Homogeneous camera-local fog, sparse
snow, Light/volume shape approximations and real-world meteorological data limits
remain unchanged. No physical-GPU, Windows or full weather-engine qualification
is claimed.
