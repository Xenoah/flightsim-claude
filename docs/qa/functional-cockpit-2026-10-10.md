# Functional 3D cockpit: development qualification

Date: 2026-10-10. Base: `be5873840dc86eb551971bad6e7290840eaf9cd2`
(alpha.22), tree `49479664f838700cf52ed7621a727db7732752e2`.

This is development evidence for the original analog cockpit described in
[the control/instrument inventory](../3d-cockpit.md) and
[ADR-0033](../adr/0033-state-coupled-3d-cockpit.md). It is not release approval,
Windows qualification, real-aircraft fidelity or physical-hardware acceptance.

## Independent source review

A separate read-only reviewer inspected the six runtime files, actual day/night
pixels and the control layout. The source review passed after these corrections:

- Pointer sampling precedes restart/suspension; held controls cannot relatch after
  modal capture, focus/cursor loss, pause, restart or aircraft replacement
- Live yokes/pedals use untrimmed axes; replay honestly uses recorded effective
  axes and cannot invent separate trim
- Physical needles and dial faces share illumination; warnings are independent
  of ordinary HUD decluttering
- Trim has a non-spinning picking proxy. Narrow yoke boss/grip proxies do not
  intercept the empty space inside the U-shaped handle
- Every neutral production control center has an unobstructed control ray; trim
  is centered clear of the copilot pedal. The HUD switch clears the steering shaft
- New-flight display refresh observes activation (`InteriorModel` addition), not
  the earlier creation of a hidden staged display
- Flap extension moves the physical paddle down, matching the drag instruction

The reviewer additionally examined 216 endpoint combinations mathematically.
Extreme yoke positions can physically hide nearby controls; centering restores
access. This is not an automated native mouse test.

Generated meshes, materials and images remain under the per-aircraft owner and
are removed through existing cancellation/replacement cleanup. Staged interiors
are excluded from active queries. Bounded-model aircraft retain their model-owned
interiors and the 2D fallback. No FDM, core/sim/input source, physics profiles,
replay codecs/identity, dependencies, component terms or distribution runtime
was changed.

## Test status

Frozen runtime source: `1ed68346659d157ca0ddf81d6d857ad43d069f3e`.
The split all-target suites passed, including their benchmark smoke targets:

| Suite | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Render/input/UI/audio/app, all targets | 1,340 | 0 | 3 |
| Core/FDM/world/sim/tilegen/assetgen/net/content, all targets | 1,387 | 0 | 0 |
| Pure doctests | 6 | 0 | 0 |

The first row includes 445 app, 346 renderer, 262 UI, 108 input and 93 audio
unit tests plus integration tests. Its three pre-existing ignored tests require
an optional regional scenery fixture, external normalized Haneda tiles, or a
manual quiet-window latency run. Nothing new was ignored. The pure group used
`flightsim-content/downloads`. This is test/benchmark smoke, not performance
measurement or a live external-service test.

Commands used the documented split groups, Rust 1.93.0, offline dependency
resolution, one compile job, dev opt-level 1 and no incremental compiler cache.
Format, architecture and whitespace checks passed. Both split all-target clippy
groups passed with `-D warnings`. The subsequent ordinary default-feature
development build also passed. Renderer/input/UI/audio doctest commands passed
with no doctest examples in those crates. The capture binary identified below
remains its own earlier artifact, not the later build output. These completed
local checks do not substitute for exact-source release CI or native smoke qualification.

Automated cockpit coverage includes finite original geometry and known gauge
unit/scale endpoints; nose-up horizon and body look-direction signs; physical flap direction;
real Bevy camera viewport conversion and nearest hit selection; every production
control center; trim re-grabbing through full revolutions; interrupted drag
ownership; cursor-independent hotkeys; paused presentation switching; zero-step
input; exact fixed-step state/recording equality across 6/30/60/144-Hz presentation;
raw live versus effective replay axes; and owned image/mesh/material cleanup
without removing unrelated assets.

The offline Python terrain packer and both aircraft schema checks, replay
identity/v3/v4, subsonic and jet identity references, and Meadow model validator
passed. The initial full source-tool suite ran 750 tests, with 9 failures,
62 errors and one skip: the old exact-source contract correctly rejects changed
runtime hashes and the two new crate members. A bounded reviewed-source
migration and complete rerun were required; these failures were not waived.

The subsequent bounded migration preserves the published alpha.22 witnesses and
all historical source/physics boundaries. Its full Python rerun on migration
checkpoint `29a7868058e224284be69d35bacdc6eacb9536d0` completed 759 methods
with zero failures/errors and one pre-existing Windows-junction skip. The focused
43-method source-admission suite also passed. The final integration changes only
this QA report, the source-admission guide and its exact analytical/capture hash
bindings relative to that tested migration; independent review and the real
clean canonical source-admission chain pass for the integration as well. These
are source checks, not Windows execution or publication approval.

## Actual renderer captures

Development day (`12:00`) and night (`22:00`) previews were rendered by the real
application, using its ordinary headless-screenshot path at 1280×720, clear
weather and traffic off. Both completed with a PNG and process status 0. The
backend is Linux Vulkan on Mesa llvmpipe software rendering, not a physical GPU.
The screenshots were visually inspected for panel framing, yoke silhouette,
gauge/stack legibility, material depth and night needle illumination.

Final captures use the frozen source above, including corrected switch/trim
placement. The airborne `--approach 1.5` capture additionally shows nonzero
airspeed/altitude, 100% flaps and the corresponding extended paddle. Day/night
capture the ground state. Each saved PNG and log ended with status 0.

| Capture | SHA-256 |
| --- | --- |
| `cockpit-final-day.png` | `9ed12354fd3307c26cd851b6a99989dfe8594aacd71c81275f26bb2f22c0985c` |
| `cockpit-final-approach.png` | `db149cb9a949eadffbc04578a9f2662cc96e9fb35b073a3d171249901243b83a` |
| `cockpit-final-night.png` | `d05ad7848e89d2030fc63a63a96cd2c920bca4978e180e854075173ddb1e5d23` |

The dev binary used for these captures has SHA-256
`b7903c98f16c256909050b99e52545ef7301aae9a9a26c1955106dad246956c8`.
No reference photograph, third-party cockpit mesh, texture or font is redistributed.

## Remaining qualification boundaries

- Native window and human mouse interaction were not run. The cloud X server
  could not create a listening socket; no security restriction was bypassed
- Actual Bevy viewport/ray and ECS input tests exercise picking and gating, but
  do not claim native operating-system cursor dispatch or pilot usability
- No physical GPU, Windows cockpit runtime, joystick/HOTAS or speaker test
- No off-axis/pedal rendered screenshot yet; look-around and target reachability
  have code/geometry coverage only
- Fuel, mixture, electrical buses, radios, magnetos, pressure/gyro dynamics and
  magnetic compass behavior remain unimplemented, as listed in the guide
- Alpha.22 terrain evidence is preserved but is not new cockpit evidence
- Existing exact-source, source archive, Windows same-archive smoke, native
  applicability, licensing and post-publication asset checks remain mandatory
