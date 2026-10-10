# ADR-0033: Bind original cockpit geometry to existing controls and state

- Status: implemented; native visual and release qualification remain separate
- Date: 2026-10-10

## Decision

Replace the legacy static boxes plus screen-fixed six-pack with original body-axis
geometry and procedural display textures. Render owns only geometry, artwork,
bounded animation and explicitly typed presentation values. App owns source
state, replay selection, input gating, picking, scene lifetime and UI composition.
No render/input/UI sibling dependency or FDM/replay schema change is introduced.

Every functional component has a typed action or state role. The central stack
shows actual simulator telemetry, not fictional avionics. Unmodeled fuel,
mixture, electrical and radio systems have no counterfeit controls. Gauge labels
state the actual sources: EAS, true heading, ellipsoid altitude, body yaw rate,
throttle command and normalized flap position. Replay lacks separate trim and
untrimmed stick displacement; N/A and effective-command labels preserve that fact.

Pointer input is sampled after device/key sampling and before restart and
suspension. It merges held commands into SampledPilotInput. Existing fixed-step
PilotControls updates and original recorder calls remain the sole physics input
path. View/presentation actions do not change simulation time or flight state.
Pause, modal capture including its close frame, replay, crash/fault, loss of
focus/cursor or cockpit view, and scene replacement clear held pointer ownership.

Picking uses the camera/target GlobalTransforms of the previously displayed
frame, avoiding mixed pre/post-rebase transforms. Positive front-facing rectangle
intersections are compared by world-ray distance. A trim wheel uses a non-spinning
hit proxy so an animated revolution cannot make it unclickable.

AircraftScene retains every generated mesh, material and image. Staged interiors
never enter active animation or interaction queries. Cancellation and new-flight
commit use the existing transactional owner and cleanup; bounded-model interiors
remain excluded. New dynamic displays refresh on addition as well as their 20-Hz
presentation cadence. Physical needles animate every rendered frame.

The 2D fallback is suppressed only with an active generated 3D interior. Ordinary
HUD text/help/log can be decluttered; independent critical notices and attribution
remain visible. Instrument lighting binds faces and physical needle materials.

## Validation

Check geometry winding, physical scale, instrument signs and known unit endpoints;
ray target transforms, finite bounds, front/back misses and trim re-grabbing;
fixed-step partition and zero-step behavior; replay/control-source separation;
modal/focus/restart/cursor interruptions; stage cleanup; day/night and look-down
rendered images. Preserve all original source/native distribution gates.
