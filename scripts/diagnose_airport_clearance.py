#!/usr/bin/env python3
"""Independent numerical airport/terrain-mesh diagnostics; never app screenshots.

Uses only the checked-in FSGT asset and numpy. It reproduces the geographic 33x33
terrain grid, NW-SW-NE/NE-SW-SE indices and tile-relative f32 vertex storage, then
intersects an ellipsoid-normal ray with the actual stored triangle planes.
Coordinates here deliberately use independent WGS84 equations for QA only; this
is not a production coordinate implementation or a replacement ground sampler.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import numpy as np

A = 6378137.0
F = 1 / 298.257223563
E2 = F * (2 - F)
ROOT = Path(__file__).resolve().parents[1]


def ecef(latitude, longitude, height):
    latitude, longitude, height = np.broadcast_arrays(latitude, longitude, height)
    p, l = np.deg2rad(latitude), np.deg2rad(longitude)
    n = A / np.sqrt(1 - E2 * np.sin(p) ** 2)
    return np.stack(((n + height) * np.cos(p) * np.cos(l),
                     (n + height) * np.cos(p) * np.sin(l),
                     (n * (1 - E2) + height) * np.sin(p)), axis=-1)


def up(latitude, longitude):
    p, l = np.deg2rad(latitude), np.deg2rad(longitude)
    return np.stack((np.cos(p) * np.cos(l), np.cos(p) * np.sin(l), np.sin(p)), axis=-1)


def ray_triangle_height(origin, direction, a, b, c):
    """Both sides; return nan if ray misses triangle or plane is degenerate."""
    ab, ac = b - a, c - a
    normal = np.cross(ab, ac)
    denominator = np.sum(normal * direction, axis=-1)
    with np.errstate(divide='ignore', invalid='ignore'):
        height = np.sum(normal * (a - origin), axis=-1) / denominator
        point = origin + height[..., None] * direction
        ap = point - a
        d00 = np.sum(ab * ab, axis=-1)
        d01 = np.sum(ab * ac, axis=-1)
        d11 = np.sum(ac * ac, axis=-1)
        d20 = np.sum(ap * ab, axis=-1)
        d21 = np.sum(ap * ac, axis=-1)
        denom = d00 * d11 - d01 * d01
        v = (d11 * d20 - d01 * d21) / denom
        w = (d00 * d21 - d01 * d20) / denom
    inside = (v >= -1e-8) & (w >= -1e-8) & (v + w <= 1 + 1e-8)
    return np.where(inside & (np.abs(denominator) > 1e-15), height, np.nan)


class Atlas:
    def __init__(self, path):
        data = Path(path).read_bytes()
        assert data[:4] == b'FSGT' and struct.unpack_from('<H', data, 4)[0] == 2
        self.sha256 = hashlib.sha256(data).hexdigest()
        self.width, self.height = struct.unpack_from('<II', data, 8)
        self.lon0, self.north = struct.unpack_from('<dd', data, 32)
        self.step = 180 / self.height
        count = self.width * self.height
        mask_size = (count + 7) // 8
        assert len(data) == 64 + count * 4 + 2 * mask_size
        values = np.frombuffer(data, dtype='<i2', offset=64, count=count * 2).reshape(self.height, self.width, 2)
        mask = np.frombuffer(data, dtype='u1', offset=64 + count * 4)
        dry, wet = [np.unpackbits(part, bitorder='little')[:count].reshape(self.height, self.width).astype(bool)
                    for part in (mask[:mask_size], mask[mask_size:])]
        self.values = np.where(dry | wet, values[..., 0], 0) + values[..., 1] / 100
        self.south = self.north - (self.height - 1) * self.step
        self.poles = self.values[[0, -1]].mean(axis=1)

    def sample(self, latitude, longitude):
        latitude, longitude = np.broadcast_arrays(latitude, longitude)
        x = ((longitude - self.lon0) % 360) / self.step
        ix = np.minimum(np.floor(x).astype(int), self.width - 1)
        fx = np.clip(x - ix, 0, 1)
        y = (self.north - latitude) / self.step
        iy = np.clip(np.floor(y).astype(int), 0, self.height - 2)
        fy = np.clip(y - iy, 0, 1)
        def row(k):
            a, b = self.values[k, ix], self.values[k, (ix + 1) % self.width]
            return a + (b - a) * fx
        result = row(iy) + (row(iy + 1) - row(iy)) * fy
        north_t = np.clip((latitude - self.north) / (90 - self.north), 0, 1)
        south_t = np.clip((self.south - latitude) / (self.south + 90), 0, 1)
        result = np.where(latitude > self.north, row(0) * (1 - north_t) + self.poles[0] * north_t, result)
        return np.where(latitude < self.south, row(self.height - 1) * (1 - south_t) + self.poles[1] * south_t, result)


def mesh_height(atlas, latitude, longitude, level, quantized=True):
    """Find the actual rendered plane under each coordinate, excluding skirts.

    Search the nominal grid cell and its eight neighbors because projected ECEF
    edges are not straight latitude/longitude lines. Quantization is per tile,
    with its exact center origin and the generated DEM's f32 sample values.
    """
    latitude, longitude = np.broadcast_arrays(latitude, longitude)
    step = 180 / (2 ** level * 32)
    gx = np.floor(((longitude + 180) % 360) / step).astype(int)
    gy = np.minimum(np.floor((90 - latitude) / step).astype(int), 2 ** level * 32 - 1)
    origin = ecef(latitude, longitude, 0)
    direction = up(latitude, longitude)
    result = np.full(latitude.shape, np.nan)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            cx = (gx + dx) % (2 ** (level + 1) * 32)
            cy = np.clip(gy + dy, 0, 2 ** level * 32 - 1)
            tx, ty = cx // 32, cy // 32
            tile_lon = -180 + (tx * 32 + 16) * step
            tile_lat = 90 - (ty * 32 + 16) * step
            tile_origin = ecef(tile_lat, tile_lon, atlas.sample(tile_lat, tile_lon).astype('f4').astype('f8'))
            corners = []
            for ox, oy in ((0, 0), (1, 0), (0, 1), (1, 1)):
                lon = -180 + (cx + ox) * step
                lat = 90 - (cy + oy) * step
                elevation = atlas.sample(lat, lon).astype('f4').astype('f8')
                p = ecef(lat, lon, elevation)
                if quantized:
                    p = tile_origin + (p - tile_origin).astype('f4').astype('f8')
                corners.append(p)
            for ia, ib, ic in ((0, 2, 1), (1, 2, 3)):
                hit = ray_triangle_height(origin, direction, corners[ia], corners[ib], corners[ic])
                result = np.fmax(result, hit)
    return result


def offset(latitude, longitude, north_m, east_m):
    p = np.deg2rad(latitude)
    w = 1 - E2 * np.sin(p) ** 2
    meridian = A * (1 - E2) / w ** 1.5
    prime = A / np.sqrt(w)
    return latitude + np.rad2deg(north_m / meridian), longitude + np.rad2deg(east_m / (prime * np.cos(p)))



def synthetic_runway_probes(atlas):
    """Reproduce existing synthetic Haneda pavement/top geometry, not OSM data.

    Each triangle is probed at its geographic centroid, using its f32-relative
    emitted vertices and a ray/plane intersection. This catches face interiors,
    but is not an exhaustive spatial clearance proof or rendered scene test.
    """
    threshold_lat, threshold_lon, heading, length, width = 35.548, 139.775, 50., 2500., 45.
    rectangles = []
    along = 56.
    while along + 30 < length - 56:
        rectangles.append((along, along + 30, -.45, .45))
        along += 50
    for far_end in (False, True):
        near = length - 6 - 30 if far_end else 6
        for key in range(4):
            off = (key + .5) * (width * .5 - 2) / 4 + 1.5
            for side in (-1, 1):
                rectangles.append((near, near + 30, side * off - .9, side * off + .9))
    def grid(span, boundaries):
        intervals = int(np.ceil(span / 10))
        values = sorted(list(np.linspace(0, span, intervals + 1)) + [v for v in boundaries if 0 < v < span])
        output = []
        for value in values:
            if not output or abs(value - output[-1]) >= 1e-6:
                output.append(value)
        return np.array(output)
    axis_a = grid(length, [v for r in rectangles for v in r[:2]])
    axis_x = grid(width, [v + width / 2 for r in rectangles for v in r[2:]]) - width / 2
    a, x = np.meshgrid(axis_a[:-1], axis_x[:-1], indexing='ij')
    af, xf = np.meshgrid(axis_a[1:], axis_x[1:], indexing='ij')
    quads = np.stack([np.stack(p, axis=-1) for p in ((a,x),(a,xf),(af,xf),(af,x))], axis=-2).reshape(-1,4,2)
    intervals = int(np.ceil(length / 60))
    lights = [(length * i / intervals, side * (width/2 + 1.5)) for i in range(intervals + 1) for side in (-1,1)]
    lights += [(a, side * (i + .5) / 5 * width/2) for a in (0,length) for i in range(5) for side in (-1,1)]
    fixtures = np.array(lights)[:,None,:] + np.array([[-.8,-.8],[-.8,.8],[.8,.8],[.8,-.8]])
    sine, cosine = np.sin(np.deg2rad(heading)), np.cos(np.deg2rad(heading))
    def position(a, x):
        return offset(threshold_lat, threshold_lon, a*cosine-x*sine, a*sine+x*cosine)
    def probes(quads, lift, origin_lift):
        lat, lon = position(quads[...,0], quads[...,1])
        mid_lat, mid_lon = position(length/2, 0.)
        origin = ecef(mid_lat, mid_lon, atlas.sample(mid_lat, mid_lon) + origin_lift)
        vertices = ecef(lat, lon, atlas.sample(lat, lon) + lift)
        vertices = origin + (vertices-origin).astype('f4').astype('f8')
        triangles = np.array([[0,1,2],[0,2,3]])
        corners = vertices[:,triangles].reshape(-1,3,3)
        p = lat[:,triangles].mean(axis=-1).ravel()
        l = lon[:,triangles].mean(axis=-1).ravel()
        heights = ray_triangle_height(ecef(p,l,0),up(p,l),corners[:,0],corners[:,1],corners[:,2])
        assert np.isfinite(heights).all(), 'airport triangle centroid missed its own face'
        return p,l,heights
    return {'pavement_triangle_centroids': probes(quads,.08,.08),
            'light_top_triangle_centroids': probes(fixtures,.47,.12)}, len(lights)


def statistics(delta):
    finite = delta[np.isfinite(delta)]
    assert finite.size, 'all terrain rays missed'
    return {'samples': int(delta.size), 'ray_misses': int(delta.size - finite.size),
            'min_terrain_minus_sampler_m': float(finite.min()),
            'max_terrain_minus_sampler_m': float(finite.max()),
            'p99_absolute_error_m': float(np.quantile(np.abs(finite), .99)),
            'fraction_above_pavement_lift_0_08m': float(np.mean(finite > .08)),
            'fraction_above_light_top_0_47m': float(np.mean(finite > .47))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--atlas', type=Path, default=ROOT / 'crates/flightsim-world/data/global-terrain.fsgt')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.output and args.output.resolve() == args.atlas.resolve():
        parser.error('diagnostic output must not replace the atlas input')
    atlas = Atlas(args.atlas)
    levels = (6, 8, 10, 11, 12, 13)
    report = {'kind': 'independent CPU data diagnostic; not simulator/rendered screenshot',
              'atlas_sha256': atlas.sha256, 'atlas_shape': [atlas.height, atlas.width],
              'convention': 'positive error means terrain plane lies ABOVE sampled physical ground',
              'limits': 'Samples test possible uniform LODs, not selected live cuts, rendered frames, light visibility, or regional DEM. No regional fsdem fixture is available in this checkout.',
              'regions': {}}
    # Coordinate-centered, east/west 3km x 55m diagnostic strips. They are NOT
    # surveyed airport layouts; include alpine and dateline terrain stress cases.
    for name, lat, lon in [('Haneda vicinity', 35.55, 139.78), ('Kathmandu vicinity', 27.70, 85.36),
                           ('Lukla vicinity', 27.69, 86.73), ('Quito vicinity', -.13, -78.36),
                           ('Alps stress', 46.5, 9.5), ('Dateline stress', 65, 179.99)]:
        east, north = np.meshgrid(np.linspace(-1500, 1500, 601), np.linspace(-27.5, 27.5, 12))
        p, l = offset(lat, lon, north.ravel(), east.ravel())
        ground = atlas.sample(p, l)
        report['regions'][name] = {str(level): statistics(mesh_height(atlas, p, l, level) - ground) for level in levels}
    airport, light_count = synthetic_runway_probes(atlas)
    report['synthetic_haneda_runway'] = {
        'source': 'Runway::synthetic: 35.548N,139.775E; heading 050; length 2500m; width 45m',
        'light_count': light_count, 'airport_geometry_source': 'runway.rs and runway_lights.rs',
        'samples': {},
    }
    for name, (p, l, airport_height) in airport.items():
        report['synthetic_haneda_runway']['samples'][name] = {}
        for level in levels:
            clearance = airport_height - mesh_height(atlas, p, l, level)
            assert np.isfinite(clearance).all()
            report['synthetic_haneda_runway']['samples'][name][str(level)] = {
                'triangle_centroid_count': int(clearance.size),
                'minimum_airport_minus_terrain_m': float(clearance.min()),
                'maximum_airport_minus_terrain_m': float(clearance.max()),
                'centroids_below_terrain': int(np.count_nonzero(clearance < 0)),
            }
    # Deterministic area-distributed probes; intentionally not a worst-case bound.
    rng = np.random.default_rng(20261002)
    p = np.rad2deg(np.arcsin(rng.uniform(-.99999, .99999, 25000)))
    l = rng.uniform(-180, 180, 25000)
    ground = atlas.sample(p, l)
    report['global_random_probes'] = {}
    for level in levels:
        delta = mesh_height(atlas, p, l, level) - ground
        stats = statistics(delta)
        worst = int(np.nanargmax(delta))
        stats['highest_positive_error_probe_lat_lon'] = [float(p[worst]), float(l[worst])]
        report['global_random_probes'][str(level)] = stats
    result = json.dumps(report, indent=2, allow_nan=False) + '\n'
    if args.output:
        args.output.write_text(result)
    print(result)


if __name__ == '__main__':
    main()
