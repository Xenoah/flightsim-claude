"""Independent v4 wire oracle. No Rust output is read to construct these bytes."""
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
    return doubles(6378137., 0., -0., 50., -0., 0., -0., 0., 0., 1., .01, .02, .03)

def fixture(block=b'', terminal=False):
    name = b'Independent jet'
    c = struct.pack('<I', len(name)) + name + struct.pack('<HHHIQ', 1, 2, 2, 1, 0x0123456789ABCDEF)
    c += struct.pack('<I', 1)
    c += doubles(0., 0., 0., .125, -0., 4., .5)
    c += struct.pack('<Q', 0x123456789ABCDEF0) + doubles(2461317.5, 60.)
    c += struct.pack('<QQdQ', 0, 0, 0., 0)
    c += struct.pack('<B', 0) + doubles(-500.)
    c += struct.pack('<I', len(block)) + block + state()
    assert len(c) == 255 + len(name) + len(block)
    data = b'FSREPLAY' + struct.pack('<HI', 4, len(c)) + c
    data += struct.pack('<I', 0) + struct.pack('<II', 1, 0) + state()
    event = b''
    if terminal:
        event = struct.pack('<I', 0) + doubles(-0., -1., 1., .75, .25, .5)
        # Outside envelope, pressure within, temperature within, Mach above.
        event += struct.pack('<BHBIB', 3, 1 | (1 << 2) | (2 << 4), 0, 0, 1)
        event += doubles(1., 1., .8)
        assert len(event) == 85
    return data + struct.pack('<I', len(event)) + event

def fixtures():
    yield 'v4_zero', fixture()
    yield 'v4_terminal_zero', fixture(terminal=True)
    no_query = fixture()[:-4] + struct.pack('<I', 61)
    no_query += struct.pack('<I', 0) + doubles(-0., -1., 1., .75, .25, .5)
    no_query += struct.pack('<BHBIB', 1, 7, 0, 0, 0)
    yield 'v4_terminal_no_query', no_query
    for i, name in enumerate(['custom_both', 'clear', 'cloud', 'fog', 'rain', 'snow', 'storm']):
        yield 'v4_' + name, fixture(weather(i))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    for name, value in fixtures():
        path = DEST / (name + '.fsreplay')
        if args.write:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(value)
        assert path.read_bytes() == value, path
        print(name, len(value), hashlib.sha256(value).hexdigest())
