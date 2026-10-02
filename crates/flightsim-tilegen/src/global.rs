//! Offline encoder for the compact complete-planet fallback atlas.
//!
//! Input arrays are already georeferenced, aligned NOAA ETOPO2022 EGM2008
//! orthometric heights, matching geoid undulation, and an independent geographic
//! land mask. The preparation script records source URLs, hashes and coordinate
//! transformations. Runtime world code reads only the validated FSGT product.

use flightsim_core::{Degrees, Meters};
use flightsim_world::global::{GLOBAL_HEADER_BYTES, GlobalTerrain, global_atlas_fingerprint};
use std::io::{self, Read, Write};
use std::path::Path;

/// Explicit geometry at this external-data boundary. Rows run north to south,
/// columns eastwards and wrap. The origin is a sample position, not a pixel edge.
#[derive(Debug, Clone, Copy)]
pub struct GlobalAtlasGeometry {
    pub width: u32,
    pub height: u32,
    pub longitude_origin: Degrees,
    pub north_latitude: Degrees,
}

impl GlobalAtlasGeometry {
    /// Validate dimensions and periodic global grid coverage before allocation.
    ///
    /// # Errors
    /// Unsupported dimensions, nonfinite origin or incomplete polar coverage.
    pub fn validate(self) -> io::Result<usize> {
        if !(2..=2_048).contains(&self.height) || self.width != 2 * self.height {
            return Err(invalid(
                "global atlas requires width=2*height and height 2..=2048",
            ));
        }
        let lon = self.longitude_origin.get();
        let north = self.north_latitude.get();
        if !lon.is_finite()
            || !(-180.0..180.0).contains(&lon)
            || !north.is_finite()
            || north >= 90.0
            || north <= 90.0 - 180.0 / f64::from(self.height)
        {
            return Err(invalid("invalid global atlas sample origin"));
        }
        Ok(self.width as usize * self.height as usize)
    }
}

/// Power-of-two sample lattice aligned with the geographic tile scheme.
/// Its sample breakpoints coincide with 33-point DEM grids from level 6 onward.
#[must_use]
pub const fn canonical_global_geometry() -> GlobalAtlasGeometry {
    GlobalAtlasGeometry {
        width: 2_048,
        height: 1_024,
        longitude_origin: Degrees(-180.0 + 180.0 / 2_048.0),
        north_latitude: Degrees(90.0 - 90.0 / 1_024.0),
    }
}

/// The raw orthometric height is retained even below the sea. The separate mask
/// determines ocean water when sampling, never the sign of this height.
#[derive(Debug, Clone, Copy)]
pub struct GlobalAtlasNode {
    pub orthometric_height: Meters,
    pub geoid_undulation: Meters,
    pub is_land: bool,
    pub is_inland_water: bool,
}

/// Encode FSGT v2: 64-byte header, interleaved signed i16 orthometric metres /
/// signed i16 geoid centimetres, then disjoint low-bit-order dry-land and
/// inland-water masks. Rounding is
/// halfway away from zero, with at most 0.5 m + 0.005 m quantization error in h.
///
/// # Errors
/// Incomplete grids, nonfinite/unphysical heights or incompatible geometry fail.
pub fn encode_global_atlas(
    geometry: GlobalAtlasGeometry,
    nodes: &[GlobalAtlasNode],
) -> io::Result<Vec<u8>> {
    let count = geometry.validate()?;
    if nodes.len() != count {
        return Err(invalid("global atlas node count does not match geometry"));
    }
    // Check every value before allocating or rounding; nodata never becomes sea.
    for node in nodes {
        let height = node.orthometric_height.get();
        let geoid = node.geoid_undulation.get();
        if !height.is_finite()
            || !(-12_000.0..=12_000.0).contains(&height)
            || !geoid.is_finite()
            || !(-200.0..=200.0).contains(&geoid)
            || (node.is_land && node.is_inland_water)
            || ((node.is_land || node.is_inland_water) && height < -500.0)
        {
            return Err(invalid(
                "global atlas node is nonfinite or outside physical bounds",
            ));
        }
    }
    let mask_offset = GLOBAL_HEADER_BYTES + count * 4;
    let mask_bytes = count.div_ceil(8);
    let mut bytes = vec![0_u8; mask_offset + mask_bytes * 2];
    bytes[..4].copy_from_slice(b"FSGT");
    bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&geometry.width.to_le_bytes());
    bytes[12..16].copy_from_slice(&geometry.height.to_le_bytes());
    for (offset, value) in [(16, 4326_u16), (18, 3855), (20, 4979), (22, 1)] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[32..40].copy_from_slice(&geometry.longitude_origin.get().to_le_bytes());
    bytes[40..48].copy_from_slice(&geometry.north_latitude.get().to_le_bytes());
    for (index, node) in nodes.iter().enumerate() {
        // Both values were checked against strict bounds before rounding.
        #[allow(clippy::cast_possible_truncation)]
        let height = node.orthometric_height.get().round() as i16;
        #[allow(clippy::cast_possible_truncation)]
        let geoid = (node.geoid_undulation.get() * 100.0).round() as i16;
        let offset = GLOBAL_HEADER_BYTES + index * 4;
        bytes[offset..offset + 2].copy_from_slice(&height.to_le_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&geoid.to_le_bytes());
        if node.is_land {
            bytes[mask_offset + index / 8] |= 1 << (index % 8);
        }
        if node.is_inland_water {
            bytes[mask_offset + mask_bytes + index / 8] |= 1 << (index % 8);
        }
    }
    let fingerprint = global_atlas_fingerprint(&bytes);
    bytes[24..32].copy_from_slice(&fingerprint.to_le_bytes());
    // Validate exactly the format the runtime will consume before it is saved.
    GlobalTerrain::from_bytes(bytes.clone()).map_err(|error| invalid(error.to_string()))?;
    Ok(bytes)
}

