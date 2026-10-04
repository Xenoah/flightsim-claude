# New-flight aircraft selection

Open the map with **M**. Beneath the map, use **PgUp / PgDn** or the arrow buttons
to choose an aircraft. Choose a departure, month and optional F12 weather preset,
then press **Start new flight**. The existing flight is replaced only after the
candidate flight and complete aircraft scene are ready. All map starts remain
1,000 m above the sampled terrain and require continued pilot control.

The ordinary development catalog contains:

- **Launch:** the validated profile used to launch the app, including custom
  physical data and explicit model/no-model, fit and sound choices. This remains
  available as a way back to the original Light Single or external profile
- **Swift Sport:** the original low-wing sport aircraft
- **Meadow Trainer:** an original high-wing visual variant using the unchanged
  Light Single legacy dynamics and controls
- **Kestrel Jet Trainer:** an original experimental dry jet, restricted to its
  authored Mach 0–0.35 operating range

Named presets require their matching JSON and GLB files in the app's resolved
asset directory. Unavailable presets show a reason. There is no asset download,
aircraft archive importer or recursive discovery. Meadow and Kestrel remain JSON
path choices at the command line; the existing `--list-aircraft` list is unchanged.
The commercial-staging picker offers only Swift. Its packaging allowlists, default
and explicit CLI inspection exceptions remain unchanged.

Selection changes are previews. **M, Esc or Close cancels preparation** and keeps
the current aircraft, weather and recording. Changing aircraft, departure, month
or weather, or opening coordinates/credits/Regions, also retires an admitted
Start. Returning to the same values does not restart it; press Start again.
On failure, the previous usable flight remains and the choices remain available
for correction. Save the current recording with F9 before starting a replacement
if you want to preserve it.

Jet selection rejects raw `--tiles` and active, selected or pending regional
packages before changing them. Clear a selected package explicitly; an active
package flight selected in the map must first start a supported global legacy
flight before selecting Kestrel. A launch with `--region` still requires restarting
without that flag; the picker does not erase the explicit launch restriction.
A flat-zero jet cannot switch to a legacy map flight with global terrain
off; restart with global terrain enabled. The picker does not override these
source contracts or turn unsupported terrain into a different physical surface.

Replay displays its recorded aircraft as locked. Neither aircraft selection nor
map preview can replace its recorded identity. New legacy flights record v3;
new jet flights record v4, subject to the existing manual-cloud and regional
recording restrictions. Profile/replay schemas and physical models are unchanged.

Preparation validates the complete scene and its fit before replacing the aircraft.
Invalid or empty GLBs, missing scene 0, failed dependencies and scenes containing
embedded cameras or lights fail visibly. These checks are a supported-preset
readiness boundary, not a general mod-package sandbox.

The implementation and tradeoffs are recorded in
[ADR-0016](adr/0016-transactional-new-flight-aircraft.md). Automated loader/ECS tests
do not establish native visual, handling, audio-device or Windows acceptance.
