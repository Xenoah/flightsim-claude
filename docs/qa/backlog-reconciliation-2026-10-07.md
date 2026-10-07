# Backlog reconciliation, 2026-10-07

## Acceptance policy and scope

All ten issues below were still open when their GitHub bodies were checked for
this audit. Their original acceptance criteria are preserved. This record adds
current checks to the [October 1 acceptance table](overnight-status-2026-10-01.md#issue-acceptance-and-remaining-work)
and [current handoff](../HANDOFF.md); it does not replace frozen evidence or
reinterpret implementation as publication.

The existing closure policy requires integration into main, successful exact-head
CI, and verified release evidence. Consequently, the six software-complete issues
**#10, #13, #14, #22, #23 and #33 remain open** pending those final gates. This local
audit does not establish final-main CI or a new qualified binary release. The
verified published release remains [alpha.20](https://github.com/Xenoah/flightsim-claude/releases/tag/v0.6.0-alpha.20);
its evidence cannot qualify the later features.

## Issue disposition

| Issue | Implemented or evidenced work | Still required; disposition |
|---|---|---|
| [#2](https://github.com/Xenoah/flightsim-claude/issues/2) Physical gamepad | Diagnostics, saved calibration/deadzone and synthetic connection recovery | Record a named physical device and its actual input/disconnect/reconnect results. **Open: hardware acceptance missing** |
| [#5](https://github.com/Xenoah/flightsim-claude/issues/5) Turbulence handling | Deterministic scenarios, numerical envelopes and separate subjective worksheet | A person must fly the reference aircraft and record handling impressions. **Open: human playtest missing** |
| [#6](https://github.com/Xenoah/flightsim-claude/issues/6) Real-DEM visuals | Sourced fixture, rehearsal, historical observations and four fresh software-Vulkan captures | High-altitude Balzers selection falls below the package's L10 roots and displays global fallback; night near-terrain continuity and moving LOD/depth acceptance remain unverified. **Open: source/LOD coverage and visual acceptance** |
| [#9](https://github.com/Xenoah/flightsim-claude/issues/9) HOTAS/remapping | Six-action axis/button mapping, curves, persistence and simulated multi-device/native-channel routing pass | The [maintainer's hardware criterion](https://github.com/Xenoah/flightsim-claude/issues/9#issuecomment-5424638518) requires simultaneous recognition of actual multiple controllers. **Open: physical multi-device verification missing** |
| [#10](https://github.com/Xenoah/flightsim-claude/issues/10) Multiple aircraft | Versioned profiles, distinct aircraft, FDM/model/control changes and selection/load-failure tests pass | **Software-complete; open pending final main/CI/release acceptance** |
| [#13](https://github.com/Xenoah/flightsim-claude/issues/13) Surrounding traffic | Provider interface, deterministic synthetic traffic, identity/position/orientation display and stale/expiry handling | **Software-complete; open pending final main/CI/release acceptance**. Live ADS-B is not claimed |
| [#14](https://github.com/Xenoah/flightsim-claude/issues/14) Synchronization | Versioned local sessions, interpolation, loss/delay/reconnect and real loopback integration tests pass | **Software-complete; open pending final main/CI/release acceptance**. Trusted local/LAN scope; no WAN/security or physical multi-machine qualification |
| [#22](https://github.com/Xenoah/flightsim-claude/issues/22) Vertical datum | Strict datum admission, offline geoid normalization, height/provenance contract and regression tests; historical independent reference evidence retained | **Software-complete; open pending final main/CI/release acceptance**. Historical large reference and real-DEM results are not fresh reruns |
| [#23](https://github.com/Xenoah/flightsim-claude/issues/23) Malformed PBF | Safe parser path, overflow/index/error fixtures and deterministic output/skip-report regression tests pass | **Software-complete; open pending final main/CI/release acceptance**. No exhaustive-input or total-memory guarantee |
| [#33](https://github.com/Xenoah/flightsim-claude/issues/33) Attitude clipping | Small reproduction retained, circular production mask, UI tests and archived rendered-case pixel checks pass | **Software-complete; open pending final main/CI/release acceptance**. Archived pixels are not a new GPU render |

## Checks executed for this audit

Rust checks used Rust 1.93.0 with locked dependencies. The audited Rust sources
are those at `5ca7ef6f2255b8cca94a649ada295751dd7e8417`; subsequent publication
preparation through `cdce0cddc06335c421dc58206b5d8ae9ec5b8208` changes only
attribution, documentation, package checkout attributes and Python regression
coverage, not these Rust sources.

- `cargo test --locked --offline -p flightsim-net`: **18 passed**, including
  nine real UDP-loopback integration tests and seven protocol/traffic tests
- App/input/UI all-target checks: **393 app passed, one optional regional fixture
  ignored; one real-GLB hierarchy test passed; 107 input and 262 UI passed**
- Tilegen all-target checks: **175 passed**, including datum/geoid/pipeline and
  hostile-PBF coverage
- Separate optimized `pbf_hostile_probe`: **17 passed**, including integer
  overflow, invalid string-table indices and deterministic valid output
- The pixel checker was rerun against the **36 archived attitude cases**:
  zero classified sky/ground pixels outside the circular aperture with the
  existing antialias tolerance. The archived reproduction still exhibits its
  original leak. See [the original rendered evidence](attitude-boundaries-2026-10-01.md)
- Full publication Python suite: **316 ran, 315 passed, one skipped**
- A fresh `core.autocrlf=true` checkout reproduced two Balzers sidecar failures;
  scoped LF attributes preserve the ZIP/sidecar bytes and all **21 focused tests
  then passed** in that checkout. The full suite includes a new checksum-ledger
  regression. The ZIP's SHA-256 and original payload notices are unchanged

Aircraft checks cover different dynamics/models/control rates, takeoff/approach,
invalid profiles, repeated family switching and failed-GLB rollback. Input checks
cover arbitrary six-action button mappings, native event routing, curves and
save/reload. Network checks cover lifecycle, packet loss/reconnect, sequence
reordering, delayed replies, bounded interpolation and stale expiry.

The [500,000-point EGM2008/EGM96 reference comparisons](data-boundaries-2026-10-01.md#independent-geoid-verification)
and earlier native aircraft/traffic/terrain observations remain **historical
evidence**. The external reference datasets were not revalidated in this audit.
No physical controller, human handling, speaker, hardware-GPU or new Windows
binary-release pass is inferred from these automated checks.

## Fresh real-DEM rendering observations

Four current-binary 1280x720 offscreen captures completed with exit status zero
using Mesa 25.0.7 / LLVM 19.1.7 llvmpipe. Binary SHA-256:
`c6fbe8a58f9867b217990e3b7473bd3084cb012b7950b37512a4e908647cfdb8`.
The authenticated Debian packages were extracted only into the local workspace.
The Balzers manifest and all 174 payloads were independently rehashed before use.

A fixed-pose 100 m AGL day/night pair uses the production v3 replay codec with
zero duration; it is not moving-flight evidence. The day image shows regional
valley detail without an obvious gap in that single view. At night the silhouette
is continuous, but fine terrain is too dark to establish seam/reversed-face
acceptance. No physical-GPU or frame-rate result is claimed.

At the same Balzers location, +3000 m AGL selects 35 tiles through L9, while the
package contains only L10--13. All displayed tiles are therefore global fallback,
which cannot qualify as high-altitude regional-DEM validation. An independent
production `LodSelector` probe found 2 regional roots at +2100 m AGL, 1 at +2200 m
and zero at +2300 m. These are location-specific observations, not general
thresholds. A designed source-aware LOD or properly prepared ancestor-data remedy
and moving-transition/depth checks remain required under #6. Four distant
synthetic-airport overlays are correctly precision-hidden; this package contains
no local airport/runway, so they are not evidence of a Balzers runway defect.

## Binary publication gate

The existing release authorization checker reports **blocked**, with nine
reported blockers for the alpha.21 recipe. These remain independent of source CI:

1. Unresolved redistribution rights for `assets/aircraft/light_single.glb`
2. The same asset lacks reviewed release-allowlist coverage
3. Missing completed dependency-review record; the exact MSVC inventory already
   exists and lists 359 packages, but does not itself complete that review
4. Dependency review is still required beyond notice collection
5. Unverified exact upstream AgX LUT license evidence
6. Unpinned Blender/Filmic source/version and asset-license evidence
7. Missing primary license-grant evidence for `constgebra@0.1.4`
8. Missing primary license-grant evidence for `hexf-parse@0.2.1`
9. Missing committed inventory-bound binary publication authorization

See [the release checker](../../scripts/check-release-authorization.py) and
[distribution review](../release/commercial-distribution-audit.md). This record
does not grant rights, select license alternatives, authorize binary publication,
or weaken the Windows extracted-package/rendering gates. None of the ten issues
is reported closed or fully accepted here.