/// Resample an already surface-corrected source grid onto another complete
/// periodic lattice. H and N use bilinear interpolation, with source ocean H=0.
/// Independent dry-land/inland-water masks use nearest-neighbour sampling,
/// never height sign. Target lake nodes retain the nearest corrected lake level
/// instead of mixing neighbouring land elevations into a water plane.
/// Polar caps use the same row-mean policy as the runtime. Target ocean nodes
/// explicitly store H=0. This is a coarse visual/physical resampling, not new detail.
///
/// # Errors
/// Invalid geometry, incomplete data, nonfinite or unphysical source samples.
pub fn resample_global_atlas(
    source_geometry: GlobalAtlasGeometry,
    source: &[GlobalAtlasNode],
    target_geometry: GlobalAtlasGeometry,
) -> io::Result<Vec<GlobalAtlasNode>> {
    let count = source_geometry.validate()?;
    let target_count = target_geometry.validate()?;
    if source.len() != count {
        return Err(invalid(
            "source node count does not match resampling geometry",
        ));
    }
    for node in source {
        if !node.orthometric_height.get().is_finite()
            || !(-12_000.0..=12_000.0).contains(&node.orthometric_height.get())
            || !node.geoid_undulation.get().is_finite()
            || !(-200.0..=200.0).contains(&node.geoid_undulation.get())
            || (node.is_land && node.is_inland_water)
            || ((node.is_land || node.is_inland_water) && node.orthometric_height.get() < -500.0)
        {
            return Err(invalid("resampling source contains invalid heights"));
        }
    }
    let width = source_geometry.width;
    let height = source_geometry.height;
    let source_step = 180.0 / f64::from(height);
    let source_north = source_geometry.north_latitude.get();
    let source_south = source_north - f64::from(height - 1) * source_step;
    let value = |column: u32, row: u32| {
        let node = source[row as usize * width as usize + column as usize];
        [
            if node.is_land || node.is_inland_water {
                node.orthometric_height.get()
            } else {
                0.0
            },
            node.geoid_undulation.get(),
        ]
    };
    let blend = |a: [f64; 2], b: [f64; 2], fraction: f64| {
        [
            a[0] + (b[0] - a[0]) * fraction,
            a[1] + (b[1] - a[1]) * fraction,
        ]
    };
    let mean = |row: u32| {
        let mut result = [0.0, 0.0];
        for column in 0..width {
            let item = value(column, row);
            result[0] += item[0];
            result[1] += item[1];
        }
        [result[0] / f64::from(width), result[1] / f64::from(width)]
    };
    let north_pole = mean(0);
    let south_pole = mean(height - 1);
    let target_step = 180.0 / f64::from(target_geometry.height);
    let mut target = Vec::with_capacity(target_count);
    for row in 0..target_geometry.height {
        let latitude = target_geometry.north_latitude.get() - f64::from(row) * target_step;
        let y = (source_north - latitude) / source_step;
        #[allow(clippy::cast_possible_truncation)]
        let nearest_row = (y + 0.5).floor().clamp(0.0, f64::from(height - 1)) as u32;
        for column in 0..target_geometry.width {
            let longitude =
                target_geometry.longitude_origin.get() + f64::from(column) * target_step;
            let x = (longitude - source_geometry.longitude_origin.get()).rem_euclid(360.0)
                / source_step;
            #[allow(clippy::cast_possible_truncation)]
            let left = (x.floor() as u32).min(width - 1);
            #[allow(clippy::cast_possible_truncation)]
            let nearest_column = (x + 0.5).floor() as u32 % width;
            let fx = (x - f64::from(left)).clamp(0.0, 1.0);
            let line = |row| blend(value(left, row), value((left + 1) % width, row), fx);
            let interpolated = if latitude > source_north {
                blend(
                    line(0),
                    north_pole,
                    ((latitude - source_north) / (90.0 - source_north)).clamp(0.0, 1.0),
                )
            } else if latitude < source_south {
                blend(
                    line(height - 1),
                    south_pole,
                    ((source_south - latitude) / (source_south + 90.0)).clamp(0.0, 1.0),
                )
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let top = (y.floor() as u32).min(height - 2);
                blend(
                    line(top),
                    line(top + 1),
                    (y - f64::from(top)).clamp(0.0, 1.0),
                )
            };
            let nearest = source[nearest_row as usize * width as usize + nearest_column as usize];
            target.push(GlobalAtlasNode {
                // A lake's corrected flat level must not acquire neighbouring
                // mountain heights through a mixed shore interpolation cell.
                orthometric_height: Meters(if nearest.is_inland_water {
                    nearest.orthometric_height.get()
                } else if nearest.is_land {
                    interpolated[0]
                } else {
                    0.0
                }),
                geoid_undulation: Meters(interpolated[1]),
                is_land: nearest.is_land,
                is_inland_water: nearest.is_inland_water,
            });
        }
    }
    // Preserve narrow, independently classified features that a point-only
    // resample can lose. Source points map to a node within half a cell per
    // axis; interpolated feature footprints can extend farther. This trades
    // local shape accuracy for preserving real lake levels and negative ground.
    let mut lakes = std::collections::HashMap::<usize, (f64, f64)>::new();
    let mut negative_ground = std::collections::HashMap::<usize, f64>::new();
    for (index, node) in source.iter().enumerate() {
        if !(node.is_inland_water || node.is_land && node.orthometric_height.get() < 0.0) {
            continue;
        }
        let row = u32::try_from(index / width as usize).expect("bounded source row");
        let column = u32::try_from(index % width as usize).expect("bounded source column");
        let latitude = source_north - f64::from(row) * source_step;
        let longitude = source_geometry.longitude_origin.get() + f64::from(column) * source_step;
        let x =
            (longitude - target_geometry.longitude_origin.get()).rem_euclid(360.0) / target_step;
        let y = (target_geometry.north_latitude.get() - latitude) / target_step;
        #[allow(clippy::cast_possible_truncation)]
        let target_column = (x + 0.5).floor() as u32 % target_geometry.width;
        #[allow(clippy::cast_possible_truncation)]
        let target_row = (y + 0.5)
            .floor()
            .clamp(0.0, f64::from(target_geometry.height - 1)) as u32;
        let target_index =
            target_row as usize * target_geometry.width as usize + target_column as usize;
        let value = node.orthometric_height.get();
        if node.is_inland_water {
            let target_width = f64::from(target_geometry.width);
            let dx = (x - f64::from(target_column) + target_width / 2.0).rem_euclid(target_width)
                - target_width / 2.0;
            let dy = y - f64::from(target_row);
            let distance = dx * dx + dy * dy;
            let entry = lakes.entry(target_index).or_insert((f64::INFINITY, value));
            if distance < entry.0 {
                *entry = (distance, value);
            }
        } else {
            let entry = negative_ground.entry(target_index).or_insert(value);
            *entry = (*entry).min(value);
        }
    }
    for (index, height) in negative_ground {
        if !lakes.contains_key(&index) {
            target[index].orthometric_height = Meters(height);
            target[index].is_land = true;
            target[index].is_inland_water = false;
        }
    }
    for (index, (_, height)) in lakes {
        target[index].orthometric_height = Meters(height);
        target[index].is_land = false;
        target[index].is_inland_water = true;
    }
    Ok(target)
}

