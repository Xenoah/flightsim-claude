//! Exercise the DEM sampling path used by actual mesh construction at ±180°.

use flightsim_core::{Ecef, Geodetic, Meters, Radians};
use flightsim_world::{DemTile, HeightGrid, MeshOptions, TileId, build_mesh};
use glam::DVec3;

fn ramp(id: TileId, west: f32, east: f32) -> DemTile {
    DemTile::new(
        id.bounds(),
        HeightGrid::new(3, 3, [west, (west + east) * 0.5, east].repeat(3)),
    )
}

#[test]
fn dem_dateline_edge_uses_east_column_and_preserves_clamping() {
    let id = TileId::new(8, TileId::columns(8) - 1, TileId::rows(8) / 2);
    let bounds = id.bounds();
    let dem = ramp(id, 100.0, 900.0);
    for turns in -3..=3 {
        for (longitude, expected) in [
            (bounds.west.get() - bounds.width().get() * 0.1, 100.0),
            (bounds.west.get(), 100.0),
            (bounds.center().longitude.get(), 500.0),
            (bounds.east.get(), 900.0),
            (bounds.east.get() + bounds.width().get() * 0.1, 900.0),
        ] {
            let point = Geodetic::new(
                bounds.center().latitude,
                Radians(longitude + f64::from(turns) * core::f64::consts::TAU),
                Meters::ZERO,
            );
            assert!(
                (dem.elevation_at(point).get() - expected).abs() < 1e-7,
                "turns {turns}, longitude {longitude}: wrong sampled column"
            );
        }
    }
    // A longitude just west of an ordinary tile must clamp west, not wrap east.
    let ordinary_id = TileId::new(8, 250, 128);
    let ordinary = ramp(ordinary_id, 100.0, 900.0);
    let bounds = ordinary_id.bounds();
    for (longitude, expected) in [
        (bounds.west.get() - bounds.width().get() * 0.1, 100.0),
        (bounds.east.get() + bounds.width().get() * 0.1, 900.0),
    ] {
        let point = Geodetic::new(bounds.center().latitude, Radians(longitude), Meters::ZERO);
        assert!((ordinary.elevation_at(point).get() - expected).abs() < 1e-9);
    }
}

#[test]
fn built_mesh_preserves_distinct_dateline_columns_and_matches_its_neighbour() {
    for level in [6, 13] {
        let east_id = TileId::new(level, TileId::columns(level) - 1, TileId::rows(level) / 2);
        let west_id = TileId::new(level, 0, TileId::rows(level) / 2);
        let options = MeshOptions {
            resolution: 5,
            skirt_depth: Some(Meters::ZERO),
        };
        let east = build_mesh(east_id, &ramp(east_id, 100.0, 900.0), &options);
        let west = build_mesh(west_id, &ramp(west_id, 900.0, 1700.0), &options);
        let world_position = |mesh: &flightsim_world::TerrainMesh, index: usize| {
            let position = mesh.positions[index];
            mesh.origin.as_vec()
                + DVec3::new(
                    f64::from(position[0]),
                    f64::from(position[1]),
                    f64::from(position[2]),
                )
        };
        for row in 0..5 {
            for column in 0..5 {
                let index = row * 5 + column;
                let expected = 100.0 + f64::from(u32::try_from(column).unwrap()) * 200.0;
                assert!((f64::from(east.elevations[index]) - expected).abs() < 1e-3);
                let actual = Ecef::from_vec(world_position(&east, index)).to_geodetic();
                assert!(
                    (actual.altitude.get() - expected).abs() < 0.05,
                    "level {level}, row {row}, column {column}: {} instead of {expected}",
                    actual.altitude.get()
                );
            }
            let east_edge = world_position(&east, row * 5 + 4);
            let west_edge = world_position(&west, row * 5);
            assert!((east_edge - west_edge).length() < 0.05);
            assert!((east.elevations[row * 5 + 4] - 900.0).abs() < 1e-3);
            assert!((west.elevations[row * 5] - 900.0).abs() < 1e-3);
        }
    }
}
