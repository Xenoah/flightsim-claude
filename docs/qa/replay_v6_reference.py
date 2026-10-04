"""Independent v6 byte witnesses. No Rust codec/output constructs these bytes."""
import argparse
import hashlib
import struct
from pathlib import Path
from replay_v3_reference import weather

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'crates/flightsim-sim/tests/fixtures'


def doubles(*values):
    return struct.pack('<' + 'd' * len(values), *values)


def state():
    return doubles(6378137., 0., -0., 50., -0., 0., -0., 0., 0., 1., .01, .02, .03,
                   -0., 180., .3)


def failure(reason, detail, mask, stage=0, substep=0, j=-0., adverse=.05,
            transverse=.025):
    # The final three entries exist only for a fully established negative-J
    # flow scale. Absence is serialized as absence, never an invented zero.
    values = [.95, 1.05, .2, j, .3, 180., 180.01, .6, .01, 20., adverse, transverse]
    event = struct.pack('<I', 0) + doubles(-0., -1., 1., .75, .25, .5)
    event += struct.pack('<BHB I H', reason, detail, stage, substep, mask)
    event += doubles(*(v for i, v in enumerate(values) if mask & (1 << i)))
    assert len(event) == 62 + 8 * mask.bit_count()
    return event


def fixture(block=b'', event=b''):
    name = b'Independent near-static turboprop'
    c = struct.pack('<I', len(name)) + name
    c += struct.pack('<HHHIQ', 1, 4, 3, 2, 0x0123456789ABCDEF)
    c += struct.pack('<I', 1)
    # Exact unchanged 112-byte environment boundary.
    env = doubles(0., 0., 0., .125, -0., 4., .5)
    env += struct.pack('<Q', 0x123456789ABCDEF0) + doubles(2461317.5, 60.)
    env += struct.pack('<QQdQ', 0, 0, 0., 0)
    assert len(env) == 112
    c += env + struct.pack('<B', 0) + doubles(-500.)
    c += struct.pack('<I', len(block)) + block + state()
    assert len(c) == 279 + len(name) + len(block)
    data = b'FSREPLAY' + struct.pack('<HI', 6, len(c)) + c
    data += struct.pack('<I', 0) + struct.pack('<II', 1, 0) + state()
    return data + struct.pack('<I', len(event)) + event


def successful_witness():
    # Structural witness, not a claim of a dynamically reproduced flight.
    # Both the periodic full-state checkpoint and mandatory final tail differ.
    base = fixture()
    length = struct.unpack_from('<I', base, 10)[0]
    data = base[:14+length] + struct.pack('<I', 121)
    for i in range(121):
        data += doubles((i % 3-1)*.25, -0., (i % 5)*.125, i/128., .5, .25)
    data += struct.pack('<I', 2)
    for cursor, engine in [(120, (.25, 190., .4)), (121, (.375, 195., .5))]:
        data += struct.pack('<I', cursor) + state()[:104] + doubles(*engine)
    return data + struct.pack('<I', 0)


def fixtures():
    yield 'v6_zero', fixture()
    yield 'v6_successful_121', successful_witness()
    for i, name in enumerate(['custom_both', 'clear', 'cloud', 'fog', 'rain', 'snow', 'storm']):
        yield 'v6_' + name, fixture(weather(i))
    # Tags 1..8 retain their old closed meanings, without changing v5 bytes.
    # Seven Within statuses are 0x1555; shaft Above changes bits 6..7.
    for name, args in [
        ('terminal_raw', (1, 12, 0)),
        ('terminal_wind', (1, 7, 0x30)),
        ('terminal_altitude', (2, 2, 0)),
        ('terminal_envelope', (3, 0x1595, 0x77)),
        ('terminal_tip', (3, 0x1955, 0x1f7)),
        ('terminal_power_map', (4, 6, 0x1f7)),
        ('terminal_propeller_map', (5, 4, 0x1ff)),
        ('terminal_aero', (6, 2, 0x1ff)),
        ('terminal_budget', (7, 0, 0x1ff, 5, 7)),
        ('terminal_disk_nonpositive', (8, 1, 0x1ff, 2, 1)),
        ('terminal_disk_below', (8, 2, 0x1ff, 3, 2)),
        ('terminal_disk_derived', (8, 3, 0x1ff, 4, 3)),
        # New power details are explicit wire codes, not Rust discriminants.
        ('terminal_static_thrust', (9, 1, 0x1ff, 0, 0, -.005)),
        ('terminal_static_power', (9, 2, 0x1ff, 1, 0, -.005)),
        ('terminal_static_floor', (9, 3, 0x1ff, 2, 1, -.005)),
        ('terminal_static_derived', (9, 4, 0x1ff, 4, 3, -.005)),
        ('terminal_adverse', (10, 6, 0xfff, 0, 0, -.005, .125, .1)),
        ('terminal_transverse', (10, 9, 0xfff, 1, 0, -.005, .1, .125)),
        ('terminal_both_inflows', (10, 10, 0xfff, 5, 7, -.005, .125, .125)),
        ('terminal_scale_underflow', (11, 0, 0x1f7)),
        ('terminal_scale_load', (11, 0, 0x1ff, 2, 1, -.005)),
        # Established negative flow survives into the old post-flow reasons.
        ('terminal_negative_aero', (6, 2, 0xfff, 1, 0, -.005)),
        ('terminal_negative_budget', (7, 0, 0xfff, 5, 7, -.005)),
        ('terminal_negative_component', (1, 14, 0xfff, 2, 1, -.005)),
        ('terminal_negative_derivative', (1, 15, 0xfff, 3, 2, -.005)),
        ('terminal_negative_ground', (1, 6, 0xfff, 5, 3, -.005)),
    ]:
        yield 'v6_' + name, fixture(event=failure(*args))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    # Header14 + conditions655 + counts8 + controls48M + checkpoints1,100,088
    # + terminal length4 + max terminal158.
    assert 14 + (279+256+120) + 4 + 48*1_000_000 + 4 + 132*8334 + 4 + 158 == 49_100_927
    for name, value in fixtures():
        path = DEST / (name + '.fsreplay')
        if args.write:
            path.write_bytes(value)
        assert path.read_bytes() == value, path
        print(name, len(value), hashlib.sha256(value).hexdigest())
