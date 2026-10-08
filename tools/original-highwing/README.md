# Original high-wing exterior for Light Single: isolated candidate

This directory prepares a replacement exterior from the project's original
Meadow Trainer geometry. It does **not** change the repository, install an asset,
alter a release gate, or constitute permission to publish. The preparation base is
`d918943a70d01644b5a6bebb8a903a5bbc89139f` of `Xenoah/flightsim-claude`.

The candidate is `assets/aircraft/light_single.glb`:

- 140,840 bytes; SHA-256
  `b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc`
- 46 meshes, eight materials, 3,759 exported vertices, 5,756 triangles
- Original Meadow GLB SHA-256
  `c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412`
- Light Single profile SHA-256
  `8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25`

The candidate profile copy is byte-identical to the existing Light Single JSON.
It keeps `id: light-single`, display name, model path `aircraft/light_single.glb`,
forward `-x`, up `+y`, length 8.3 m, all dynamics, all controls, camera eye, and
the existing `turbine` sound category. This task does not silently change that
sound to Meadow's `piston` category. No profile or replay format is introduced.

## Provenance and exact transformation

The `source/` directory contains unchanged Meadow GLB, editable `.blend`, generator,
validator, profile, Light Single JSON and both repository license files. Their
exact source paths, sizes and hashes are in `evidence/source-manifest.json`.
The project records Meadow geometry, materials and procedural authoring as
MIT OR Apache-2.0. No downloaded mesh, texture, brand, blueprint, or unresolved
Meshy Light Single geometry is used by the adapter or renderer.

`adapt_original_highwing.py` accepts only the pinned original Meadow bytes and
emits only within this isolated candidate directory. The transformation is a
right-handed -90 degree rotation around glTF +Y:

    original (x,y,z) -> candidate (-z,y,x)

This is an exact sign/permutation of float32 positions and normals, with
determinant +1. It does not reflect, rescale or recenter geometry. Triangle indices,
UV data, materials, node identities, names, body-coordinate contact extras and
embedded buffer layout are unchanged. Existing geometric accessor bounds are
updated. The GLB JSON is reserialized; its smaller byte length reflects bound
number spellings and padding, not omitted geometry. The source asset's exporter
metadata is retained, and this sidecar explicitly records the subsequent adapter.

## ModelFit, contact and identity checks

The repository's `crates/flightsim-render/src/model.rs` defines forward-to-body-X,
right = forward cross up, and up-to-body-negative-Z. Therefore:

    original Meadow body = (z,-x,-y)
    adapted Light Single body = (-x,-z,-y)

All vertex coordinates and normals match exactly in those respective body bases.
The triangle cross products also match exactly, proving preserved winding under
the rotation. Units stay metres. Origin stays the intended dynamics CG; no new
mass/inertia estimate is inferred from the mesh.

Candidate glTF bounds are `(-3.250000238,-1,-5.5)` to
`(5.050000191,2.220000029,5.5)`. Extents are 8.300000429 m nose-to-tail,
3.220000029 m vertical, and 11 m span. The intended double-precision fit factor
is 0.9999999483; float32 length scaling, as used by ModelFit, yields 1.0. These are
analytical checks against current source, not a newly observed simulator log.

The three bottom tyre centers agree with the unchanged FDM contacts to 2 micrometres:

- Nose: body `(1.6,0,1.0)` m
- Port main: body `(-0.8,-1.3,1.0)` m
- Starboard main: body `(-0.8,1.3,1.0)` m

The unchanged eye, body `(0.6,-0.25,-0.9)`, is candidate glTF
`(-0.6,0.9,0.25)`. Eye and CG pass three distinct ray-parity containment checks
against the closed exported fuselage. Identity scene transforms preserve their
placement. The cockpit remains the existing shared procedural cockpit.

`crates/flightsim-sim/src/replay/identity.rs` identifies the validated physical
configuration and FDM revision, explicitly excluding visual model, sound, name
and input mapping. Neither that code nor its inputs are edited here. A byte-identical
profile therefore preserves the replay identity inputs. This is source/byte
evidence, **not** a new runtime replay test or a claim of complete trajectory
reproducibility. Historical partial replay identities retain their limitations.

## Reproduction and evidence

From this directory:

    python adapt_original_highwing.py
    python validate_candidate.py
    python check_rejection_cases.py
    python adapt_original_highwing.py --output evidence/rebuilt.glb
    cmp assets/aircraft/light_single.glb evidence/rebuilt.glb

