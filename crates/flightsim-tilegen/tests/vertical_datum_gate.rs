//! 鉛直基準が合わない DEM を、黙って焼かせないことの検査。
//!
//! **焼いてしまうと実行時からは「正しい標高」と区別が付かない。**
//! 滑走路も機体も同じだけずれるので描画も接地も辻褄が合い、
//! 焼き直すまで誰も気付けない。だから入口で止める。

use flightsim_tilegen::testing::GeoTiffBuilder;
use flightsim_tilegen::vertical_datum::{
    EPSG_EGM2008_HEIGHT, EPSG_WGS84_ELLIPSOIDAL_3D, GeoidModel, VerticalDatum,
};
use flightsim_tilegen::{RasterSet, geotiff::GeoRaster};

/// 指定した鉛直基準で小さなラスタを書き、`RasterSet` にして返す。
fn raster_set(codes: &[Option<u16>]) -> (RasterSet, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "flightsim-vgate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("temp dir");

    let mut rasters = Vec::new();
    for (index, code) in codes.iter().enumerate() {
        #[allow(
            clippy::cast_precision_loss,
            reason = "入力は数本。精度は問題にならない"
        )]
        let offset = index as f64;
        let mut builder = GeoTiffBuilder::new(2, 2, vec![5.0_f32; 4])
            .origin(139.0 + offset, 36.0)
            .pixel_size(0.001, 0.001);
        if let Some(code) = code {
            builder = builder.vertical_cs_type(*code);
        }
        let path = directory.join(format!("raster{index}.tif"));
        std::fs::write(&path, builder.build()).expect("write raster");
        rasters.push(GeoRaster::open(&path).expect("raster reads"));
    }
    (RasterSet::new(rasters), directory)
}

#[test]
fn an_ellipsoidal_source_passes_without_complaint() {
    let (set, directory) = raster_set(&[Some(EPSG_WGS84_ELLIPSOIDAL_3D)]);
    assert!(
        set.non_ellipsoidal_sources().is_empty(),
        "an ellipsoidal DEM should need no flag"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_copernicus_source_is_flagged() {
    // **本題。** Copernicus DEM GLO-30（EPSG:3855）は変換なしで焼けない。
    let (set, directory) = raster_set(&[Some(EPSG_EGM2008_HEIGHT)]);
    let flagged = set.non_ellipsoidal_sources();
    assert_eq!(flagged.len(), 1, "the EGM2008 source should be flagged");
    assert_eq!(flagged[0].0, 0, "the input index should be reported");
    assert_eq!(flagged[0].1, VerticalDatum::Geoid(GeoidModel::Egm2008));
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_source_without_a_datum_is_flagged() {
    let (set, directory) = raster_set(&[None]);
    assert_eq!(set.non_ellipsoidal_sources().len(), 1);
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn the_reported_index_points_at_the_offending_input() {
    // 複数入力のうち**どれが**問題かを言えないと、利用者は直せない。
    let (set, directory) = raster_set(&[
        Some(EPSG_WGS84_ELLIPSOIDAL_3D),
        Some(EPSG_EGM2008_HEIGHT),
        Some(EPSG_WGS84_ELLIPSOIDAL_3D),
    ]);
    let flagged = set.non_ellipsoidal_sources();
    assert_eq!(flagged.len(), 1, "only the middle raster is wrong");
    assert_eq!(
        flagged[0].0, 1,
        "the index must identify the second input, got {}",
        flagged[0].0
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn every_mismatched_source_is_listed_not_just_the_first() {
    // 1 つ直して再実行、をくり返させない。
    let (set, directory) = raster_set(&[Some(EPSG_EGM2008_HEIGHT), None]);
    assert_eq!(
        set.non_ellipsoidal_sources().len(),
        2,
        "both mismatched inputs should be reported at once"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn an_empty_set_is_not_flagged() {
    let set = RasterSet::new(Vec::new());
    assert!(set.non_ellipsoidal_sources().is_empty());
}
