#!/usr/bin/env python3
"""Bake the bundled NOAA NCEP/NCAR 1991–2020 climate atlas (offline only).

Requires numpy and h5py. This script never installs software or fetches
data. Inputs, downloads, SHA-256 values and licenses are described in the
generated provenance JSON and docs/data/global-climate.md.

Example:
  python scripts/bake_climate.py --input-dir /path/to/noaa \
      --geoid /path/to/etopo_geoid_north_to_south_10min.npy \
      --geoid-metadata /path/to/terrain-preparation.json

The geoid array must be the ETOPO 2022 EGM2008 60s source sampled at [0:10:N]
(not a rescaled image), reordered north-to-south and to longitudes 0..360.
Its exact first cell centre is 89.8416666667 N / 0.0083333333 E, spacing
1/6 degree. The axis contract is checked against --geoid-metadata.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import tempfile

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "crates/flightsim-world/data/ncep-ncar-1991-2020.fsclim"
DEFAULT_METADATA_OUTPUT = ROOT / "crates/flightsim-world/src/climate_metadata.rs"
SOURCE_ROOT = "https://downloads.psl.noaa.gov/Datasets/ncep.reanalysis/"
FILES = {
    "temperature": ("air.2m.mon.ltm.1991-2020.nc", "air", "Monthlies/surface_gauss/", "degK", 150., 350.),
    "precipitation": ("prate.sfc.mon.ltm.1991-2020.nc", "prate", "Monthlies/surface_gauss/", "Kg/m^2/s", 0., .001),
    "cloud": ("tcdc.eatm.mon.ltm.1991-2020.nc", "tcdc", "Monthlies/other_gauss/", "%", 0., 100.),
    "model_height": ("hgt.sfc.gauss.nc", "hgt", "Monthlies/surface_gauss/", "m", -1200., 6300.),
}
EXPECTED_SOURCE_SHA256 = {
    "air.2m.mon.ltm.1991-2020.nc": "c86a3c575010ca2c9414b24022361c43be6966dcdc62c821c1918e9c44ca9be9",
    "prate.sfc.mon.ltm.1991-2020.nc": "35c390d9b82128925f960b8189992f3f617d131d8f4af4051dcd216d40825798",
    "tcdc.eatm.mon.ltm.1991-2020.nc": "61c7021d96fdf65d06ca63c8b1106b2e0dd94b93412a41b80463f3b306f7ef7e",
    "hgt.sfc.gauss.nc": "0862a41820743c04e94c89b0475d734232f517431951dd9d413658e87d6f5d88",
}
EXPECTED_GEOID_SHA256 = "85a413d02f13f92c8571797331e65574038e70530ac7846e9c577178c50fa318"
MONTH_STARTS = np.array([0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334])
MAGIC = b"FSCLIM01"
DATASET_ID = "NOAA-NCEP-NCAR-R1-1991-2020-monthly-EGM2008-v1"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def atomic_write(path: Path, data: bytes) -> None:
    """Replace each file atomically; the provenance completion marker is last."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def fnv64(data: bytes) -> int:
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


def attr_text(value) -> str:
    return value.decode("utf-8") if isinstance(value, bytes) else str(value)


def polar_rows(values: np.ndarray) -> np.ndarray:
    """Close both Gaussian polar caps with longitude-independent zonal means."""
    north = np.repeat(values[..., :1, :].mean(axis=-1, keepdims=True), values.shape[-1], axis=-1)
    south = np.repeat(values[..., -1:, :].mean(axis=-1, keepdims=True), values.shape[-1], axis=-1)
    return np.concatenate((north, values, south), axis=-2)


def validate_geoid_axes(axes: dict) -> None:
    """The pinned array's registration is part of its data identity too."""
    if (axes["schema"] not in ["flightsim-global-terrain-inputs-v1", "flightsim-global-terrain-inputs-v2"]
            or axes["geoid"] != "float32 little-endian EGM2008 undulation N in metres; h = H + N"):
        raise ValueError("geoid must be EGM2008 undulation, not terrain elevation")
    if (axes["shape"] != [1080, 2160] or axes["row_order"] != "north-to-south"
            or axes["column_order"] != "eastward, periodic"):
        raise ValueError("geoid grid shape and order differ from the pinned source")
    expected = {
        "latitude_origin_deg": 89.84166666666667,
        "longitude_origin_deg": 0.008333333333325754,
        "latitude_step_deg": -1/6,
        "longitude_step_deg": 1/6,
    }
    for field, value in expected.items():
        actual = axes[field]
        if (not isinstance(actual, (float, int)) or not math.isfinite(actual)
                or not math.isclose(actual, value, rel_tol=0, abs_tol=1e-12)):
            raise ValueError(f"geoid {field} differs from the pinned grid registration")
    last_latitude = axes["latitude_origin_deg"] + 1079 * axes["latitude_step_deg"]
    if not math.isclose(last_latitude, -89.99166666666666, rel_tol=0, abs_tol=1e-12):
        raise ValueError("geoid must span the documented full latitude grid")


