"""Independent stdlib ZIP32/FSDM fixture; no external terrain or license claims."""
import hashlib
import io
import json
from pathlib import Path
import struct
import zipfile

payload = b"\0" * 8
checksum = 0xCBF29CE484222325
for byte in payload:
    checksum = ((checksum ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
terrain = struct.pack("<4sHBBIIIIdddQ", b"FSDM", 1, 9, 0, 900, 180, 2, 2, 350.0, 0.0, 0.0, checksum) + payload
license_text = b"Synthetic test fixture; no external inputs.\n"
files = {"terrain/9/900/180.fsdem": terrain, "docs/LICENSE.txt": license_text}
manifest = {
    "schema_version": 1, "id": "org.example.download", "version": "1.0.0", "title": "Synthetic download fixture",
    "content_kinds": ["terrain_dem"],
    "terrain": {"bounds_degrees": {"west": -180.0, "south": -90.0, "east": 180.0, "north": 90.0}, "nominal_resolution_m": 90.0, "datum": "EPSG:4979"},
    "sources": [{"id": "synthetic", "url": "https://example.test/fixture", "revision": "test-v1", "provenance": "Constant 350 metre ellipsoidal test grid, generated independently from format specification", "credits": "Synthetic test fixture", "license": {"name": "Test fixture only", "url": "https://example.test/fixture-terms", "text_path": "docs/LICENSE.txt"}}],
    "files": [{"path": name, "kind": "terrain_dem" if name.endswith(".fsdem") else "documentation", "size_bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "source": "synthetic"} for name, data in files.items()],
}
files = {"manifest.json": json.dumps(manifest, separators=(",", ":")).encode(), **files}
out = io.BytesIO()
with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
    for name, data in files.items():
        info = zipfile.ZipInfo(name, (2026, 10, 4, 0, 0, 0))
        info.create_system = 3
        info.external_attr = 0o100644 << 16
        archive.writestr(info, data)
Path(__file__).with_name("download-fixture.zip").write_bytes(out.getvalue())
