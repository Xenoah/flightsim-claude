# Aircraft data package v1

This is an explicit **offline validation and basic import** contract for an
original aircraft profile plus one embedded, static, untextured GLB. It adds no
executable MODs, scripts, dependency resolution, downloads, catalog, picker row or
in-flight activation route. Existing aircraft/profile/replay meanings, transforms,
Light defaults and regional compatibility limits remain unchanged.

- [Public manifest schema](../schemas/aircraft-package-v1.schema.json), Draft 2020-12,
  `urn:flightsim:aircraft-package:v1`
- [Original Swift package](examples/aircraft-packages/swift-sport-1.0.0.zip)
- [Uncompressed example manifest](examples/aircraft-packages/swift/manifest.json)
- [Design and boundaries](adr/0027-offline-aircraft-data-packages.md)

The example contains the exact existing Swift profile and model bytes. It is
licensed under the repository's MIT OR Apache-2.0 terms, with both complete texts
and original provenance retained. Validation is not publisher authentication,
flight qualification or clearance to distribute the application and dependencies.

## Validate and import

Use an ordinary development build from the repository root:

```sh
cargo run -p flightsim-app -- --validate-aircraft-package docs/examples/aircraft-packages/swift-sport-1.0.0.zip
cargo run -p flightsim-app -- --import-aircraft-package docs/examples/aircraft-packages/swift-sport-1.0.0.zip --aircraft-store data/aircraft-packages
cargo run -p flightsim-app -- --inspect-aircraft-package data/aircraft-packages/swift-sport-original/1.0.0
```

These three commands exit before graphics/audio/world startup. Validation uses
private temporary storage and publishes nothing. Import uses a user-selected,
trusted local store, validates the original profile with the app's existing
version-specific loader, and atomically publishes a new `id/version` directory.
The commands are exclusive; import requires exactly `--aircraft-store DIR`.
All package commands reject in `commercial-staging`, whose fixed adjacent-asset
inventory and environment-variable isolation remain unchanged.

A content-only tool is available without Bevy:

```sh
cargo run -p flightsim-content --example validate_aircraft_package -- validate docs/examples/aircraft-packages/swift-sport-1.0.0.zip
```

It also supports `inspect INSTALLED_DIR` and `cancel ZIP`. It deliberately says
“content-only”: it does not run the authoritative profile loader, publish a
package, load Bevy scenes or certify flight behavior.

## Ordinary-build manual use

After successful app validation/import, the existing profile loader can select
the installed example. On POSIX shells, from the repository root:

```sh
BEVY_ASSET_ROOT="$PWD/data/aircraft-packages/swift-sport-original/1.0.0" \
  cargo run -p flightsim-app -- \
  --aircraft "$PWD/data/aircraft-packages/swift-sport-original/1.0.0/profile.json" --view chase
```

For a prebuilt ordinary executable, substitute its absolute path for
`cargo run -p flightsim-app --`. On PowerShell, set the same absolute package
root in `$env:BEVY_ASSET_ROOT` for that process, run the ordinary executable with
`--aircraft <absolute package root>/profile.json`, and restore the variable when
finished. The package's `assets/` directory contains the exact profile-referenced
`aircraft/swift_sport.glb`; no profile field is rewritten. Renderer/UI shaders,
the default font and global baseline data use existing embedded resources.
Other aircraft presets whose profile files are absent from this isolated asset
root remain unavailable; Launch retains the selected profile. No preset files
are silently installed or copied from the developer checkout.

This uses the existing ordinary startup path. **Static package acceptance is not
proof of full Bevy scene/dependency/spawn readiness.** The existing in-flight
`AircraftScene` transaction still independently checks Scene0, all dependencies,
unsupported cameras/lights and fit before swapping a flight; this milestone does
not add an imported-package selection to that transaction. A native observation
of this Swift example qualifies only that exact load/fit case. Startup, native
rendering, manual flight, devices and platform qualification remain separate.
Do not use this environment-based route with `commercial-staging`.

## Manifest and byte identity

The archive has a root `manifest.json` with exactly these top-level fields:

- `schema_version: 1`, `kind: "aircraft"`, portable lowercase package `id`, canonical
  three-part numeric `version`, and bounded `title`
- `profile: {"path":"profile.json", "version":1..4}`
- `model: {"path":"assets/<model path>.glb", "format":"static-untextured-glb-v1", "scene":0}`
- `sources`: one to eight original source/revision/provenance/credits/license
  records; every license text names a declared documentation member
- `files`: one profile, one model, and documentation, each with exact path,
  `kind`, positive `size_bytes`, lowercase SHA-256 and declared source ID

Only `profile.json`, the declared `assets/*.glb`, and declared `docs/*.txt` or
`docs/*.md` exist in the package. Documentation must be nonempty UTF-8 without
unsupported controls. Every allowed payload byte is hashed; the manifest itself
is identified by SHA-256 of its exact original bytes. Changing whitespace in a
manifest or profile changes package content identity. Recompressing the same
members changes archive bytes/hash but not that content identity.

Paths use ASCII letters/digits/dot/underscore/hyphen, forward slash, at most
180 bytes and eight components. Empty, dot, parent, drive, absolute, backslash,
Windows reserved names/trailing dots, duplicate and case-colliding paths are
rejected, including conflicting directory prefixes. Directories in ZIPs are
allowed only as ancestors of declared files. Archive links/special modes and
installed symlinks, multiply linked Unix files and Windows reparse points
(including junctions) are rejected.
Fresh ZIP extraction always creates new regular files. Unix hardlink rejection
was tested on Linux only; Windows hardlink rejection and qualified Windows
package import are not claimed. Snapshot/hash checks do not replace that
platform qualification.
The store must be trusted local application state; hostile simultaneous writers
inside it are outside the API threat model.