/// Read explicit f32 little-endian elevation/geoid arrays and byte land mask
/// (`0` not dry land, `1` dry land) plus an optional disjoint inland-water mask,
/// then encode. Omitting the inland-water mask means all its bits are zero. File size limits are enforced while reading
/// as well as with metadata, so a file changed after metadata cannot grow memory.
///
/// # Errors
/// IO, incorrect lengths, invalid mask values or any encoder validation error.
pub fn encode_global_atlas_from_raw(
    geometry: GlobalAtlasGeometry,
    elevation_path: &Path,
    geoid_path: &Path,
    mask_path: &Path,
    inland_water_path: Option<&Path>,
) -> io::Result<Vec<u8>> {
    let nodes = read_global_raw_nodes(
        geometry,
        elevation_path,
        geoid_path,
        mask_path,
        inland_water_path,
    )?;
    encode_global_atlas(geometry, &nodes)
}

/// Read prepared source arrays, resample onto the canonical power-of-two lattice,
/// then validate and encode. No source data is downloaded.
///
/// # Errors
/// IO, malformed raw arrays, invalid geometry or failed encoding validation.
pub fn encode_canonical_global_atlas_from_raw(
    geometry: GlobalAtlasGeometry,
    elevation_path: &Path,
    geoid_path: &Path,
    mask_path: &Path,
    inland_water_path: Option<&Path>,
) -> io::Result<Vec<u8>> {
    let nodes = read_global_raw_nodes(
        geometry,
        elevation_path,
        geoid_path,
        mask_path,
        inland_water_path,
    )?;
    let target_geometry = canonical_global_geometry();
    let target = resample_global_atlas(geometry, &nodes, target_geometry)?;
    encode_global_atlas(target_geometry, &target)
}

