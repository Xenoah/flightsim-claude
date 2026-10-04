# Original aircraft final scene hierarchy — 2026-10-04

The actual Kestrel GLB reproduces **46 Bevy B0004 warnings**, while its completed
scene passes exhaustive hierarchy and propagation checks. Swift reproduces no
B0004 warnings and passes the same checks. This closes the specific uncertainty
that these warnings might indicate missing parent transform/visibility components
in the final loaded original-aircraft scenes. No production code, engine code,
assets or warning filters were changed.

Base: `dd42b3dac78f4f9227e80c7e4b0f49041bcb5eba`; pinned Bevy 0.18.1.
The motivating native Kestrel run also reported 46 B0004 warnings before a
successful 8.50 m / scale 1.0000 fit. A successful fit alone was insufficient:
`fit_loaded_model` filters descendants through an `Aabb`/`GlobalTransform` query,
which would omit entities missing either component.

## Real-asset test

`crates/flightsim-app/tests/aircraft_scene_hierarchy.rs` loads each shipped GLB
with `AssetServer` and `GltfAssetLabel::Scene(0)`, then instantiates `SceneRoot`
through the normal `ScenePlugin` / `SceneSpawner` path. It waits for both recursive
asset dependencies and scene-instance readiness, followed by another update.
A 15-second deadline fails with load-state details rather than hanging.

The fixture follows the pinned glTF loader's own GPU-free test setup, adding
`TransformPlugin` and `VisibilityPlugin` to exercise normal propagation. It
registers the generic `MeshMaterial3d<StandardMaterial>` type as `MaterialPlugin`
normally does. Other scene components use the application's existing
`reflect_auto_register` feature. No renderer, window, GPU or substitute mesh is
involved. These exact assets have no image, texture, animation or skin dependency.
The normal `LogPlugin` remains enabled.

| Original asset | Authored nodes / mesh primitives | Instantiated scene entities | B0004 warnings | Final checks |
| --- | ---: | ---: | ---: | --- |
| Kestrel Jet Trainer | 53 / 53 | 107 | 46 | PASS |
| Swift Sport | 31 / 31 | 63 | 0 | PASS |

Both original assets have flat scene-root nodes, one mesh primitive per node.
Bevy adds one scene root plus a primitive child per authored node. Counts above
exclude the test's external aircraft and `SceneRoot` entities.

For each asset, the test verifies:

- The set of all `SceneSpawner` instance entities exactly equals all descendants
  reached from the `SceneRoot`, with no duplicate/cyclic or detached entity
- Every instantiated entity has `Transform`, `GlobalTransform`, `Visibility`,
  `InheritedVisibility` and `ChildOf`; the parent exists, contains matching global
  transform and inherited-visibility components, and links back through `Children`
- Every local/global matrix is finite, every global transform is nonsingular,
  and every global matrix matches `parent_global * local` within `1e-4`
- Exactly all 53 / 31 mesh primitives exist, each with loaded nonempty geometry,
  a loaded material, and finite nonempty bounds
- All descendants inherit visible state, then hidden state after moving and
  hiding a nonidentity ancestor, then visible state again; the complete hierarchy
  and transform assertions are repeated after both changes

No transform/visibility query can filter an invalid entity out of these checks.
Warning counts are observations, not assertions or acceptance criteria.

## Pinned engine evidence and interpretation

The local official crate sources were inspected:

- `bevy_gltf-0.18.1/src/loader/mod.rs`: `load_node` spawns each authored node with
  `Transform` and `Visibility`; the loader also creates a transformed visible
  scene root and mesh-primitive children
- `bevy_scene-0.18.1/src/scene.rs`: `Scene::write_to_world_with` first allocates
  all destination entities empty, then copies their components by archetype
- `bevy_transform-0.18.1/src/components/global_transform.rs` and
  `bevy_camera-0.18.1/src/visibility/mod.rs`: `GlobalTransform` and
  `InheritedVisibility` use `validate_parent_has_component` insertion hooks
- `bevy_ecs-0.18.1/src/hierarchy.rs`: this hook checks the parent's components
  synchronously at insertion, rather than waiting for scene readiness

This provides an insertion-order explanation consistent with the warnings and
the passing final-state checks. The test establishes that the reproduced
warnings do not describe a persistent missing-component hierarchy in either
completed scene. The build disables component debug names, so the warning text
alone does not identify which of the two component hooks fired. This work does
not claim that all possible B0004 warnings are harmless.

## Verification

```sh
cargo test -j 2 -p flightsim-app --test aircraft_scene_hierarchy -- --nocapture
cargo clippy -j 2 -p flightsim-app --test aircraft_scene_hierarchy -- -D warnings
cargo fmt --all --check
```

The focused real-asset test passed (one test covering both GLBs), focused Clippy
passed with warnings denied, and workspace formatting passed. Logs are
`aircraft-scene-hierarchy-test.log` and `aircraft-scene-hierarchy-clippy.log` in the
integration QA expansion log directory. This is focused CPU scene-graph coverage;
it does not replace native cockpit/chase, lighting/transparency or flight QA.

Verified unchanged GLB SHA-256:

- Kestrel: `5a9d8747084f7ad6ff93dbadc80d87acbef42c6882030c0b88ca3aded16bef21`
- Swift: `9f30f6f9babe87a54d0f1f5da104f719d7e99cb05aada2848f8ea88b0bf9e3b1`

No publication, distribution or asset-rights conclusion is made by this check.
