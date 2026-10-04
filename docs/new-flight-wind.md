# New-flight wind and turbulence

Open the new-flight map with M, then choose **Wind / turbulence**. Enter the
direction the wind comes **from**, in degrees true, and its speed in knots.
270 deg / 20 kt means wind from the west toward the east. 360 and 0 both select
north when explicitly entered. The accepted speed range is 0–300 kt; that limit
is a software input bound, not a safe or validated aircraft operating envelope.

Click a numeric field, use Ctrl+A to clear it, then type. Tab changes numeric
fields. Enter or Apply keeps the draft as pending settings and returns to the
map; it does not launch. Escape or Cancel discards that editor draft. Start on
the map commits aircraft, destination, visual weather and physical settings
together. Closing the map keeps the active flight unchanged.

The retained/custom turbulence row shows the actual horizontal-component bound
and deterministic seed. Keep current preserves them exactly. Selecting Calm,
Light, Moderate or Severe explicitly chooses the existing 0/1.5/3/6 m/s bounds.
Every strength choice preserves the current physical turbulence seed, including
Calm. Vertical component bounds are 0.7 times the horizontal bound. These are
authored noise controls, not observed gust maxima, RMS values or FAA
aircraft-response categories.

Displayed wind values are rounded for readability. An untouched field keeps its
original exact value. Changing only speed preserves the exact original direction;
changing only turbulence preserves both wind values. Existing difficulty defaults
and CLI overrides stay in force until explicitly edited.

Clear, Cloud, Fog, Rain, Snow and Storm describe independently authored visual
weather. Choosing one never changes wind or turbulence. Rain plus an explicit
crosswind uses the existing aerodynamic relative-air-velocity law, but rain does
not add force, wet friction or icing. Storm is not a convective model. Cloud and
particle transport retain their documented visual approximations.

Physical editing is unavailable during replay and LAN sessions. Manual cloud
flags retain their existing recording restriction. A pending edit or canceled
editor invalidates any earlier Start, including while an aircraft model or region
is loading; press Start again after completing the choices. Failed preparation
leaves the current flight intact. Restart keeps committed physical conditions;
v3/v4 recordings keep exact existing wind, turbulence amplitude and seed.

See [the transaction decision](adr/0017-explicit-new-flight-forces.md) and
[modeled-weather limits](modeled-weather.md).
