"""Independent component-byte oracle, from the written subsonic contract.

No imports or reads of production Rust. Fixture values are authored, exactly
representable test numbers, not aircraft measurements. Run with --write only
when intentionally updating the schema-1 golden; default verifies existing data.
"""
import json
import struct
import sys
from pathlib import Path

FIELDS = (
    'lift_zero lift_alpha lift_flaps stall_angle_rad stall_blend_rate '
    'drag_min oswald_efficiency drag_flaps side_beta side_rudder '
    'roll_beta roll_rate_p roll_rate_r roll_aileron roll_rudder '
    'pitch_zero pitch_alpha pitch_rate_q pitch_elevator pitch_flaps '
    'yaw_beta yaw_rate_p yaw_rate_r yaw_aileron yaw_rudder'
).split()


def doubles(values):
    return b''.join(struct.pack('<d', value) for value in values)


def axis(values):
    return struct.pack('<I', len(values)) + doubles(values)


def jet_bytes(d):
    return (b'flightsim/subsonic-dry-jet\0' + struct.pack('<H', d['schema'])
            + b''.join(axis(d[key]) for key in ('pressure_ratios', 'temperature_ratios', 'mach'))
            + struct.pack('<I', len(d['cells']))
            + b''.join(doubles([cell['idle_n'], cell['maximum_dry_n']]) for cell in d['cells']))


def aero_bytes(d):
    return (b'flightsim/subsonic-mach-aero\0' + struct.pack('<H', d['schema'])
            + axis([k['mach'] for k in d['knots']])
            + b''.join(doubles(k['aero'][field] for field in FIELDS) for k in d['knots']))


fixture = Path(__file__).resolve().parents[2] / 'crates/flightsim-fdm/tests/fixtures/subsonic-identity-v1.json'
if '--write' in sys.argv:
    jet = dict(schema=1, pressure_ratios=[0.0, 0.5, 1.0], temperature_ratios=[0.5, 1.25], mach=[0.0, 0.5], cells=[])
    for i in range(12):
        jet['cells'].append(dict(idle_n=-0.0 if i < 4 else -10.0*i, maximum_dry_n=0.0 if i < 4 else 1000.0+100.0*i))
    base = [0.25, 5.0, 0.5, 0.25, 40.0, 0.03125, 0.75, 0.0625, -0.25, 0.0625, -0.125, -0.5, 0.125, 0.0625, 0.0, -0.0, -1.0, -12.0, 0.5, -0.125, 0.0625, -0.03125, -0.125, -0.015625, 0.03125]
    aero = dict(schema=1, knots=[dict(mach=mach, aero=dict(zip(FIELDS, [v if i == 0 else v+i/128.0 for v in base]))) for i,mach in enumerate([0.0,0.5,0.75])])
    data = dict(jet=jet, aero=aero, jet_hex=jet_bytes(jet).hex(), aero_hex=aero_bytes(aero).hex())
    fixture.write_text(json.dumps(data, indent=2)+'\n')
else:
    data = json.loads(fixture.read_text())
    assert jet_bytes(data['jet']).hex() == data['jet_hex']
    assert aero_bytes(data['aero']).hex() == data['aero_hex']
    print('Independent subsonic component identity fixtures verified')
