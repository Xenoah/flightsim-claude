# ADR-0026: Explicit analytical tonemapping build

- Status: implemented in isolated source; qualification and release review remain separate
- Date: 2026-10-05

## Decision

Preserve Bevy 0.18.1 and the ordinary LUT/Tony appearance. Runnable app/render
packages own a default LUT feature, with a separate `analytic-tonemapping`
feature selecting explicit KTX2 decoding and Reinhard. The workspace render
dependency disables render defaults so the app controls its mode. Retain the
existing zstd backend, dependency versions and Cargo.lock.

The app and sun_clock explicitly select Reinhard only under the alternate
feature. The render plugin's alternate-only Last system maps Camera3d
AgX/Tony/Blender Filmic methods to Reinhard after current camera producers and
before extraction. Ordinary builds do not register that system. Camera2d and
explicit None/analytical methods are unchanged.

Reject mixed default/analytical app and render features. Reject a mode-less app
or runnable example. A mode-less render library is valid because ordinary app
supplies the dependency features. Existing ordinary multi-package CI commands
retain their defaults. Cargo's additive graph cannot be made subtractive by a
local feature guard, so exclusion claims require exact package/target graphs,
compiled feature/fingerprint evidence and payload absence with ordinary positive
controls. Tests exercise the real plugin registration in both modes.

## Consequences

The alternate curve is darker and changes highlights; it is not visual parity.
Prior native qualification is Linux software Vulkan only. Other lookup textures
remain. There is no license or release approval, engine upgrade, material,
exposure, physics, replay or control change. Existing candidate/source pins and
release checks remain binding; review the conditional main.rs delta separately
rather than weakening those gates. See [build policy and audit commands](../analytic-tonemapping.md).
