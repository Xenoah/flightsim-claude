# Independent review of isolated original high-wing candidate

Reviewed against repository commit `d918943a70d01644b5a6bebb8a903a5bbc89139f`.

**Result: PASS for the bounded asset-preparation contract and inspected studio views.** No blocking defect found in the adapter, candidate bytes or reviewed views. Live simulator acceptance remains unrun; this review is not a publication, packaging or release approval.

## Exact inputs and output

All eight copied source files match their named paths in the pinned Git commit byte for byte, including the original GLB, Blender source, generator, validator, both profiles and both licence texts. Candidate GLB SHA-256 is `b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc` (140,840 bytes). Re-executing `adapt()` in memory reproduces those exact bytes. A modified source is rejected by its hash pin.

The candidate Light Single JSON equals the pinned production JSON exactly, SHA-256 `8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25`. This includes `id`, dynamics, controls, model path/axes/length, camera eye and the existing engine sound field. Repository code, replay policy, profiles, production assets and packaging files were not edited; Git status was clean when checked.

The upstream generator starts with an empty Blender scene and constructs the shape with procedural meshes and standard primitives. No imported mesh path or Meshy geometry contributes to this adapter. `ATTRIBUTION.md`, `docs/aircraft/meadow-trainer.md` and the exact `assets-aircraft-meadow_trainer-glb` entry in `docs/release/asset-rights-manifest.json` record the original source under MIT OR Apache-2.0. That inventory entry remains `source_only` / `original_source_recorded`; it is not a binary admission. This verifies the source chain and declared project licence, not an independent legal title guarantee.

## Independent geometry checks

The renderer's `ModelFit::rotation()` forms right from forward cross up and maps model forward/up to body +X/-Z. Deriving those bases independently from the two JSON profiles gives Meadow `(z,-x,-y)` and Light Single `(-x,-z,-y)`. The adapter's `(x,y,z) -> (-z,y,x)` is a proper -90-degree Y rotation with determinant +1. The two ideal body-space maps are identical after adaptation.

A separate parser checked 7,169 unique position/normal vectors across 88 accessors at IEEE-754 bit level: each target vector consists of the exact required lane permutation and sign-bit change. All other BIN bytes, including index and UV bytes, are identical. JSON metadata is identical after removing only geometric accessor bounds, and all bounds present in the target are recomputed correctly. There is no rescale, origin shift, reflection, triangle reordering, node transform, material change or added resource.

Counts stay 46 mesh nodes, 3,759 exported vertices, 5,756 triangles and eight materials. Target bounds are `(-3.2500002384,-1,-5.5)` to `(5.0500001907,2.2200000286,5.5)` metres. The forward extent is 8.3000004292 m, the span is 11 m. The length-8.3 adapter predicts a double-precision scale of 0.9999999483 and direct float32 scale of 1.0. Actual Bevy quaternion conversion, transformed-AABB recovery and live scale logging were not run, so exact ideal basis equality is not a claim of bit-identical runtime vertex transforms.

Independent body-space bounds put the three tyre-bottom contacts within 2.4e-8 m of `(1.6,0,1)`, `(-0.8,-1.3,1)` and `(-0.8,1.3,1)`. The unchanged eye maps to candidate coordinates `(-0.6,0.9,0.25)`. A separate signed-solid-angle containment method, rather than the author's ray parity method, yields 4π for both origin and eye, confirming both lie inside the closed fuselage. The origin remains a placement convention; this is not a new mass/inertia calculation or proof of an unobstructed cockpit view.

## Validator checks

The author validator runs successfully and reproduces its saved JSON report exactly after JSON normalization. Nine in-memory negative controls all fail with `ValueError`: unadapted source geometry, position perturbation, normal perturbation, triangle winding reversal, node translation, changed material metadata, external buffer URI, incorrect accessor bound and a nonfinite position. Full exception messages and source/output hashes are in `independent-review.json`.

## Remaining acceptance boundaries

- All three final candidate studio previews have now been inspected; their exact hashes and findings appear below
- The author completed Blender import and studio rendering; this reviewer inspected that evidence and the actual image pixels. FlightSim loading/rendering was not executed
- No new replay, controls, parked/liftoff, approach/landing or camera-switching run was performed; unchanged configuration/code inputs are the evidence available for those contracts
- Historic Meadow native evidence in the repository concerns the +Z Meadow profile and an older executable; it cannot be relabeled as a current -X Light Single candidate run
- The copied Meadow `.blend` is the original +Z editable source; reproduce the adapted -X GLB through `adapt_original_highwing.py`
- Runtime visual quality, cockpit visibility and commercial packaging require their own later acceptance

No Cargo/Rust command, shared-target build, Blender render, publish or push was performed by this reviewer. Only these independent review files were written under candidate `evidence/`.

## Additional evidence review

Reviewed the candidate README, rejection-control script, render script and author-provided `evidence/blender-import-and-render.json`. No blocking claim discrepancy was found in the README. The import record binds the same candidate GLB hash, reports 46 mesh objects and eight materials, and its Blender bounds correctly apply glTF `(x,y,z)` to Blender `(x,-z,y)`. The initial import-only record reported zero rendered images; the final record now binds the three completed images reviewed below. The reviewer did not execute Blender. The render script imports the pinned final GLB into an empty scene and positions its camera using the candidate basis; the QA ground and lighting are separate additions and are never exported into the asset.

## Final studio-image review

Independently inspected the actual pixels of all three 1200×800 JPEGs generated from the pinned emitted candidate. The final render record reports Cycles CPU, 32 samples, denoising disabled, Standard display transform and None look. File hashes were recomputed and match that record:

- `previews/candidate-three-quarter.jpg`: `ee23f0a80e9265b5eabbdbdaf4210c1e77ef2e763f70dbcdf4be55ef54985ef9`
- `previews/candidate-starboard.jpg`: `9ab03659591b40bab709c68d044047a6b5d19e27227159826e7b4518d5c4abfc`
- `previews/candidate-front.jpg`: `a4d28c3172f3688e4d64213ebd63b3ab3871068a80533b3b971b6e92a665d4ad`

The three-quarter view shows a coherent upright high-wing silhouette with joined fuselage, wing and tail; propeller, glazing, paint bands, struts and visible landing gear are intact. The opposite starboard view confirms continuity of the other side's glazing and bands, intact tail/wing shapes, three wheels and a plausible parked stance. The front view shows symmetric upright wings, struts and main gear; centered nose, propeller, divided windscreen and vertical tail; and three visible tyres. No obvious missing faces, reversed exterior orientation, detached parts or gross ground-contact mismatch is apparent at these angles.

Nonblocking preview limitations: white upper-surface highlights are bright enough to reduce fine shading detail; 32-sample undenoised grain is visible; and the front image contains a narrow studio-floor/background boundary along the bottom edge. These limit image presentation and fine-surface inspection, not the separately verified geometry contract. No rerender is required to establish the bounded preparation result.

These are studio asset images, not FlightSim captures. They do not establish live ModelFit, cockpit clearance, asynchronous scene load, aircraft switching, replay or controls, moving surfaces, native performance, flight behavior or release readiness. Candidate GLB/profile and adapter/validator hashes were rechecked unchanged after rendering. Main repository status remains clean.
