# Aircraft data package: app and native observation

2026-10-05. The original Swift package passed the actual ordinary application's
validate/import/inspect path and one Linux desktop load, fit and screenshot.
The implementation remains an offline data contract and basic import; it adds no
package picker, in-flight activation, textured/animated GLB support or new aircraft
flight qualification. Windows import and distribution readiness remain open.

## Exact local evidence

- Runtime source checkpoint: `1427ac12d0cfa23e62737172a8e1bfd6209c44fc`,
  tree `5256f5d7de870accaf37f4a25063be05c2dd541b`
- Reviewed source-binding migration: `d647619ac09eae6a326c80cb930f51b946115145`,
  with no Rust, Cargo, schema or asset changes relative to that runtime checkpoint
- Rust 1.93.0, Bevy 0.18.1, Linux GNU dev profile; ordinary default features
- Observed executable SHA-256:
  `fb727cd8e0bb0f67d115c9c84622da060f7a43b4c646add22ed271032a4d4718`
- Cargo.lock SHA-256 remains
  `9ef5a7ccfa27755027201438dd15b973ff2a834565e62c25b41086f85b612c06`

These are local build/review checkpoints, not claims that those commit IDs are
published GitHub release tags. The final source commit's CI is separate.

## Automated and CLI checks

- Content: 36 tests, including 23 existing terrain tests, plus all-target Clippy,
  formatting, four manifest schema checks and architecture validation
- Ordinary app: 393 unit tests plus one original-GLB hierarchy integration passed;
  one existing optional real-region fixture test remains ignored
- Analytical/commercial app: 351 unit tests plus that integration passed; the
  same existing optional fixture test remains ignored
- Both app feature selections passed all-target Clippy and actual builds
- Nine actual ordinary CLI checks cover validation, import, inspection, unchanged
  profile/GLB bytes, no overwrite, hash-valid invalid physics with no publication,
  corrupt bytes, installed tamper and mutually exclusive command handling
- All three actual commercial-binary package commands reject before package I/O
- Independent review: 125 archive/GLB/cancellation and 18 Linux installed-tree
  expectations passed, including the repaired implicit-tree-entry and Scene0
  allocation-budget boundaries
- Source migration: 149 pins, with 128 old hashes and all 102 independent anchors
  preserved; two reviewed old changes and 19 additive inputs. The regression
  suite ran 118 cases (117 passed, one Windows-only skip); the final cascade
  suite ran 35 cases (34 passed, one Windows-only skip)

Cooperative cancellation belongs to the content API/example. The app CLI has no
invented graceful-cancel flag. Process termination is the separately documented
crash/ignored-temporary-directory case. Windows reparse rejection is implemented;
NTFS hardlink rejection and native Windows package import are not claimed.
The trusted-store limitation remains explicit in [the package contract](../aircraft-packages.md).

## One actual imported-model observation

The prebuilt ordinary app ran from an unrelated working directory. Its
`BEVY_ASSET_ROOT` selected only the imported package, while `--aircraft` selected
that package's exact `profile.json`. No model or other preset was copied from the
developer checkout. Embedded shaders/font/global data were available normally.

The existing full-identity Swift replay was accepted. The log names the installed
`assets/aircraft/swift_sport.glb`, reports its 7.12 m length and scale 1.0000, and
contains no placeholder or asset-load/panic error. After stable CPU terrain
readiness, the actual app saved a decoded 1180 by 812 PNG and exited 0. The
executable and every installed package byte remained unchanged.

PNG SHA-256:
`0fe96d730f55a36f7a9ed10f69e46737d823d56a12ab67133d5f855591112862`.
The retained image visibly shows the original Swift, expected chase pose and
scale, terrain and HUD. Independent review checked the image, actual model path,
fit, log/status records and hashes.

The historical ordinary-mode scene comparison differs in 3,519 of 958,160 pixels;
only 35 exceed one channel level, with maximum 34 at sparse horizon/terrain edges.
No exact pixel-parity claim follows. Capture delays were 50 and 45 seconds on a
completed zero-duration replay, so this is neither flight-duration nor loading/
frame-performance evidence. It does not test new handling behavior.

Rendering used llvmpipe/Mesa 25.0.7 software Vulkan with no audio device. This
single original-model observation does not qualify arbitrary imported scenes,
full startup/dependency readiness, manual flight, devices, audio or Windows.
The commercial adjacent-asset policy, legacy/replay identities and all genuine
platform/dependency/publication requirements remain unchanged.