fn read_global_raw_nodes(
    geometry: GlobalAtlasGeometry,
    elevation_path: &Path,
    geoid_path: &Path,
    mask_path: &Path,
    inland_water_path: Option<&Path>,
) -> io::Result<Vec<GlobalAtlasNode>> {
    let count = geometry.validate()?;
    let elevations = read_exact_file(elevation_path, count * 4)?;
    let geoid = read_exact_file(geoid_path, count * 4)?;
    let mask = read_exact_file(mask_path, count)?;
    let water = if let Some(path) = inland_water_path {
        read_exact_file(path, count)?
    } else {
        vec![0; count]
    };
    let mut nodes = Vec::with_capacity(count);
    for index in 0..count {
        if mask[index] > 1 || water[index] > 1 || (mask[index] == 1 && water[index] == 1) {
            return Err(invalid(
                "dry-land and inland-water masks must be disjoint 0/1 bytes",
            ));
        }
        let offset = index * 4;
        let decode = |bytes: &[u8]| {
            f64::from(f32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("validated array length"),
            ))
        };
        nodes.push(GlobalAtlasNode {
            orthometric_height: Meters(decode(&elevations)),
            geoid_undulation: Meters(decode(&geoid)),
            is_land: mask[index] == 1,
            is_inland_water: water[index] == 1,
        });
    }
    Ok(nodes)
}

