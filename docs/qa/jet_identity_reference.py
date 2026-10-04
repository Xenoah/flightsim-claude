"""Independent schema-2 model identity encoder using authored profile decimals.

No Rust code or Rust-produced digest is imported. Python binary64 conversion and
struct.pack independently implement the documented SI bit/order contract.
"""
import argparse
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / 'docs/examples/aircraft-profiles-v2/numerical-jet.json'
FIXTURE = ROOT / 'crates/flightsim-sim/tests/fixtures/jet-identity-v2.json'
FIELDS = ('lift_zero lift_alpha lift_flaps stall_angle_rad stall_blend_rate '
          'drag_min oswald_efficiency drag_flaps side_beta side_rudder '
          'roll_beta roll_rate_p roll_rate_r roll_aileron roll_rudder '
          'pitch_zero pitch_alpha pitch_rate_q pitch_elevator pitch_flaps '
          'yaw_beta yaw_rate_p yaw_rate_r yaw_aileron yaw_rudder').split()

def doubles(values):
    return b''.join(struct.pack('<d', value) for value in values)

def axis(values):
    return struct.pack('<I', len(values)) + doubles(values)

def canonical(profile):
    d = profile['dynamics']
    a = d['airframe']
    out = b'flightsim/model-identity\0' + struct.pack('<HHHI', 1, 2, 2, 1)
    xx, yy, zz, xz = a['inertia_kg_m2']
    out += doubles([a['mass_kg'], xx, 0., -xz, 0., yy, 0., -xz, 0., zz])
    out += doubles(a[key] for key in ['wing_area_m2', 'wing_span_m', 'mean_chord_m'])
    out += doubles(a[key] for key in ['rolling_friction', 'braking_friction', 'lateral_friction', 'friction_transition_mps'])
    out += struct.pack('<I', 3)
    for gear in a['landing_gear']:
        out += doubles(gear['contact_m'])
        out += doubles(gear[key] for key in ['spring_n_per_m', 'damping_ns_per_m', 'max_stroke_m', 'bottom_stop_travel_m', 'max_recoil_mps'])
    for key in ['pressure_ratio', 'temperature_ratio', 'mach']:
        out += doubles(d['envelope'][key])
    thrust = d['thrust']
    jet = b'flightsim/subsonic-dry-jet\0' + struct.pack('<H', thrust['schema'])
    jet += b''.join(axis(thrust[key]) for key in ['pressure_ratios', 'temperature_ratios', 'mach'])
    jet += struct.pack('<I', len(thrust['cells']))
    jet += b''.join(doubles([cell['idle_n'], cell['maximum_dry_n']]) for cell in thrust['cells'])
    aero = d['aero']
    schedule = b'flightsim/subsonic-mach-aero\0' + struct.pack('<H', aero['schema'])
    schedule += axis([knot['mach'] for knot in aero['knots']])
    schedule += b''.join(doubles(knot['aero'][key] for key in FIELDS) for knot in aero['knots'])
    for component in [jet, schedule]:
        out += struct.pack('<I', len(component)) + component
    return out

def digest(encoded):
    result = 0xcbf29ce484222325
    for byte in encoded:
        result = ((result ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--profile', type=Path, default=PROFILE)
    args = parser.parse_args()
    encoded = canonical(json.loads(args.profile.read_text()))
    value = dict(schema=2, kind=2, law_revision=1, fingerprint=f'{digest(encoded):016x}', canonical_hex=encoded.hex())
    if args.write:
        FIXTURE.write_text(json.dumps(value, indent=2) + '\n')
    assert json.loads(FIXTURE.read_text()) == value
    print(f'Independent jet identity: {len(encoded)} bytes, FNV-1a64 {value["fingerprint"]}')
