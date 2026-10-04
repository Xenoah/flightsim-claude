"""Independent schema-4 bytes from source JSON, no Rust output or hashes read.

The retained complete forward component uses the separately audited Python
schema-3 encoder. The new extension is encoded here independently from Rust.
Run without --write to verify the golden; --write creates it from authored JSON.
"""
import argparse
import copy
import json
import struct
from pathlib import Path
from turboprop_identity_reference import canonical as forward_canonical, digest, doubles

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / 'docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json'
FIXTURE = ROOT / 'crates/flightsim-sim/tests/fixtures/near-static-turboprop-identity-v4.json'


def component(data):
    return struct.pack('<I', len(data)) + data


def canonical(profile):
    propeller = profile['dynamics']['propeller']
    retained = copy.deepcopy(profile)
    retained['dynamics']['propeller'] = propeller['forward']
    out = b'flightsim/model-identity\0' + struct.pack('<HHHI', 1, 4, 3, 2)
    out += component(forward_canonical(retained))
    domain = propeller['near_static_domain']
    assert domain['interpolation'] == 'signed_pitch_then_advance_ratio'
    assert domain['static_power_bound'] == 'positive_static_actuator_disk_floor'
    assert domain['inflow_scale'] == 'current_positive_thrust_hover_velocity'
    extension = struct.pack('<HHHH', propeller['schema'], 1, 1, 1)
    extension += struct.pack('<I', 2) + doubles(propeller['negative_advance_ratio'])
    extension += struct.pack('<I', len(propeller['negative_rows']))
    for row in propeller['negative_rows']:
        extension += struct.pack('<I', len(row))
        extension += b''.join(doubles([cell['ct'], cell['cp']]) for cell in row)
    extension += doubles(domain[k] for k in ['maximum_adverse_inflow_ratio',
        'maximum_transverse_inflow_ratio', 'static_power_epsilon_multiplier'])
    return out + component(extension)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    # Universal-newline reading makes CRLF/LF input equivalent; source numeric
    # conversion uses correctly rounded Python binary64, never serde's Value.
    source = PROFILE.read_text()
    encoded = canonical(json.loads(source))
    assert encoded == canonical(json.loads(source.replace('\n', '\r\n')))
    assert encoded == canonical(json.loads(json.dumps(json.loads(source))))
    fixture = dict(algorithm=1, schema=4, kind=3, law_revision=2,
        canonical_length=len(encoded), fingerprint=f'{digest(encoded):016x}',
        canonical_hex=encoded.hex())
    if args.write:
        FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
    assert fixture == json.loads(FIXTURE.read_text())
    print(f'Independent law-2 identity: {len(encoded)} bytes, FNV-1a64 {fixture["fingerprint"]}')
