# Local commercial candidate staging

This is a preparation path for a possible future commercial distribution. It
does not publish a release, install the Steamworks SDK, accept agreements, access
an account, pay a fee, or establish legal clearance. See
[the rights audit](commercial-distribution-audit.md) for outstanding decisions.

## Separate build profile

The ordinary developer build remains unchanged: Light Single is the default,
both historical aircraft profiles are recognized, and the existing developer
asset-directory search still works.

`flightsim-app` has an **opt-in** `commercial-staging` Cargo feature. It changes
only the application distribution policy:

- Swift Sport is the argument-free default, including its original model,
  +X-forward/+Y-up orientation, 7.12 m model length, dynamics, camera, controls
  and piston sound
- Assets must be in `assets/` directly beside the executable. The candidate does
  not search the current directory, parent directories, `CARGO_MANIFEST_DIR` or
  `BEVY_ASSET_ROOT` for a developer copy of an excluded asset. Explicit model
  paths must also remain within that directory; absolute/parent paths and
  existing symlink escapes are rejected
- A missing selected model, including the default model, exits with status 2
  before graphics/audio startup. `--no-model` is an explicit placeholder choice
- `--distribution-info` alone prints deterministic JSON and exits before
  starting the engine. This identifies the compiled feature, default profile,
  model, package version and platform. It does not attest source provenance

The existing release workflow is deliberately not switched to this feature.
Historical source files, including the Meshy model, are not deleted or relicensed.

## Replay and profile compatibility

No aircraft definition, FDM parameter, replay format, world-data fingerprint or
fingerprint algorithm changes. The Light Single JSON remains embedded so its
profile identity and exact legacy dynamics can still be selected explicitly.

Because the default aircraft is different in the commercial candidate, an old
Light Single recording given with only `--replay` is rejected as an aircraft
mismatch; it is never silently replayed as Swift Sport. For legacy numerical
inspection without the excluded model, use:

```sh
./flightsim-app --aircraft light-single --no-model --replay flight.fsreplay
```

Selecting `--aircraft light-single` without an explicit replacement model or
`--no-model` fails cleanly in the Swift-only bundle. A user-provided asset or
profile is not thereby licensed for redistribution. Replay reproducibility
continues to require the same compatible build, aircraft and terrain/climate
data; this feature is not a cross-version replay guarantee.

## Build and gather exact dependency notices

Run on the target platform. The staging script executes the resulting binary's
metadata command; it cannot validate a Windows binary on a Linux host.

```sh
cargo build --locked --release -j2 -p flightsim-app --features commercial-staging \
  --target x86_64-pc-windows-msvc
cargo metadata --locked --format-version 1 --features flightsim-app/commercial-staging \
  --filter-platform x86_64-pc-windows-msvc > metadata.json
python scripts/collect-dependency-notices.py --metadata metadata.json \
  --root-package flightsim-app --target x86_64-pc-windows-msvc --output dependency-notices
```

For Linux review work, replace the build/metadata/collector target with
`x86_64-unknown-linux-gnu` and use the extensionless executable at
`target/x86_64-unknown-linux-gnu/release/flightsim-app`. A Windows GNU build must
instead use `x86_64-pc-windows-gnu` consistently; its inventory is not
interchangeable with MSVC. A Linux inventory and software-Vulkan smoke are not
Windows, Steam Deck, real GPU or controller qualification. System graphics,
audio and windowing libraries are not copied into this candidate; target-host
runtime prerequisites still need verification. Keep build logs,
source revision, Cargo.lock, feature/compiler flags and the executable SHA-256
as separate exact-build evidence. The executable's profile handshake alone
cannot prove that it was built from the current checkout.

The collector preserves exact available upstream notice texts and records
unresolved entries. Collection does not authorize a license or substitute for
review. The Fira Mono font and embedded tonemapping LUTs require their own
asset inventory entries and applicable notice texts, even though they are
compiled into the executable.

## Assemble a new local directory

```sh
python scripts/stage-commercial-candidate.py \
  --executable target/x86_64-pc-windows-msvc/release/flightsim-app.exe \
  --dependency-notices dependency-notices --output local-commercial-candidate
```

The script only copies explicit source paths. The two external runtime assets
are `swift_sport.glb` and its profile JSON. It does not recursively copy the
repository's `assets/`, developer secrets, local OSM packs, generated regional
tiles, the Meshy model or editable Blender source. World/climate atlases remain
embedded in the executable, with their provenance and redistribution notices
beside it. A missing or empty required notice, symlink, unexpected dependency
file, wrong-platform inventory or integrity failure refuses staging. Dependency
notice files must be referenced by the inventory or explicit review evidence,
and must be bounded UTF-8 text without NUL bytes. Renaming an excluded asset
as a notice does not permit its inclusion.

The new directory contains:

- `flightsim-app[.exe]` and `assets/aircraft/swift_sport.{glb,json}`
- Software licenses, world-data provenance/terms, exact font/LUT notices and the
  asset-rights audit/manifest under their existing relative paths
- `third-party/dependency-inventory.json` and `third-party/licenses/`
- `distribution-info.json`, `commercial-readiness.json`, `LOCAL-CANDIDATE.txt`
  and `bundle-manifest.json` (byte sizes and SHA-256 of every other staged file)

An optional `--dependency-review FILE` can copy and submit separately completed
review evidence. Never create a positive review merely to make a check pass.
The output must not already exist; previous candidates are never overwritten.

Exit meanings for the staging script:

- **0**: local candidate written; automated inventory checks passed; release
  authorization and platform/store gates are still outstanding
- **1**: local candidate written for review, clearly marked **BLOCKED**;
  `commercial-readiness.json` contains unresolved review gates
- **2**: no candidate written because input validation, integrity or the checker
  failed. The readiness checker itself uses exit 2 for blockers; these are
  distinct command interfaces

## Verify the extracted candidate

Run the staged executable from an unrelated working directory, with no aircraft
argument. It must use Swift Sport from its adjacent assets directory. Verify an
actual chase-view capture and clean exit, not only a successful metadata query:

```sh
/absolute/path/local-commercial-candidate/flightsim-app \
  --headless-screenshot /absolute/path/swift-candidate.png \
  --view chase --screenshot-delay 5
```

On Windows use the `.exe` and the normal screenshot/exit flags appropriate to
that platform's rendering setup. Check the log for Swift Sport, model loading
and fit; verify a complete PNG and process status 0. Also check that selecting
the absent Light Single model returns status 2, and no-model legacy selection
retains its original fingerprint. Re-run the readiness gate against the final
directory after copying or archiving it; a manifest is not a tamper-proof seal.

Physical Windows GPU/controller/audio checks, exact Windows dependency
collection/review, all source rights questions, Steam agreements/store review
and actual distribution approval remain separate gates.
