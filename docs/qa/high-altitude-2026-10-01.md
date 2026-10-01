# Real-DEM level cruise and night visual QA — 2026-10-01

Additional full-size native PNGs are retained locally and are not included in this
publication snapshot. Observations, binary hashes, commands and actual logs are
provided below; no missing image links are substituted for public evidence.

## Result and scope

A full **360-second, 17.533 km, level-flight replay** completed in the actual
native Linux application over normalized **Copernicus GLO-30** terrain. This
replaces the earlier approximately 3 km **falling-aircraft** capture as the bounded
high-altitude evidence for issue #6. The native scene was inspected during playback
and after actual renderer-origin changes. The cockpit stayed attached, the chase
view retained the aircraft and terrain horizon, and the replay reached its visible
`REPLAY COMPLETE 6:00 / 6:00` state without a replay mismatch.

This is software-Vulkan evidence on the cloud desktop, **not human flight testing,
physical-GPU performance, a 60 fps claim, or exhaustive LOD/terrain-clearance QA**.
Four terrain tiles were live throughout this route; the log does not identify their
individual IDs or establish every possible LOD refinement transition. Static
samples cannot exclude a one-frame artifact between samples. The underlying DEM
is a surface model, and the scene has no real satellite imagery or city buildings.

- Native executable SHA-256: `be80a129cf2fbfe4d488673b7be3561d27c1a8ec5bf7aee2a81bbdec694923b6`
- Renderer: Mesa 25.0.7 llvmpipe / LLVM 19.1.7, CPU Vulkan
- Aircraft: Light Single, fingerprint `e8ca8e4cac33cd04`
- Wind and turbulence: calm
- DEM source, geoid conversion, licenses and bake: [data-boundaries QA](data-boundaries-2026-10-01.md)
- 429 unchanged `.fsdem` files, 3,649,074 bytes; same inventory before/after generation and replay
- Region: simulated westbound cruise near Haneda
- The runway in the night test is the simulator's synthetic runway over real DEM; it is not an OSM airport survey

## Evidence

Actual native completed cruise (local capture; full-size PNG not published in this snapshot)



Raw telemetry, detailed logs and metadata are retained locally and are not included
in this publication. The bounded aggregate observations and reproducible fixture
generator are described below.

| Quantity | Observed range/result |
|---|---:|
| Recorded frames | 21,600 at 1/60 s; 180 exact keyframes |
| Ellipsoidal altitude | 3017.088931–3017.092641 m |
| AGL | 2941.710445–2981.085938 m |
| Pitch | 3.014843–3.015513 deg |
| Roll | 0.001627 deg |
| True airspeed | 48.836021–48.837569 m/s |
| Vertical speed | −0.000240–0.000000 m/s |
| Distance | 17,533.176720 m |
| Actual renderer rebases | 84.100, 167.867, 251.650, 335.317 replay seconds |
| Missing-terrain frames, crashes, divergence | 0 |

The fixture's pure-Rust aircraft-anchor rebases occur at 81.917, 163.833, 245.750
and 327.667 seconds. Those are **not substituted for renderer evidence**: the
application log separately records the real `RenderOrigin` changes. The renderer
uses a surface-anchored local frame and rebases on rendered updates, so its exact
timestamps differ. The final candidate was also exercised with real native replay
controls: F5 paused at displayed 0:29, the display remained at 0:29 while paused,
F8 rewound to displayed 0:19, and F5 resumed. F7 selected x2, x4 and later x8;
C changed cockpit to chase. The same flight then reached the completed 6:00 state
without a fault. An earlier uninterrupted x4 integration run was visually sampled
at 0:28, 0:58, 2:03, 4:18 and completion; the published PNG/log are the final
alpha.21 repetition, rather than reusing that earlier executable's evidence.

The native capture was saved and closed before the explicit batch exit. Its log
ends with `Batch capture complete: status 0`, and the invoking script observed exit 0.

## Deterministic fixture

Generate, then launch from the repository root (replace `$TILES` with the normalized
Copernicus directory from the linked data QA):

```sh
cargo run -p flightsim-sim --example record_visual_flight -- \
  --tiles "$TILES" --output cruise-360.fsreplay

cargo run -p flightsim-app -- \
  --aircraft light-single --tiles "$TILES" --replay cruise-360.fsreplay \
  --time 09:00 --view cockpit --screenshot cruise-complete.png \
  --screenshot-delay 150 --exit-after-screenshot
# In the native window: press F7 twice for x4, then C for chase.
# The final QA also paused, rewound ten seconds, resumed and selected x8.
# Without acceleration, use --screenshot-delay 365 instead.
```

