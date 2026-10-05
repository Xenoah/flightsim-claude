# Explicit analytical build and cockpit help — native QA, 2026-10-05

## Result and scope

The ordinary application retains its existing tone response. An explicitly selected
`--no-default-features --features analytic-tonemapping` build uses Reinhard and
excludes the three reviewed Bevy tonemapping LUT payloads. This is an alternative
build configuration, not a runtime quality tier or a realism improvement.
Reinhard is darker and changes low-sun highlights. The earlier isolated proof's
roughly 8–11% lower daylight sample luminance remains a compatibility tradeoff.

The separate cockpit UI correction clears the replay help from the V/S dial at
830×582. The ordinary and analytical fixed captures both preserve every pixel
outside the affected help area relative to their respective earlier controls.
The known 640×480 long-notice/HUD versus dial limitation remains open; see the
[layout report](cockpit-replay-help-2026-10-05.md).

All native observations here use Linux Mesa 25.0.7 software Vulkan/LLVM 19.1.7.
They do not establish physical-GPU performance, Windows rendering compatibility,
aircraft flight envelopes, audio-device behavior, or distribution rights.

## Exact source and artifact boundaries

- Feature runtime: local commit `c36100a08128076cb7d266356e1b8106c601849f`, tree `c57083e26a7c7fcfa5726aa87e8fb4490fafa2dc`
- Audit-only follow-up: `98de359aae533dad4aa914bc4caabcd5239d82f1`
- Independently reviewed integrated runtime/UI/bindings: `d00e651cd164b90ec7f25a382c875f3ee26c9f7a`, tree `e5876e807b5579443f4b2c7cb5545efedd766c92`
- Later CI/docs commits do not change those compiled runtime inputs

The integrated ordinary binary SHA-256 is
`40098f431235de208bd21a29566907d500876bf609ebf67062c14820de1d50d5`
(151,611,808 bytes). The analytical binary is
`5180937baa42c2a84fd401f0174092799de556feb9dc50aa459d1dcd5d9e31bb`
(150,940,040 bytes). These are frozen local QA binaries, not release artifacts.
Cargo.lock remains `9ef5a7ccfa27755027201438dd15b973ff2a834565e62c25b41086f85b612c06`.

Both app modes and both sun_clock modes were separately built and audited.
Ordinary positive controls contain all three exact payloads; analytical artifacts
contain none. Selected compiled features/fingerprints, decoder features, linked
libraries and source dep-info agree with the package/target graphs. A graph-only
result or raw byte search alone is not the evidence. The integrated app binaries
were rebuilt and audited after the UI/binding merge.

Ordinary package graphs retain 338 Linux / 347 Windows normal-build nodes with the
same versions/features. Exact Windows candidate workspace metadata has 359
closure packages and additionally records render/default routing; it requires a
fresh inventory even though package-scoped versions are unchanged. These Windows
graph checks are not a Windows build or runtime result.

## Native matrix

Every case used a unique directory, exact source/binary/config hashes, actual
native window and a separate cold shader cache. Successful PNGs were decoded and
hash-checked against their launcher receipts; native runs exited 0. Preparation
and compilation costs are not steady-frame performance measurements.

| Cases | New source and scene | Comparison result |
|---|---|---|
| 044/045 | Both modes, fixed clear daytime Chase | Ordinary preserves the visible scene; mean absolute channel delta 0.000834/255 against its older ordinary control. Analytical delta 0.000020/255 against its earlier proof |
| 046/047 | Both modes, fixed night cockpit | Each image is pixel-identical to its respective older-mode control; very dark exterior limitation remains |
| 048 | Analytical HIGH graphics, ULTRA inside-cloud Rain | Pixel-identical to earlier analytical control; actual cloud runtime ready |
| 049 | Analytical HIGH graphics, ULTRA low-sun ocean | Maximum difference 1 channel code; actual water runtime ready with 61 ready pairs. Existing regular wave bands remain |
| Four sun_clock cases | Both modes, HDR atmosphere and LDR/no atmosphere | Each new image is pixel-identical to its respective older-mode example |
| 050/051 | Integrated ordinary/analytical 830×582 cockpit | Help clears V/S; all pixels outside x500–819/y395–484 match each respective original defect image exactly |
| 052 | Integrated analytical actual 37.2 s replay lifecycle | Native PNG saves the intermediate 830×582 paused 4 s Chase state; later completed cockpit is separately retained as a CUA JPEG |