def sample_geoid(geoid: np.ndarray, axes: dict, latitudes: np.ndarray, longitudes: np.ndarray) -> np.ndarray:
    """Periodic bilinear indexing; axes in degrees, no coordinate conversion."""
    validate_geoid_axes(axes)
    if geoid.shape != tuple(axes["shape"]):
        raise ValueError("geoid dimensions differ from the documented source grid")
    if not np.isfinite(geoid).all() or np.min(geoid) < -200 or np.max(geoid) > 200:
        raise ValueError("geoid contains missing or physically implausible values")
    if axes["latitude_step_deg"] >= 0 or axes["longitude_step_deg"] <= 0:
        raise ValueError("geoid must be north-to-south / west-to-east")
    if abs(geoid.shape[1] * axes["longitude_step_deg"] - 360) > 1e-7:
        raise ValueError("geoid longitude grid must cover exactly one periodic globe")
    x = np.remainder((longitudes - axes["longitude_origin_deg"]) / axes["longitude_step_deg"], geoid.shape[1])
    y = np.clip((latitudes - axes["latitude_origin_deg"]) / axes["latitude_step_deg"], 0, geoid.shape[0] - 1)
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    x1, y1 = (x0 + 1) % geoid.shape[1], np.minimum(y0 + 1, geoid.shape[0] - 1)
    fx, fy = x - x0, y - y0
    north = geoid[y0[:, None], x0] * (1 - fx) + geoid[y0[:, None], x1] * fx
    south = geoid[y1[:, None], x0] * (1 - fx) + geoid[y1[:, None], x1] * fx
    return north * (1 - fy[:, None]) + south * fy[:, None]


def output_paths(args) -> tuple[Path, Path, Path]:
    """Validate the complete output family before reading or replacing files."""
    metadata_output = args.metadata_output
    if metadata_output is None:
        metadata_output = (DEFAULT_METADATA_OUTPUT if args.output.resolve() == DEFAULT_OUTPUT.resolve()
                           else args.output.with_suffix(".metadata.rs"))
    provenance = args.output.with_suffix(".json")
    outputs = [args.output, metadata_output, provenance]
    resolved = {path.resolve() for path in outputs}
    if len(resolved) != len(outputs):
        raise ValueError("binary, metadata and provenance need distinct output paths")
    inputs = {args.geoid.resolve(), args.geoid_metadata.resolve()}
    inputs.update((args.input_dir / spec[0]).resolve() for spec in FILES.values())
    if resolved & inputs:
        raise ValueError("output paths must not overwrite source data or the input manifest")
    return tuple(outputs)


