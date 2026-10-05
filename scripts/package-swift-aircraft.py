#!/usr/bin/env python3
"""Build the fixed original Swift data package offline, without changing input bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "docs/examples/aircraft-packages/swift"
MEMBERS = {
    "profile.json": ROOT / "assets/aircraft/swift_sport.json",
    "assets/aircraft/swift_sport.glb": ROOT / "assets/aircraft/swift_sport.glb",
    "docs/LICENSE-MIT.txt": FIXTURE / "LICENSE-MIT.txt",
    "docs/LICENSE-APACHE.txt": FIXTURE / "LICENSE-APACHE.txt",
    "docs/PROVENANCE.md": FIXTURE / "PROVENANCE.md",
}

def build(output):
    manifest_bytes = (FIXTURE / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    payload = {name: source.read_bytes() for name, source in MEMBERS.items()}
    for record in manifest["files"]:
        data = payload[record["path"]]
        if len(data) != record["size_bytes"] or hashlib.sha256(data).hexdigest() != record["sha256"]:
            raise ValueError(f"fixed source bytes changed: {record['path']}")
    if set(payload) != {f["path"] for f in manifest["files"]}:
        raise ValueError("fixed manifest member set changed")
    payload = {"manifest.json": manifest_bytes, **payload}
    # Exclusive file creation: never replace a user's existing package.
    with output.open("xb") as target:
        with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
            for name, data in payload.items():
                info = zipfile.ZipInfo(name, date_time=(2026, 10, 5, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data)
    print(f"{output}: {output.stat().st_size} bytes; SHA-256 {hashlib.sha256(output.read_bytes()).hexdigest()}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    build(parser.parse_args().output)
