"""Independent FSREPLAY encoder for reviewed v1/v2/v3 binary fixtures.

Python stdlib only; no Rust writer or generated production bytes. Run normally
for byte-for-byte verification, or --write to intentionally regenerate fixtures.
Identity constants are independently checked by replay_identity_reference.py.
World fingerprints are pinned dataset identities, not recomputed from Rust code.
"""

import argparse
import hashlib
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "crates/flightsim-sim/tests/fixtures"
TERRAIN = 0x500E8DB32BDE7019
CLIMATE = 0x33F6A12038B9F6B8
COMPLETE = 0xB7FA864DC47824F7
LEGACY = 0x0505E6644BB29A53


def numbers(*values):
    return struct.pack("<" + "d" * len(values), *values)


def weather(preset):
    # Resolved ellipsoidal values for departure latitude .25, longitude -.5,
    # reference height -500 m. Custom (0) is Rain plus an explicit fog layer.
    ambient, phase, rate, cloud, fog = {
        0: (10000., 1, .005 / 3600, (1, 100., 2100., 1., 250.), (-500., -200., 250.)),
        1: (100000., 0, 0., None, None),
        2: (40000., 0, 0., (2, 1000., 2200., .65, 500.), None),
        3: (50000., 0, 0., None, (-500., -200., 250.)),
        4: (10000., 1, .005 / 3600, (1, 100., 2100., 1., 250.), None),
        5: (3000., 2, .001 / 3600, (1, -200., 1300., 1., 200.), None),
        6: (5000., 1, .025 / 3600, (3, 0., 7500., 1., 150.), None),
    }[preset]
    data = struct.pack("<HHIHHQ", 1, 1, 1, preset, phase, 0xFFFFFFFFFFFFFFFF)
    data += numbers(.25, -.5, -500., ambient, rate)
    data += struct.pack("<H", bool(cloud) | (bool(fog) << 1))
    if cloud:
        data += struct.pack("<H", cloud[0]) + numbers(*cloud[1:])
    if fog:
        data += numbers(*fog)
    assert len(data) == 62 + (34 if cloud else 0) + (24 if fog else 0)
    return data


def environment():
    return (numbers(.25, -.5, 1000., .125, -0., 4., .5)
            + struct.pack("<Q", 0x123456789ABCDEF0) + numbers(2461317.5, 60.))


def frame(dt):
    return numbers(dt, -0., -1., 1., .75, .25, .5)


def current(block):
    conditions = struct.pack("<I", 1) + b"L" + struct.pack("<HHIQ", 1, 1, 2, COMPLETE)
    conditions += environment() + struct.pack("<QQdQ", 3, TERRAIN, .25, CLIMATE)
    conditions += struct.pack("<I", len(block)) + block
    assert len(conditions) == 137 + len(block)
    data = b"FSREPLAY" + struct.pack("<HI", 3, len(conditions)) + conditions
    data += struct.pack("<II", 3, 1)
    data += frame(-0.) + frame(1. / 120.) + frame(.025)
    data += struct.pack("<I", 0) + numbers(6378137., 0., -0., 50., -0., 0., -0., 0., 0., 1., .01, .02, .03)
    assert len(data) == 435 + len(block)
    return data


def legacy(version, world):
    data = b"FSREPLAY" + struct.pack("<HI", version, 1) + b"L" + struct.pack("<Q", LEGACY)
    data += environment()
    if version == 2:
        data += struct.pack("<QQdQ", world, TERRAIN if world else 0, 0., 0)
    return data + struct.pack("<II", 1, 0) + frame(-0.)


def fixtures():
    yield "legacy_v1", legacy(1, False)
    yield "legacy_v2_disabled", legacy(2, False)
    yield "legacy_v2_world", legacy(2, True)
    yield "v3_legacy_weather", current(b"")
    for preset, name in enumerate(["custom_both", "clear", "cloud", "fog", "rain", "snow", "storm"]):
        yield f"v3_{name}", current(weather(preset))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    for stem, encoded in fixtures():
        path = DEST / f"{stem}.fsreplay"
        if args.write:
            DEST.mkdir(parents=True, exist_ok=True)
            path.write_bytes(encoded)
        assert path.read_bytes() == encoded, f"fixture differs: {path}"
        print(f"{stem}: {len(encoded)} bytes, sha256 {hashlib.sha256(encoded).hexdigest()}")
