# New-flight wind/turbulence verification

2026-10-04. Implementation checkpoint `9f463d241ad2c67415a1aad119975b0dd0d449b7`;
included-test formatting checkpoint `e05cf711ef58b09536e8e65154baef3427827b08`.
The latter changes no runtime behavior. Independent source/transaction review
approved the implementation. This record describes Linux numerical, transaction,
input and layout evidence; native visual acceptance and Windows qualification
are separate gates.

## What changed

The map's bounded Wind / turbulence child editor stages true wind-from degrees,
speed in knots and an existing authored turbulence strength. Apply changes only
pending choices. Successful Start commits them with the prepared aircraft/scene;
opening/canceling an editor, closing the map, failed loading and stale results
cannot replace the active flight. Replay/LAN do not accept physical edits.

Each numeric field tracks whether it was edited. Untouched display strings never
become physical inputs. Exact f64 components, a custom turbulence amplitude, u64
seed and override flags survive no-edit and visual-only operations. Each strength
button changes only amplitude, retaining the seed even for Calm. Existing CLI,
difficulty defaults, physics laws, fixed-step clocks, render quality, controls,
identity revisions and v1/v2/v3/v4 bytes are unchanged. No external service was
introduced. See [ADR-0017](../adr/0017-explicit-new-flight-forces.md) and the
[user guide](../new-flight-wind.md).

## Focused evidence

- Seven app conditions tests cover exact no-edit retention, each individually
  edited component, atomic invalid-action rejection, close/reopen, replay/LAN and
  child-modal lockout, manual-cloud independence, signed-zero/seed matching, and
  explicit override survival through later difficulty resolution
- Eleven real input/layout tests cover ordered click/Ctrl+A/text/Tab batches,
  finite and length bounds, per-field omission, retained custom turbulence,
  edit-then-restore generation invalidation, modal-owned F12/aircraft/Start keys,
  cancellation/reopening, focus loss/session lock and 720p layout
- Production scene-spawner transactions switch through jet, Meadow, Swift and
  launch profiles with nonzero exact wind/custom amplitude/nondefault seed. The
  active snapshot stays unchanged while staging. Candidate initial ground
  velocity differs from the calm candidate by the selected wind, proving that
  conditions resolve before airborne construction
- App-origin recordings execute 150 render advances and reproduce exact final
  state bits and physics time at different replay cadences, then restart/rewind.
  V3/V4 byte roundtrip retains the exact physical environment. Visual presets
  alternate Clear/Rain while aircraft cycles preserve physical values
- Failed GLB and environment preparation retain a distinct pending physical
  snapshot and the active scene/conditions/recording. Correcting a failure alone
  cannot retry; an explicit Start commits the retained pending conditions
- Three pure tests use independent compass/diagonal and nautical-mile unit
  expectations, head/tail/crosswind relative-air vectors, actual aerodynamic
  side-force signs and open-loop displacement for both dynamics families.
  Turbulence amplitude and seed change force and trajectory; repeated seeds
  reproduce state bits. No in-flight stabilizing controller is added
- Two pure replay tests use nonzero 310-degree wind, moderate turbulence seed
  `0x0123456789abcdef`, and independent visual weather seeds. Four seconds at
  30/144 Hz, v3/v4 exact byte roundtrip, 165 Hz playback and repeated rewind
  restore state/clock/history. Clear and Rain retain identical controls,
  checkpoints and physical outcomes
- Existing terminal jet tests cover rejection at zero, after committed steps,
  and deferred cursor 240 with no extra committed physical/weather time. Existing
  old-format independent fixtures and complete-identity checks remain intact

## Executed checks

All commands used Rust/Cargo 1.93.0, the existing shared toolchain environment,
`-j 2`, debug information off and incremental compilation off. No dependency,
toolchain or package installation was needed.

| Check | Result |
| --- | --- |
| Headless core/fdm/world/sim/tilegen/assetgen/net/content all targets | 1,183 passed; no ignored tests |
| Render/input/ui/audio/app all targets | 1,160 passed; 3 existing opt-in ignores |
| App + UI focused/full run | 318 app + 1 app integration + 233 UI passed; 1 existing app ignore |
| App/content/assetgen with region-downloads | 413 passed; 1 existing app ignore |
| App with commercial-staging | 309 passed; 1 existing app ignore |
| New pure wind/force/replay tests | 5 passed |
| Headless doctests with content/downloads | 6 passed |
| Workspace all-targets Clippy with region-downloads, `-D warnings` | passed |
| Workspace private documentation with region-downloads, `-D warnings` | passed |
| Workspace rustfmt and explicit formatting of included UI tests | passed |
| Architecture dependency checks | passed |
| Independent Python aircraft identity, v3, v4, jet identity and subsonic goldens | all passed |
| Headless benchmark compilation, no benchmark execution | passed |

The render split's three existing ignores require an optional real regional
fixture, external normalized Copernicus Haneda tiles, or a separate quiet-window
planning-cost run. These do not represent passes. The existing GPU-free aircraft
hierarchy fixture emits B0004 warnings; its structural assertions pass. This is
not evidence about warnings or appearance during a native renderer launch.

Raw local logs and the immutable native build receipt are held in the sibling
`flightsim-weather-controls-qa` directory. No GPU/CUA run, publication, network
service, source-package installation, rights approval or Windows qualification
was performed by this implementation task. Native editor usability, manual pilot
handling and speaker output still require the separate acceptance pass.

The later [native acceptance report](new-flight-wind-native-2026-10-04.md) records
the completed editor/flight/replay checks and final footer-only correction.