fn read_exact_file(path: &Path, length: usize) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() != length as u64 {
        return Err(invalid(format!(
            "{} has unexpected byte length; expected {length}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(length);
    file.take(length as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() != length {
        return Err(invalid(format!(
            "{} was truncated or changed while reading",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Atomically replace one validated atlas in its destination directory.
/// Directory creation is permitted, but this is not a multi-file transaction.
///
/// # Errors
/// Invalid encoded atlas or output filesystem failure.
pub fn write_global_atlas(path: &Path, bytes: &[u8]) -> io::Result<()> {
    GlobalTerrain::from_bytes(bytes.to_vec()).map_err(|error| invalid(error.to_string()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Geodetic;

    fn geometry() -> GlobalAtlasGeometry {
        GlobalAtlasGeometry {
            width: 4,
            height: 2,
            longitude_origin: Degrees(-135.0),
            north_latitude: Degrees(45.0),
        }
    }
    fn nodes() -> Vec<GlobalAtlasNode> {
        vec![
            GlobalAtlasNode {
                orthometric_height: Meters(-420.5),
                geoid_undulation: Meters(32.125),
                is_land: true,
                is_inland_water: false
            };
            8
        ]
    }

    #[test]
    fn roundtrip_preserves_below_sea_land_and_exact_datum_sign() {
        let bytes = encode_global_atlas(geometry(), &nodes()).unwrap();
        let atlas = GlobalTerrain::from_bytes(bytes.clone()).unwrap();
        assert_eq!(atlas.fingerprint(), global_atlas_fingerprint(&bytes));
        let sample = atlas
            .sample(Geodetic::from_degrees(45.0, -135.0, 0.0))
            .unwrap();
        assert_eq!(sample.elevation_msl, Meters(-421.0));
        assert_eq!(sample.geoid_undulation, Meters(32.13));
        assert!(sample.is_land);
        assert!((sample.surface_height.get() - (-388.87)).abs() < 1e-10);
        assert_eq!(bytes, encode_global_atlas(geometry(), &nodes()).unwrap());
    }

    #[test]
    fn canonical_geometry_aligns_with_geographic_tile_breakpoints() {
        let target = canonical_global_geometry();
        assert_eq!((target.width, target.height), (2_048, 1_024));
        assert_eq!(target.longitude_origin, Degrees(-179.912_109_375));
        assert_eq!(target.north_latitude, Degrees(89.912_109_375));
        let source = vec![
            GlobalAtlasNode {
                orthometric_height: Meters(-420.0),
                geoid_undulation: Meters(32.0),
                is_land: true,
                is_inland_water: false
            };
            8
        ];
        let target = GlobalAtlasGeometry {
            width: 8,
            height: 4,
            longitude_origin: Degrees(-157.5),
            north_latitude: Degrees(67.5),
        };
        let result = resample_global_atlas(geometry(), &source, target).unwrap();
        assert_eq!(result.len(), 32);
        assert!(result.iter().all(|node| node.is_land
            && node.orthometric_height == Meters(-420.0)
            && node.geoid_undulation == Meters(32.0)));
    }

    #[test]
    fn canonical_resampling_never_blends_ocean_bathymetry_into_land() {
        let mut source = vec![
            GlobalAtlasNode {
                orthometric_height: Meters(1_000.0),
                geoid_undulation: Meters(30.0),
                is_land: true,
                is_inland_water: false
            };
            8
        ];
        source[0] = GlobalAtlasNode {
            orthometric_height: Meters(-8_000.0),
            geoid_undulation: Meters(30.0),
            is_land: false,
            is_inland_water: false,
        };
        let target = GlobalAtlasGeometry {
            width: 8,
            height: 4,
            longitude_origin: Degrees(-157.5),
            north_latitude: Degrees(67.5),
        };
        let result = resample_global_atlas(geometry(), &source, target).unwrap();
        assert!(
            result
                .iter()
                .all(|node| (0.0..=1_000.0).contains(&node.orthometric_height.get()))
        );
        assert!(
            result
                .iter()
                .filter(|node| !node.is_land)
                .all(|node| node.orthometric_height == Meters::ZERO)
        );
    }

    #[test]
    fn lake_resampling_retains_surface_height_and_independent_water_mask() {
        let source = vec![
            GlobalAtlasNode {
                orthometric_height: Meters(455.0),
                geoid_undulation: Meters(30.0),
                is_land: false,
                is_inland_water: true
            };
            8
        ];
        let target = GlobalAtlasGeometry {
            width: 8,
            height: 4,
            longitude_origin: Degrees(-157.5),
            north_latitude: Degrees(67.5),
        };
        let result = resample_global_atlas(geometry(), &source, target).unwrap();
        assert!(result.iter().all(|node| !node.is_land
            && node.is_inland_water
            && node.orthometric_height == Meters(455.0)));
        let atlas =
            GlobalTerrain::from_bytes(encode_global_atlas(target, &result).unwrap()).unwrap();
        let lake = atlas
            .sample(Geodetic::from_degrees(45.0, -135.0, 0.0))
            .unwrap();
        assert!(!lake.is_land && lake.is_inland_water);
        assert_eq!(lake.surface_height, Meters(485.0));
    }

    #[test]
    fn pooled_negative_ground_and_lake_conflicts_are_deterministic() {
        let source_geometry = GlobalAtlasGeometry {
            width: 6,
            height: 3,
            longitude_origin: Degrees(-175.0),
            north_latitude: Degrees(60.0),
        };
        let mut source = vec![
            GlobalAtlasNode {
                orthometric_height: Meters(1_000.0),
                geoid_undulation: Meters(30.0),
                is_land: true,
                is_inland_water: false
            };
            18
        ];
        source[0].orthometric_height = Meters(-84.0);
        source[1] = GlobalAtlasNode {
            orthometric_height: Meters(455.0),
            geoid_undulation: Meters(30.0),
            is_land: false,
            is_inland_water: true,
        };
        source[2].orthometric_height = Meters(-50.0);
        let result = resample_global_atlas(source_geometry, &source, geometry()).unwrap();
        // Both source0 (negative ground) and source1 (lake) map into target0;
        // independently classified lake surface wins that explicit conflict.
        assert!(!result[0].is_land && result[0].is_inland_water);
        assert_eq!(result[0].orthometric_height, Meters(455.0));
        assert!(result[1].is_land && !result[1].is_inland_water);
        assert_eq!(result[1].orthometric_height, Meters(-50.0));
        assert!(
            result
                .iter()
                .all(|node| !(node.is_land && node.is_inland_water))
        );
        let repeated = resample_global_atlas(source_geometry, &source, geometry()).unwrap();
        assert_eq!(
            encode_global_atlas(geometry(), &result).unwrap(),
            encode_global_atlas(geometry(), &repeated).unwrap()
        );
    }

    #[test]
    fn rejects_missing_nodes_nonfinite_values_and_wrong_geometry() {
        assert!(encode_global_atlas(geometry(), &nodes()[..7]).is_err());
        for invalid_value in [f64::NAN, f64::INFINITY, 12_000.1, -12_000.1] {
            let mut values = nodes();
            values[0].orthometric_height = Meters(invalid_value);
            assert!(encode_global_atlas(geometry(), &values).is_err());
        }
        for invalid_value in [f64::NAN, f64::NEG_INFINITY, 200.1, -200.1] {
            let mut values = nodes();
            values[0].geoid_undulation = Meters(invalid_value);
            assert!(encode_global_atlas(geometry(), &values).is_err());
        }
        for invalid_geometry in [
            GlobalAtlasGeometry {
                width: u32::MAX,
                ..geometry()
            },
            GlobalAtlasGeometry {
                height: u32::MAX,
                ..geometry()
            },
            GlobalAtlasGeometry {
                north_latitude: Degrees(90.0),
                ..geometry()
            },
            GlobalAtlasGeometry {
                longitude_origin: Degrees(f64::NAN),
                ..geometry()
            },
        ] {
            assert!(encode_global_atlas(invalid_geometry, &nodes()).is_err());
        }
    }

    #[test]
    fn raw_files_require_exact_length_valid_mask_and_no_nodata() {
        let directory = tempfile::tempdir().unwrap();
        let elevation = directory.path().join("elevation.f32le");
        let geoid = directory.path().join("geoid.f32le");
        let mask = directory.path().join("land.u8");
        let elevation_bytes: Vec<_> = (0..8).flat_map(|_| (-420.5_f32).to_le_bytes()).collect();
        let geoid_bytes: Vec<_> = (0..8).flat_map(|_| 32.125_f32.to_le_bytes()).collect();
        std::fs::write(&elevation, &elevation_bytes).unwrap();
        std::fs::write(&geoid, geoid_bytes).unwrap();
        std::fs::write(&mask, [1; 8]).unwrap();
        let bytes =
            encode_global_atlas_from_raw(geometry(), &elevation, &geoid, &mask, None).unwrap();
        assert_eq!(bytes, encode_global_atlas(geometry(), &nodes()).unwrap());
        let output = directory.path().join("nested/atlas.fsgt");
        write_global_atlas(&output, &bytes).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        std::fs::write(&mask, [2; 8]).unwrap();
        assert!(encode_global_atlas_from_raw(geometry(), &elevation, &geoid, &mask, None).is_err());
        std::fs::write(&mask, [1; 9]).unwrap();
        assert!(encode_global_atlas_from_raw(geometry(), &elevation, &geoid, &mask, None).is_err());
        std::fs::write(&mask, [1; 8]).unwrap();
        std::fs::write(&elevation, &elevation_bytes[..31]).unwrap();
        assert!(encode_global_atlas_from_raw(geometry(), &elevation, &geoid, &mask, None).is_err());
        assert!(write_global_atlas(&output, b"corrupt").is_err());
        assert_eq!(
            std::fs::read(&output).unwrap(),
            bytes,
            "invalid replacement must leave old atlas intact"
        );
    }
}
