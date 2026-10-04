# ADR-0017: Keep new-flight wind and authored turbulence explicit

- Status: implemented; automated and native acceptance are separate gates
- Date: 2026-10-04

## Decision

The map has a small Wind / turbulence child editor, separate from the six visual
weather presets. The app retains exact `Wind`, `Turbulence` and explicit-override
flags in a pending snapshot. UI strings are presentation only. Each numeric draft
tracks whether the user edited it; Apply sends only those typed fields. Untouched
wind components, a custom turbulence amplitude, and its seed never round-trip
through rounded display text. A turbulence-only edit preserves both wind fields.

Direction is wind **from**, in true degrees, 0 through 360 inclusive. An explicit
360 normalizes to 0 using the existing CLI parser. Speed is in knots, 0 through
300 inclusive, using the same parser and core conversion. This is an engineering
input bound, not a usable aircraft envelope. Both UI and app reject invalid or
nonfinite input; a multi-field action is validated on a copy before assignment.
Explicit wind/turbulence edits set their existing override flags, so later
difficulty resolution cannot erase them. No edit preserves the original flags.

Keep current retains the exact turbulence amplitude and seed. Explicit Calm,
Light, Moderate and Severe change only the amplitude to the existing
0/1.5/3/6 m/s values; **all retain the current seed, including Calm**. The displayed
seed belongs to physical turbulence and never comes from visual weather. These
are horizontal-component noise bounds, not RMS, FAA response categories or
METAR gust maxima. The existing vertical ratio, spatial/temporal scales and
deterministic fixed-step sampling remain unchanged.

The editor cannot change a running flight. Apply changes only pending map
selection; Cancel discards the editor draft and closing the map discards pending
choices. Replay/LAN disable this editor. Manual cloud flags retain their existing
recording restriction but do not disable independent physical controls.

Opening, editing, applying or canceling the child invalidates the existing Start
generation. `PendingFlight` snapshots exact physical values alongside aircraft,
destination, month, visual weather and region. Every later regional/scene result
must match both that snapshot and its generation. Edit-then-restore cannot revive
admitted work. The candidate applies resolved conditions before constructing its
wind-aware airborne state. A successful complete-aircraft transaction commits
conditions, scene, simulation and recorder together; failed preparation keeps
the current flight intact and requires a fresh Start.

Existing `EnvironmentConditions` already records all selected physical fields.
V3/V4, physics identity, prior bytes and commercial Swift-only restrictions do
not change. Restart/replay use recorded or committed values, never a visual preset
or a new editor default. Clear/Cloud/Fog/Rain/Snow/Storm remain visual scenarios;
none implies calm, a particular turbulence level, precipitation force or icing.

## Alternatives and costs

Automatic Clear-to-calm or Storm-to-severe mapping was rejected: visual labels
are not measured meteorology and would silently overwrite CLI/user conditions.
Parsing every displayed field on Apply was rejected because it changes unedited
f64 values. Resetting a seed when strength changes was rejected because that
would change a second independent input. Live edits were deferred because
existing replay formats contain one initial environment. Separate gust peaks,
vertical shear, convection and observation services need their own physical,
data and recording design.

The pending exact copy and bounded text drafts add UI state and explicit
transaction checks, without adding a settings framework or a second simulation
environment. Native visual usability, human turbulence handling and platform
qualification remain separate from deterministic numerical evidence.
