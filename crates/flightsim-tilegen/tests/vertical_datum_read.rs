//! GeoTIFF から鉛直基準を読めることの検査。
//!
//! 純関数の検査（`vertical_datum` モジュール内）は「EPSG コード → 種別」の
//! 対応だけを見る。**ここは実際に GeoTIFF を書いて読み直す**ので、
//! GeoKey ディレクトリの並びやタグの結線まで通る。
//!
//! これが無いと「判定関数は正しいが、そもそもキーを読んでいない」という
//! 状態を見逃す。

use flightsim_tilegen::geotiff::GeoRaster;
use flightsim_tilegen::testing::GeoTiffBuilder;
use flightsim_tilegen::vertical_datum::{
    EPSG_EGM96_HEIGHT, EPSG_EGM2008_HEIGHT, EPSG_MEAN_SEA_LEVEL, EPSG_WGS84_ELLIPSOIDAL_3D,
    GeoidModel, VerticalDatum,
};

/// 小さな正当ラスタを書いて読み直す。
fn read_with(vertical_cs: Option<u16>) -> VerticalDatum {
    let samples = vec![10.0_f32; 4];
    let mut builder = GeoTiffBuilder::new(2, 2, samples)
        .origin(139.0, 36.0)
        .pixel_size(0.001, 0.001);
    if let Some(code) = vertical_cs {
        builder = builder.vertical_cs_type(code);
    }

    let directory = std::env::temp_dir().join(format!(
        "flightsim-vdatum-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let path = directory.join("probe.tif");
    std::fs::write(&path, builder.build()).expect("write the probe raster");

    let raster = GeoRaster::open(&path).expect("the probe raster should read");
    let datum = raster.vertical_datum();
    std::fs::remove_dir_all(&directory).ok();
    datum
}

#[test]
fn a_copernicus_style_raster_reports_egm2008() {
    // **本題。** Copernicus DEM GLO-30 は EPSG:3855。
    // ここが Ellipsoidal に化けると、ジオイド高ぶんの誤差が黙って入る。
    let datum = read_with(Some(EPSG_EGM2008_HEIGHT));
    assert_eq!(
        datum,
        VerticalDatum::Geoid(GeoidModel::Egm2008),
        "a GeoTIFF tagged EPSG:3855 must read back as EGM2008"
    );
    assert!(!datum.is_ellipsoidal());
}

#[test]
fn an_ellipsoidal_raster_reports_ellipsoidal() {
    let datum = read_with(Some(EPSG_WGS84_ELLIPSOIDAL_3D));
    assert!(
        datum.is_ellipsoidal(),
        "a GeoTIFF tagged EPSG:4979 should be usable as-is, got {datum}"
    );
}

#[test]
fn other_geoid_codes_read_back_correctly() {
    assert_eq!(
        read_with(Some(EPSG_EGM96_HEIGHT)),
        VerticalDatum::Geoid(GeoidModel::Egm96)
    );
    assert_eq!(
        read_with(Some(EPSG_MEAN_SEA_LEVEL)),
        VerticalDatum::Geoid(GeoidModel::UnspecifiedMeanSeaLevel)
    );
}

#[test]
fn a_raster_without_the_key_reads_as_unspecified() {
    // **「書いていない」を「楕円体高」に化けさせない。**
    // 既存の合成 DEM（synthetic_dem.rs）もこの経路を通る。
    let datum = read_with(None);
    assert_eq!(datum, VerticalDatum::Unspecified);
    assert!(!datum.is_ellipsoidal());
}

#[test]
fn an_unknown_code_survives_the_round_trip_with_its_number() {
    let datum = read_with(Some(6789));
    assert_eq!(datum, VerticalDatum::Unsupported(6789));
}

#[test]
fn adding_the_key_does_not_disturb_the_rest_of_the_georeference() {
    // GeoKey を 1 つ足したせいで、位置や画素サイズの読み取りが
    // ずれていないこと。**キー数の書き換えを間違えるとここが壊れる。**
    let samples = vec![1.0_f32, 2.0, 3.0, 4.0];
    let directory = std::env::temp_dir().join(format!(
        "flightsim-vdatum-geo-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("temp dir");

    let plain = directory.join("plain.tif");
    let tagged = directory.join("tagged.tif");
    std::fs::write(
        &plain,
        GeoTiffBuilder::new(2, 2, samples.clone())
            .origin(139.0, 36.0)
            .pixel_size(0.001, 0.002)
            .build(),
    )
    .expect("write plain");
    std::fs::write(
        &tagged,
        GeoTiffBuilder::new(2, 2, samples)
            .origin(139.0, 36.0)
            .pixel_size(0.001, 0.002)
            .vertical_cs_type(EPSG_EGM2008_HEIGHT)
            .build(),
    )
    .expect("write tagged");

    let plain = GeoRaster::open(&plain).expect("plain reads");
    let tagged = GeoRaster::open(&tagged).expect("tagged reads");

    assert_eq!(
        plain.coverage(),
        tagged.coverage(),
        "adding the vertical key changed the georeference"
    );
    std::fs::remove_dir_all(&directory).ok();
}
