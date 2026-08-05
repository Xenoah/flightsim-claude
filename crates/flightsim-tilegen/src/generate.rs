//! ラスタからタイルを焼く。
//!
//! # 全球を一度に焼かない
//!
//! level 12 を全球で焼くと 3 350 万タイルになる。生データも数百 GB あり、
//! 現実的でない。**対象地域を指定して焼く**のがこのツールの前提（ADR-0003）。

use crate::geotiff::{GeoRaster, RasterError};
use crate::region::Region;
use flightsim_core::{Meters, Radians};
use flightsim_world::dem::io::{TileWriteError, tile_relative_path, write_tile};
use flightsim_world::{HeightGrid, TileId};
use std::path::{Path, PathBuf};

/// 焼き込みの設定。
#[derive(Debug, Clone, Copy)]
pub struct TileGenOptions {
    /// タイル 1 辺の格子点数。
    ///
    /// `2^n + 1` が扱いやすい（隣接 LOD との継ぎ目で頂点が揃うため）。
    pub grid_size: u32,
    /// 被覆の無い格子点を埋める標高。
    ///
    /// 実行時形式に穴の概念は無いので、ここで必ず値を決める（ADR-0005）。
    pub fill: Meters,
}

impl Default for TileGenOptions {
    fn default() -> Self {
        Self {
            grid_size: 65,
            fill: Meters::ZERO,
        }
    }
}

/// 1 タイルの焼き込み結果。
#[derive(Debug, Clone, PartialEq)]
pub struct TileBuild {
    pub grid: HeightGrid,
    /// 元データに被覆が無く、`fill` で埋めた格子点の数。
    pub filled_points: u32,
}

/// 複数の入力ラスタ。最初に値を返したものを採用する。
#[derive(Debug, Default)]
pub struct RasterSet {
    rasters: Vec<GeoRaster>,
}

impl RasterSet {
    #[must_use]
    pub const fn new(rasters: Vec<GeoRaster>) -> Self {
        Self { rasters }
    }

    /// GeoTIFF を順に読み込む。
    ///
    /// # Errors
    ///
    /// いずれかのファイルが開けない、または GeoTIFF として読めない場合。
    pub fn load(paths: &[PathBuf]) -> Result<Self, RasterError> {
        paths
            .iter()
            .map(|path| GeoRaster::open(path))
            .collect::<Result<Vec<_>, _>>()
            .map(Self::new)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rasters.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rasters.len()
    }

    #[must_use]
    pub fn sample(
        &self,
        position: flightsim_core::Geodetic,
        footprint: (Radians, Radians),
    ) -> Option<Meters> {
        self.rasters
            .iter()
            .find_map(|raster| raster.sample(position, footprint))
    }

    /// 全ラスタを包含する範囲。ラスタが無ければ `None`。
    #[must_use]
    pub fn coverage(&self) -> Option<Region> {
        self.rasters
            .iter()
            .map(|raster| {
                let coverage = raster.coverage();
                Region::from_radians(coverage.west, coverage.south, coverage.east, coverage.north)
            })
            .reduce(Region::union)
    }
}

