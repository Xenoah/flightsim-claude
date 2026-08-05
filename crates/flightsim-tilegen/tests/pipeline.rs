//! GeoTIFF から `.fsdem` までを、公開 API だけで一気に通す統合テスト。
//!
//! # CI に実データを置かない
//!
//! Copernicus DEM は全球で数百 GB あり、CI には置けない。かといって地理参照の
//! 解釈をモックにすると、**実際のデコード経路が一度も検査されない**。
//!
//! そこで合成 GeoTIFF を実ファイルとして書き出し、`GeoRaster::open` から
//! タイル書き出し・読み戻しまでを通す。単体テストが飛ばしている
//! 「ファイルを開く」経路をここで踏む。

use flightsim_core::{Geodetic, Meters, Radians};
use flightsim_tilegen::testing::GeoTiffBuilder;
use flightsim_tilegen::{RasterSet, Region, TileGenOptions, generate_tiles};
use flightsim_world::TileId;
use flightsim_world::dem::io::{read_tile, tile_relative_path};
use std::path::{Path, PathBuf};

/// テスト毎に独立した一時ディレクトリ。並行実行しても衝突しない。
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flightsim-tilegen-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&path).expect("the temporary directory should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// 東へ向かって上る斜面の GeoTIFF を実ファイルとして書く。
///
/// 標高 `= 経度方向の画素番号 * 10 m`。読み戻した値を計算で検証できる形にしてある。
fn write_ramp_geotiff(
    directory: &Path,
    name: &str,
    west: f64,
    north: f64,
    size: u32,
    pixel: f64,
) -> PathBuf {
    #[allow(
        clippy::cast_precision_loss,
        reason = "テスト用の合成標高。size は高々数百"
    )]
    let samples: Vec<f32> = (0..size)
        .flat_map(|_| (0..size).map(|column| column as f32 * 10.0))
        .collect();

    let bytes = GeoTiffBuilder::new(size, size, samples)
        .origin(west, north)
        .pixel_size(pixel, pixel)
        .build();

    let path = directory.join(name);
    std::fs::write(&path, bytes).expect("the synthetic GeoTIFF should be writable");
    path
}

#[test]
fn a_geotiff_on_disk_becomes_tiles_that_read_back_correctly() {
    let workspace = TempDir::new("roundtrip");
    let source = write_ramp_geotiff(workspace.path(), "ramp.tif", 139.0, 36.0, 256, 0.01);

    let rasters = RasterSet::load(&[source]).expect("the file we just wrote should open");
    assert_eq!(rasters.len(), 1);

    let region = rasters.coverage().expect("one raster");
    let output = workspace.path().join("tiles");

    let report = generate_tiles(
        &rasters,
        region,
        10..=10,
        &TileGenOptions::default(),
        &output,
        false,
    )
    .expect("generation should succeed");

    assert!(report.tiles_written > 0, "no tiles were produced");
    assert!(output.exists(), "the output directory was not created");

    let mut verified = 0_u32;
    for id in region.tiles(10) {
        let path = output.join(tile_relative_path(id));
        let Ok(bytes) = std::fs::read(&path) else {
            continue; // 被覆が無く書かれなかったタイル
        };

        let stored = read_tile(&mut bytes.as_slice()).expect("a tile we just wrote must read back");
        assert_eq!(stored.id, id, "the tile id did not survive the round trip");
        assert_eq!(stored.tile.bounds(), id.bounds());
        assert!(stored.tile.geometric_error().get() >= 0.0);

        let centre = id.center();
        let footprint = (
            Radians(id.bounds().width().get() / 64.0),
            Radians(id.bounds().height().get() / 64.0),
        );
        // 縁のタイルは中心に被覆が無いことがある。突き合わせ対象からだけ外す。
        let Some(expected) = rasters.sample(centre, footprint) else {
            continue;
        };

        let actual = stored.tile.elevation_at(centre);
        assert!(
            (actual.get() - expected.get()).abs() < 1.0,
            "tile {id:?} reads {actual} m at its centre but the source raster says {expected} m"
        );
        verified += 1;
    }

    assert!(
        verified > 0,
        "no generated tile was verified against the source"
    );
}

#[test]
fn every_written_tile_is_a_valid_tile_file() {
    let workspace = TempDir::new("validity");
    let source = write_ramp_geotiff(workspace.path(), "ramp.tif", 0.0, 10.0, 128, 0.02);
    let rasters = RasterSet::load(&[source]).expect("open");
    let region = rasters.coverage().expect("one raster");
    let output = workspace.path().join("tiles");

    generate_tiles(
        &rasters,
        region,
        9..=11,
        &TileGenOptions::default(),
        &output,
        false,
    )
    .expect("generation");

    // 出力ディレクトリを走査し、見つかったファイルが全て読めることを確かめる。
    // 書けたが読めないタイルが 1 つでもあると、実行時に地形が欠ける。
    let mut found = 0_u32;
    let mut stack = vec![output.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("readable directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable tile");
            read_tile(&mut bytes.as_slice())
                .unwrap_or_else(|error| panic!("{} failed to read back: {error}", path.display()));
            found += 1;
        }
    }

    assert!(found > 0, "no tile files were written at all");
}

