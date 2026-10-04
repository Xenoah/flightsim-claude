# ADR-0016: Stage a complete aircraft before committing a new flight

- Status: implemented; automated evidence and native acceptance are separate gates
- Date: 2026-10-04

## Context

The map previously relocated the current aircraft. Profile, camera eye, guidance,
model hierarchy and synthesized engine source were effectively startup-owned.
Switching only `Startup.aircraft` or only the simulation would leave those owners
inconsistent. A file-existence check also cannot prove that asynchronous GLB scene
loading, dependency loading, instantiation and model fitting will succeed.

## Decision

Keep one app-owned transaction. UI carries at most four presentation choices and
a generation-tagged request. The app snapshots the validated target profile,
destination, month, pending weather and regional selection before dispatch. Names
and catalog indices never select physical laws or substitute for replay identity.

The ordinary catalog contains the exact launch selection and the installed
Swift Sport, Meadow Trainer and Kestrel Jet Trainer presets. Preset IDs, dynamics
family and model paths must match their fixed row. Files are resolved through the
same asset directory as Bevy. Missing or invalid presets remain visibly unavailable.
The launch row retains custom physical coefficients and explicit model/no-model,
fit and sound choices, including the existing implicit development placeholder.
Commercial-staging offers only the Swift preset; its existing explicit CLI
inspection exceptions, metadata and distribution allowlists do not change.

Prepare all terrain/weather/startup/session/recorder/control/clock/tower values
without replacing active resources. Target jets reject raw tiles and every
active, selected, pending or ready regional source before consuming the request.
Target legacy aircraft retain the existing flat/no-global map restriction.
Regional inspection carries the exact request generation and aircraft index.

Load the candidate GLB document and scene 0 while retaining the current flight.
Check parent-document and dependency failures; a failed document can leave its
requested scene label pending. Before instantiation, reject embedded cameras and
lights, which entity visibility cannot suppress. Instantiate only beneath a hidden
root with separate staging markers. Require complete scene-spawner readiness,
another transform/bounds pass, finite transforms and bounds, nonempty geometry
and a valid model fit. A 30-second presentation timeout fails the attempt.

Only then, in one exclusive system before controls, simulation and presentation,
replace the active aircraft hierarchy and commit the prepared flight. Reset the
camera eye/history, guidance, warning hysteresis, both-family HUD display history,
crash/replay/landing state and recorder ownership. Reuse the existing map terrain,
scenery and airport invalidation contract; do not rerun world setup.

The audio crate explicitly retires the tagged old source and bridge and creates
the target synthesized source. Engine kind is baked into that source; updating
`AudioSettings.engine` alone is insufficient. Preserve enabled/master settings,
mute the retired stream immediately and retain exactly one player when enabled.
UI state readers are ordered after the transaction and publishers so closing the
map cannot expose one frame of old flight notices.

An admitted Start is consumed once. Close, cancellation, aircraft/destination/
month/weather changes or entering a child modal/editor invalidate its generation.
Late loader or regional results cannot commit, even if the user later restores the
same values. A failed attempt leaves editable choices and its visible error, but
only a fresh explicit Start retries. Cancellation removes only staged entities and
generated assets; shared GLB assets are released through their handles.

## Alternatives rejected

- Mutating the current profile and repairing dependent resources later: exposes
  mixed physical, recording and presentation state and cannot roll back loading
- Calling startup setup again: duplicates airport, terrain, camera and sky owners
- Treating the first mesh AABB or file existence as readiness: accepts partial or
  failed asynchronous scenes
- Reusing the old audio bridge with a new settings value: retains the old synth
- Consuming a regional request according to the active family: can clear a source
  selection before discovering that the target jet rejects it
- Broad aircraft discovery/import or a new package/hash schema: unnecessary for
  the already available presets and a separate installation/rights boundary

## Costs and verification

The old and one hidden candidate aircraft temporarily coexist in memory. Asset
loading and cleanup are asynchronous; this is not a frame-time guarantee or a
package sandbox. The GLB document is held during preparation to observe parent
failures. Current startup behavior and the data-only v1/v2 profile/replay schemas
remain unchanged. No FDM arithmetic, model revision, physical profile value,
replay byte format, terrain model or rendering quality default changes.

Tests use the real Bevy GLB loader and scene spawner without a GPU. They cover
family transitions, recording identity/mismatch, sound ownership, invalid/empty
scenes, unsupported scene owners, cancellation at distinct loading phases,
regional generation mismatches, warning/notice/HUD reset and modal layout/input.
Native visuals, manual controls, speaker listening and platform builds require
separate evidence. See [the user guide](../aircraft-picker.md).
