# Kestrel Jet Trainer asset QA — 2026-10-04

Scope: original Blender exterior/interior geometry, initial fictional version-2
profile, binary asset contract and actual CPU studio previews. Base commit:
`ff409d322372b7af028a15fd2ae7517e4a65ad6a`. No runtime Rust, FDM fixture, existing
asset, built-in selector or commercial packaging file was changed.

## Executed checks

| Check | Result |
| --- | --- |
| Blender 4.3.2 background generation/export | PASS |
| Four Cycles CPU previews, 1440×960, 40 samples | PASS, all visually inspected |
| Python binary/profile/schema validator | PASS |
| Existing v2 JSON-schema test corpus | PASS |
| Python syntax compilation of three new tools | PASS |
| Separate-process GLB rebuild into `/tmp` | Byte-identical |
| Blender import of canonical exported GLB | PASS, 53 mesh objects |
| Prior aircraft/source/profile and numerical-fixture hashes | Unchanged |

The 164,292-byte GLB contains 53 meshes/primitives, eight materials, 4,284
exported vertices and 6,062 triangles. No image/texture/animation/skin or external
buffer is required. All nodes have identity transforms. Bounds are GLB
`(-5.5,-1,-4.849999904632568)` to `(5.5,1.8600000143051147,3.6500000953674316)` m;
measured forward extent is exactly 8.5 m, so ModelFit scale is exactly one.

All three tyre bottom centres match their physical contacts to float32
precision. Port/red is +GLB X; starboard/green is −GLB X. The eye lies inside the
canopy and its closest opaque triangle is 0.2674324243 m away. All five tested
forward rays are clear of opaque geometry. Eighteen contact checks under three
arbitrary attitudes and two large-ECEF origins have maximum render-space error
`1.1920928955078125e-7` m. These are independent asset coordinate-contract checks,
not calls into the app's transform implementation.

The exact P=1, T=1, Mach=0 thrust knot is 100 N idle / 2500 N maximum dry. The
initial profile uses the documented synthetic table and bounded Mach≤0.35
operating box. Schema validation does not substitute for the exact Rust numeric
loader or flight handling acceptance.

[Generated evidence and hashes](kestrel-jet-trainer-asset-validation.json)
contains the inspected binary bounds, contact/nav/eye coordinates, baseline hashes
and four preview hashes. The GLB SHA-256 from both independent exports was
`5a9d8747084f7ad6ff93dbadc80d87acbef42c6882030c0b88ca3aded16bef21`.

## Independent asset review

The integration owner independently inspected the actual three-quarter and eye
previews, reviewed the geometry/profile/export/validator contracts, reran the
final binary/schema validator, and confirmed new-file-only staged scope and
fixture preservation. Asset review approved with no blocking findings. Native
canopy/lighting and manual flight behavior remain separate acceptance gates.

## Limits and integration handoff

No Cargo or native GPU lane was used by the asset author. Runtime loading,
trim, takeoff/contact, control/release, replay, native cockpit/chase appearance
and manual handling require separate integration evidence. The authored interior
has static seats, a glare shield and blank display surfaces; it has no functional
3D instruments. Exterior geometry must stay visible in jet cockpit mode while
the unrelated legacy procedural interior is omitted. Check native camera and
transparent-material behavior before accepting the cockpit presentation.

Controls and approach metadata initially match the separate numerical fixture
and remain provisional. All low-speed aero coefficients are inherited synthetic
assumptions. This report provides no real-aircraft, certification, performance,
release, distribution or trademark-clearance claim.
