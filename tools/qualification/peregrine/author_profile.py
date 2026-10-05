"""Generate the fictional Peregrine experimental profile, never an old profile.

Every dimensional/coefficient/thrust value below is an authored assumption.
NASA/FAA references in the protocol explain principles, not these data.
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "docs/examples/aircraft-profiles-v2/peregrine-experimental.json"
# name, kg, design-frame x, box (forward, right, down) dimensions in metres.
BOXES = [
    ("fuselage/equipment", 800, 0., (6.7, 1.15, 1.25)),
    ("wing", 280, 0., (1.7, 8.4, .16)),
    ("engine", 250, -1.5, (1.8, .8, .8)),
    ("occupants", 100, 1.4, (.8, .7, 1.1)),
    ("fixed fuel mass", 200, .6, (1.4, 2.4, .35)),
    ("tail", 120, -3.1, (1., 3.2, .25)),
]
mass = sum(row[1] for row in BOXES)
cg = sum(m * x for _, m, x, _ in BOXES) / mass
inertia = [0., 0., 0., 0.]
for _, m, x, (lx, ly, lz) in BOXES:
    inertia[0] += m * (ly * ly + lz * lz) / 12
    inertia[1] += m * (lx * lx + lz * lz) / 12 + m * (x - cg) ** 2
    inertia[2] += m * (lx * lx + ly * ly) / 12 + m * (x - cg) ** 2
span, area, taper = 8.4, 13.2, .45
root = 2 * area / (span * (1 + taper))
mac = 2 * root / 3 * (1 + taper + taper * taper) / (1 + taper)
mach = [0., .2, .35, .5, .6, .65]
# Deliberate subsonic schedule, no PG singularity/extrapolation/shock claim.
slopes = [4.6, 4.65, 4.75, 4.95, 5.1, 5.2]
drag = [.025, .025, .0255, .0265, .029, .033]
efficiency = [.82, .82, .81, .8, .78, .76]
knots = []
for i, m in enumerate(mach):
    r = m / .65
    a = dict(lift_zero=.16 - .015 * r, lift_alpha=slopes[i],
             lift_flaps=.85 - .08 * r, stall_angle_rad=math.radians(17 - r),
             stall_blend_rate=45., drag_min=drag[i], oswald_efficiency=efficiency[i],
             drag_flaps=.085 + .015 * r, side_beta=-.55 - .05 * r,
             side_rudder=-.085 + .005 * r, roll_beta=-.07 - .015 * r,
             roll_rate_p=-.46 - .02 * r, roll_rate_r=.08 - .01 * r,
             roll_aileron=.08 - .008 * r, roll_rudder=-.009 + .001 * r,
             pitch_zero=.018 - .003 * r, pitch_alpha=-.55 - .10 * r,
             pitch_rate_q=-12. - 2 * r, pitch_elevator=.55 - .035 * r,
             pitch_flaps=-.10 - .01 * r, yaw_beta=.12 + .015 * r,
             yaw_rate_p=-.025, yaw_rate_r=-.26 - .02 * r,
             yaw_aileron=-.004, yaw_rudder=.029 - .002 * r)
    knots.append(dict(mach=m, aero=a))
p_axis, t_axis = [0., .45, .7, 1., 1.08], [.75, .9, 1., 1.1]
cells = [dict(idle_n=p * (500 - 900 * m) / math.sqrt(t),
              maximum_dry_n=12500 * p * (1 - .20 * m - .12 * m * m) / math.sqrt(t))
         for p in p_axis for t in t_axis for m in mach]
gear = []
for contact, spring, damping in [([2., 0., 1.15], 80000., 7000.),
                                 ([-.85, -1.5, 1.15], 150000., 13000.),
                                 ([-.85, 1.5, 1.15], 150000., 13000.)]:
    gear.append(dict(contact_m=contact, spring_n_per_m=spring,
                     damping_ns_per_m=damping, max_stroke_m=.22,
                     bottom_stop_travel_m=.05, max_recoil_mps=.45))
profile = dict(version=2, id="peregrine-sport-jet-experimental", dynamics=dict(
    kind="dry_jet_table", revision=1,
    airframe=dict(name="Peregrine Sport Jet (fictional experimental)",
                  mass_kg=mass, inertia_kg_m2=inertia, wing_area_m2=area,
                  wing_span_m=span, mean_chord_m=mac, landing_gear=gear,
                  rolling_friction=.018, braking_friction=.7,
                  lateral_friction=.8, friction_transition_mps=.25),
    thrust=dict(schema=1, pressure_ratios=p_axis, temperature_ratios=t_axis,
                mach=mach, cells=cells),
    aero=dict(schema=1, knots=knots),
    envelope=dict(pressure_ratio=[.45, 1.08], temperature_ratio=[.75, 1.1], mach=[0., .65])),
    model=dict(path="aircraft/peregrine-not-provided.glb", forward="+z", up="+y", length_m=9.1),
    controls=dict(surface_rate=1.2, elevator_rate=.15, elevator_centering_rate=5.,
                  centering_rate=1.8, throttle_rate=.2, flap_rate=.2, default_trim=.05,
                  trim_rate=.08, approach_speed_mps=60., approach_throttle=.3,
                  approach_flaps=.5, approach_pitch_rad=0., approach_trim=.08),
    camera_eye_m=[.75, 0., -.85], engine_sound="turbine")
OUT.write_text(json.dumps(profile, indent=2, allow_nan=False) + "\n")
print(json.dumps(dict(profile=str(OUT), mass_kg=mass, design_cg_x_m=cg,
                     inertia_kg_m2=inertia, root_chord_m=root, tip_chord_m=root*taper,
                     mean_chord_m=mac, aspect_ratio=span*span/area), indent=2))
