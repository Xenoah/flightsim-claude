//! Automatic coverage must preserve unwrapped raster longitudes and pole edges.
use flightsim_tilegen::{
    GeoRaster, RasterSet, Region,
    testing::{GeoTiffBuilder, PixelConvention},
};
use std::{io::Cursor, path::Path};

fn raster(width: u32, height: u32, west: f64, north: f64, pixel: f64) -> GeoRaster {
    let bytes = GeoTiffBuilder::new(width, height, vec![10.0; width as usize * height as usize])
        .origin(west, north)
        .pixel_size(pixel, pixel)
        .convention(PixelConvention::Point)
        .vertical_cs_type(4979)
        .build();
    GeoRaster::decode(Cursor::new(bytes), Path::new("coverage.tif")).unwrap()
}

#[test]
fn an_unwrapped_dateline_raster_covers_both_sides_automatically() {
    let set = RasterSet::new(vec![raster(3, 3, 179.0, 1.0, 1.0)]);
    let actual = set.coverage().unwrap();
    let expected = Region::from_degrees(178.5, -1.5, -178.5, 1.5).unwrap();
    assert!(actual.crosses_dateline());
    for level in 0..=8 {
        assert_eq!(actual.tiles(level), expected.tiles(level));
    }
}

#[test]
fn zero_to_360_longitudes_and_full_world_polar_extents_normalize() {
    let set = RasterSet::new(vec![raster(3, 3, 359.0, 1.0, 0.5)]);
    let expected = Region::from_degrees(-1.25, -0.25, 0.25, 1.25).unwrap();
    assert_eq!(set.coverage().unwrap().tiles(8), expected.tiles(8));
    let global = RasterSet::new(vec![raster(5, 3, 0.0, 90.0, 90.0)]);
    let expected = Region::from_degrees(-180.0, -90.0, 180.0, 90.0).unwrap();
    assert_eq!(global.coverage().unwrap().tiles(3), expected.tiles(3));
}

#[test]
fn multiple_dateline_rasters_union_the_covered_arc_in_either_order() {
    let a = raster(2, 2, 179.2, 0.2, 0.2);
    let b = raster(2, 2, -179.4, 0.2, 0.2);
    let expected = Region::from_degrees(179.1, -0.1, -179.1, 0.3).unwrap();
    for set in [
        RasterSet::new(vec![a.clone(), b.clone()]),
        RasterSet::new(vec![b, a]),
    ] {
        let coverage = set.coverage().unwrap();
        assert!(coverage.crosses_dateline());
        assert_eq!(coverage.tiles(8), expected.tiles(8));
    }
}

#[test]
fn regional_unions_preserve_wide_arcs_and_recognize_complete_longitude() {
    for (a, b, expected) in [
        ((170.0, -170.0), (-175.0, -160.0), (170.0, -160.0)),
        ((-170.0, 170.0), (160.0, -160.0), (-180.0, 180.0)),
        ((-170.0, -160.0), (160.0, 170.0), (160.0, -160.0)),
    ] {
        let region = |(west, east)| Region::from_degrees(west, -5.0, east, 5.0).unwrap();
        let actual = region(a).union(region(b));
        for level in 0..=6 {
            assert_eq!(actual.tiles(level), region(expected).tiles(level));
        }
    }
}
