#!/usr/bin/env python3
"""Authored test-only rows; preserves the exact original law-1 profile bytes.

No production profile, identity or replay format is created. The reference in
the CP recipe is a near-static continuation reference, not a reverse-flow law.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "crates/flightsim-sim/tests/fixtures"
PROFILE = FIXTURES / "cedar-law1-profile.json"
ROWS = FIXTURES / "cedar-law2-negative-rows.json"
PROFILE_SHA256 = "a4b13ac7c59830967666bb874e25416587a5fbf9945cbad248afa5bc7c9344e1"


def authored_rows():
    assert hashlib.sha256(PROFILE.read_bytes()).hexdigest() == PROFILE_SHA256
    original = json.loads(PROFILE.read_text())
    pitch = original["dynamics"]["propeller"]["blade_pitch_rad"]
    rows = []
    for j in (-0.01, -0.005):
        row = []
        for beta in pitch:
            ct = 0.5 * (beta - math.atan(j / (0.75 * math.pi)))
            reference = ct / 2 * (j + math.sqrt(j * j + 8 * ct / math.pi))
            cp = 1.22 * reference + 0.005 + 0.01 * beta
            assert ct > 0 and cp >= math.sqrt(2 / math.pi) * ct ** 1.5
            row.append(dict(ct=ct, cp=cp))
        rows.append(row)
    return json.dumps(rows, indent=2, allow_nan=False) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    content = authored_rows()
    if args.check:
        assert ROWS.read_text() == content, "stored row fixture differs from authored recipe"
        print("Original profile SHA256 and explicit negative row recipe match")
    else:
        ROWS.write_text(content)
        print(ROWS.relative_to(ROOT))
