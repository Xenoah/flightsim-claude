#!/usr/bin/env python3
"""Prepare the reproducible, coarse global fallback inputs, entirely offline by default.

The optional --download flag fetches five public, no-key data files (about 26 MB).
This script never installs software. Build-time Python dependencies: numpy, pyshp,
shapely. These are not runtime simulator dependencies. See docs/data/global-sources.md.

Outputs are latitude-descending, longitude-periodic 1080 x 2160 arrays. Orthometric
height and geoid undulation remain separate: WGS84 ellipsoidal h = H + N. The
independent Natural Earth land mask must not be replaced with height >= 0.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import struct
import tempfile
from urllib.request import Request, urlopen
import zipfile

HEIGHT = 1080
WIDTH = 2160
BASE = "https://www.ngdc.noaa.gov/thredds/dodsC/global/ETOPO2022/60s/"
QUERY = ".dods?z[0:10:10799][0:10:21599]"
SOURCES = {
    "etopo_surface_10min.dods": {
        "url": BASE
        + "60s_surface_elev_netcdf/ETOPO_2022_v1_60s_N90W180_surface.nc"
        + QUERY,
        "bytes": 9357398,
        "sha256": "575c7a0c84f18b41430a723f26475fa41b1d787c94fa641f0d0705291e3991fd",
    },
    "etopo_geoid_10min.dods": {
        "url": BASE
        + "60s_geoid_netcdf/ETOPO_2022_v1_60s_N90W180_geoid.nc"
        + QUERY,
        "bytes": 9357389,
        "sha256": "0bdb4e88ed0d65c08052d2ae62bd981353333859c24a80dfd57c088efe45a600",
    },
    "natural_earth_land_10m.zip": {
        "url": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_land.zip",
        "bytes": 3269070,
        "sha256": "e547d749445eaa0964aba76738090ec88f5e63c4585122170f98c67a7ea922dc",
    },
    "natural_earth_lakes_10m.zip": {
        "url": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_lakes.zip",
        "bytes": 2349685,
        "sha256": "0803a06f9c3cb4671d89b68c48b142aad9366ba40f665245e12a913fbc61722a",
    },
    "copernicus_90m_tilelist.txt": {
        "url": "https://copernicus-dem-90m.s3.amazonaws.com/tileList.txt",
        "bytes": 1111950,
        "sha256": "e5a5efe088e70506bc1007d22006bdcb09b0ec03177b62f9652363c13f49ed97",
    },
}
EXPECTED_ARRAY_SHA256 = {
    "elevation.f32le": "1d651c98645ce9a0604f912754a5720c3b5a4d406706172554be98903dd98b65",
    "geoid.f32le": "d7dc76e9a96a42e0de9668665f4b7c9ce66196131c594139d32266b8c64ef193",
    "land.u8": "b70a947473ab876287d6d237f3b0714904929af61f0483d54e5d6f732628399d",
}
CORRECTION_MANIFEST = Path(__file__).resolve().parents[1] / "docs/data/global-surface-corrections.json"
CORRECTION_MANIFEST_SHA256 = "0c6a217df2f0987cbdbbfe0e8c0347eea77ea5fc259516a0f7246d955c294cab"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(data)
        handle.flush()
    os.replace(temporary, path)


def source_bytes(name: str, directory: Path, download: bool) -> bytes:
    spec = SOURCES[name]
    path = directory / name
    if path.exists():
        if path.stat().st_size != spec["bytes"]:
            raise ValueError(f"{path}: source size changed; inspect before replacing")
        data = path.read_bytes()
    elif download:
        request = Request(spec["url"], headers={"User-Agent": "flightsim-global-data/1"})
        with urlopen(request, timeout=120) as response:
            # A faulty endpoint must not silently download the full ~933 MB grid.
            data = response.read(int(spec["bytes"]) + 1)
        if len(data) != spec["bytes"]:
            raise ValueError(f"{name}: unexpected response size {len(data)}")
        if sha256(data) != spec["sha256"]:
            raise ValueError(f"{name}: source checksum changed; review provenance")
        atomic_write(path, data)
    else:
        raise FileNotFoundError(f"Missing {path}; provide it or explicitly pass --download")
    if sha256(data) != spec["sha256"]:
        raise ValueError(f"{path}: source checksum does not match the reviewed dataset")
    return data


def read_dap_grid(data: bytes, np):
    """Read this exact DAP2 Grid's XDR float32 array and float64 coordinate maps.

    Numeric arrays have two equal big-endian uint32 count fields, followed by
    big-endian elements. This intentionally is not a general DAP implementation.
    The complete source hash, declarations, counts, extent and length are checked.
    """
    header, payload = data.split(b"\nData:\n", 1)
    if not re.search(rb"Float32 z\[lat = 1080\]\[lon = 2160\]", header):
        raise ValueError("Unexpected DAP2 elevation grid declaration")
    if not re.search(rb"Float64 lat\[lat = 1080\]", header):
        raise ValueError("Unexpected DAP2 latitude declaration")
    if not re.search(rb"Float64 lon\[lon = 2160\]", header):
        raise ValueError("Unexpected DAP2 longitude declaration")
    offset = 0

    def array(count, dtype):
        nonlocal offset
        if len(payload) < offset + 8:
            raise ValueError("Truncated DAP2 array counts")
        lengths = struct.unpack_from(">II", payload, offset)
        if lengths != (count, count):
            raise ValueError(f"Unexpected DAP2 counts {lengths}; expected {count}")
        offset += 8
        size = count * np.dtype(dtype).itemsize
        if len(payload) < offset + size:
            raise ValueError("Truncated DAP2 numeric array")
        result = np.frombuffer(payload, dtype=dtype, count=count, offset=offset).copy()
        offset += size
        return result

    values = array(HEIGHT * WIDTH, ">f4").reshape(HEIGHT, WIDTH)
    latitude = array(HEIGHT, ">f8")
    longitude = array(WIDTH, ">f8")
    if offset != len(payload):
        raise ValueError("Unexpected trailing DAP2 bytes")
    if not np.isfinite(values).all():
        raise ValueError("Non-finite source values")
    expected_lat = -90.0 + 1 / 120 + np.arange(HEIGHT) / 6
    expected_lon = -180.0 + 1 / 120 + np.arange(WIDTH) / 6
    if not np.allclose(latitude, expected_lat, rtol=0, atol=1e-10):
        raise ValueError("Unexpected ETOPO latitude registration")
    if not np.allclose(longitude, expected_lon, rtol=0, atol=1e-10):
        raise ValueError("Unexpected ETOPO longitude registration")
    return values, latitude, longitude


def inland_water_ids(lake_bytes, land_geometry, latitude, longitude, np, shapefile, shapely, shape):
    """Independent NE lake polygons plus its enclosed Caspian Sea land-ring.

    Ordinal IDs are reproducibility keys, not global lake identifiers. Natural
    Earth's one enclosed land-ring is inland water; it must not become ocean.
    """
    with zipfile.ZipFile(io.BytesIO(lake_bytes)) as archive:
        reader = shapefile.Reader(
            shp=io.BytesIO(archive.read("ne_10m_lakes.shp")),
            shx=io.BytesIO(archive.read("ne_10m_lakes.shx")),
            dbf=io.BytesIO(archive.read("ne_10m_lakes.dbf")),
        )
        polygons = [shape(item.__geo_interface__) for item in reader.shapes()]
    for polygon in land_geometry.geoms:
        # Independent land polygons can contain islands inside an enclosed sea.
        # Subtract them rather than turning the entire outer ring into water.
        polygons.extend(shapely.geometry.Polygon(ring).difference(land_geometry) for ring in polygon.interiors)
    ids = np.zeros((len(latitude), len(longitude)), dtype=np.int16)
    lon180 = (longitude + 180) % 360 - 180
    for identifier, geometry in enumerate(polygons, 1):
        left, bottom, right, top = geometry.bounds
        rows = np.flatnonzero((latitude >= bottom) & (latitude <= top))
        columns = np.flatnonzero((lon180 >= left) & (lon180 <= right))
        if len(rows) == 0 or len(columns) == 0:
            continue
        inside = shapely.contains_xy(geometry, lon180[columns][None, :], latitude[rows][:, None])
        yy, xx = np.where(inside)
        ids[rows[yy], columns[xx]] = identifier
    return ids, polygons


def load_corrections():
    if not CORRECTION_MANIFEST.exists() or CORRECTION_MANIFEST.stat().st_size > 16 * 1024 * 1024:
        raise ValueError("Missing or oversized reviewed surface-correction manifest")
    data = CORRECTION_MANIFEST.read_bytes()
    if sha256(data) != CORRECTION_MANIFEST_SHA256:
        raise ValueError("Surface-correction manifest hash differs from reviewed complete acquisition")
    manifest = json.loads(data)
    if manifest.get("status") != "COMPLETE" or manifest.get("errors"):
        raise ValueError("Refusing incomplete surface-correction acquisition")
    if manifest.get("schema") != "flightsim-global-surface-corrections-v1":
        raise ValueError("Unknown surface-correction schema")
    return manifest


def verify_copernicus_sources(manifest, cache_dir: Path, download: bool, polygons, np, shapely) -> None:
    """Optionally recheck every bounded source range and sampled pixel.

    The default rebuild uses the checksum-pinned derived manifest. This audit
    additionally decodes the provider's pinned averaged overviews with Pillow;
    it does not fetch complete 1-degree DEM files or install dependencies.
    """
    try:
        from PIL import Image
    except ImportError as error:
        raise RuntimeError("Source audit requires Pillow; no software was installed") from error
    cache_dir.mkdir(parents=True, exist_ok=True)
    budget = 0
    points_by_tile = {}
    for point in manifest["points"]:
        points_by_tile.setdefault(point["tile"], []).append(point)
    for name, tile in manifest["tiles"].items():
        if tile.get("absent_from_tile_list"):
            continue
        if not re.fullmatch(r"Copernicus_DSM_COG_30_[NS][0-9]{2}_00_[EW][0-9]{3}_00_DEM", name):
            raise ValueError("Invalid Copernicus tile identifier")
        expected_url = f"https://copernicus-dem-90m.s3.amazonaws.com/{name}/{name}.tif"
        if tile["url"] != expected_url:
            raise ValueError("Unexpected Copernicus source destination")
        count = tile["range_bytes"]
        if not isinstance(count, int) or not 0 < count <= 1024 * 1024:
            raise ValueError("Copernicus overview range exceeds the 1 MiB per-file bound")
        path = cache_dir / (name + ".overview.tif")
        if path.exists():
            if path.stat().st_size != count:
                raise ValueError(f"Invalid cached overview size: {name}")
            data = path.read_bytes()
        elif download:
            budget += count
            if budget > 512 * 1024 * 1024:
                raise ValueError("Copernicus source audit exceeds the 512 MiB acquisition bound")
            request = Request(expected_url, headers={"Range": f"bytes=0-{count - 1}"})
            with urlopen(request, timeout=120) as response:
                if response.status != 206 or not response.headers.get("Content-Range", "").startswith(
                    f"bytes 0-{count - 1}/"
                ):
                    raise ValueError("Provider did not honor the bounded HTTP Range request")
                data = response.read(count + 1)
            if len(data) != count or sha256(data) != tile["range_sha256"]:
                raise ValueError(f"Source overview changed: {name}")
            atomic_write(path, data)
        else:
            raise FileNotFoundError(f"Missing source-audit overview {path}")
        if sha256(data) != tile["range_sha256"]:
            raise ValueError(f"Source overview checksum mismatch: {name}")
        with Image.open(io.BytesIO(data)) as image:
            if list(image.size) != tile["base_size"]:
                raise ValueError("Unexpected source native dimensions")
            if list(image.tag_v2[33550]) != tile["pixel_scale"] or list(image.tag_v2[33922]) != tile["tiepoint"]:
                raise ValueError("Source georeferencing differs from the correction ledger")
            directory = image.tag_v2[34735]
            geo_keys = {directory[i]: tuple(directory[i + 1:i + 4]) for i in range(4, len(directory), 4)}
            if geo_keys.get(1025) != (0, 1, 2) or geo_keys.get(2048) != (0, 1, 4326):
                raise ValueError("Correction source must declare WGS84 geographic RasterPixelIsPoint")
            image.seek(tile["overview_index"])
            image.load()
            if list(image.size) != tile["overview_size"]:
                raise ValueError("Unexpected source overview dimensions")
            for point in points_by_tile.get(name, []):
                sx, sy, _ = tile["pixel_scale"]
                _, _, _, west, north, _ = tile["tiepoint"]
                bw, bh = tile["base_size"]
                px = (point["longitude"] - west) / sx
                py = (north - point["latitude"]) / sy
                ix = min(image.width - 1, max(0, math.floor((px + 0.5) * image.width / bw)))
                iy = min(image.height - 1, max(0, math.floor((py + 0.5) * image.height / bh)))
                if [ix, iy] != point["overview_pixel"]:
                    raise ValueError("Correction pixel does not reproduce from its geographic coordinates")
                actual = float(image.getpixel(tuple(point["overview_pixel"])))
                if actual != point["sample_h_m"]:
                    raise ValueError(f"Source pixel mismatch at {point['row']},{point['column']}")
    statistics = derive_lake_modes_from_overviews(manifest, polygons, cache_dir, np, shapely)
    if statistics != manifest["lake_modal_statistics"]:
        raise ValueError("Lake modal surface statistics do not reproduce from the source overview pixels")


def derive_lake_levels(points, polygons, shapely, np):
    """Static lake plane: median of the most-interior quarter of source samples.

    Polygon distance is a deterministic ranking in geographic degrees, not a
    claimed metric/geodesic buffer. Ties use row/column order. Keeping only the
    interior-ranked samples reduces mixed shore pixels, but small lakes may have
    a single coarse sample and remain approximate. No elevation is invented for
    a lake with no corroborating source value.
    """
    grouped = {}
    for point in points:
        if point["lake_id"] > 0 and point["sample_h_m"] is not None:
            grouped.setdefault(point["lake_id"], []).append(point)
    levels = {}
    support = {}
    for identifier, samples in grouped.items():
        geometry = polygons[identifier - 1]
        distances = shapely.distance(
            shapely.points([p["longitude"] for p in samples], [p["latitude"] for p in samples]),
            geometry.boundary,
        )
        ranked = sorted(
            zip(samples, distances),
            key=lambda pair: (-float(pair[1]), pair[0]["row"], pair[0]["column"]),
        )
        chosen = [point for point, _ in ranked[:max(1, math.ceil(len(samples) / 4))]]
        levels[identifier] = float(np.median([point["sample_h_m"] for point in chosen]))
        support[identifier] = [[point["row"], point["column"]] for point in chosen]
    return levels, support


def derive_lake_modes_from_overviews(manifest, polygons, cache_dir: Path, np, shapely):
    """Select the dominant exact flat DEM value within independent lake geometry.

    Copernicus explicitly hydro-flattens lakes. Pure-water averaged pixels retain
    the repeated flat value, while mixed shoreline pixels usually differ. The
    independent polygon is inset by half the overview-pixel diagonal to reduce
    mixing; a subpixel polygon uses its uninset geometry. Only already acquired
    tiles touched by a source lake node participate. Tied modes choose the lower
    value deterministically. A flat value must have at least eight supporting
    pixels, 10% of the pixels and twice the runner-up support. Otherwise the
    in-polygon pixel median is an explicitly uncertain modeled level, not a
    conveniently chosen minimum/mode. A lake with no overview centre inside the
    polygon uses its interior-ranked coarse samples and is flagged as weak.
    """
    from collections import Counter
    from PIL import Image

    by_tile = {}
    all_ids = set()
    for point in manifest["points"]:
        identifier = point["lake_id"]
        if identifier > 0:
            all_ids.add(identifier)
            by_tile.setdefault(point["tile"], set()).add(identifier)
    histograms = {identifier: Counter() for identifier in all_ids}
    tile_counts = {identifier: 0 for identifier in all_ids}
    fallback_levels, fallback_support = derive_lake_levels(manifest["points"], polygons, shapely, np)
    for name in sorted(by_tile):
        tile = manifest["tiles"][name]
        if tile.get("absent_from_tile_list"):
            continue
        data = (cache_dir / (name + ".overview.tif")).read_bytes()
        if len(data) != tile["range_bytes"] or sha256(data) != tile["range_sha256"]:
            raise ValueError("Lake statistics encountered an unverified source overview")
        with Image.open(io.BytesIO(data)) as image:
            image.seek(tile["overview_index"])
            values = np.asarray(image, dtype=np.float32)
        height, width = values.shape
        bw, bh = tile["base_size"]
        sx, sy, _ = tile["pixel_scale"]
        _, _, _, west, north, _ = tile["tiepoint"]
        dx, dy = sx * bw / width, sy * bh / height
        xs = west + ((np.arange(width) + 0.5) * bw / width - 0.5) * sx
        ys = north - ((np.arange(height) + 0.5) * bh / height - 0.5) * sy
        inset_distance = math.hypot(dx, dy) / 2
        for identifier in sorted(by_tile[name]):
            polygon = polygons[identifier - 1]
            inset = polygon.buffer(-inset_distance)
            if inset.is_empty:
                inset = polygon
            left, bottom, right, top = inset.bounds
            rows = np.flatnonzero((ys >= bottom) & (ys <= top))
            columns = np.flatnonzero((xs >= left) & (xs <= right))
            if len(rows) == 0 or len(columns) == 0:
                continue
            inside = shapely.contains_xy(inset, xs[columns][None, :], ys[rows][:, None])
            selected = values[np.ix_(rows, columns)][inside]
            if selected.size == 0:
                continue
            if not np.isfinite(selected).all() or np.min(selected) <= -500 or np.max(selected) >= 10000:
                raise ValueError("Invalid source pixel inside a mapped lake")
            unique, counts = np.unique(selected, return_counts=True)
            histograms[identifier].update({float(value): int(count) for value, count in zip(unique, counts)})
            tile_counts[identifier] += 1
    statistics = {}
    for identifier in sorted(all_ids):
        histogram = histograms[identifier]
        if not histogram:
            if identifier not in fallback_levels:
                raise ValueError("Lake has neither overview-pixel nor coarse-source support")
            statistics[str(identifier)] = {
                "surface_h_m": fallback_levels[identifier],
                "method": "subpixel_polygon_interior_coarse_sample_fallback",
                "source_pixel_count": 0,
                "modal_pixel_count": 0,
                "contributing_tiles": 0,
                "weak_support": True,
                "coarse_source_nodes": fallback_support[identifier],
            }
            continue
        ranked = sorted(histogram.items(), key=lambda pair: (-pair[1], pair[0]))
        mode, count = ranked[0]
        total = sum(histogram.values())
        ordered = sorted(histogram.items())
        cumulative = np.cumsum([count for _, count in ordered])

        def quantile(fraction):
            rank = (total - 1) * fraction
            lower, upper = math.floor(rank), math.ceil(rank)
            a = ordered[int(np.searchsorted(cumulative, lower + 1, side="left"))][0]
            b = ordered[int(np.searchsorted(cumulative, upper + 1, side="left"))][0]
            return float(a + (b - a) * (rank - lower))

        runner_up = ranked[1][1] if len(ranked) > 1 else 0
        isolated_flat = count >= 8 and count >= 0.1 * total and count >= 2 * runner_up
        statistics[str(identifier)] = {
            "surface_h_m": mode if isolated_flat else quantile(0.5),
            "method": "dominant_flat_value_of_inset_lake_overview_pixels" if isolated_flat
                else "uncertain_median_of_inset_lake_overview_pixels",
            "source_pixel_count": total,
            "modal_value_m": mode,
            "modal_pixel_count": count,
            "runner_up_pixel_count": runner_up,
            "contributing_tiles": tile_counts[identifier],
            "pixel_range_m": [min(histogram), max(histogram)],
            "pixel_quartiles_m": [quantile(0.25), quantile(0.5), quantile(0.75)],
            "flat_surface_isolated": isolated_flat,
            "weak_support": not isolated_flat,
        }
    return statistics


def apply_corrections(elevation, undulation, land, inland_ids, polygons, manifest, np, shapely):
    expected = set(map(tuple, np.argwhere((land & (elevation < 0)) | (inland_ids > 0))))
    statistics = manifest["lake_modal_statistics"]
    levels = {int(identifier): entry["surface_h_m"] for identifier, entry in statistics.items()}
    if set(levels) != set(int(value) for value in np.unique(inland_ids) if value > 0):
        raise ValueError("A classified lake lacks independent surface-height support")
    lake_ids = [lake["id"] for lake in manifest["lakes"]]
    if set(lake_ids) != set(levels) or len(lake_ids) != len(set(lake_ids)):
        raise ValueError("Lake-level provenance is incomplete or contains duplicate identities")
    for lake in manifest["lakes"]:
        identifier = lake["id"]
        if levels[identifier] != lake["surface_h_m"] or lake["modal_statistics"] != statistics[str(identifier)]:
            raise ValueError("Lake surface level/statistic does not reproduce from the source samples")
    seen = set()
    for point in manifest["points"]:
        row, column = point["row"], point["column"]
        key = (row, column)
        if key not in expected or key in seen:
            raise ValueError("Duplicate or out-of-scope surface correction")
        seen.add(key)
        expected_lat = -90 + 1 / 120 + (HEIGHT - 1 - row) / 6
        expected_lon = (1 / 120 + column / 6 + 180) % 360 - 180
        if abs(point["latitude"] - expected_lat) > 1e-9 or abs(point["longitude"] - expected_lon) > 1e-9:
            raise ValueError("Correction geographic coordinates do not match its source grid node")
        if float(elevation[row, column]) != point["old_h_m"]:
            raise ValueError("Correction source elevation does not match the pinned ETOPO node")
        if int(inland_ids[row, column]) != point["lake_id"]:
            raise ValueError("Correction lake identity differs from independent polygon classification")
        value = point["surface_h_m"]
        if not isinstance(value, (float, int)) or not math.isfinite(value) or not -500 < value < 10000:
            raise ValueError("Invalid corrected surface height; refusing to clamp unknown terrain")
        derived = levels[point["lake_id"]] if point["lake_id"] else point["sample_h_m"]
        if value != derived:
            raise ValueError("Correction differs from its independent source value/derived lake level")
        elevation[row, column] = value
    if seen != expected:
        raise ValueError("Incomplete correction coverage; no partial physical surface is emitted")
    inland = inland_ids > 0
    dry_land = land & ~inland
    if np.any(dry_land & inland):
        raise ValueError("Dry land and inland water overlap")
    if np.any(elevation[dry_land | inland] < -500):
        raise ValueError("Unexplained deep land/lake bathymetry remains after correction")
    for name, array in {
        "elevation.f32le": elevation,
        "geoid.f32le": undulation,
        "land.u8": dry_land.astype("u1"),
        "inland_water.u8": inland.astype("u1"),
    }.items():
        if sha256(array.tobytes(order="C")) != manifest["derived_array_sha256"][name]:
            raise ValueError(f"Corrected {name} differs from the reviewed derived surface")
    return dry_land, inland


def prepare(source_dir: Path, output_dir: Path, download: bool, audit_dir: Path | None = None) -> None:
    try:
        import numpy as np
        import shapefile
        import shapely
        from shapely.geometry import shape
    except ImportError as error:
        raise RuntimeError(
            "Offline preparation needs numpy, pyshp and shapely. "
            "No packages were installed. See docs/data/global-sources.md."
        ) from error

    source_dir.mkdir(parents=True, exist_ok=True)
    sources = {name: source_bytes(name, source_dir, download) for name in SOURCES}
    height, lat, lon = read_dap_grid(sources["etopo_surface_10min.dods"], np)
    geoid, geoid_lat, geoid_lon = read_dap_grid(sources["etopo_geoid_10min.dods"], np)
    if not np.array_equal(lat, geoid_lat) or not np.array_equal(lon, geoid_lon):
        raise ValueError("Elevation and geoid source nodes do not coincide")
    if np.min(height) < -12000 or np.max(height) > 10000:
        raise ValueError("Unexpected ETOPO elevation range or nodata sentinel")
    if np.min(geoid) < -120 or np.max(geoid) > 120:
        raise ValueError("Unexpected EGM2008 geoid range or nodata sentinel")

    # Both input grids ascend south-to-north and use -180..180 longitude.
    # Reordering is lossless; the decimation happened only in the DAP stride.
    order = np.argsort(lon % 360)
    latitude = lat[::-1].astype("<f8")
    longitude = (lon[order] % 360).astype("<f8")
    elevation = height[::-1, order].astype("<f4")
    undulation = geoid[::-1, order].astype("<f4")

    with zipfile.ZipFile(io.BytesIO(sources["natural_earth_land_10m.zip"])) as archive:
        # No ZIP extraction; only three known shapefile members are read.
        reader = shapefile.Reader(
            shp=io.BytesIO(archive.read("ne_10m_land.shp")),
            shx=io.BytesIO(archive.read("ne_10m_land.shx")),
            dbf=io.BytesIO(archive.read("ne_10m_land.dbf")),
        )
        geometry = shapely.union_all([shape(item.__geo_interface__) for item in reader.shapes()])
    shapely.prepare(geometry)
    land = shapely.contains_xy(
        geometry, ((longitude + 180) % 360 - 180)[None, :], latitude[:, None]
    )
    outputs = {
        "elevation.f32le": elevation.tobytes(order="C"),
        "geoid.f32le": undulation.tobytes(order="C"),
        "land.u8": land.astype("u1").tobytes(order="C"),
    }
    for name, data in outputs.items():
        if sha256(data) != EXPECTED_ARRAY_SHA256[name]:
            raise ValueError(f"{name}: derived data differs from the reviewed reference")

    inland_ids, lake_polygons = inland_water_ids(
        sources["natural_earth_lakes_10m.zip"], geometry, latitude, longitude,
        np, shapefile, shapely, shape,
    )
    corrections = load_corrections()
    listed_tiles = set(sources["copernicus_90m_tilelist.txt"].decode("ascii").splitlines())
    if corrections["source_tile_list_sha256"] != sha256(sources["copernicus_90m_tilelist.txt"]):
        raise ValueError("Correction ledger uses a different public Copernicus tile inventory")
    for name, tile in corrections["tiles"].items():
        if bool(tile.get("absent_from_tile_list")) == (name in listed_tiles):
            raise ValueError("Correction tile availability differs from the independent source inventory")
    if audit_dir is not None:
        verify_copernicus_sources(corrections, audit_dir, download, lake_polygons, np, shapely)
    land, inland = apply_corrections(
        elevation, undulation, land, inland_ids, lake_polygons, corrections, np, shapely
    )
    outputs = {
        "elevation.f32le": elevation.tobytes(order="C"),
        "geoid.f32le": undulation.tobytes(order="C"),
        "land.u8": land.astype("u1").tobytes(order="C"),
        "inland_water.u8": inland.astype("u1").tobytes(order="C"),
    }

    npy_arrays = {
        "etopo_orthometric_north_to_south_10min.npy": elevation,
        "etopo_geoid_north_to_south_10min.npy": undulation,
        "ne_land_north_to_south_10min.npy": land,
        "inland_water.npy": inland,
        "terrain_latitude_north_to_south_10min.npy": latitude,
        "terrain_longitude_10min.npy": longitude,
    }
    for name, array in npy_arrays.items():
        stream = io.BytesIO()
        np.save(stream, array, allow_pickle=False)
        outputs[name] = stream.getvalue()
    for name, data in outputs.items():
        atomic_write(output_dir / name, data)

    manifest = {
        "schema": "flightsim-global-terrain-inputs-v2",
        "reviewed_access_date_utc": "2026-10-02",
        "sources": SOURCES,
        "surface_corrections": {
            "manifest": "docs/data/global-surface-corrections.json",
            "sha256": CORRECTION_MANIFEST_SHA256,
            "status": corrections["status"],
            "source": "Copernicus GLO-90 EGM2008 hydro-edited surface, provider averaged overviews",
            "license": "Copernicus WorldDEM-90 free and open license; required derivative notices apply",
        },
        "shape": [HEIGHT, WIDTH],
        "row_order": "north-to-south",
        "column_order": "eastward, periodic",
        "latitude_origin_deg": float(latitude[0]),
        "longitude_origin_deg": float(longitude[0]),
        "latitude_step_deg": -1 / 6,
        "longitude_step_deg": 1 / 6,
        "elevation": "float32 little-endian EGM2008 orthometric height H in metres",
        "geoid": "float32 little-endian EGM2008 undulation N in metres; h = H + N",
        "land": "uint8: 1 dry land, 0 otherwise; independent Natural Earth geometry minus inland water",
        "inland_water": "uint8: 1 inland lake/reservoir/Caspian water, 0 otherwise; never overlaps dry land",
        "method": "ETOPO2022 v1 60s ice-surface source, point decimation stride10; "
        "Natural Earth5.1.1 land and5.0.0 lake polygons tested at exact sample nodes; "
        "reviewed Copernicus surface corrections remove lake/coastal bathymetry, preserving genuine negative ground; "
        "geoid unchanged; no ellipsoidal conversion, ocean flattening or height quantization performed here",
        "limitations": [
            "Coarse 10 arc-minute samples (~18.5 km at the equator), not high-resolution terrain",
            "Peaks and small islands may be missed; coastline and DEM use different source geometry",
            "Inland water levels are static modeled levels derived from Copernicus averaged surfaces, not current lake observations",
            "Natural Earth shorelines are generalized; low-resolution Copernicus overview pixels may mix shore and water",
            "Strided grid is not symmetrically cell-centered; retain the explicit origins",
            "Neither source samples the exact poles; runtime must define a continuous pole cap",
            "NOAA data are not for navigation; this is a derived simulator asset, not an official product",
        ],
        "outputs": {
            name: {"bytes": len(data), "sha256": sha256(data)} for name, data in outputs.items()
        },
        "validation": {
            "land_nodes": int(land.sum()),
            "inland_water_nodes": int(inland.sum()),
            "below_sea_level_land_nodes": int(((elevation < 0) & land).sum()),
            "elevation_range_m": [float(elevation.min()), float(elevation.max())],
            "geoid_range_m": [float(undulation.min()), float(undulation.max())],
        },
        "preparation_packages": {
            "numpy": np.__version__, "pyshp": shapefile.__version__, "shapely": shapely.__version__
        },
    }
    atomic_write(output_dir / "terrain-preparation.json", (json.dumps(manifest, indent=2) + "\n").encode())
    print(f"Prepared {HEIGHT} x {WIDTH} globe at {output_dir}")
    print("Raw arrays and source checksums match the reviewed reference; see terrain-preparation.json")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True, help="cache of five pinned source files")
    parser.add_argument("--output-dir", type=Path, required=True, help="prepared array destination")
    parser.add_argument("--download", action="store_true", help="explicitly allow missing public data downloads")
    parser.add_argument("--verify-copernicus-sources", type=Path, metavar="CACHE_DIR", help="also audit every pinned Copernicus source overview and sample; --download permits bounded missing-range downloads")
    args = parser.parse_args()
    try:
        prepare(args.source_dir, args.output_dir, args.download, args.verify_copernicus_sources)
    except (ValueError, OSError, RuntimeError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Global terrain preparation failed: {error}\n")


if __name__ == "__main__":
    main()