/// 1 タイルぶんの標高格子を作る。
///
/// どの格子点にも被覆が無ければ `None`（タイル自体を作らない）。
/// 一部だけ欠ける場合は `options.fill` で埋め、その数を報告する。
///
/// # Panics
///
/// `options.grid_size` が 2 未満の場合。呼び出し側で検査すること。
#[must_use]
pub fn build_tile(rasters: &RasterSet, id: TileId, options: &TileGenOptions) -> Option<TileBuild> {
    assert!(
        options.grid_size >= 2,
        "grid_size must be at least 2 for bilinear interpolation, got {}",
        options.grid_size
    );

    let bounds = id.bounds();
    let steps = f64::from(options.grid_size - 1);
    // 各格子点が代表する足跡。元画素より粗ければ面積平均される。
    let footprint = (
        Radians(bounds.width().get() / steps),
        Radians(bounds.height().get() / steps),
    );

    let mut samples = Vec::with_capacity((options.grid_size as usize).pow(2));
    let mut filled_points = 0_u32;
    let mut covered_points = 0_u32;

    for row in 0..options.grid_size {
        for column in 0..options.grid_size {
            let u = f64::from(column) / steps;
            let v = f64::from(row) / steps;
            let position = flightsim_core::Geodetic::new(
                // v = 0 が北端。DEM 格子の行順に合わせている。
                Radians(bounds.north.get() - v * bounds.height().get()),
                Radians(bounds.west.get() + u * bounds.width().get()),
                Meters::ZERO,
            );

            let elevation = match rasters.sample(position, footprint) {
                Some(elevation) => {
                    covered_points += 1;
                    elevation
                }
                None => {
                    filled_points += 1;
                    options.fill
                }
            };

            #[allow(
                clippy::cast_possible_truncation,
                reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で十分"
            )]
            samples.push(elevation.get() as f32);
        }
    }

    if covered_points == 0 {
        return None;
    }

    Some(TileBuild {
        grid: HeightGrid::new(options.grid_size, options.grid_size, samples),
        filled_points,
    })
}

/// 焼き込みのエラー。
#[derive(Debug)]
pub enum GenerateError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Encode {
        tile: TileId,
        source: TileWriteError,
    },
    /// `grid_size` が 2 未満。
    InvalidGridSize(u32),
    /// `min_level > max_level`。
    InvalidLevelRange { min: u8, max: u8 },
}

impl core::fmt::Display for GenerateError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to write {}: {source}", path.display())
            }
            Self::Encode { tile, source } => {
                write!(formatter, "failed to encode tile {tile:?}: {source}")
            }
            Self::InvalidGridSize(size) => write!(
                formatter,
                "grid size {size} is too small; bilinear interpolation needs at least 2 per axis"
            ),
            Self::InvalidLevelRange { min, max } => {
                write!(formatter, "min level {min} is deeper than max level {max}")
            }
        }
    }
}

impl std::error::Error for GenerateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Encode { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// 焼き込みの集計。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GenerationReport {
    pub tiles_written: usize,
    /// 元データの被覆が全く無く、作らなかったタイルの数。
    pub tiles_without_coverage: usize,
    /// `fill` で埋めた格子点の総数。**0 でないなら地形に平坦な穴がある。**
    pub grid_points_filled: u64,
    pub bytes_written: u64,
}

