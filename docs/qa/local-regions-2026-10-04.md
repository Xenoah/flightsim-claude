# Local regional package application integration, 2026-10-04

## Scope

Prepared schema-v1 local ZIP import/list, installed version selection in the world
map, cancellable installed-package inspection, explicit new-flight activation,
source/airport cleanup and legacy replay gates. No network downloads, raw DEM
conversion, executable MODs, new replay format or renderer changes are included.

The public workflow and limits are in [content packages](../content-packages.md).
The test ZIP is synthetic 350 m ellipsoidal terrain, not surveyed geography.

## Automated evidence

With the existing Rust 1.93 toolchain and `RUSTFLAGS=-D warnings`, these passed:

- `cargo test -j 2 -p flightsim-app -p flightsim-ui -p flightsim-content`:
  app 216 passed / 1 existing optional real-scenery test ignored; UI 191 passed;
  package integration 23 passed
- `cargo clippy -j 2 -p flightsim-app -p flightsim-ui -p flightsim-content --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `bash scripts/check-architecture.sh`

Eleven application region tests cover import → metadata list → explicit selection →
background full inspection → new flight; CLI maintenance helpers and no-overwrite;
conflicting flags; replay rejection before file reads; no replay frames in a
package-backed flight; 350 m source / 1,350 m spawn agreement; global fallback;
old airport entity, mesh and overlay-registration removal; later baseline free
flight; bad/cancelled imports and corrupt inspection leaving the old flight;
one cancelled worker retaining its slot; dismissal, changed departure/month,
stale completion/progress, same-frame Cancel/drop/Enter races, and refreshed metadata
being unable to silently change a previously selected manifest identity.

World-map UI tests include the actual Bevy text/layout systems at 1,280 × 720,
asserting that the five visible rows, bounded credit page and all controls fit
the panel. Input tests cover repeated open/close, concurrent keys, busy/unavailable
actions, single-slot commands, paginated 256-row storage, credit truncation and
explicit guidance when UTF-8 characters cannot be shown by the default font.
These are CPU layout and interaction checks, not screenshots.

The content suite additionally cancels installed inspection during metadata/tree,
payload and Ready checkpoints, verifies that installed identities remain intact,
and retains the existing ZIP/hash/path/CRC/resource-limit security cases.

## Remaining qualification and intentional limits

The later [integrated runtime qualification](expansion-integration-2026-10-04.md)
covers the real 765-tile prepared package, CLI import/list, native keyboard
selection, regional activation, return to baseline, replay gates and corrected
logical-arrow credits paging. Final app/UI tests passed 219/194 cases, with the
existing optional app fixture ignore. The exact default MSVC inventory includes
the new `flightsim-content` edge. OS drag-and-drop delivery, native cancellation
while a long import is active, and Windows packaging remain unqualified.

Source changes intentionally remove the previous airport/scenery depiction and
runway-specific landing evaluation. Returning to baseline does not restore those
old entities. Runtime levels/LOD and sparse-tile policy remain unchanged; existing
`--max-level` configuration still applies. Regional v1/v2 replay recording/export
and playback are disabled, without changing legacy bytes. Full original license
files remain installed; the UI displays bounded inert metadata, never rights
clearance, URL fetching or license acceptance.
