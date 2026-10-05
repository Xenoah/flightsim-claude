# ADR-0027: Separate aircraft data integrity from profile and runtime authority

- Status: implemented local contract; native/platform/release gates remain separate
- Date: 2026-10-05

## Context

Public profile schemas v1–v4 do not identify or install a complete aircraft. The
existing regional package schema is explicitly terrain-only and its payload
validator invokes a DEM reader. An aircraft package must not be guessed from
that format, interpreted as a physical identity or mistaken for rights clearance.
The required first outcome is one original usable profile/GLB package with an
explicit validator/basic importer, not a complete MOD ecosystem.

## Decision

Publish independent `kind: aircraft`, manifest schema 1, and the closed
`static-untextured-glb-v1` resource subset. Reuse only private ZIP snapshot,
portable-path and atomic publication mechanisms through explicit per-format
limits/manifest policy. Terrain `Manifest`, kinds, payload decoders, accepted
budgets and public APIs remain terrain-specific. There is no format autodetect.

Content bounds byte counts, exact member sets, hashes, CRC, documentation,
embedded GLB JSON/binary layout, finite actual geometry, node topology/TRS and
expanded resource work. Unknown features and external URI dependencies reject.
The first package preserves the exact original Swift model/profile and records
its original MIT OR Apache-2.0 permissions, complete license texts and provenance.
No external crate/dependency or network capability is added.

Content returns an uncommittable stage. App validates its original profile bytes
through the existing family dispatcher, then binds manifest version, original
model path and original axes/fit to that stage. Only this validated stage exposes
atomic publication. No physical defaults, new family reinterpretation, model
rotation, replay revision or profile mutation occurs. Content cannot import
app/render/FDM/sim. Package identity is SHA-256 of exact manifest bytes binding
exact declared members, separate from transport ZIP hash and from physical
profile/FDM identity, replay compatibility and rights evidence.

Ordinary app commands validate, import or inspect and exit before world/render
startup. Commercial staging rejects these commands and keeps its fixed adjacent
asset boundary. Document existing ordinary profile/asset-root manual usage for
one imported Swift package; do not add a picker or runtime activation API.
The original startup path is not the in-flight `AircraftScene::poll` readiness
transaction. Static acceptance is not general proof of Bevy startup readiness.
Existing Scene0/camera/light/fit transactions remain unchanged and separate.

## Alternatives rejected

- Add aircraft kinds to terrain schema 1 or route it through DEM validation:
  silently broadens an accepted format and decoder authority
- Import app/render into content: reverses dependency direction and puts Bevy in
  GUI-independent package validation
- Copy physical profile DTOs into content: creates competing semantics and risks
  number-token/replay identity differences
- General GLB/import library or texture decoder: unnecessary for original Swift;
  broad formats, sparse data, compression and image allocation require new gates
- Put package identity into existing replay bytes: changes established physical
  and replay contracts without a matching playback design
- Package manager, network/catalog, executable hooks or auto-activation: outside
  the bounded original profile/model milestone

## Consequences and costs

The supported model subset is deliberately narrow: no matrix transforms,
textures, animations, skins, cameras, lights or extensions. Artists must export
static untextured TRS geometry. A second independent resource format needs its
own contract rather than relaxing this one silently. Installed local stores are
trusted state; hostile concurrent filesystem mutation is outside the API threat
model. Cooperative cancellation checks occur between bounded payload work and
cannot undo a completed publication. Native load/fit, flight handling, platform
builds and distribution rights require separate evidence. Other presets absent
from a package-only asset root remain unavailable.

Pure tests preserve original bytes, check semantic-gate rejection and immutable
publication, exercise cancellation/links/corruption and keep the existing hostile
terrain suite passing. Independent mutated archives/GLBs, app exact-loader tests
and one observed original Swift load are distinct validation layers.
See [the public contract](../aircraft-packages.md) for the complete bounds/API.