/// 指定範囲・指定レベルのタイルを焼いて出力ディレクトリへ書く。
///
/// `dry_run` が真ならファイルを書かずに集計だけ返す。
///
/// # Errors
///
/// 設定が不正な場合、ディレクトリを作れない場合、書き込みに失敗した場合。
pub fn generate_tiles(
    rasters: &RasterSet,
    region: Region,
    levels: core::ops::RangeInclusive<u8>,
    options: &TileGenOptions,
    output: &Path,
    dry_run: bool,
) -> Result<GenerationReport, GenerateError> {
    if options.grid_size < 2 {
        return Err(GenerateError::InvalidGridSize(options.grid_size));
    }
    if levels.start() > levels.end() {
        return Err(GenerateError::InvalidLevelRange {
            min: *levels.start(),
            max: *levels.end(),
        });
    }

    let mut report = GenerationReport::default();

    for level in levels {
        for id in region.tiles(level) {
            let Some(build) = build_tile(rasters, id, options) else {
                report.tiles_without_coverage += 1;
                continue;
            };

            let mut encoded = Vec::new();
            write_tile(&mut encoded, id, &build.grid)
                .map_err(|source| GenerateError::Encode { tile: id, source })?;

            report.tiles_written += 1;
            report.grid_points_filled += u64::from(build.filled_points);
            report.bytes_written += encoded.len() as u64;

            if dry_run {
                continue;
            }

            let path = output.join(tile_relative_path(id));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| GenerateError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            std::fs::write(&path, &encoded).map_err(|source| GenerateError::Io {
                path: path.clone(),
                source,
            })?;
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の合成ラスタ生成。f32 の精度で十分"
    )]

    use super::*;
    use crate::testing::GeoTiffBuilder;
    use flightsim_core::Degrees;
    use flightsim_world::dem::io::read_tile;
    use std::path::Path as StdPath;

    /// 経度に比例して上る斜面を持つ、指定範囲の合成ラスタ。
    fn ramp_raster(west: f64, north: f64, size: u32, pixel: f64) -> GeoRaster {
        let samples: Vec<f32> = (0..size)
            .flat_map(|row| (0..size).map(move |column| (column * 10 + row) as f32))
            .collect();
        let bytes = GeoTiffBuilder::new(size, size, samples)
            .origin(west, north)
            .pixel_size(pixel, pixel)
            .build();
        GeoRaster::decode(std::io::Cursor::new(bytes), StdPath::new("<memory>"))
            .expect("the synthetic raster should decode")
    }

    fn covering_region(raster: &GeoRaster) -> Region {
        let coverage = raster.coverage();
        Region::from_radians(coverage.west, coverage.south, coverage.east, coverage.north)
    }

    // --- タイル生成 ---

    #[test]
    fn a_tile_fully_inside_the_raster_has_nothing_filled() {
        let raster = ramp_raster(139.0, 36.0, 64, 0.05);
        let rasters = RasterSet::new(vec![raster]);

        // ラスタ内部に完全に収まる小さなタイルを選ぶ。
        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(34.5, 140.5, 0.0));
        let build = build_tile(&rasters, id, &TileGenOptions::default()).expect("covered");

        assert_eq!(
            build.filled_points, 0,
            "a tile inside the raster should need no fill"
        );
    }

    #[test]
    fn a_tile_outside_the_raster_is_not_created_at_all() {
        // 被覆ゼロのタイルまで焼くと、海の底が平坦な板で埋め尽くされる。
        let rasters = RasterSet::new(vec![ramp_raster(139.0, 36.0, 32, 0.05)]);
        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(-40.0, -70.0, 0.0));

        assert!(build_tile(&rasters, id, &TileGenOptions::default()).is_none());
    }

    #[test]
    fn a_partially_covered_tile_is_filled_and_reported() {
        let rasters = RasterSet::new(vec![ramp_raster(139.0, 36.0, 16, 0.05)]);
        // ラスタの南東角にまたがるタイル。
        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(35.2, 139.79, 0.0));

        let build =
            build_tile(&rasters, id, &TileGenOptions::default()).expect("partially covered");
        assert!(
            build.filled_points > 0,
            "a tile straddling the raster edge should report filled points"
        );
    }

    #[test]
    fn the_fill_value_is_what_lands_in_uncovered_points() {
        let rasters = RasterSet::new(vec![ramp_raster(139.0, 36.0, 16, 0.05)]);
        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(35.2, 139.79, 0.0));

        let options = TileGenOptions {
            grid_size: 17,
            fill: Meters(-1_234.0),
        };
        let build = build_tile(&rasters, id, &options).expect("partially covered");

        let (min, _) = build.grid.elevation_range();
        assert!(
            (min.get() + 1_234.0).abs() < 1e-3,
            "the fill value should appear in the grid, minimum was {min}"
        );
    }

    #[test]
    fn tiles_follow_the_grid_size_option() {
        let rasters = RasterSet::new(vec![ramp_raster(139.0, 36.0, 64, 0.05)]);
        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(34.5, 140.5, 0.0));

        for grid_size in [2_u32, 5, 33, 65] {
            let build = build_tile(
                &rasters,
                id,
                &TileGenOptions {
                    grid_size,
                    ..TileGenOptions::default()
                },
            )
            .expect("covered");
            assert_eq!(
                (build.grid.width(), build.grid.height()),
                (grid_size, grid_size)
            );
        }
    }

    #[test]
    fn the_grid_is_oriented_with_north_first() {
        // 行順を取り違えると地形が南北反転し、山と谷が入れ替わる。
        // 北で高く南で低いラスタを作って確かめる。
        let size = 32_u32;
        let samples: Vec<f32> = (0..size)
            .flat_map(|row| (0..size).map(move |_| (size - row) as f32 * 100.0))
            .collect();
        let bytes = GeoTiffBuilder::new(size, size, samples)
            .origin(139.0, 36.0)
            .pixel_size(0.05, 0.05)
            .build();
        let raster = GeoRaster::decode(std::io::Cursor::new(bytes), StdPath::new("<memory>"))
            .expect("decode");
        let rasters = RasterSet::new(vec![raster]);

        let id = TileId::containing(9, flightsim_core::Geodetic::from_degrees(35.2, 139.5, 0.0));
        let build = build_tile(&rasters, id, &TileGenOptions::default()).expect("covered");

        let north_edge = build.grid.sample_at(build.grid.width() / 2, 0).get();
        let south_edge = build
            .grid
            .sample_at(build.grid.width() / 2, build.grid.height() - 1)
            .get();
        assert!(
            north_edge > south_edge,
            "the northern row sampled {north_edge} m and the southern {south_edge} m; \
             the grid may be flipped"
        );
    }

    #[test]
    fn later_rasters_fill_gaps_left_by_earlier_ones() {
        let west = ramp_raster(0.0, 10.0, 16, 0.1);
        let east = ramp_raster(10.0, 10.0, 16, 0.1);
        let rasters = RasterSet::new(vec![west, east]);

        let footprint = (Degrees(0.01).to_radians(), Degrees(0.01).to_radians());
        assert!(
            rasters
                .sample(
                    flightsim_core::Geodetic::from_degrees(9.5, 0.5, 0.0),
                    footprint
                )
                .is_some()
        );
        assert!(
            rasters
                .sample(
                    flightsim_core::Geodetic::from_degrees(9.5, 10.5, 0.0),
                    footprint
                )
                .is_some()
        );
        assert_eq!(rasters.len(), 2);
    }

    #[test]
    fn the_coverage_of_a_set_spans_all_of_its_rasters() {
        let rasters = RasterSet::new(vec![
            ramp_raster(0.0, 10.0, 8, 0.1),
            ramp_raster(10.0, 10.0, 8, 0.1),
        ]);
        let coverage = rasters.coverage().expect("two rasters");

        assert!((coverage.west().to_degrees().get() - 0.0).abs() < 1e-9);
        assert!((coverage.east().to_degrees().get() - 10.8).abs() < 1e-9);
        assert!(RasterSet::default().coverage().is_none());
    }

    // --- 書き出し ---

    #[test]
    fn generated_tiles_can_be_read_back_and_match_the_source() {
        // このテストが CI で実データを必要としないことが要件そのもの。
        let raster = ramp_raster(139.0, 36.0, 128, 0.02);
        let region = covering_region(&raster);
        let rasters = RasterSet::new(vec![raster]);

        let directory = temporary_directory("tilegen-roundtrip");
        let report = generate_tiles(
            &rasters,
            region,
            9..=9,
            &TileGenOptions::default(),
            &directory,
            false,
        )
        .expect("generation should succeed");

        assert!(report.tiles_written > 0, "no tiles were written");
        assert!(report.bytes_written > 0);

        // 書いたタイルを全部読み戻し、元ラスタの標高と突き合わせる。
        let mut checked = 0_u32;
        for id in region.tiles(9) {
            let path = directory.join(tile_relative_path(id));
            let Ok(bytes) = std::fs::read(&path) else {
                continue; // 被覆が無く書かれなかったタイル
            };
            let stored = read_tile(&mut bytes.as_slice()).expect("a tile we just wrote must read");
            assert_eq!(stored.id, id);

            let centre = id.center();
            // 縁のタイルは一部の格子点にしか被覆が無く、中心が範囲外のことがある。
            // 生成されること自体は正しいので、突き合わせ対象からだけ外す。
            let Some(from_raster) = rasters.sample(
                centre,
                (
                    Radians(id.bounds().width().get() / 64.0),
                    Radians(id.bounds().height().get() / 64.0),
                ),
            ) else {
                continue;
            };
            let from_tile = stored.tile.elevation_at(centre);

            assert!(
                (from_tile.get() - from_raster.get()).abs() < 1.0,
                "tile {id:?} centre reads {from_tile} m but the raster says {from_raster} m"
            );
            checked += 1;
        }
        assert!(checked > 0, "no generated tile was verified");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_dry_run_writes_nothing_but_still_counts() {
        let raster = ramp_raster(139.0, 36.0, 64, 0.02);
        let region = covering_region(&raster);
        let rasters = RasterSet::new(vec![raster]);

        let directory = temporary_directory("tilegen-dryrun");
        let report = generate_tiles(
            &rasters,
            region,
            9..=9,
            &TileGenOptions::default(),
            &directory,
            true,
        )
        .expect("dry run should succeed");

        assert!(report.tiles_written > 0);
        assert!(
            !directory.exists(),
            "a dry run must not create the output directory"
        );
    }

    #[test]
    fn tiles_land_at_the_documented_paths() {
        let raster = ramp_raster(139.0, 36.0, 64, 0.02);
        let region = covering_region(&raster);
        let rasters = RasterSet::new(vec![raster]);

        let directory = temporary_directory("tilegen-paths");
        generate_tiles(
            &rasters,
            region,
            8..=8,
            &TileGenOptions::default(),
            &directory,
            false,
        )
        .expect("generation");

        let expected = region
            .tiles(8)
            .into_iter()
            .filter(|id| directory.join(tile_relative_path(*id)).exists())
            .count();
        assert!(expected > 0, "no tile landed at its documented path");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn deeper_levels_produce_more_tiles() {
        let raster = ramp_raster(139.0, 36.0, 64, 0.05);
        let region = covering_region(&raster);
        let rasters = RasterSet::new(vec![raster]);

        let shallow = generate_tiles(
            &rasters,
            region,
            7..=7,
            &TileGenOptions::default(),
            StdPath::new("<unused>"),
            true,
        )
        .expect("dry run");
        let deep = generate_tiles(
            &rasters,
            region,
            9..=9,
            &TileGenOptions::default(),
            StdPath::new("<unused>"),
            true,
        )
        .expect("dry run");

        assert!(deep.tiles_written > shallow.tiles_written);
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let rasters = RasterSet::default();
        let region = Region::from_degrees(0.0, 0.0, 1.0, 1.0).expect("valid");

        assert!(matches!(
            generate_tiles(
                &rasters,
                region,
                8..=8,
                &TileGenOptions {
                    grid_size: 1,
                    fill: Meters::ZERO
                },
                StdPath::new("<unused>"),
                true
            ),
            Err(GenerateError::InvalidGridSize(1))
        ));

        // リテラルで `9..=7` と書くと clippy が空レンジとして弾く。
        // ここでは「逆転したレンジを渡した呼び出し側」を再現したいので明示的に作る。
        let inverted = core::ops::RangeInclusive::new(9_u8, 7_u8);
        assert!(matches!(
            generate_tiles(
                &rasters,
                region,
                inverted,
                &TileGenOptions::default(),
                StdPath::new("<unused>"),
                true
            ),
            Err(GenerateError::InvalidLevelRange { min: 9, max: 7 })
        ));
    }

    #[test]
    fn an_empty_raster_set_produces_no_tiles() {
        let report = generate_tiles(
            &RasterSet::default(),
            Region::from_degrees(0.0, 0.0, 1.0, 1.0).expect("valid"),
            8..=8,
            &TileGenOptions::default(),
            StdPath::new("<unused>"),
            true,
        )
        .expect("dry run");

        assert_eq!(report.tiles_written, 0);
        assert!(report.tiles_without_coverage > 0);
    }

    /// テスト用の一時ディレクトリ。プロセス ID と名前で衝突を避ける。
    fn temporary_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }
}
