# Experimental turboprop foundation integration

This source milestone adds a separately reviewed pure turboprop law, strict
public profile v3 and complete physical identity, plus an original staged Cedar
visual asset. The existing app's aircraft choices, controls and replay dispatch
are unchanged. Cedar is not yet a selectable or flight-qualified preset.

## Accepted inputs

- FDM source `7fdf76ad138431050a1daf9136ae0412754dd3dd`: 27 new component,
  independent energy/momentum, governor/convergence, contact and whole-state
  rejection cases. Seven pure packages pass 1,175 tests/doctests; this count
  includes core, fdm, world, sim, tilegen, net and content, not assetgen.
  [Numerical report](turboprop-fdm-foundation-2026-10-04.md)
- Profile source `07febca75a6c90f0aab56bfb9bf8b0405008cb70`: 18 new Rust tests,
  a 120-case schema/loader corpus and 152 physical-scalar mutation witnesses.
  The full independent canonical fixture is 1,379 bytes. Five pure packages
  pass 1,152 tests/doctests; their scope is stated in the
  [profile report](aircraft-profile-v3-2026-10-04.md)
- Static Cedar source `60785f7f0e7e6c8bb7cb4cd02fe1311655ffa185`: original Blender
  geometry, deterministic byte-identical GLB rebuild, reimport/geometry checks
  and inspected neutral previews. The GLB is 97,404 bytes with 2,422 vertices,
  4,380 triangles and eight materials. [Asset provenance and limits](../aircraft/cedar-turboprop-experimental.md)

The two Rust totals overlap and must not be summed. Strict scoped Clippy/docs,
formatting and architecture checks pass in each implementation. Independent
reviews verified the exact source and evidence. The integration keeps those
production files exact and combines their commits; source CI is a separate gate.

The combined source additionally passes entire-workspace/all-target Clippy with
app region downloads and content downloads enabled (`-D warnings`), all 36
assetgen tests (zero failures/ignores), workspace formatting, architecture and
diff checks. Compilation logs identify the integration checkout. This verifies
that the unused new pure interfaces still compile with the existing app/render
and optional-download paths; it adds no native or Windows qualification claim.

Review caught an inertia-constructor boundary where an algebraically regrouped
determinant passed while the actual matrix determinant reached the old assertion.
The new family now validates the identical matrix operation first and returns a
typed error. Both direct FDM and raw-JSON profile regressions pass. A separate
LF-only test substitution was corrected and explicitly exercised with LF/CRLF.

## Compatibility and version boundary

Profile 3, identity schema 3/kind 3/law 1 and component schema 1 are allocated
to this additive interface. Its fixed byte ordering covers the complete physical
configuration. Initial turbine/shaft/pitch values remain explicit initial
conditions, excluded from physical identity. Jet's existing supported gate stays
schema-2 only; actual replay-v4 bytes with a turboprop identity are rejected.
No old profile, physics law, identity fixture or replay format is reinterpreted.

The two ModelFit comments now distinguish its retained legacy -Z default from
glTF's +Z asset-front convention. Default values, rotation and zero translation
are unchanged. New Cedar metadata explicitly uses +Z forward/+Y up. The glTF
asset convention differs from its camera's local viewing direction; see
[Khronos coordinate-system specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#coordinate-system-and-units).

## Remaining gates

The numerical fixture is an original restrained bench, not Cedar flight data.
The supported state is the intersection of map, atmosphere, RPM, forward-flow
and tip/crossflow limits. Tailwind at rest or backward taxi may leave that domain.
Pitch-monotone load is not a general governor-stability certificate.

Cedar's revised gear provides 0.40 m nominal level, uncompressed design-disk
clearance. Dynamic suspension, braking pitch, touchdown, uneven ground, cockpit
visibility, propeller presentation and actual native rendering remain unqualified.
The studio previews are authoring evidence, not simulator captures.

Complete 16-scalar host rollback, recorder/player/codec, an authored flight preset,
native controls/handling and performance checks must precede app exposure.
Windows full-scene rendering and dependency/rights authorization remain separate;
this milestone does not clear a binary release.