def bake(args) -> dict:
    output_path, metadata_output, provenance = output_paths(args)
    try:
        import h5py
    except ImportError as error:
        raise RuntimeError(
            "Baking NOAA NetCDF sources requires h5py in the selected Python environment; "
            "this script does not install dependencies. Boundary tests do not require h5py."
        ) from error
    arrays = {}
    inputs = []
    latitude = longitude = None
    for field, (name, variable, directory, units, lower, upper) in FILES.items():
        path = args.input_dir / name
        if path.stat().st_size > 2_000_000 or sha256(path) != EXPECTED_SOURCE_SHA256[name]:
            raise ValueError(f"{name} differs from the reviewed NOAA source; inspect before rebaking")
        with h5py.File(path, "r") as source:
            data = source[variable]
            values = data[...].astype(np.float64)
            lat, lon = source["lat"][...].astype(np.float64), source["lon"][...].astype(np.float64)
            if attr_text(data.attrs["units"]) != units:
                raise ValueError(f"unexpected units for {name}")
            expected_shape = (1 if field == "model_height" else 12, 94, 192)
            if values.shape != expected_shape:
                raise ValueError(f"unexpected dimensions for {name}: {values.shape}")
            if latitude is None:
                latitude, longitude = lat, lon
                if not (np.diff(lat) < 0).all() or not np.allclose(lon, np.arange(192) * 1.875):
                    raise ValueError("unexpected NOAA Gaussian latitude/longitude axes")
            elif not np.array_equal(latitude, lat) or not np.array_equal(longitude, lon):
                raise ValueError("NOAA fields use mismatched grids")
            if field != "model_height":
                if attr_text(source["time"].attrs["climo_period"]) != "1991/01/01 - 2020/12/31":
                    raise ValueError("expected exactly the 1991–2020 climatology")
                if not np.all(source["valid_yr_count"][...] == 30):
                    raise ValueError("climatology contains incomplete sample counts")
                if (not np.array_equal(source["time"][...], -15769752. + MONTH_STARTS * 24.)
                        or attr_text(source["time"].attrs["units"]) != "hours since 1800-01-01 00:00:0.0"
                        or attr_text(source["time"].attrs["delta_t"]) != "0000-01-00 00:00:00"
                        or attr_text(data.attrs["statistic"]) != "Long Term Mean"):
                    raise ValueError("climatology months are not January–December on the expected time axis")
            if not np.isfinite(values).all() or values.min() < lower or values.max() > upper:
                raise ValueError(f"missing or implausible values in {name}")
            # These NOAA files are unpacked float32. Refuse another encoding
            # rather than accidentally applying or omitting its scale/offset.
            if np.any(data.attrs.get("scale_factor", 1) != 1) or np.any(data.attrs.get("add_offset", 0) != 0):
                raise ValueError("packed source input is not supported")
            arrays[field] = polar_rows(values)
            inputs.append({"file": name, "url": SOURCE_ROOT + directory + name,
                           "sha256": sha256(path), "units": units,
                           "minimum": float(values.min()), "maximum": float(values.max())})

    axes = json.loads(args.geoid_metadata.read_text())
    geoid_hash = sha256(args.geoid)
    if (geoid_hash != axes["outputs"][args.geoid.name]["sha256"]
            or geoid_hash != EXPECTED_GEOID_SHA256):
        raise ValueError("geoid input differs from its preparation manifest")
    geoid = np.load(args.geoid, allow_pickle=False)
    grid_geoid = polar_rows(sample_geoid(geoid, axes, latitude, longitude))
    latitudes = np.concatenate(([90.], latitude, [-90.]))
    payload = bytearray(np.deg2rad(latitudes).astype("<f8").tobytes())
    static = np.empty((96, 192, 2), dtype="<i2")
    static[:, :, 0] = np.rint(arrays["model_height"][0])
    static[:, :, 1] = np.rint(grid_geoid * 10)
    payload.extend(static.tobytes())
    temperature = np.rint(arrays["temperature"] * 100).astype("<u2")
    # kg/m²/s / 1000 kg/m³ => metres of water per second; encoded in 1e-10 m/s.
    precipitation = np.rint(arrays["precipitation"] * 1e7).astype("<u2")
    cloud = np.rint(arrays["cloud"] * 2.55).astype("u1")
    record = np.empty((12, 96, 192), dtype=np.dtype([("t", "<u2"), ("p", "<u2"), ("c", "u1")]))
    record["t"], record["p"], record["c"] = temperature, precipitation, cloud
    payload.extend(record.tobytes())
    fingerprint = fnv64(payload)
    header = struct.pack("<8s6HIQQ", MAGIC, 1, 96, 192, 12, 5, 4, 0, len(payload), fingerprint)
    assert len(header) == 40
    output = header + payload
    metadata = {
        "status": "COMPLETE",
        "dataset_id": DATASET_ID,
        "description": "NCEP/NCAR Reanalysis 1 monthly 1991–2020 long-term means, provided by NOAA PSL",
        "data_status": "reanalysis climatology, not live observations or forecast",
        "license": "US Government public domain; https://www.psl.noaa.gov/data/help/",
        "source_grid": {"rows": 94, "columns": 192, "months": 12, "latitude": "nonuniform Gaussian, exact NOAA coordinates preserved", "longitude_spacing_degrees": 1.875},
        "polar_cap": "Two extra +/-90 degree rows are longitude-independent means of the nearest Gaussian row, before quantization",
        "quantization": {"temperature_kelvin": .01, "precipitation_water_metres_per_second": 1e-10, "cloud_fraction": 1/255, "model_geopotential_height_metres": 1, "geoid_undulation_metres": .1},
        "geoid_input": {"file": args.geoid.name, "sha256": sha256(args.geoid), "axes": axes},
        "inputs": inputs,
        "baked": {"file": args.output.name, "metadata_file": metadata_output.name, "bytes": len(output), "sha256": hashlib.sha256(output).hexdigest(), "fnv1a64_payload": f"{fingerprint:016x}"},
    }
    rust = f'''// Generated by scripts/bake_climate.py; do not edit independently of the atlas.
/// Identity of the bundled monthly climate data and quantization.
pub const GLOBAL_CLIMATE_DATASET_ID: &str = "{DATASET_ID}";
/// FNV-1a identity of the climate payload; replay compatibility, not security.
pub const GLOBAL_CLIMATE_FINGERPRINT: u64 = 0x{fingerprint:016x};
'''
    incomplete = {**metadata, "status": "INCOMPLETE"}
    atomic_write(provenance, (json.dumps(incomplete, indent=2, ensure_ascii=False) + "\n").encode())
    atomic_write(output_path, output)
    atomic_write(metadata_output, rust.encode())
    atomic_write(provenance, (json.dumps(metadata, indent=2, ensure_ascii=False) + "\n").encode())
    return metadata


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--geoid", type=Path, required=True)
    parser.add_argument("--geoid-metadata", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--metadata-output", type=Path, help="Rust identity destination; defaults beside alternate output, or to the bundled module for the canonical output")
    arguments = parser.parse_args()
    print(json.dumps(bake(arguments)["baked"], indent=2))
