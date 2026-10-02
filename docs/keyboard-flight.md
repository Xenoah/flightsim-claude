# Keyboard takeoff, climb and trim

These are game-specific training cues for the bundled **Light Single** and
**Swift Sport**, not certified aircraft procedures or performance speeds. Both
are generic aircraft. The guide never moves the controls for you.

## First takeoff

1. Use a clear, long, level runway for practice. Check the displayed trim: normal
   runway starts use **+0.09 for Light Single**, **+0.08 for Swift Sport**. Leave
   that initial trim alone while learning the takeoff. Release Space (brakes)
   and use PageUp until THR reads 100%.
2. Keep the aircraft straight. Watch **EAS**, not scenery motion. Near
   **75 kt EAS**, start a gentle **S / Down** input. **Ease off at the FIRST
   visible nose rise**, roughly **3 degrees** on the pitch display for this
   level-runway practice. An uphill runway can already show positive pitch, so
   watch the change in nose attitude rather than an absolute number. Observe the
   response before another small correction. Use the nose movement as feedback,
   not a fixed number of seconds or a tap of a particular duration. Do not keep
   pulling until liftoff. The speed cue is not a promise of immediate liftoff;
   a prolonged pull can still stall either aircraft even at full power.
3. After liftoff, aim for roughly **70–80 kt EAS** with small S / W adjustments.
   **S / Down pulls the nose up; W / Up pushes it down.** Watch both airspeed
   and pitch: the required climb attitude differs between aircraft and with
   conditions, so there is no shared pitch angle to hold throughout the climb.
4. If airspeed is falling, ease the pull and lower the nose as needed to rebuild
   speed. During initial climb, confirm full power. If you begin to sink before
   reaching pattern altitude, CHECK CLIMB asks for full power and small pitch
   corrections toward the target speed. BUILD AIRSPEED appears when slow. A fast
   descent may need a gentle pull instead; do not push down automatically merely
   because the aircraft is sinking.
5. Once speed and attitude are steady, adjust trim in small amounts. If you still
   need steady back pressure, tap **]** (nose-up trim); if you need steady forward
   pressure, tap **[** (nose-down trim). Then relax pressure and observe. Do not
   add nose-up trim automatically after every takeoff or use trim to recover a
   stall. The THR and TRM values show the retained settings.

The 75 kt EAS ground-rotation cue was tested with these generic aircraft in calm,
clean, full-power conditions at their normal loading and trim, with feedback-based
release. It is not a certified POH speed or a guarantee of clearance or recovery
in every terrain, wind, loading, flap or altitude condition. Use a less demanding
starting location if there is not enough space to accelerate or climb clear.

## What happens when a key is released?

The keyboard pitch control returns toward neutral, but **throttle and trim stay
where you set them**. Releasing PageUp does not shut off the engine. Releasing S
is not an instruction to hold the current pitch, speed or altitude. The aircraft
continues to respond to its existing speed, attitude, pitch rate, power and trim.
A takeoff at too little airspeed can settle after releasing the pull even without
a stall; an excessive pull can already have caused a stall before the release.

Trim changes the elevator bias and the aircraft's equilibrium. It is not an
autopilot, attitude lock or unlimited lift. A well-trimmed aircraft may still
need corrections, particularly close to terrain or in changing conditions.

## Climb has a power and airspeed cost

Pitching up can trade speed for height briefly. Sustained climb requires excess
power after drag is overcome. Pulling harder does not create extra engine power:
it can slow the aircraft, increase drag and eventually exceed its critical angle
of attack. At high or hot locations, reduced air density can also reduce climb
performance. When speed is dropping, prioritize an appropriate airspeed and a
shallower climb rather than chasing height with a longer pull.

The primary speed display and cockpit dial read **EAS (equivalent airspeed)**:
TAS multiplied by the square root of local air density divided by standard
sea-level density. It represents the model's dynamic-pressure-equivalent speed.
It is **not** a calibrated pitot/static IAS instrument, and the simulator does
not model instrument/position errors. At lower density, the same EAS requires a
higher true airspeed. The takeoff cues use EAS so they do not trigger prematurely
from a high TAS at an elevated location.

## Stall warning and recovery

The visual STALL WARN and audible warning follow the modeled angle of attack,
not a fixed airspeed threshold. They can activate at more than one speed.
A custom profile with no usable modeled lift peak shows STALL WARN N/A;
the absence of a warning tone then says nothing about its stall margin.
Reduce angle of attack first: ease the pull and use **W / Up** as needed to lower
the nose. Level the wings and adjust power as needed; during initial climb this
normally means confirming full power. As speed and control effectiveness return,
recover gently without immediately pulling into another stall.

Height can be lost during recovery. A warning, full power, or a corrected input
does not guarantee recovery before ground contact. In a crash, use R to restart;
Esc pauses the flight. H toggles the contextual guide. An explicit --approach
start seeds approach guidance separately from a runway takeoff.

## Basis and limits

The current ground-rotation guidance uses **75 kt EAS**, a gentle input and
release at the first visible nose rise (roughly 3 degrees in the tested
level-runway setup). This supersedes the earlier 65 kt EAS suggestion: those earlier release tests began already airborne
and did **not** validate rotation from the runway. Ground tests and reaction-delay
checks informed the revised cue and slower keyboard pitch-input rate; they do not
guarantee the outcome of an arbitrary key press or a continued pull after liftoff.
The 70–80 kt EAS initial-climb target remains a model-specific training aid, not a
certified V-speed. See [keyboard guidance QA](qa/keyboard-guidance-2026-10-02.md) and
[longitudinal physics QA](qa/longitudinal-physics-2026-10-02.md)
for evidence scope and measured limits. UI tests verify which cues appear and which
speed drives the display; they do not substitute for native takeoff testing or
qualify a new screen size.

The control principles are consistent with the FAA Airplane Flying Handbook:
[Chapter 4, energy management](https://www.faa.gov/sites/faa.gov/files/regulations_policies/handbooks_manuals/aviation/airplane_handbook/05_afh_ch4.pdf),
[Chapter 5, stall prevention and recovery](https://www.faa.gov/sites/faa.gov/files/regulations_policies/handbooks_manuals/aviation/airplane_handbook/06_afh_ch5.pdf),
and [Chapter 6, takeoffs and departure climbs](https://www.faa.gov/sites/faa.gov/files/regulations_policies/handbooks_manuals/aviation/airplane_handbook/07_afh_ch6.pdf).
Those sources inform the general explanation, not the game's numerical targets.
