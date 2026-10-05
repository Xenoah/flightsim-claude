# ADR-0025: Edit authored background visibility and departure-relative cloud base

- Status: implemented in the weather-editor branch; native acceptance and release are separate gates
- Date: 2026-10-04

## Decision

The map offers one optional Visibility / cloud base child, starting from the six
existing authored presets. Legacy/monthly weather, manual cloud flags, replay and
LAN cannot use this editor. Clear/Fog have no editable cloud base. The provenance
is "Authored, not live. Applies on Start." Background visibility describes only
one extinction contribution; cloud base is an authored height above the departure
ground reference, never a measured aviation ceiling or an automatic flight rule.

UI strings and dirty flags belong to UI. App owns a canonical exact authored
template and optional unit-bearing visibility/base edits. Named templates are
reused while preset and seed remain unchanged; routine map updates do not
reconstruct or revalidate them. Untouched fields never
round-trip through display strings; an untouched Apply retains the canonical
preset, and any explicit visual edit produces Custom. The canonical layer
thickness, coverage, morphology, cloud/fog visibility, precipitation and seed are
retained. Cloudless presets cannot gain a layer. Wind and turbulence remain in
their independent editor, including their exact values, seeds and override flags.

Background visibility uses the existing inclusive 10..200000 m bound. Base
starts at zero metres above departure ground; the maximum possible offset is
31000 m minus the retained thickness, from the existing -1000 m minimum departure
and 30000 m maximum cloud height. Invalid multi-field Apply changes nothing.
The actual selected departure can impose a lower maximum: the existing complete
new-flight preparation samples the selected terrain source, resolves the template
once to absolute WGS84 ellipsoidal bounds, and applies the unchanged sim weather
validator. An invalid candidate fails the existing transaction without changing
any active owner and requires a fresh Start. There is no silent clamp.

PendingFlight snapshots the complete exact draft and its app revision, together
with the existing request generation, aircraft, position, month, region and exact
physical conditions. Draft equality compares every template scalar bit and tag,
optional field shape and edited scalar bit. An edit then restore cannot revive
admitted regional/model work: UI opening/editing/Apply/Cancel invalidates the
existing Start generation, and consumed pending edits also advance app revision.
The existing transaction alone commits the prepared scene/session/recorder.

Replay still reads immutable recorded absolute parameters. No new weather codec,
physical law, FDM identity, source service, preset or rendering allocation/work
budget is introduced. All-family v3/v4/v5/v6 tests exercise their own actual app
recorders/players; independent pre-existing goldens remain unchanged.

## Alternatives and costs

Parsing both displayed values on every Apply was rejected because rounded text
would modify an untouched parameter. Comparing only the Custom preset tag was
rejected because distinct drafts would appear equal while a scene loads. A new
Start path was rejected because weather must commit with the existing aircraft,
terrain and recorder transaction. Resolving base against aircraft altitude or
every frame would move a layer over hills and destroy recorded meaning.

The editor retains small bounded strings and a copied app draft. It adds no
weather volume or runtime request. Departure-specific bounds can reject a valid
relative draft at Start; showing that error is preferable to inventing a ground
sample before a regional source is selected or silently changing the user's
value. Cloud Off has no visible cloud geometry. Homogeneous camera-local fog,
Light/volume shape approximations and sparse snow remain unchanged. Tests of
layout and numerical replay do not qualify native visual quality, physical GPUs,
Windows, human handling, or real-world meteorological distribution.
