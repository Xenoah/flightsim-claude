//! Reject unreasonable CLI/API sizes before shifts, allocation or tile creation.
use flightsim_core::Meters;
use flightsim_tilegen::generate::{GenerateError, planned_tile_count};
use flightsim_tilegen::{RasterSet, Region, TileGenOptions, generate_tiles};

#[test]
fn allocation_free_counts_match_enumeration_at_small_levels() {
    for (west, south, east, north) in [
        (-180.0, -90.0, 180.0, 90.0),
        (170.0, -5.0, -170.0, 5.0),
        (2.0, 0.0, 1.0, 1.0),
        (139.0, 35.0, 140.0, 36.0),
        (0.0, 0.0, 0.0, 0.0),
    ] {
        let region = Region::from_degrees(west, south, east, north).unwrap();
        for level in 0..=6 {
            assert_eq!(
                region.tile_count(level).unwrap(),
                region.tiles(level).len() as u64
            );
        }
    }
}

#[test]
fn oversized_grids_levels_and_invalid_fill_fail_without_creating_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("tiles");
    let region = Region::from_degrees(0.0, 0.0, 1.0, 1.0).unwrap();
    let rasters = RasterSet::default();
    for options in [
        TileGenOptions {
            grid_size: u32::MAX,
            ..Default::default()
        },
        TileGenOptions {
            fill: Meters(f64::NAN),
            ..Default::default()
        },
        TileGenOptions {
            fill: Meters(f64::MAX),
            ..Default::default()
        },
    ] {
        assert!(generate_tiles(&rasters, region, 0..=0, &options, &output, false).is_err());
        assert!(!output.exists());
    }
    assert!(matches!(
        generate_tiles(
            &rasters,
            region,
            0..=255,
            &TileGenOptions::default(),
            &output,
            false
        ),
        Err(GenerateError::InvalidLevelRange { .. })
    ));
    assert_eq!(region.tile_count(255), None);
    assert!(!output.exists());
}

#[test]
fn enormous_global_requests_are_counted_and_rejected_without_enumeration() {
    let region = Region::from_degrees(-180.0, -90.0, 180.0, 90.0).unwrap();
    assert_eq!(region.tile_count(24), Some(1_u64 << 49));
    assert!(matches!(
        planned_tile_count(region, 0..=24, &TileGenOptions::default()),
        Err(GenerateError::TooManyTiles(_))
    ));
}
