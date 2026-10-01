//! GeoTIFF metadata must fail closed before values acquire the WGS84 contract.
use flightsim_core::{Geodetic, Radians};
use flightsim_tilegen::{
    GeoRaster,
    testing::{GeoTiffBuilder, PixelConvention},
};
use std::{io::Cursor, path::Path};

fn decode(bytes: Vec<u8>) -> Result<GeoRaster, flightsim_tilegen::RasterError> {
    GeoRaster::decode(Cursor::new(bytes), Path::new("synthetic-metadata.tif"))
}

fn builder() -> GeoTiffBuilder {
    GeoTiffBuilder::new(2, 2, vec![1.0, 2.0, 3.0, 4.0])
        .origin(139.0, 36.0)
        .pixel_size(0.001, 0.001)
}

#[test]
fn non_wgs84_crs_and_non_degree_or_non_metre_units_are_rejected() {
    for bytes in [
        builder().geographic_type(4269).build(),
        builder().geographic_type(32767).build(),
        builder().angular_units(9101).build(),
        builder().vertical_units(9002).build(),
    ] {
        assert!(decode(bytes).is_err());
    }
    assert!(decode(builder().vertical_units(9001).build()).is_ok());
}

fn geo_key_offset(bytes: &[u8]) -> usize {
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap());
    for entry in bytes[ifd + 2..].chunks_exact(12).take(usize::from(count)) {
        if u16::from_le_bytes(entry[..2].try_into().unwrap()) == 34735 {
            return u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        }
    }
    panic!("fixture contains GeoKeys")
}

fn set_double_tag_value(bytes: &mut [u8], tag: u16, index: usize, value: f64) {
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()));
    let entry = (0..count)
        .map(|entry| ifd + 2 + entry * 12)
        .find(|&entry| u16::from_le_bytes(bytes[entry..entry + 2].try_into().unwrap()) == tag)
        .expect("fixture contains requested tag");
    let offset = u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
    let at = offset + index * 8;
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn vertical_sample_transforms_are_identity_or_rejected() {
    for scale in [0.0, 1.0] {
        let mut bytes = builder().vertical_cs_type(4979).build();
        set_double_tag_value(&mut bytes, 33550, 2, scale);
        assert!(decode(bytes).is_ok());
    }
    for (tag, index, value) in [
        (33550, 2, 2.0),
        (33550, 2, -1.0),
        (33550, 2, f64::NAN),
        (33550, 2, f64::INFINITY),
        (33922, 2, 1.0),
        (33922, 2, f64::NAN),
        (33922, 5, 1000.0),
        (33922, 5, f64::NAN),
        (33922, 5, f64::NEG_INFINITY),
    ] {
        let mut bytes = builder().vertical_cs_type(4979).build();
        set_double_tag_value(&mut bytes, tag, index, value);
        assert!(decode(bytes).is_err(), "tag {tag}[{index}] = {value}");
    }
}

#[test]
fn malformed_directories_and_short_keys_cannot_disguise_the_datum() {
    for (word, value) in [
        (0, 2),
        (3, 500),
        (4, 0),
        (6, 0),
        (6, 2),
        (5, 34735),
        (8, 1024),
    ] {
        let mut bytes = builder().build();
        let start = geo_key_offset(&bytes) + word * 2;
        bytes[start..start + 2].copy_from_slice(&u16::to_le_bytes(value));
        assert!(decode(bytes).is_err(), "word {word}, value {value}");
    }
}

#[test]
fn explicit_three_dimensional_wgs84_is_ellipsoidal_but_conflicting_keys_fail() {
    assert!(
        decode(builder().geographic_type(4979).build())
            .unwrap()
            .vertical_datum()
            .is_ellipsoidal()
    );
    assert!(
        decode(
            builder()
                .geographic_type(4979)
                .vertical_cs_type(3855)
                .build()
        )
        .is_err()
    );
}

#[test]
fn nonfinite_or_out_of_globe_pixel_centres_are_rejected() {
    for bytes in [
        builder().origin(f64::MAX, f64::MAX).build(),
        builder().origin(f64::MAX, 35.0).build(),
        builder().pixel_size(f64::MAX, 0.1).build(),
        builder().pixel_size(f64::from_bits(1), 0.1).build(),
        builder().origin(0.0, -90.0).build(),
        builder().origin(0.0, 91.0).build(),
    ] {
        assert!(decode(bytes).is_err());
    }
}

#[test]
fn a_declared_but_malformed_nodata_value_is_not_silently_ignored() {
    let mut data = builder().nodata(0.0).build();
    let ifd = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_le_bytes(data[ifd..ifd + 2].try_into().unwrap()));
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if u16::from_le_bytes(data[entry..entry + 2].try_into().unwrap()) == 42113 {
            data[entry + 4..entry + 8].copy_from_slice(&4_u32.to_le_bytes());
            data[entry + 8..entry + 12].copy_from_slice(b"bad\0");
        }
    }
    assert!(decode(data).is_err());
}

#[test]
fn tiny_pixels_and_outside_bilinear_coordinates_do_not_overflow() {
    let raster = decode(builder().origin(0.0, 0.0).pixel_size(1e-30, 1e-30).build()).unwrap();
    assert!(
        raster
            .sample(
                Geodetic::from_degrees(-80.0, 120.0, 0.0),
                (Radians::ZERO, Radians::ZERO)
            )
            .is_none()
    );
}

#[test]
fn huge_footprints_are_bounded_to_the_raster() {
    let raster = decode(builder().build()).unwrap();
    let position = Geodetic::from_degrees(35.999, 139.001, 0.0);
    let height = raster
        .sample(position, (Radians(1e100), Radians(1e100)))
        .unwrap();
    assert!((height.get() - 2.5).abs() < 1e-6);
    for footprint in [
        (Radians(f64::NAN), Radians(1.0)),
        (Radians(-1.0), Radians(1.0)),
    ] {
        assert!(raster.sample(position, footprint).is_none());
    }
}

#[test]
fn a_global_point_grid_samples_its_eastern_half() {
    let raster = decode(
        GeoTiffBuilder::new(5, 3, [1.0, 2.0, 3.0, 4.0, 1.0].repeat(3))
            .origin(0.0, 90.0)
            .pixel_size(90.0, 90.0)
            .convention(PixelConvention::Point)
            .build(),
    )
    .unwrap();
    for (longitude, expected) in [(0.0, 1.0), (90.0, 2.0), (180.0, 3.0), (-90.0, 4.0)] {
        let height = raster
            .sample(
                Geodetic::from_degrees(0.0, longitude, 0.0),
                (Radians::ZERO, Radians::ZERO),
            )
            .unwrap();
        assert!((height.get() - expected).abs() < 1e-6);
    }
}