The generator warms up for 120 seconds, then constructs a fresh Simulation from
the settled frame-zero state. It records actual director controls, not teleported
poses. The director holds a 3035 m ellipsoidal **target**; its proportional controller
settles about 18 m below the target, as the telemetry explicitly shows. It converts
the fixed target to an AGL target using the exact sampled ground. The earlier
AGL-following attempt was rejected by the fixture: ground bumps drove a −2.75 deg/s
pitch rate. The corrected fixture keeps the stricter envelope: altitude variation
≤1 m, |vertical speed| ≤0.1 m/s, |roll| ≤1 deg, |pitch| ≤5 deg, all angular rates
≤0.1 deg/s, AGL 2850–3100 m, ≥10 km traversal and ≥2 core rebases.

Before publishing any output, the generator checks:

1. No terrain fallback, missing tile, crash, nonfinite telemetry or divergence
2. Exact serialized readback, every stored keyframe and final rigid-body state
3. Exact final simulation clock, flight log, sampled ground and interpolated pose
4. Unchanged sorted terrain-byte inventory before and after replay
5. Existing replay/CSV/metadata/terrain-manifest files are not overwritten

Two independent 360-second invocations produced byte-identical replay files:
SHA-256 `059d7ac65832fbc9278ed704439d540832bdb33e729ba99d652479e95c4ab0bc`.
The accepted CLI duration endpoints, 300 and 480 seconds, also passed, covering
14.611 km / 3 core rebases and 23.378 km / 5 core rebases respectively.
Five example tests and warning-denied clippy passed. Re-running against an existing
output correctly returned exit 1 and left the original replay hash unchanged.
The example has no Bevy dependency. No real DEM bytes are included in this evidence.

## Night capture

Actual night ground scene (local capture; full-size PNG not published in this snapshot)



```sh
cargo run -p flightsim-app -- \
  --aircraft light-single --tiles "$TILES" --start 35.55,139.78 \
  --time 22:00 --difficulty beginner \
  --headless-screenshot night-ground.png --screenshot-delay 5
```

The actual offscreen scene and UI contain the visible runway-light row and lit
instruments over real DEM, with 11 live terrain tiles and a camera near 40 m
ellipsoidal altitude. PNG completion and exit 0 were checked. A separate 1.5 NM
night approach capture showed only a few tiny light pixels at approximately 2.6 km;
**useful long-range approach-light visibility is not established**. Dense-runway
mesh/sampled-DEM mismatch and arbitrary terrain occlusion remain bounded by the
[runway QA limitations](runway-terrain-drape-2026-10-01.md).

## Capture evidence hashes

| File | SHA-256 |
|---|---|
| Native cruise PNG | `1945c5f302fe2b88bd4490f5ed2f6d28494de46f0981295a52e16d817e1d2e1f` |
| Night ground PNG | `742aff961fe9a9ac3b6579477b437aedbced2ad15ed628e2cdd366096dfc52df` |
| Telemetry CSV | `cd9baecacec92ca32159e25fc94e21cd7e05d8ccb79034949646f2d314e6ae6c` |
| Native log (ANSI color escapes removed) | `e1d5184a8aaf133ddba02ef663af5fb53433227ed48d7007f325f1d1f0970261` |


## Final-candidate additional images

The following scenes were repeated using final alpha.21 executable SHA-256
`be80a129cf2fbfe4d488673b7be3561d27c1a8ec5bf7aee2a81bbdec694923b6`.
The night-ground image above is now from this build. All three offscreen runs
saved actual scene/UI PNGs and reported batch status 0.

- Drop / compact vertical-speed gauge (local capture; full-size PNG not published in this snapshot):
  `--drop 3000` over real DEM. The extreme negative V/S value stays on one line
  using its compact `k` representation. This deliberately falling image is for
  gauge layout and camera attachment only, not the level-cruise acceptance.
- Swift Sport and synthetic traffic (local capture; full-size PNG not published in this snapshot):
  original Swift model at scale 1.0000, live synthetic traffic panel, chase view.
  This run intentionally has no DEM and explicitly displays `[no terrain data]`.
- Detailed night, drop and Swift process logs are retained locally.
- [Separate two-native-instance connection lifecycle evidence](high-altitude-lan-alpha21-2026-10-01.md).
