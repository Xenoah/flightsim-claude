# Aircraft-specific cockpits: development qualification

Date: 2026-10-10. Base: `d9008c3e94dc3ebf2a3a23b25fc2fc06b616bfb2`.
Runtime source: `64b9d06ee97fcb1689d3df697c55da2db7581edc`.

This report covers the authored layouts in [the aircraft cockpit inventory](../aircraft-cockpit-layouts.md) and [ADR-0034](../adr/0034-aircraft-specific-cockpit-layouts.md). It supplements the [initial cockpit QA](functional-cockpit-2026-10-10.md); it is development evidence, not a Windows or physical-hardware qualification.

## Scope and independent review

- Light Single retains the analog high-wing layout and dual yokes. Meadow receives a lighter utilitarian panel and upholstery
- Swift receives dual center sticks, low canopy framing and an original primary flight display
- Kestrel receives a side-by-side analog jet-trainer layout, dual sticks, wider display scales and one aggregate throttle lever
- Exact family/profile/model bindings select the layout. Unknown legacy aircraft retain the trainer fallback; unknown bounded aircraft, missing/custom Kestrel models and unadmitted models retain the existing model/2D fallback
- Every visible flight control and flight instrument remains bound to existing simulation state or input. Decorative framing, screws and nameplates are passive structural details

An independent read-only review inspected runtime selection, staged/active visibility, cleanup ownership, control picking, display units/signs and actual pixels. Corrections included raising the jet throttle clear of the passive console, separating momentary service-brake animation from the parking latch, clearing the trainer switch row from the yoke shaft, moving nameplates below the glare shield, and closing the forward footwell behind the full pedal travel. The wall overlaps the panel, sidewalls and extended floor; actual mesh bounds test all layouts and pedal endpoints.

The reviewer inspected the actual manufacturer DA20 and historical T-37 reference pixels, then the rendered day/night/approach images. Those photographs are references only and are not included in the repository or evidence package. No physics, aircraft/profile JSON, GLB asset, replay identity/codec, dependency, distribution allowlist or licensing terms changed.

## Automated validation

| Suite | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Render/input/UI/audio/app, all targets | 1,356 | 0 | 3 |
| Core/FDM/world/sim/tilegen/assetgen/net/content, all targets | 1,387 | 0 | 0 |
| Pure doctests | 6 | 0 | 0 |

The render group includes 452 app, 355 renderer, 262 UI, 108 input and 93 audio unit tests plus integration tests. Both split suites include benchmark smoke targets, not performance measurement. The three existing ignored tests require an optional regional sample, external normalized Haneda tiles, or a manual quiet-window latency run. No new test is ignored.

The final render suite and render all-target clippy passed after the reviewed footwell fix. The pure suite/doctests and pure all-target clippy passed at `e97d436`; their source files, dependency graph, lockfile and build inputs are unchanged by the subsequent renderer-only fix. The final format, architecture and whitespace checks passed. Both clippy groups used `-D warnings`.

Commands used Rust 1.93.0, offline resolution, one compile job, dev opt-level 1 and no incremental cache. The pure group enabled `flightsim-content/downloads`. Reproduction follows the split package groups in CI: `cargo test --all-targets` for each group, `cargo test --doc` for the pure group, and `cargo clippy --all-targets -- -D warnings` for both groups. These local checks do not substitute for exact-commit CI or native release smoke.

Coverage includes exact supported layout selection and unknown/custom fallback; staged scene isolation and cancellation; old/new scene cleanup and visibility; real Bevy viewport/ray hit testing for all control centers; pilot display/stick/throttle framing; stick full-axis endpoints; passive console/shaft clearance; throttle and stick direction; glass display attitude/data changes; jet scale endpoints; Home centering per layout; pause/map/focus/cursor/view/restart interruption; supported Kestrel fixed-step input and replay lock; service versus parking brakes; and existing fixed-step/replay equality tests.

The independent terrain packer, both aircraft schema suites, replay identity/v3/v4, subsonic and jet identity reference scripts, and Meadow model validator passed. Full canonical-source mutation tests are a separate release migration gate: the prior inventory must not silently admit these new runtime hashes and modules. Source/release migration, full source-tool sweep and exact-commit CI remain the release lead’s responsibility.

## Actual production-renderer captures

All eight captures use the ordinary application headless-screenshot path, 1280×720, clear weather and traffic off. Ground captures use 12:00 or 22:00; approach captures additionally use `--approach 1.5`. Each process returned 0 and wrote a nonempty PNG. Linux Vulkan used Mesa llvmpipe software rendering.

The captured binary has SHA-256
`7148c88650277a94b416ab4a0f4c9763d645e71f6f4f01e92430e439941fdc70`.

| Capture | SHA-256 |
| --- | --- |
| `kestrel-approach.png` | `58d2ba059915f142113356765206e87f499b1244d519aca080c02049e4c5038f` |
| `kestrel-day.png` | `554af00ce575553505ac5b397058babd9b6331ad62365863914994d1f9923471` |
| `kestrel-night.png` | `d6738a26e3d357a04c929ac90469fd805bf4df0b2132d893bb3d6b2e9fc9f1a0` |
| `meadow-day.png` | `b549d0e5bac1eb479127686d4176a0783a395c2cdcc77a8cdd6efee0e9b1cc77` |
| `swift-approach.png` | `b228d425d357a0e9e453275b41bc22a05e82c483b8377c541bdf026e85616c31` |
| `swift-day.png` | `debce876ee4d78137adc36a1621970fc241262c6f7b06857108a5eb545acb22b` |
| `swift-night.png` | `2181435098008c61b0929fcd6ff33568501bf9a158671171b0bc9c4054af0d73` |
| `trainer-day.png` | `2321503f0425c5c492d92c223a7f6b9b78564bc0ff9a8f8b044b4482f19b9684` |

The actual pixels were inspected for readable instruments, control silhouettes, nameplate clearance, closed footwells, approach-state indications and night face/needle illumination. The night cabin is intentionally dark; readable instruments do not establish that every physical control is easy to find with a native mouse in darkness. Ground Kestrel shows PARK ON/BRK TOT 100% with a neutral service-brake handle; the approach shows PARK OFF and the current aggregate throttle/flaps state.

Capture logs also record the software-adapter/no-audio-device warnings, GLB hierarchy warnings for Meadow/Kestrel, and terrain overlay precision guards hiding unsupported thin overlays. These successful captures are not a claim of warning-free native runtime.

## Remaining limits

- No native window, human mouse, physical GPU, Windows cockpit runtime, joystick/HOTAS or speaker test was performed
- Automated ECS pointer and camera/ray tests do not establish operating-system cursor dispatch or pilot usability
- No off-axis/pedal screenshot is included; look-around, camera reset, endpoint reachability and pedal clearance have automated coverage
- The generated interior hides the entire exterior GLB, removing Swift’s opaque canopy and Kestrel’s static interior overlap. Exterior nose and wings are consequently absent from the cockpit view; chase/tower restore the original GLB
- Fuel, mixture, electrical buses, radios, engine start, pressure/gyro dynamics and magnetic compass behavior remain outside the simulated systems. Throttle percentage is not engine RPM/N1; BODY YAW is not a turn coordinator; altitude is ellipsoid height
- Real aircraft informed broad ergonomics. These fictional aircraft are not certified replicas, and no flight-model equivalence is claimed
- Regional live-download, exact-source archive, source admission, native same-archive smoke and post-publication asset gates remain mandatory for release
