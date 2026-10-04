"""Independent schema-3 identity witness from original authored JSON decimals.

No Rust-produced bytes, digests or helper code are read. Python's binary64
conversion and struct.pack implement the published ordering independently.
Run without --write to check the committed golden; --write authors the fixture.
"""
import argparse
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / 'docs/examples/aircraft-profiles-v3/numerical-turboprop.json'
FIXTURE = ROOT / 'crates/flightsim-sim/tests/fixtures/turboprop-identity-v3.json'
AERO_FIELDS = ('lift_zero lift_alpha lift_flaps stall_angle_rad stall_blend_rate '
               'drag_min oswald_efficiency drag_flaps side_beta side_rudder '
               'roll_beta roll_rate_p roll_rate_r roll_aileron roll_rudder '
               'pitch_zero pitch_alpha pitch_rate_q pitch_elevator pitch_flaps '
               'yaw_beta yaw_rate_p yaw_rate_r yaw_aileron yaw_rudder').split()


def doubles(values):
    return b''.join(struct.pack('<d', value) for value in values)


def counted(values):
    return struct.pack('<I', len(values)) + doubles(values)


def canonical(profile):
    d = profile['dynamics']
    a = d['airframe']
    out = b'flightsim/model-identity\0' + struct.pack('<HHHI', 1, 3, 3, 1)
    xx, yy, zz, xz = a['inertia_kg_m2']
    out += doubles([a['mass_kg'], xx, 0., -xz, 0., yy, 0., -xz, 0., zz])
    out += doubles(a[k] for k in ['wing_area_m2', 'wing_span_m', 'mean_chord_m'])
    out += doubles(a[k] for k in ['rolling_friction', 'braking_friction',
                                  'lateral_friction', 'friction_transition_mps'])
    out += struct.pack('<I', 3)
    for leg in a['landing_gear']:
        out += doubles(leg['contact_m'])
        out += doubles(leg[k] for k in ['spring_n_per_m', 'damping_ns_per_m',
                                        'max_stroke_m', 'bottom_stop_travel_m', 'max_recoil_mps'])
    e = d['envelope']
    for k in ['pressure_ratio', 'temperature_ratio', 'mach', 'relative_shaft_rad_s', 'absolute_spin_rad_s']:
        out += doubles(e[k])
    out += doubles(e[k] for k in ['maximum_helical_tip_mach', 'maximum_crossflow_tip_ratio'])

    t = d['turbine']
    turbine = struct.pack('<H', t['schema'])
    turbine += counted(t['pressure_ratios']) + counted(t['temperature_ratios'])
    turbine += struct.pack('<I', len(t['cells']))
    turbine += b''.join(doubles([c['idle_w'], c['maximum_w']]) for c in t['cells'])
    turbine += doubles(t[k] for k in ['rise_seconds', 'fall_seconds', 'output_torque_limit_nm'])

    p = d['propeller']
    assert p['convention'] == 'isolated_axial_propeller'
    propeller = struct.pack('<HH', p['schema'], 1)
    propeller += doubles(p[k] for k in ['diameter_m', 'rotor_axial_inertia_kg_m2'])
    propeller += struct.pack('<b', p['rotation_sense'])
    propeller += counted(p['advance_ratio']) + counted(p['blade_pitch_rad'])
    propeller += struct.pack('<I', len(p['cells']))
    propeller += b''.join(doubles([c['ct'], c['cp']]) for c in p['cells'])

    g = d['governor']
    governor = struct.pack('<H', g['schema'])
    governor += doubles(g[k] for k in ['reference_rad_s', 'gain', 'fine_rate_rad_s',
                                       'coarse_rate_rad_s', 'minimum_pitch_rad', 'maximum_pitch_rad'])
    aero = d['aero']
    schedule = b'flightsim/subsonic-mach-aero\0' + struct.pack('<H', aero['schema'])
    schedule += counted([knot['mach'] for knot in aero['knots']])
    schedule += b''.join(doubles(knot['aero'][k] for k in AERO_FIELDS) for knot in aero['knots'])
    for component in [turbine, propeller, governor, schedule]:
        out += struct.pack('<I', len(component)) + component
    return out


def digest(data):
    result = 0xcbf29ce484222325
    for byte in data:
        result = ((result ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    encoded = canonical(json.loads(PROFILE.read_text()))
    fixture = dict(algorithm=1, schema=3, kind=3, law_revision=1,
                   canonical_length=len(encoded), fingerprint=f'{digest(encoded):016x}',
                   canonical_hex=encoded.hex())
    if args.write:
        FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
    assert fixture == json.loads(FIXTURE.read_text())
    print(f'Independent turboprop identity: {len(encoded)} bytes, FNV-1a64 {fixture["fingerprint"]}')
