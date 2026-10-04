"""Independent identity golden encoder; run from any directory with Python 3.

Uses shipped JSON fixtures and the byte specification in docs/replay-identity.md,
not Rust serialization or the production fingerprint implementation.
"""

import json
import struct
from pathlib import Path


def scalars(values):
    return b"".join(struct.pack("<d", value) for value in values)


def fnv1a64(payload):
    result = 0xCBF29CE484222325
    for byte in payload:
        result = ((result ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return result


def identities(dynamics):
    d = dynamics
    xx, yy, zz, xz = d["inertia_kg_m2"]
    mass_geometry = [
        d["mass_kg"], xx, 0.0, -xz, 0.0, yy, 0.0, -xz, 0.0, zz,
        d["wing_area_m2"], d["wing_span_m"], d["mean_chord_m"],
    ]
    aero_names = [
        "lift_zero", "lift_alpha", "lift_flaps", "stall_angle_rad", "stall_blend_rate",
        "drag_min", "oswald_efficiency", "drag_flaps", "side_beta", "side_rudder",
        "roll_beta", "roll_rate_p", "roll_rate_r", "roll_aileron", "roll_rudder",
        "pitch_zero", "pitch_alpha", "pitch_rate_q", "pitch_elevator", "pitch_flaps",
        "yaw_beta", "yaw_rate_p", "yaw_rate_r", "yaw_aileron", "yaw_rudder",
    ]
    engine = [d["max_shaft_power_w"], d["propeller_efficiency"], d["static_thrust_n"]]
    friction = [d[k] for k in (
        "rolling_friction", "braking_friction", "lateral_friction", "friction_transition_mps"
    )]
    legs = []
    for gear in d["landing_gear"]:
        legs.extend(gear["contact_m"])
        legs.extend(gear[k] for k in (
            "spring_n_per_m", "damping_ns_per_m", "max_stroke_m",
            "bottom_stop_travel_m", "max_recoil_mps"
        ))
    complete = (
        b"flightsim/aircraft-identity\0" + struct.pack("<HHI", 1, 1, 2)
        + scalars(mass_geometry + [d["aero"][k] for k in aero_names])
        + struct.pack("<H", 1) + scalars(engine + friction)
        + struct.pack("<I", 3) + scalars(legs)
    )
    legacy_names = aero_names[:21] + ["yaw_rate_r", "yaw_rudder", "yaw_aileron"]
    legacy = (
        scalars(mass_geometry + [d["aero"][k] for k in legacy_names] + engine + friction + legs)
        + b"flightsim-fdm-model" + struct.pack("<I", 2)
    )
    assert len(complete) == 594
    return fnv1a64(legacy), fnv1a64(complete)


root = Path(__file__).resolve().parents[2]
for stem, expected in (
    ("light_single", (0x0505E6644BB29A53, 0xB7FA864DC47824F7)),
    ("swift_sport", (0x06068A31D11A75E0, 0x172167174CF90012)),
):
    dynamics = json.loads((root / "assets" / "aircraft" / f"{stem}.json").read_text())["dynamics"]
    result = identities(dynamics)
    assert result == expected, (stem, result)
    dynamics["aero"]["yaw_rate_p"] += 0.01
    changed = identities(dynamics)
    assert changed[0] == result[0] and changed[1] != result[1]
    print(f"{stem}: legacy={result[0]:016x} complete={result[1]:016x}; yaw omission confirmed")
