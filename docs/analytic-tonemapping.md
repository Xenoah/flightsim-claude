# Explicit analytical tonemapping build

The ordinary application still uses Bevy 0.18.1's bundled LUTs and implicit
Tony McMapface camera default. Existing ordinary, `commercial-staging` and
`region-downloads` build commands keep their tone response. Graphics/Cloud/Water
Light defaults are unchanged.

`analytic-tonemapping` is a separate, explicitly selected build appearance.
It selects Reinhard for the app and `sun_clock` cameras and replaces only
Camera3d AgX/Tony/Blender Filmic selections in `Last`, before extraction.
Camera2d, explicit None, and other analytical methods are preserved. Current
Startup/SpawnScene/PostUpdate camera producers are covered; a future unordered
Last camera writer needs its own ordering review.

The earlier isolated proof was validated only on Linux with software Vulkan.
Reinhard darkens daylight regions (roughly 8–11% in the proof's fixed PNG samples)
and shifts low-sun highlights. It is not visual parity or a realism improvement.
Moonless exteriors remain very dark, and some near-black pixels become black.
No Windows runtime, physical-GPU performance or new aircraft qualification
follows. The ordinary Windows candidate and release contracts remain separate.
LUT exclusion is mechanical evidence about three embedded payloads, not rights
clearance or a license grant. Other lookup textures, including atmosphere, remain.

## Select one mode

Ordinary commands continue to work:

```sh
cargo build --locked -j2 -p flightsim-app
cargo build --locked -j2 -p flightsim-app --features commercial-staging
cargo build --locked -j2 -p flightsim-app --features region-downloads
cargo build --locked -j2 -p flightsim-render --example sun_clock
```

Select the alternate build explicitly:

```sh
cargo build --locked -j2 -p flightsim-app --no-default-features --features analytic-tonemapping
cargo build --locked -j2 -p flightsim-render --example sun_clock --no-default-features --features analytic-tonemapping
```

Add `--offline` with the locked dependencies already cached. The application
switch does not change controls, physics, aircraft identity, replay formats,
weather, exposure, materials, geometry or runtime quality defaults. There is no
runtime tone-mode setting. The standalone analytical option is not admitted by
the existing Windows candidate recipe.

Both `default` plus `analytic-tonemapping` are rejected in app/render, including
`--all-features`. Neither is rejected in the app binary and `sun_clock` example.
The render library alone accepts neither: the ordinary app disables its defaults
and enables Bevy's LUTs itself. The ordinary multi-package test/Clippy commands
in CI retain defaults and remain supported. Standalone input/UI/audio packages
have no tone-mode router; they do not by themselves establish a 3D tone policy.

Cargo features are additive. A package-local guard cannot detect an unrelated
dependency re-enabling `bevy/tonemapping_luts`. In particular, selecting multiple
workspace roots can reactivate defaults. A successful analytical compile alone
is not exclusion evidence. Build and audit each exact package/target separately;
a separate target directory prevents output replacement but cannot prevent
within-command feature unification. Do not use workspace metadata or
`--all-features` as evidence for the selected analytical executable.

## Capture exact build evidence

Use Rust/Cargo 1.93.0, warnings denied, and the same target/profile/environment
for the graph and build. On a Linux x86-64 host, with `CARGO_BUILD_TARGET` unset,
the following uses the host compiler target and an explicit matching graph:

```sh
mkdir -p qa-tonemapping
export RUSTFLAGS='-D warnings'
cargo tree --locked --offline -p flightsim-app --target x86_64-unknown-linux-gnu \
  --no-default-features --features analytic-tonemapping --edges normal,build \
  --prefix none --format '{p} features=[{f}]' > qa-tonemapping/analytic-graph.txt
cargo build --locked --offline -j2 -p flightsim-app \
  --no-default-features --features analytic-tonemapping \
  --message-format=json > qa-tonemapping/analytic-build.jsonl
python scripts/check-tonemapping-build.py --mode analytic --kind app \
  --graph qa-tonemapping/analytic-graph.txt --messages qa-tonemapping/analytic-build.jsonl \
  > qa-tonemapping/analytic-audit.json
```

Audit immediately after the build, before reusing the target directory. Check
each command's exit status. Retain its exact command, source commit/tree,
Cargo.lock hash, toolchain/target/environment and logs with the report. For a
cross-target build, add the same explicit `--target` to build and graph commands;
graph inspection alone is not a build or runtime result.

For an ordinary positive control, omit both analytical flags, use new filenames
and `--mode ordinary`. For `sun_clock`, graph `-p flightsim-render`, build
`-p flightsim-render --example sun_clock`, and audit `--kind sun-clock`.
The audit deliberately accepts only these minimal single-mode recipes; it does
not qualify optional feature combinations or change existing release tools.
Freeze and hash the executable before another command can replace it.

The checker requires the successful executable's Cargo feature record, six
compiled library feature records/fingerprints, decoder presence, source dep-info
and the three exact reviewed Bevy LUT payloads to agree. It reads only the
fingerprints selected by that build's artifacts, not every historical fingerprint
in a shared cache. Its payload byte search supplements the graph/source evidence;
it is not a universal detector of transformed content. The ordinary control must
contain all three full payloads, and the analytical artifact must contain none.
Neither a graph-only result nor a screenshot substitutes for these checks.
The executable must be one of Cargo's reported filenames and the selected
libraries must share its target output directory. The audit records the captured
profile; it does not authenticate supplied logs, establish source/target/toolchain
provenance on its own, or authorize a release. Those provenance records and the
ordinary candidate/release gates remain separate requirements.
It audits the selected app/example artifact; additional artifact records are
not proof that the original invocation selected only that root package. Retain
the exact command alongside the report.

## Regression and contract checks

Run `tonemapping_modes` in both ordinary and analytical render configurations.
It uses the actual FlightsimRenderPlugin to exercise startup/late camera creation,
PostUpdate method changes, retained Camera2d methods, and all supported analytical
methods. Existing app/render suites and Clippy must also pass in both modes.
The checker has negative Python fixtures for graph reactivation, missing decoders,
failed/incomplete builds, wrong feature/version records, and wrong artifact kinds.

```sh
cargo test --locked --offline -j2 -p flightsim-render --test tonemapping_modes
cargo test --locked --offline -j2 -p flightsim-render --test tonemapping_modes \
  --no-default-features --features analytic-tonemapping
python -m unittest discover -s scripts/tests -p test_tonemapping_build.py -v
```

Also confirm actual Cargo rejection of app/render mixed modes and runnable
neither-mode commands. Keep the render-library neither-mode check separate.

The ordinary candidate contract independently pins the conditional app main.rs
and cockpit UI source. Its [reviewed binding migration](qa/replay-candidate-tonemapping-cockpit-pin-review-2026-10-05.md)
refreshes the three affected whole-file digests and adds the shared instrument
panel geometry: 130 source pins and all 102 preserved independent anchors.
Candidate workspace metadata also gains render's `default` router node, so fresh
inventory evidence is required even where the package-scoped ordinary dependency
graph is unchanged. Exact app `default,commercial-staging`, full LUT bundle,
target/toolchain, offline identity and release authorization requirements remain.
The analytical option is not admitted by that ordinary candidate contract.

An additive Linux CI job repeats analytical app/render tests and Clippy, actual
mode-guard rejection checks, and isolated ordinary/analytical app artifact
audits. It retains text/JSON evidence and no executable. Existing ordinary OS
jobs, application smoke and candidate/release workflows remain separate. A green
source/build job is not native rendering, Windows or distribution qualification.

The [exact native matrix and integrated build evidence](qa/analytical-build-native-2026-10-05.md)
records the tested Linux scope, darker tone tradeoff and remaining limitations.
