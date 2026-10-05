# Authored weather editor: fresh post-reset automated verification

[Native acceptance](authored-weather-native-2026-10-05.md) records the subsequent
interaction, actual replay and controlled rendering checks.

Date: 2026-10-05 UTC. Runtime source:
`357156bf383d8282e57c21fee56092630ddd9bd2`, on recovered baseline
`aa600b64382abd9b02b31de42ba97c933ad13490` (exact public `23a0a23` tree).
All results below were rerun after the verified 2026-10-04 23:42 UTC filesystem
reset. No pre-reset test or incomplete native46 result is credited.

## Source and changes

`59d33ca` recovered the complete authored source from retained write/patch
commands, then `3ebcc4c` simplified the read-only precipitation line to its phase.
The full rate remains in diagnostics/replay. `357156b` reuses a retained exact
named template while its preset and seed match, instead of reconstructing and
validating it during unchanged map updates. Custom drafts remain authoritative;
closing the map restores only committed Custom state. A signed-zero exact-copy,
name-change, seed-change and Custom-preservation regression covers this reuse.

The independent integration work renumbered the weather decision to ADR-0025
because ADR-0022 already identifies the accepted near-static model decision.
That integration-only documentation rename does not change the runtime source.
This evidence followup also changes documentation only.

No sim/FDM/render source, codec, physical identity, weather schema/model revision,
canonical preset, network request or rendering budget was changed. The same
existing complete-aircraft transaction owns Start; initialization remains
1000 m above departure ground. Native visual acceptance and publication remain
separate acceptance gates.

## Fresh executed results

All commands used restored official Rust 1.93.0, the supplied `build-env.sh`,
`-j2`, disabled incremental/debug data and `RUSTFLAGS=-D warnings`. Checkout
entrypoints were retimestamped to avoid sharing stale build outputs across
checkouts. The first run rebuilt Bevy from a cold cache; the final run uses the
final runtime source above. These commands did not perform native rendering.

| Gate | Fresh result |
|---|---|
| App/UI all-target tests on `357156b` | 380 app tests + 1 integration test; 259 UI tests; 0 failures |
| Existing sim weather/replay/identity suites | 194 passed in 13 suites, including independent v1-v6 goldens |
| App/UI strict all-target Clippy | passed |
| App/UI private docs, `RUSTDOCFLAGS=-D warnings` | passed |
| App/UI all-features/all-target check | passed |
| Formatting, diff whitespace and architecture rules | passed |
| Actual `cargo build -j2 -p flightsim-app` | passed; immutable executable/hash receipt prepared |

One unchanged optional regional fixture test is ignored:
`scenery_runtime::tests::optional_real_region_build_reports_cpu_work_and_respects_scene_caps`.
It requires explicitly supplied regional sample data. No assertion was removed
or relaxed and no new ignore was added.

The earlier fresh run on recovered `3ebcc4c` passed 379 app tests + 1 integration
and 259 UI tests. Those results are retained separately; the table uses the
final cache-reuse source and its additional regression.

The pure command selected `modeled_weather`, `replay_v3`, `jet_replay_v4`,
`turboprop_replay_v5`, `near_static_turboprop_replay_v6`, `replay_file_boundaries`,
`replay_player_versions`, `replay_world_climate`, `replay_complete_identity`,
`replay_model_identity`, `jet_model_identity`, `turboprop_model_identity`, and
`near_static_turboprop_model_identity`. Their original golden files and assertions
remain unchanged.

Raw fresh logs are in task evidence `flightsim-qa/weather-editor-recovered/logs/`:
`app-ui-first.log`, `app-ui-final.log`, `replay-goldens.log`, `clippy.log`,
`docs.log`, `all-features-check.log`, `architecture.log`, `format.log`,
`diff-check.log`, and `native-build.log`.

## Prepared executable and remaining acceptance

The actual default-feature dev executable was built at clean source `357156b`,
then copied with permissions 0555 into the task's native evidence directory:
`weather-editor-native-recovered/bin/flightsim-app-357156b-cd9be6580e04`.

- SHA-256: `cd9be6580e049a8cad80cd25f1c8f8904c14cfbd783d8f1f7fec8704bcdb8efd`
- Size: 151,466,016 bytes
- Adjacent JSON receipt records source/tree, compiler, platform, build-env hash,
  command, assets path and final hash verification

This is a build/automated-test receipt, not native acceptance. Independent Python
binding/mutation gates, actual 640x480/720p interaction, day/night/cloud-boundary
captures, matched-pose visibility/fallback/resource/timing checks, final CI and
publication require separate evidence.

For native comparison, Rain base200 m with retained thickness2000 m starts inside
the layer at the existing1000 m Start. A separate base1200 m case keeps the same
Start below cloud while varying only background10000 m to2000 m. No altitude
control or alternate Start path was added. Camera-local homogeneous fog, sparse
snow, Light/volume shape differences, Cloud Off without cloud geometry and all
real-world meteorological data limitations remain. No physical-GPU, Windows,
human-handling or full weather-engine qualification is claimed here.