The adapter and validator use only Python's standard library. The validator is
separate from the adapter and does not import its transformation code. It checks
GLB/chunk/accessor bounds, finite attributes and JSON, embedded-only resources,
identity nodes, normalized normals, real accessor bounds, nondegenerate triangles,
preserved nongeometry bytes/metadata, body-space equivalence, contacts, fit,
containment and unchanged profile. The negative checks reject an unrotated
renaming, missing normal rotation, reversed winding, external buffer, NaN position
and unexpected node translation. A second adapter process emitted identical bytes.
The original repository Meadow validator also passes, with its original model
and profile hashes unchanged.

`render_candidate.py` imports the final candidate GLB from an empty Blender scene;
it does not render the original source in its place. It creates a separate QA
studio and never exports studio ground, lights, camera or new geometry into the
candidate. To inspect/import without rendering:

    blender --background --threads 2 --python render_candidate.py -- --no-render

For actual three-view studio renders, coordinate CPU resource use first, then:

    blender --background --threads 2 --python render_candidate.py

The installed executable reports Blender 4.3.2 and build hash `Unknown`.
Its executable SHA-256 is
`0dfe9af0f3e67643ecf15327fa5e14e7315f61d1a81afdcafe76a71b1918e2b4`.
No fresh installation or independently authenticated vendor package is claimed.
It successfully imports all 46 meshes and eight materials with correct bounds.
The JSON record `evidence/blender-import-and-render.json` records the three
completed image outputs and hashes. Blender exited successfully after rendering
all three from the emitted candidate GLB. The author inspected the actual
three-quarter, starboard and front pixels. They show a coherent upright high-wing
aircraft with intact wings, struts, tail, paint bands, glazing, propeller and
tricycle gear. No obvious missing faces, detached parts or orientation defect
was observed. Bright white highlights and undenoised grain limit fine shading
assessment; the front frame has a narrow studio-floor/background edge at its
bottom. These are QA-image limitations, not new exported components.

Images in `previews/` are **asset studio renders, not FlightSim screenshots**:

- [Forward port three-quarter asset preview](previews/candidate-three-quarter.jpg)
- [Aft starboard asset preview](previews/candidate-starboard.jpg)
- [Front asset preview](previews/candidate-front.jpg)

The QA display's Standard transform is not a change to any simulator tone recipe
or a choice of release appearance. The independent static and image review is
recorded separately in `evidence/independent-review.md` and `.json`.

## Integration plan, requiring a separately approved change

1. Review this exact original-asset candidate and its visual evidence. Keep the
   original Meadow generator, `.blend`, GLB and profile intact. Preserve the
   historical Meshy provenance record; this candidate establishes no rights over
   those older bytes.
2. In an isolated review branch, replace only the current Light Single exterior
   payload with this pinned candidate. Keep the existing Light Single profile
   byte-identical, including model path, axes, dynamics, controls and sound. Add
   the adapter, source/provenance record, current validation evidence and focused
   source-tool checks. No runtime or FDM change is needed for the proposed basis.
3. Review the implications for current source-archive path exclusion, asset rights
   inventory, attribution and hardcoded historical Light Single hashes in Meadow,
   Kestrel and Cedar asset validators. Preserve historical evidence while making
   any current checks explicit about the exact original replacement bytes. This
   preparation does not perform those changes or reinterpret a gate as passed.
4. Run ordinary development integration on the resulting exact revision: original
   Light Single selector/path, async scene load, actual ModelFit log, parked wheel
   appearance, cockpit/chase views, aircraft switching and replay compatibility.
   Recheck Swift Sport. The historical Meadow +Z application checks are useful
   background, but do not qualify this newly adapted -X asset.
5. A two-aircraft binary remains a separate release proposal. Any allowlist and
   expected two-aircraft acceptance change must be explicit and independently
   reviewed. Verify both aircraft from the extracted Windows candidate, checksums,
   native dependencies, normal exit and actual screenshots. A new asset does not
   settle AgX/tonemapping, native/platform rights, user variant choice, CI or
   publication authorization. The reviewed Tony+Filmic source proposal is not
   adopted by this preparation.

No binary, source archive, source push, production profile or packaging policy is
changed here. No Rust build or shared target directory is used. Propeller, wheels,
gear and control surfaces remain static; glazing remains opaque. No aerodynamic
calibration, certification, performance figure, release readiness or newly cleared
third-party right is claimed.
