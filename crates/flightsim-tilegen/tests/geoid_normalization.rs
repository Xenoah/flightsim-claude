//! Normalization must precede resampling, preserve missing pixels and match models.
use flightsim_core::{Geodetic, Radians};
use flightsim_tilegen::{
    GeoRaster,
    geoid::GeoidGrid,
    testing::{GeoTiffBuilder, PixelConvention},
    vertical_datum::GeoidModel,
};
use std::{io::Cursor, path::Path};

fn grid() -> GeoidGrid {
    let header =
        b"P5\n# Description WGS84 EGM2008, synthetic\n# Offset -10\n# Scale 1\n4 3\n65535\n";
    let pixels: [u16; 12] = [10, 10, 10, 10, 0, 10, 20, 30, 20, 20, 20, 20];
    let bytes = [
        header.to_vec(),
        pixels.iter().flat_map(|x| x.to_be_bytes()).collect(),
    ]
    .concat();
    GeoidGrid::read(Cursor::new(bytes), GeoidModel::Egm2008).unwrap()
}

fn raster(code: Option<u16>, samples: Vec<f32>) -> GeoRaster {
    let mut builder = GeoTiffBuilder::new(2, 2, samples)
        .origin(0.0, 0.0)
        .pixel_size(90.0, 90.0)
        .convention(PixelConvention::Point);
    if let Some(code) = code {
        builder = builder.vertical_cs_type(code);
    }
    GeoRaster::decode(Cursor::new(builder.build()), Path::new("fixture.tif")).unwrap()
}

fn height(raster: &GeoRaster, lat: f64, lon: f64) -> f64 {
    raster
        .sample(
            Geodetic::from_degrees(lat, lon, 0.0),
            (Radians::ZERO, Radians::ZERO),
        )
        .unwrap()
        .get()
}

#[test]
fn both_positive_and_negative_undulations_are_added_to_orthometric_heights() {
    let converted = raster(Some(3855), vec![100.0; 4])
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    assert!(converted.vertical_datum().is_ellipsoidal());
    for (lat, lon, expected) in [(0.0, 0.0, 90.0), (0.0, 90.0, 100.0), (-90.0, 0.0, 110.0)] {
        assert!((height(&converted, lat, lon) - expected).abs() < 1e-6);
    }
}

#[test]
fn correction_is_applied_at_source_centres_before_coarse_averaging() {
    let converted = raster(Some(3855), vec![100.0; 4])
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    let mean = converted
        .sample(
            Geodetic::from_degrees(-45.0, 45.0, 0.0),
            (Radians(10.0), Radians(10.0)),
        )
        .unwrap();
    // Source h values: 90, 100, 110, 110. Their mean is 102.5.
    assert!((mean.get() - 102.5).abs() < 1e-6);
}

#[test]
fn unknown_and_mismatched_datums_are_not_guessed() {
    for code in [None, Some(5773), Some(5714), Some(1234)] {
        assert!(
            raster(code, vec![100.0; 4])
                .normalize_to_ellipsoid(&grid())
                .is_err()
        );
    }
}

#[test]
fn a_source_declaration_fills_missing_metadata_but_cannot_replace_a_conflict() {
    use flightsim_tilegen::vertical_datum::VerticalDatum;
    let declared = raster(None, vec![100.0; 4])
        .declare_vertical_datum(VerticalDatum::Geoid(GeoidModel::Egm2008))
        .unwrap()
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    assert!((height(&declared, 0.0, 0.0) - 90.0).abs() < 1e-6);
    assert!(
        raster(Some(5773), vec![100.0; 4])
            .declare_vertical_datum(VerticalDatum::Geoid(GeoidModel::Egm2008))
            .is_err()
    );
    assert!(
        raster(Some(4979), vec![100.0; 4])
            .declare_vertical_datum(VerticalDatum::Geoid(GeoidModel::Egm2008))
            .is_err()
    );
}

#[test]
fn library_generation_cannot_bypass_the_vertical_datum_gate() {
    let set = flightsim_tilegen::RasterSet::new(vec![raster(Some(3855), vec![100.0; 4])]);
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("tiles");
    let region = flightsim_tilegen::Region::from_degrees(0.0, -1.0, 1.0, 0.0).unwrap();
    assert!(matches!(
        flightsim_tilegen::generate_tiles(
            &set,
            region,
            0..=0,
            &flightsim_tilegen::TileGenOptions::default(),
            &output,
            false
        ),
        Err(flightsim_tilegen::GenerateError::NonEllipsoidalSource { .. })
    ));
    assert!(!output.exists());
    let acknowledged = set.assume_ellipsoidal();
    assert!(acknowledged.non_ellipsoidal_sources().is_empty());
}

#[test]
fn ellipsoidal_sources_are_unchanged_and_cannot_be_corrected_twice() {
    let first = raster(Some(3855), vec![100.0; 4])
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    let expected = height(&first, 0.0, 0.0);
    let twice = first.normalize_to_ellipsoid(&grid()).unwrap();
    assert_eq!(height(&twice, 0.0, 0.0).to_bits(), expected.to_bits());
    let ellipsoid = raster(Some(4979), vec![123.0; 4])
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    assert!((height(&ellipsoid, 0.0, 0.0) - 123.0).abs() < 1e-6);
}

#[test]
fn nodata_remains_missing_and_valid_heights_can_equal_the_old_sentinel() {
    let bytes = GeoTiffBuilder::new(3, 3, vec![10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        .origin(0.0, 0.0)
        .pixel_size(45.0, 45.0)
        .convention(PixelConvention::Point)
        .nodata(0.0)
        .vertical_cs_type(3855)
        .build();
    let converted = GeoRaster::decode(Cursor::new(bytes), Path::new("nodata.tif"))
        .unwrap()
        .normalize_to_ellipsoid(&grid())
        .unwrap();
    // The valid source 10 m becomes 0 m; clearing the old sentinel is essential.
    assert!(height(&converted, 0.0, 0.0).abs() < 1e-6);
    assert!(
        converted
            .sample(
                Geodetic::from_degrees(-90.0, 90.0, 0.0),
                (Radians::ZERO, Radians::ZERO)
            )
            .is_none(),
        "{converted:?}"
    );
}