#[test]
fn deeper_levels_resolve_the_terrain_more_closely() {
    // LOD の前提。細かいレベルほど元データに近くなければ、細分化する意味がない。
    let workspace = TempDir::new("levels");
    let source = write_ramp_geotiff(workspace.path(), "ramp.tif", 0.0, 10.0, 512, 0.005);
    let rasters = RasterSet::load(&[source]).expect("open");

    // ラスタ内部に確実に収まる点を選ぶ。
    let probe = Geodetic::from_degrees(8.0, 1.0, 0.0);
    let fine_footprint = (
        Radians(f64::to_radians(0.001)),
        Radians(f64::to_radians(0.001)),
    );
    let truth = rasters
        .sample(probe, fine_footprint)
        .expect("the probe is inside the raster");

    let mut previous_error = f64::INFINITY;
    for level in [8_u8, 10, 12] {
        let id = TileId::containing(level, probe);
        let build = flightsim_tilegen::build_tile(&rasters, id, &TileGenOptions::default())
            .expect("the probe tile has coverage");

        let (u, v) = id.bounds().normalise(probe);
        let sampled = build.grid.sample_normalised(u, v);
        let error = (sampled.get() - truth.get()).abs();

        assert!(
            error <= previous_error + 1.0,
            "level {level} was less accurate ({error} m) than the level above it ({previous_error} m)"
        );
        previous_error = error;
    }
}

#[test]
fn a_region_crossing_the_dateline_produces_tiles_on_both_sides() {
    // 日付変更線は地形コードのバグの定番の巣。実際に焼けることまで確認する。
    let workspace = TempDir::new("dateline");
    // 179°E から 180° にかかるラスタ。
    let east_side = write_ramp_geotiff(workspace.path(), "east.tif", 179.0, 1.0, 64, 0.015);
    // -180° から -179°。
    let west_side = write_ramp_geotiff(workspace.path(), "west.tif", -180.0, 1.0, 64, 0.015);

    let rasters = RasterSet::load(&[east_side, west_side]).expect("both files should open");
    let region = Region::from_degrees(179.5, 0.5, -179.5, 0.9).expect("valid dateline region");
    assert!(region.crosses_dateline());

    let output = workspace.path().join("tiles");
    let report = generate_tiles(
        &rasters,
        region,
        11..=11,
        &TileGenOptions::default(),
        &output,
        false,
    )
    .expect("generation across the dateline should succeed");

    assert!(
        report.tiles_written > 0,
        "no tiles were written for a dateline-crossing region"
    );

    let columns = TileId::columns(11);
    let written: Vec<TileId> = region
        .tiles(11)
        .into_iter()
        .filter(|id| output.join(tile_relative_path(*id)).exists())
        .collect();

    assert!(
        written.iter().any(|id| id.x < columns / 2),
        "nothing was written east of the dateline"
    );
    assert!(
        written.iter().any(|id| id.x >= columns / 2),
        "nothing was written west of the dateline"
    );
}

#[test]
fn uncovered_points_are_filled_with_the_requested_elevation() {
    let workspace = TempDir::new("fill");
    let source = write_ramp_geotiff(workspace.path(), "ramp.tif", 0.0, 1.0, 32, 0.01);
    let rasters = RasterSet::load(&[source]).expect("open");

    // ラスタより広い範囲を要求し、外側が埋められることを確かめる。
    let region = Region::from_degrees(-0.5, 0.0, 1.0, 1.5).expect("valid");
    let report = generate_tiles(
        &rasters,
        region,
        11..=11,
        &TileGenOptions {
            grid_size: 33,
            fill: Meters(-500.0),
        },
        Path::new("<dry-run>"),
        true,
    )
    .expect("dry run");

    assert!(
        report.grid_points_filled > 0,
        "a region wider than the raster should report filled points"
    );
    assert!(
        report.tiles_without_coverage > 0,
        "tiles entirely outside the raster should be skipped, not filled"
    );
}

#[test]
fn a_plain_tiff_is_rejected_with_a_useful_message() {
    let workspace = TempDir::new("plain");
    let bytes = GeoTiffBuilder::new(4, 4, vec![1.0; 16])
        .without_georeference()
        .build();
    let path = workspace.path().join("plain.tif");
    std::fs::write(&path, bytes).expect("writable");

    let error = RasterSet::load(&[path]).expect_err("a plain TIFF must not be accepted");
    let message = error.to_string();
    assert!(
        message.contains("GeoTIFF"),
        "the error should explain that georeferencing is missing, got: {message}"
    );
}

#[test]
fn a_missing_input_file_is_reported_rather_than_panicking() {
    let error = RasterSet::load(&[PathBuf::from("does-not-exist.tif")])
        .expect_err("a missing file must be an error");
    assert!(error.to_string().contains("does-not-exist.tif"));
}