The schema is an authoring aid. Runtime also checks raw integer tokens,
duplicate fields, UTF-8 byte counts, reserved paths/collisions, exact file-set and
reference bindings, file hashes/CRC, cross-field constraints and GLB contents.
Schema-only success cannot grant runtime acceptance.

## Fixed resource budgets

| Boundary | Limit |
| --- | --- |
| Archive bytes and aggregate uncompressed members, including manifest | 20 MiB each |
| Manifest | 64 KiB |
| Declared payload files / total ZIP or installed tree entries | 16 / 64 |
| Profile v1 | Existing 128 KiB |
| Profile v2–v4 | Existing 1 MiB; original family parser still authoritative |
| Single GLB / its JSON chunk | 16 MiB / 1 MiB |
| Each documentation file | 64 KiB |
| Source records | 8 |
| Compression ratio | At most 1024:1 per member |
| GLB nodes / mesh definitions / primitives / materials | 256 / 256 / 512 / 64 |
| GLB accessors / buffer views | 2048 / 2048 |
| Source and expanded Scene0 vertices / triangle indices | 500,000 / 1,500,000 |
| Decoded accessor scalar work | 5,500,000 |
| Node depth | 64 edges |
| Installed versions / enumerated store entries | 256 / 1024 |

Only single-disk ZIP32 Stored/Deflate is accepted. The private input snapshot,
local/central header consistency, strict envelope/no prefix/no ZIP64/no archive
comment, actual decompressed length and CRC are checked before publication.
Unexpected data members, directories, chunk types and external dependencies fail.
No decoder installs dependencies or interprets arbitrary executable content.

## Closed static GLB subset

This is intentionally narrower than general glTF/GLB 2.0. There is exactly one
Scene0, one embedded BIN buffer and one JSON chunk. Accept only indexed or
non-indexed triangle primitives with f32 VEC3 POSITION, optional unit f32 VEC3
NORMAL, optional f32 VEC2 TEXCOORD_0, and packed u16/u32 indices. POSITION has
validated actual min/max, and every primitive has a nondegenerate triangle.
Optional UV coordinates do not imply texture support.

Accept original node TRS only (unit quaternion, finite nonsingular transform).
Reject matrix transforms, cycles, repeated/multiple parents, unreachable nodes,
over-depth trees, invalid indices/accessor ranges/alignment, unsupported array
formats, nonfinite/oversized binary values and expanded work over budget.
Actual transformed vertex coordinates and transform elements are bounded to
±1,000,000 model units; determinants must have magnitude at least 1e-12.
Bounds are decoded from actual Scene0 vertices, not trusted metadata.

Materials support bounded ordinary metallic/roughness/base-color factors,
emissive factors, alpha mode/cutoff and double-sided flags. Reject all images,
textures/samplers, URI buffers/images (including data URIs), extensions, cameras,
lights, animations, skins, morph targets, sparse accessors, compression and
unknown JSON fields. Empty or wholly degenerate geometry fails. The app binds
manifest profile version and exact `assets/`-relative model path to the unchanged
profile decoder, then uses its original declared axes and `ModelFit` length to
reject unusable forward extents. It neither rotates a model nor edits physics.

## Authority, cancellation and compatibility

`flightsim_content::aircraft::stage_zip_with_progress` returns an uncommittable
`StagedAircraftPackage`. Its `validate_profile` callback must invoke the caller's
authoritative family decoder on original bytes and check manifest/model/fit
bindings. Only the resulting `ValidatedAircraftPackage` exposes `commit`.
The app CLI supplies that callback using `SelectedAircraftProfile::from_bytes`.
Content never imports app, render, Bevy, FDM or sim to infer physical semantics.
Installed inspection rechecks the complete tree/hashes/GLB; app inspection also
repeats profile semantics. A caller that supplies a no-op callback has not
validated a physical aircraft.

Dropping either stage, callback cancellation, invalid bytes or failed semantics
removes only the current temporary work. Cancellation is checked during bounded
snapshot/decompression reads, each archive/tree entry, each payload and Ready;
one bounded GLB validation runs between callbacks. Cooperating importers use a
nonblocking store lock and atomic same-filesystem rename. Existing versions are
never overwritten. Cancellation/generation must be checked immediately before
commit; a completed atomic publication cannot be undone by a later cancellation.
Import and inspection contain no reference to a flight and cannot activate one.
Existing app cancellation/stale-generation/failed-flight transaction rules are
therefore unchanged.

Package integrity and declared license provenance do not select a physical law,
change the physical profile identity, upgrade replay formats or grant rights.
Replay matching uses the exact existing model revision/configuration rules;
old missing evidence remains missing. Package metadata is not stored in existing
replays, so those formats do not attest to the model/package appearance. Existing
regional terrain replay and jet/turboprop source restrictions still apply.
No ordinary turboprop/faster/stealth aircraft admission follows from this feature.

## Reproduce the original package and checks

```sh
python3 scripts/package-swift-aircraft.py /tmp/swift-sport-1.0.0.zip
cargo test -j 2 -p flightsim-content
cargo test -j 2 -p flightsim-app aircraft_package_cli
python3 schemas/tests/test_aircraft_package_schema.py
bash scripts/check-architecture.sh
```

The builder has a fixed source inventory, verifies all declared sizes/hashes and
creates the output exclusively. It never regenerates or relabels the model.
The checked-in ZIP is deterministic and contains no build scripts or source code.
The content tests are GUI-independent. App tests and native/Windows runs require
their own coordinated build lane and recorded evidence.
