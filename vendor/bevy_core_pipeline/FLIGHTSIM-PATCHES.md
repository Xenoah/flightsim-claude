# FlightSim tonemapping subset (source candidate)

Based on the exact crates.io `bevy_core_pipeline 0.18.1` archive:
SHA-256 `4d0810e85c2436e50c67448d48a83bf0bb1b5849899619ae2c7ea817221e9172`,
upstream revision `f667c282dad2c1419afb5836ded22a3ec263970e`.
The archive hash equals the previously locked registry checksum. The complete
original member inventory is in `FLIGHTSIM-UPSTREAM-SOURCE.json`; the original
archive itself is not included. The registry `.cargo_vcs_info.json` is omitted
because it does not identify this modified tree.

Only `src/tonemapping/mod.rs` changes among retained upstream files. All other
upstream members, including both Cargo manifests, the internal Cargo.lock,
licences and shader files, are byte-identical. The workspace patch selects this
path package with the same version; its root lockfile entry deliberately has no
registry source or checksum. It must be inventoried as modified local source.
`Cargo.toml.orig` is retained as historical upstream source; the normalized
`Cargo.toml` is the manifest used by this workspace patch.

## Exact behavior

- `tonemapping_luts` retains its existing KTX2/zstd dependency feature closure,
  but embeds only the exact TonyMcMapface and Blender Filmic payloads
- The app and standalone render ordinary defaults remain TonyMcMapface; image
  decoding, samplers, color math and supported-method pipeline descriptors are
  unchanged. This source comparison is not a GPU pixel-equivalence result
- `AgX-default_contrast.ktx2` is absent from this source package. Its enum variant
  and public resource field remain for source compatibility, but the field is
  ignored: no AgX LUT is created or bound. Its default handle may resolve to
  Bevy's generic default image; rendering AgX is still rejected unconditionally
- Any AgX selection reaching HDR pipeline specialization or shared LUT-binding
  selection panics with an explicit unsupported-method diagnostic. Supplying
  a custom AgX handle does not bypass this policy. Shared bindings also serve
  PBR/deferred, sprite and mesh2d views. No AgX shader is selected by HDR
- Methods that do not sample a LUT reuse the already-loaded Tony handle only
  to satisfy the common layout. With the feature off it is the existing shared
  placeholder. This preserves the node's ready-image bind-group cache behavior;
  no image color is used by those methods
- Feature-off behavior retains upstream Tony/Filmic missing-feature diagnostics
  and placeholders. The app's explicit analytical configuration still selects
  Reinhard and remaps Camera3d AgX/Tony/Filmic before render extraction. Camera2d
  policy is unchanged: an explicit AgX Camera2d now fails instead of rendering
  unsupported content. Merely having the enum in an ECS test is not rendering

The AgX shader implementation remains as unchanged Bevy source, but this subset
does not support selecting it through the renderer. It is not a redistributable
copy of the missing LUT. The `agx` handle cannot be used to opt back into AgX.

## Notices and limits

Bevy's MIT and Apache-2.0 texts remain. Exact Tony MIT and the scoped Blender
Filmic BSD notice and attribution are retained in `third-party-notices`.
Filmic's configuration-level notice application is the explicit scoped source
determination in `docs/release/filmic-asset-source-decision.json`, not a claim of
an individual table header or a new upstream licence grant. No AgX licence is
asserted. The root project's existing licence texts accompany these changes.

No release recipe, acceptance checker, source pin, authorization ledger or
historical rights record is changed. Existing audits require the original
three-LUT recipe and must refuse this changed subset until separately reviewed
evidence and a separately authorized recipe exist. Both default and analytical
whole-target native inventories, compiled fingerprints/dep-info/payload audits,
render regression tests, same-bundle native captures and normal release gates
remain required. Existing green builds cannot qualify this changed dependency.
