#!/usr/bin/env python3
"""Offline deterministic FSGT v2 packer (numpy required, no network or installs).

Input: the exact .npy arrays + preparation JSON emitted by prepare-global-terrain.py.
The Rust flightsim-globalgen accepts equivalent explicit little-endian raw arrays.
FSGT is documented in docs/global-terrain.md. It is never decoded as GeoTIFF at runtime.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import tempfile
import numpy as np

HEADER_BYTES = 64
DATASET_ID = "ETOPO2022-COP90-NE-2048x1024-surface-v2"

def fingerprint(data):
    value = 0xCBF29CE484222325
    for index, byte in enumerate(data):
        if not 24 <= index < 32:
            value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value

def quantize(array, scale):
    scaled = array.astype(np.float64) * scale
    # Rust f64::round is halfway away from zero; numpy.rint uses ties-to-even.
    return np.where(scaled >= 0, np.floor(scaled + 0.5), np.ceil(scaled - 0.5)).astype('<i2')

def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        file.write(data)
        file.flush()
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)

def canonical_resample(elevation, geoid, land, inland_water, longitude_origin, north_latitude):
    """Resample the physical surface, never unmasked ocean bathymetry.

    Float arithmetic/order mirrors tilegen::global::resample_global_atlas.
    Target breakpoints align with 33-point geographic DEM grids at level >= 6.
    """
    source_height, source_width = elevation.shape
    source_step = 180.0 / source_height
    source_south = north_latitude - (source_height - 1) * source_step
    target_height, target_width = 1024, 2048
    target_step = 180.0 / target_height
    target_lon, target_north = -180 + 180 / target_width, 90 - 90 / target_height
    surface = np.where(land | inland_water, elevation.astype(np.float64), 0.0)
    geoid = geoid.astype(np.float64)
    # Summation order matches Rust (numpy.mean uses a different reduction tree).
    def sequential_mean(values):
        total = 0.0
        for value in values:
            total += float(value)
        return total / source_width
    poles = [np.array([sequential_mean(surface[row]), sequential_mean(geoid[row])])
             for row in [0, source_height - 1]]
    longitude = target_lon + np.arange(target_width, dtype=np.float64) * target_step
    x = np.remainder(longitude - longitude_origin, 360.0) / source_step
    left = np.minimum(np.floor(x).astype(np.int64), source_width - 1)
    right = (left + 1) % source_width
    fx = np.clip(x - left, 0, 1)
    nearest_column = np.floor(x + 0.5).astype(np.int64) % source_width
    output_h = np.empty((target_height, target_width), dtype=np.float64)
    output_n = np.empty_like(output_h)
    output_land = np.empty((target_height, target_width), dtype=np.bool_)
    output_water = np.empty_like(output_land)
    def line(array, row):
        a, b = array[row, left], array[row, right]
        return a + (b - a) * fx
    for row in range(target_height):
        latitude = target_north - row * target_step
        y = (north_latitude - latitude) / source_step
        nearest_row = int(np.clip(math.floor(y + 0.5), 0, source_height - 1))
        output_land[row] = land[nearest_row, nearest_column]
        output_water[row] = inland_water[nearest_row, nearest_column]
        if latitude > north_latitude:
            fraction = min(1, max(0, (latitude - north_latitude) / (90 - north_latitude)))
            values = [line(array, 0) for array in (surface, geoid)]
            values = [value + (poles[0][i] - value) * fraction for i, value in enumerate(values)]
        elif latitude < source_south:
            fraction = min(1, max(0, (source_south - latitude) / (source_south + 90)))
            values = [line(array, source_height - 1) for array in (surface, geoid)]
            values = [value + (poles[1][i] - value) * fraction for i, value in enumerate(values)]
        else:
            top = min(math.floor(y), source_height - 2)
            fraction = min(1, max(0, y - top))
            values = []
            for array in (surface, geoid):
                a, b = line(array, top), line(array, top + 1)
                values.append(a + (b - a) * fraction)
        output_h[row] = np.where(output_water[row], elevation[nearest_row, nearest_column],
                                 np.where(output_land[row], values[0], 0.0))
        output_n[row] = values[1]
    # Feature-preserving cell pooling: an independently corrected inland-water
    # point must not disappear between target centres; a genuine negative dry-
    # land source minimum must not be averaged away. This can widen/shift a
    # source point's assigned node by at most half a canonical cell per axis.
    # That is NOT a bound on interpolated shoreline/feature displacement.
    source_rows, source_columns = np.nonzero(inland_water | (land & (elevation < 0)))
    source_latitudes = north_latitude - source_rows * source_step
    source_longitudes = longitude_origin + source_columns * source_step
    target_x = np.remainder(source_longitudes - target_lon, 360.0) / target_step
    target_y = (target_north - source_latitudes) / target_step
    target_columns = np.floor(target_x + 0.5).astype(np.int64) % target_width
    target_rows = np.clip(np.floor(target_y + 0.5).astype(np.int64), 0, target_height - 1)
    target_indices = target_rows * target_width + target_columns
    source_values = elevation[source_rows, source_columns].astype(np.float64)
    lake_candidates = inland_water[source_rows, source_columns]
    lake_indexes = target_indices[lake_candidates]
    lake_values = source_values[lake_candidates]
    dx = np.remainder(target_x[lake_candidates] - target_columns[lake_candidates] + target_width / 2, target_width) - target_width / 2
    dy = target_y[lake_candidates] - target_rows[lake_candidates]
    distances = dx * dx + dy * dy
    # Sort by target cell, then distance, then original row-major source index.
    # Numpy arrays keep worst-case preparation memory bounded without millions
    # of Python dictionary objects when a synthetic fixture is entirely water.
    source_order = (source_rows * source_width + source_columns)[lake_candidates]
    order = np.lexsort((source_order, distances, lake_indexes))
    lake_indexes, lake_values = lake_indexes[order], lake_values[order]
    first = np.r_[True, lake_indexes[1:] != lake_indexes[:-1]] if len(lake_indexes) else np.empty(0, dtype=bool)
    lake_indexes, lake_values = lake_indexes[first], lake_values[first]
    negative_values = np.full(target_height * target_width, np.inf)
    np.minimum.at(negative_values, target_indices[~lake_candidates], source_values[~lake_candidates])
    negative_values[lake_indexes] = np.inf  # Inland water wins conflicts.
    negative_indexes = np.flatnonzero(np.isfinite(negative_values))
    negative_values = negative_values[negative_indexes]
    flat_h, flat_land, flat_water = output_h.reshape(-1), output_land.reshape(-1), output_water.reshape(-1)
    changed_land = int(np.count_nonzero((flat_h[negative_indexes] != negative_values) | ~flat_land[negative_indexes]))
    changed_water = int(np.count_nonzero((flat_h[lake_indexes] != lake_values) | ~flat_water[lake_indexes]))
    max_shift = max(float(np.max(np.abs(flat_h[negative_indexes] - negative_values), initial=0)),
                    float(np.max(np.abs(flat_h[lake_indexes] - lake_values), initial=0)))
    flat_h[negative_indexes], flat_land[negative_indexes], flat_water[negative_indexes] = negative_values, True, False
    flat_h[lake_indexes], flat_land[lake_indexes], flat_water[lake_indexes] = lake_values, False, True
    pooling = {
        'policy': 'inland-water source points and negative dry-land minima retained in their nearest canonical cell; inland water wins conflicts; closest lake source point wins, equal-distance ties use source row-major order',
        'changed_negative_land_cells': changed_land, 'changed_inland_water_cells': changed_water,
        'largest_node_height_change_from_unpooled_resample_m': max_shift,
        'maximum_source_point_to_assigned_node_degrees_per_axis': target_step / 2,
        'limitation': 'the half-cell bound applies only to source-point-to-assigned-node coordinates; interpolated shoreline/negative-height footprints can extend farther and can shift or widen; neighbouring terrain still mixes; not surveyed geometry',
    }
    return output_h, output_n, output_land, output_water, target_lon, target_north, pooling

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepared', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--rust-metadata', type=Path, help='optional generated Rust dataset-id/fingerprint constants')
    args = parser.parse_args()
    root = args.prepared
    manifest_path = root / 'terrain-preparation.json'
    if manifest_path.stat().st_size > 1024 * 1024:
        raise ValueError('preparation manifest exceeds 1 MiB')
    terrain = json.loads(manifest_path.read_text())
    if terrain.get('schema') != 'flightsim-global-terrain-inputs-v2':
        raise ValueError('unrecognized preparation manifest')
    names = ['etopo_orthometric_north_to_south_10min.npy', 'etopo_geoid_north_to_south_10min.npy', 'ne_land_north_to_south_10min.npy', 'inland_water.npy']
    inputs = {(root / name).resolve() for name in names + ['terrain-preparation.json', 'elevation.f32le', 'geoid.f32le', 'land.u8', 'inland_water.u8']}
    outputs = {args.output.resolve(), args.output.with_suffix('.provenance.json').resolve()}
    if args.rust_metadata is not None:
        if args.rust_metadata.resolve() in outputs:
            raise ValueError('Rust metadata output must be distinct from atlas/provenance')
        outputs.add(args.rust_metadata.resolve())
    if len(outputs) != (3 if args.rust_metadata is not None else 2) or inputs & outputs:
        raise ValueError('atlas and provenance outputs must be distinct and must not overwrite an input')
    for name in names:
        if (root / name).stat().st_size > 40 * 1024 * 1024:
            raise ValueError(f'prepared input exceeds 40 MiB: {name}')
        with (root / name).open('rb') as handle:
            data = handle.read(40 * 1024 * 1024 + 1)
        if len(data) > 40 * 1024 * 1024:
            raise ValueError(f'prepared input grew past 40 MiB: {name}')
        expected = terrain['outputs'][name]
        if len(data) != expected['bytes'] or hashlib.sha256(data).hexdigest() != expected['sha256']:
            raise ValueError(f'prepared input checksum mismatch: {name}')
    elevation, geoid, land, inland_water = [np.load(root / name, allow_pickle=False, mmap_mode='r') for name in names]
    if elevation.ndim != 2 or geoid.shape != elevation.shape or land.shape != elevation.shape or inland_water.shape != elevation.shape:
        raise ValueError('all input channels must have the same two-dimensional shape')
    height, width = elevation.shape
    if (height, width) != (1080, 2160) or list(elevation.shape) != terrain['shape']:
        raise ValueError('invalid global raster dimensions or mismatched provenance')
    if elevation.dtype != np.dtype('<f4') or geoid.dtype != np.dtype('<f4'):
        raise ValueError('elevation and geoid must be explicit float32 little-endian data')
    if land.dtype != np.bool_ or inland_water.dtype != np.bool_ or np.any(land & inland_water):
        raise ValueError('dry-land and inland-water masks must be explicit disjoint boolean data')
    if not np.all(np.isfinite(elevation)) or not np.all(np.isfinite(geoid)):
        raise ValueError('nodata/nonfinite values cannot be encoded')
    if np.max(np.abs(elevation)) > 12000 or np.max(np.abs(geoid)) > 200 or np.any((land | inland_water) & (elevation < -500)):
        raise ValueError('terrain/geoid outside physical surface bounds; unexplained bathymetry is rejected, never clipped')
    lon, north = terrain['longitude_origin_deg'], terrain['latitude_origin_deg']
    if (not math.isfinite(lon) or not math.isfinite(north)
            or abs(lon - 0.008333333333325754) > 1e-12
            or abs(north - 89.84166666666667) > 1e-12
            or terrain.get('row_order') != 'north-to-south'
            or terrain.get('column_order') != 'eastward, periodic'):
        raise ValueError('reviewed ETOPO source registration/orientation mismatch')
    if not math.isfinite(lon) or not -180 <= lon < 180 or not 90 - 180 / height < north < 90:
        raise ValueError('invalid explicit sample origin')
    if terrain['longitude_step_deg'] != 360 / width or terrain['latitude_step_deg'] != -180 / height:
        raise ValueError('unexpected sample spacing')
    if terrain['elevation'] != 'float32 little-endian EGM2008 orthometric height H in metres' or terrain['geoid'] != 'float32 little-endian EGM2008 undulation N in metres; h = H + N':
        raise ValueError('source datum must explicitly identify EGM2008 metres')
    source_geometry = {'shape': [height, width], 'longitude_origin_deg': lon, 'latitude_origin_deg': north}
    elevation, geoid, land, inland_water, lon, north, pooling = canonical_resample(elevation, geoid, land, inland_water, lon, north)
    height, width = elevation.shape
    nodes = np.empty((height, width, 2), dtype='<i2')
    nodes[:, :, 0] = quantize(elevation, 1)
    nodes[:, :, 1] = quantize(geoid, 100)
    header = bytearray(HEADER_BYTES)
    struct.pack_into('<4sHHIIHHHHQdd', header, 0, b'FSGT', 2, 0, width, height, 4326, 3855, 4979, 1, 0, lon, north)
    data = (header + nodes.tobytes(order='C')
            + np.packbits(land.reshape(-1), bitorder='little').tobytes()
            + np.packbits(inland_water.reshape(-1), bitorder='little').tobytes())
    content_fingerprint = fingerprint(data)
    struct.pack_into('<Q', data, 24, content_fingerprint)
    provenance_path = args.output.with_suffix('.provenance.json')
    atomic_write(provenance_path, (json.dumps({
        'status': 'INCOMPLETE', 'format': 'FSGT v2', 'dataset_id': DATASET_ID,
        'fingerprint_fnv1a64': f'{content_fingerprint:016x}'
    }, indent=2) + '\n').encode())
    atomic_write(args.output, data)
    if args.rust_metadata is not None:
        metadata = (
            '// Generated from the reviewed bundled atlas by scripts/pack-global-terrain.py.\n'
            '/// Stable identity of the bundled source mix and resampling policy.\n'
            f'pub const GLOBAL_TERRAIN_DATASET_ID: &str = \"{DATASET_ID}\";\n'
            '/// Exact FSGT content fingerprint (corruption/reproducibility, not authentication).\n'
            f'pub const GLOBAL_TERRAIN_FINGERPRINT: u64 = 0x{content_fingerprint:016x};\n'
        )
        atomic_write(args.rust_metadata, metadata.encode())
    manifest = {
        'status': 'COMPLETE', 'format': 'FSGT v2', 'dataset_id': DATASET_ID,
        'sha256': hashlib.sha256(data).hexdigest(), 'fingerprint_fnv1a64': f'{content_fingerprint:016x}',
        'bytes': len(data), 'shape': [height, width], 'sample_origin': {'longitude_deg': lon, 'latitude_deg': north},
        'spacing_degrees': 180 / height, 'source_geometry': source_geometry,
        'resampling': 'physical surface H (source ocean H=0) and N bilinear; independent dry-land/inland-water masks nearest-neighbour; target lake H nearest corrected source level, ocean H=0; canonical 2048x1024 cell-centred power-of-two lattice',
        'quantization': {'EGM2008_height_m': 1, 'geoid_m': 0.01, 'rounding': 'halfway away from zero'},
        'height_contract': 'surface ellipsoid h = H + N; ocean H=0, inland-water H=corrected lake level; below-sea-level dry land retained; coast bilinear blend',
        'polar_policy': 'continuous interpolation from nearest row to longitude-independent row mean at each pole',
        'native_detail': 'stride-10 from NOAA 60 arcsecond source (10 arcminutes), then resampled to 10.546875 arcminutes (~19.5 km at equator); not high-resolution terrain',
        'prepared_sha256': {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in names},
        'source_preparation': terrain, 'feature_preserving_pooling': pooling,
        'license': 'NOAA ETOPO2022 CC0-1.0; NGA-derived geoid public domain; Natural Earth public domain; modified Copernicus DEM GLO-90 under its free-use license with required attribution/redistributed license text (see ATTRIBUTION.md and docs/data/copernicus-glo90-license.pdf)',
        'reproduce': 'scripts/prepare-global-terrain.py then scripts/pack-global-terrain.py, or flightsim-globalgen on equivalent raw arrays',
    }
    atomic_write(provenance_path, (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode())
    print(json.dumps({key: manifest[key] for key in ('bytes', 'sha256', 'fingerprint_fnv1a64')}))

if __name__ == '__main__':
    main()