These are 13 successful app/example PNG captures plus one supplemental native CUA
JPEG. They are not 14 application screenshot-pipeline captures. The analytical
comparisons above are against the earlier analytical proof, not a claim of
Tony/Reinhard pixel parity. The ordinary control and earlier proof each retain
independent immutable binaries and scene fixtures.

The ocean case has four deliberately precision-hidden distant airport overlays
and no required runway at capture, matching its older control. It must not be
summarized as zero hidden overlays across all scenes. Existing fog/cloud horizon
artifacts, aircraft-edge outlines, regular water bands and moonless darkness are
not fixed by selecting a different tone curve.

## Lifecycle observations

Case 052 used the authentic 37.2-second Rain/wind recording. Native observations
and logs verify pause at 14.0083 s, rewind to 4.0083 s, Cockpit→Chase→Free→Tower→Cockpit,
map open/Close/reopen/keyboard return, and 1180×812→830×582→1180×812 resizing.
HIGH and ULTRA graphics were selected, then Shift+F4 restored LIGHT. Final logs
show the same active camera without IBL and an empty helper list. Resume reached
37.2 s COMPLETE. Cloud and water remained LIGHT throughout this lifecycle case.

Some immediate CUA observations showed a stale frame after resize/view/map
transitions; settled observations establish the result. The bound-window Close
attempt was not confirmed; the full-desktop button click subsequently closed the
map. A later native screenshot verifies the completed cockpit, restored full-size
help, six dials and Light state. No continuous video or hardware input-latency
claim follows.

The one-shot app PNG was admitted at 08:13:48.735694 UTC and saved at 08:13:49.375883,
before later quality/view changes. Its 830×582 PNG is 498,722 bytes, SHA-256
`e45e620ca92688de093fce82d7adc2b1da2df83ba4f1278a84701e623bfe61d8`.
The unedited CUA-returned final JPEG is 1180×812, 87,081 bytes, SHA-256
`284f3ffa26d0ebc5ecbd9dcc6228009fbe926e1828a7be1ddc126d37679efbaa`.
These two files document different moments and different capture routes.

## Regression and distribution boundaries

- 825 distinct app/render cases pass in each feature mode: 435 render and 390 app,
  with three existing ignored cases per mode. The 1,650 full-suite executions are
  feature repetitions, not 1,650 distinct tests. Both all-target Clippy runs pass
- 262 UI tests and UI Clippy pass, including the reproduced pre-fix overlap and
  additive layout/lifecycle regressions
- 205 candidate/tool Python tests pass, plus six additional CI-command runner
  tests. Focused reruns are not added again to these counts
- Four invalid Cargo mode combinations fail with their intended diagnostics;
  render-library neither-mode compilation succeeds
- The additive Linux CI job covers analytical source/build regressions and exact
  positive/negative artifact audits. Hosted CI status belongs to the exact public
  commit and must be checked separately

The [reviewed source boundary](replay-candidate-tonemapping-cockpit-pin-review-2026-10-05.md)
has 130 whole-file pins and 102 preserved independent anchors. Ordinary candidate
LUT, Swift-only, dependency, source, authorization and release requirements remain.
Removing these three embedded LUT payloads is not a grant for any dependency or
asset. The ordinary e643748 Windows candidate failed before delivering its first
PNG after copy submission; this Linux option neither reruns that diagnostic nor
claims to resolve it. No new binary release follows from this report.
