## alpha.23 original analog 3D cockpit

The first original Cessna-style light-aircraft cockpit now couples eight physical
instruments, needles, yokes, pedals and panel controls to simulator state. This
shared implementation is used by Light Single, Swift Sport and legacy-model
external profiles; it is not a replica of an identified real aircraft. Right-drag
look-around, mouse-operated flight controls, panel illumination and a hideable
ordinary HUD complement the existing keyboard and configured-controller inputs.

Gauge labels describe their actual sources: equivalent airspeed, ellipsoid
altitude, true heading, body yaw rate and throttle command. They do not claim a
calibrated pitot/static, barometric, magnetic-compass, gyro or engine-RPM model.
Replay controls are locked; recorded effective-axis animation is labeled, and
separate trim is unavailable rather than reconstructed.

Fuel, mixture, starter/magnetos, electrical failures, circuit breakers,
COM/NAV radio tuning, transponder and pressure/gyro dynamics remain unmodeled.
Bounded-model jets and turboprops retain their existing model-owned interiors and
2D fallback. Aircraft-specific cockpit differentiation remains pending. Generated
geometry, display images and bitmap glyphs are original project source; no reference
photograph or third-party mesh, texture or font is included.

See the [cockpit control/instrument guide](https://github.com/Xenoah/flightsim-claude/blob/81bb3ea9c19c0016521a610dbeafb5029caa8713/docs/3d-cockpit.md)
for operating instructions and exact limitations. That guide is frozen source
documentation, not final-build evidence. Development tests and Linux llvmpipe
captures do not establish Windows, physical-GPU, controller, audio or pilot-handling
acceptance. This release requires its own source-bound Windows build and extracted
two-aircraft captures. Terrain issue #6 remains open, with the earlier bounded
terrain QA residuals and precision limits unchanged.

## Windows component terms: review before download or redistribution

This Windows executable includes proprietary Microsoft native runtime/startup
code with separate [component terms, version 2026-10-09-2](https://github.com/Xenoah/flightsim-claude/blob/@SOURCE_SHA@/docs/release/components/MICROSOFT-COMPONENT-TERMS.txt).
Read the complete English terms, [Japanese reference translation](https://github.com/Xenoah/flightsim-claude/blob/@SOURCE_SHA@/docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt)
and [component notice](https://github.com/Xenoah/flightsim-claude/blob/@SOURCE_SHA@/docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt).
All three are included in the ZIP with the existing copyright and license notices.
The supplement covers only incorporated Microsoft code, including its protection,
Microsoft/supplier warranty and liability terms, and redistribution obligations.
FlightSim's MIT/Apache-2.0 source rights, separately licensed components and ordinary
FlightSim benchmarking are unchanged.

At first launch, review the full terms before entering the simulator, then choose
**Agree and continue / 同意して続行** or **Decline and exit / 同意せず終了**.
Agreement is not preselected. Declining exits without recording acceptance.
Only your explicit choice stores local acceptance bound to the exact terms
version/hash. Changed terms require a new choice. No account, telemetry or
server-side consent receipt is added. The screen also provides the component notice.

**Redistributors: before onward distribution, run `flightsim-app.exe --component-terms`,
review the same complete terms and explicitly agree.** This terms-only route exits
after your choice without starting the simulator and also lets you reopen the
terms later. Retain the terms/notices and require onward distributors and external
end users to agree to protective terms as the supplement requires. Downloading,
opening a notice or a successful build/smoke test is not recipient assent.

Windows, UCRT, graphics drivers and the **x64 Visual C++ v14 runtime** remain
external prerequisites. Obtain a current supported x64 v14 package from
[Microsoft's official runtime guidance](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170).
No Microsoft DLL, LIB or redistributable installer is separately bundled. Do not
download individual DLLs from unofficial sites. Visual Studio purchase or account
creation is not required by this component supplement.

日本語：初回起動前に英語正文と日本語参考訳を確認し、同意するか終了するかを選択してください。
再配布者は再配布前に `flightsim-app.exe --component-terms` で同じ規約への同意を明示し、
規約・表示を残して次の配布先にも必要な同意を求めてください。専用操作はシミュレーターを
起動せず終了し、同意記録は正確な版・ハッシュに対応して利用端末内だけに保存します。
ソースの MIT／Apache ライセンスと通常のベンチマークは変わりません。
