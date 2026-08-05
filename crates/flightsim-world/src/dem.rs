//! 数値標高モデル（DEM）の格子とサンプリング。
//!
//! # 実行時に生データを読まない
//!
//! Copernicus DEM は GeoTIFF で配布されるが、**実行時にこれをパースしてはならない**
//! （ADR-0003）。オフラインでタイル境界にクリップし、正規化した格子へ焼いてから読む。
//! GeoTIFF のデコードはフレーム予算に収まらない。
//!
//! # 格子の並び
//!
//! 行優先で、**先頭行が最北端**。タイルの正規化座標 `(u, v)` と直接対応する
//! （`v = 0` が北）。この向きを間違えると地形が南北反転し、山と谷が入れ替わる。

pub mod io;

use crate::tile::GeoBounds;
use flightsim_core::{Geodetic, Meters};

/// 標高格子。
///
/// 標高は `f32` で保持する。地球上の標高は ±9 000 m の範囲で、`f32` の分解能は
/// そこで約 0.001 m。十分な精度でメモリを半分にできる。
/// **世界座標（`f64` ECEF）とは別の話であることに注意**（ADR-0002）。
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    width: u32,
    height: u32,
    /// 行優先、北から南へ。長さは `width * height`。
    samples: Vec<f32>,
}

impl HeightGrid {
    /// # Panics
    ///
    /// 幅・高さが 2 未満の場合、またはサンプル数が `width * height` と一致しない場合に
    /// パニックする。バイリニア補間には各方向 2 点以上が必要。
    #[must_use]
    pub fn new(width: u32, height: u32, samples: Vec<f32>) -> Self {
        assert!(
            width >= 2 && height >= 2,
            "a height grid needs at least 2×2 samples for bilinear interpolation, got {width}×{height}"
        );
        let expected = (width as usize) * (height as usize);
        assert!(
            samples.len() == expected,
            "expected {expected} samples for a {width}×{height} grid, got {}",
            samples.len()
        );
        Self {
            width,
            height,
            samples,
        }
    }

    /// 全点が同じ標高の格子。海面や検査用。
    #[must_use]
    pub fn flat(width: u32, height: u32, elevation: Meters) -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "標高は ±9000 m の範囲で f32 の分解能は約 0.001 m。DEM 格子の表現として十分"
        )]
        let value = elevation.get() as f32;
        Self::new(
            width,
            height,
            vec![value; (width as usize) * (height as usize)],
        )
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// 格子点の標高。範囲外の添字は端にクランプされる。
    #[must_use]
    pub fn sample_at(&self, column: u32, row: u32) -> Meters {
        let column = column.min(self.width - 1) as usize;
        let row = row.min(self.height - 1) as usize;
        Meters(f64::from(
            self.samples[row * (self.width as usize) + column],
        ))
    }

    /// 正規化座標 `(u, v)` におけるバイリニア補間標高。
    ///
    /// `u = 0` が西端、`v = 0` が北端。範囲外は端にクランプされる。
    ///
    /// **格子点上では格子値と厳密に一致する**（補間の基本要件）。
    #[must_use]
    pub fn sample_normalised(&self, u: f64, v: f64) -> Meters {
        // NaN は clamp を素通りするため、先に潰す。
        let u = if u.is_nan() { 0.0 } else { u.clamp(0.0, 1.0) };
        let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };

        let x = u * f64::from(self.width - 1);
        let y = v * f64::from(self.height - 1);

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "u, v は [0,1] にクランプ済みなので x, y は格子範囲内の非負有限値"
        )]
        let (column, row) = (x.floor() as u32, y.floor() as u32);

        let fraction_x = x - f64::from(column);
        let fraction_y = y - f64::from(row);

        let north_west = self.sample_at(column, row).get();
        let north_east = self.sample_at(column + 1, row).get();
        let south_west = self.sample_at(column, row + 1).get();
        let south_east = self.sample_at(column + 1, row + 1).get();

        let north = north_west + (north_east - north_west) * fraction_x;
        let south = south_west + (south_east - south_west) * fraction_x;

        Meters(north + (south - north) * fraction_y)
    }

    /// 最小・最大標高。
    #[must_use]
    pub fn elevation_range(&self) -> (Meters, Meters) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for &sample in &self.samples {
            min = min.min(sample);
            max = max.max(sample);
        }
        (Meters(f64::from(min)), Meters(f64::from(max)))
    }

    /// この格子を 1 段粗い表現で置き換えた場合に生じる最大の標高誤差 `m`。
    ///
    /// LOD 選択に使う幾何誤差そのもの。**定数にしてはいけない。**
    /// 平野と山岳では必要ポリゴン数が桁で違うため、実データから算出する（ADR-0003）。
    ///
    /// 1 つおきの格子点だけを残した粗い格子を作り、元の各点との差の最大値を取る。
    #[must_use]
    pub fn geometric_error(&self) -> Meters {
        let coarse_width = self.width.div_ceil(2);
        let coarse_height = self.height.div_ceil(2);

        // 粗くできない格子（2×2）では、それ以上の簡略化による誤差は定義できない。
        if coarse_width < 2 || coarse_height < 2 {
            return Meters::ZERO;
        }

        let mut coarse_samples = Vec::with_capacity((coarse_width * coarse_height) as usize);
        for row in 0..coarse_height {
            for column in 0..coarse_width {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "元の格子が f32 で保持している値をそのまま複製している"
                )]
                let value = self.sample_at(column * 2, row * 2).get() as f32;
                coarse_samples.push(value);
            }
        }
        let coarse = Self::new(coarse_width, coarse_height, coarse_samples);

        let mut max_error: f64 = 0.0;
        for row in 0..self.height {
            for column in 0..self.width {
                let u = f64::from(column) / f64::from(self.width - 1);
                let v = f64::from(row) / f64::from(self.height - 1);
                let error = (self.sample_at(column, row).get()
                    - coarse.sample_normalised(u, v).get())
                .abs();
                max_error = max_error.max(error);
            }
        }

        Meters(max_error)
    }
}

/// 1 タイル分の標高データ。
#[derive(Debug, Clone, PartialEq)]
pub struct DemTile {
    bounds: GeoBounds,
    grid: HeightGrid,
    geometric_error: Meters,
}

impl DemTile {
    #[must_use]
    pub fn new(bounds: GeoBounds, grid: HeightGrid) -> Self {
        let geometric_error = grid.geometric_error();
        Self {
            bounds,
            grid,
            geometric_error,
        }
    }

    /// 既知の幾何誤差からタイルを組み立てる。
    ///
    /// [`Self::new`] は幾何誤差を格子から再計算する（格子点数に比例するコスト）。
    /// ファイルから読む場合は生成時に算出済みの値が埋まっているので、
    /// **ストリーミング中に再計算しないためにこちらを使う**（ADR-0005）。
    ///
    /// 呼び出し側が渡す値を検証しない。ファイル由来の値は
    /// [`io::read_tile`] が有限性と符号を検査したうえで渡してくる。
    #[must_use]
    pub const fn from_parts(bounds: GeoBounds, grid: HeightGrid, geometric_error: Meters) -> Self {
        Self {
            bounds,
            grid,
            geometric_error,
        }
    }

    #[must_use]
    pub const fn bounds(&self) -> GeoBounds {
        self.bounds
    }

    #[must_use]
    pub const fn grid(&self) -> &HeightGrid {
        &self.grid
    }

    /// このタイルを使わず親で代用した場合の最大標高誤差。LOD 選択に使う。
    #[must_use]
    pub const fn geometric_error(&self) -> Meters {
        self.geometric_error
    }

    /// 測地座標における地形標高。範囲外はタイル端の値にクランプされる。
    ///
    /// FDM はこの値を引数として受け取る（`flightsim-fdm` は `flightsim-world` に
    /// 依存できないため）。
    #[must_use]
    pub fn elevation_at(&self, position: Geodetic) -> Meters {
        let (u, v) = self.bounds.normalise(position);
        self.grid.sample_normalised(u, v)
    }

    /// 概算のメモリ使用量 `バイト`。キャッシュの容量管理に使う。
    #[must_use]
    pub fn memory_footprint(&self) -> usize {
        core::mem::size_of::<Self>()
            + (self.grid.width as usize) * (self.grid.height as usize) * core::mem::size_of::<f32>()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の標高データ生成。格子は最大でも数十点で、f32 の精度で十分"
    )]

    use super::*;
    use crate::tile::TileId;

    /// 西から東へ 0, 100, 200… と増える斜面。補間の検証用。
    fn ramp(width: u32, height: u32) -> HeightGrid {
        let mut samples = Vec::new();
        for _ in 0..height {
            for column in 0..width {
                samples.push(column as f32 * 100.0);
            }
        }
        HeightGrid::new(width, height, samples)
    }

    // --- サンプリング ---

    #[test]
    fn bilinear_sampling_is_exact_at_grid_points() {
        // 補間の基本要件。ここがずれていると地形全体が微妙にずれる。
        let grid = ramp(5, 5);

        for row in 0..5 {
            for column in 0..5 {
                let u = f64::from(column) / 4.0;
                let v = f64::from(row) / 4.0;
                let interpolated = grid.sample_normalised(u, v).get();
                let exact = grid.sample_at(column, row).get();
                assert!(
                    (interpolated - exact).abs() < 1e-9,
                    "at grid point ({column}, {row}) interpolation gave {interpolated} \
                     but the stored value is {exact}"
                );
            }
        }
    }

    #[test]
    fn bilinear_sampling_interpolates_linearly_between_grid_points() {
        let grid = ramp(5, 5);
        // 格子点 0 (=0 m) と 1 (=100 m) のちょうど中間。
        let midpoint = grid.sample_normalised(0.125, 0.0).get();
        assert!(
            (midpoint - 50.0).abs() < 1e-9,
            "midpoint sampled as {midpoint}, expected 50"
        );
    }

    #[test]
    fn flat_terrain_samples_flat_everywhere() {
        let grid = HeightGrid::flat(8, 8, Meters(1_234.5));
        for i in 0..=20 {
            let t = f64::from(i) / 20.0;
            assert!((grid.sample_normalised(t, 1.0 - t).get() - 1_234.5).abs() < 1e-3);
        }
    }

    #[test]
    fn out_of_range_coordinates_clamp_to_the_edge() {
        let grid = ramp(4, 4);
        let west_edge = grid.sample_at(0, 0).get();
        let east_edge = grid.sample_at(3, 0).get();

        assert!((grid.sample_normalised(-5.0, 0.0).get() - west_edge).abs() < 1e-9);
        assert!((grid.sample_normalised(5.0, 0.0).get() - east_edge).abs() < 1e-9);
    }

    #[test]
    fn nan_coordinates_do_not_produce_nan_elevations() {
        // f64::clamp は NaN を素通りさせる。ここで潰さないと標高が NaN になり、
        // 接地判定と地形メッシュの両方が壊れる。
        let grid = ramp(4, 4);
        assert!(grid.sample_normalised(f64::NAN, 0.5).get().is_finite());
        assert!(grid.sample_normalised(0.5, f64::NAN).get().is_finite());
        assert!(grid.sample_normalised(f64::NAN, f64::NAN).get().is_finite());
    }

    #[test]
    fn elevation_range_finds_the_extremes() {
        let grid = ramp(5, 5);
        let (min, max) = grid.elevation_range();
        assert!((min.get() - 0.0).abs() < 1e-9);
        assert!((max.get() - 400.0).abs() < 1e-9);
    }

    // --- 幾何誤差 ---

    #[test]
    fn flat_terrain_has_no_geometric_error() {
        // 平坦なら粗くしても誤差が出ない。細分化する理由がない。
        let grid = HeightGrid::flat(17, 17, Meters(200.0));
        assert!(
            grid.geometric_error().get() < 1e-6,
            "flat terrain reported a geometric error of {}",
            grid.geometric_error()
        );
    }

    #[test]
    fn a_linear_ramp_has_almost_no_geometric_error() {
        // 一定勾配は線形補間で正確に再現できるので、粗くしても誤差はほぼ出ない。
        let grid = ramp(17, 17);
        assert!(
            grid.geometric_error().get() < 1.0,
            "a linear ramp reported a geometric error of {}",
            grid.geometric_error()
        );
    }

    #[test]
    fn rough_terrain_has_a_larger_geometric_error_than_smooth_terrain() {
        // LOD 選択の前提。これが逆転すると平野を過剰に細分化し、山岳で不足する。
        let size = 33;

        let mut smooth = Vec::new();
        let mut rough = Vec::new();
        for row in 0..size {
            for column in 0..size {
                let x = f64::from(column) / f64::from(size - 1);
                let y = f64::from(row) / f64::from(size - 1);
                smooth.push((500.0 * (x + y)) as f32);
                // 高周波成分を持つ地形。粗い格子では再現できない。
                rough.push((500.0 * (x + y) + 300.0 * (x * 40.0).sin() * (y * 40.0).cos()) as f32);
            }
        }

        let smooth_error = HeightGrid::new(size, size, smooth).geometric_error();
        let rough_error = HeightGrid::new(size, size, rough).geometric_error();

        assert!(
            rough_error.get() > smooth_error.get() * 10.0,
            "rough terrain error ({rough_error}) should far exceed smooth terrain error ({smooth_error})"
        );
    }

    #[test]
    fn geometric_error_is_never_negative_or_nan() {
        for grid in [HeightGrid::flat(4, 4, Meters(0.0)), ramp(9, 9), ramp(2, 2)] {
            let error = grid.geometric_error();
            assert!(
                error.is_finite() && error.get() >= 0.0,
                "geometric error was {error}"
            );
        }
    }

    // --- タイルとの結合 ---

    #[test]
    fn elevation_lookup_uses_the_correct_orientation() {
        // 格子の先頭行は最北端。ここを取り違えると地形が南北反転する。
        let tile = TileId::new(4, 8, 5);
        let bounds = tile.bounds();

        // 北の行を 1000 m、南の行を 0 m にする。
        let mut samples = vec![0.0_f32; 9];
        samples[0..3].fill(1_000.0);
        let dem = DemTile::new(bounds, HeightGrid::new(3, 3, samples));

        let north_edge = Geodetic::new(bounds.north, bounds.center().longitude, Meters::ZERO);
        let south_edge = Geodetic::new(bounds.south, bounds.center().longitude, Meters::ZERO);

        assert!(
            (dem.elevation_at(north_edge).get() - 1_000.0).abs() < 1e-6,
            "the northern edge sampled as {} m, expected 1000 m — the grid may be flipped",
            dem.elevation_at(north_edge)
        );
        assert!(dem.elevation_at(south_edge).get().abs() < 1e-6);
    }

    #[test]
    fn memory_footprint_scales_with_the_grid() {
        let small = DemTile::new(
            TileId::new(2, 0, 0).bounds(),
            HeightGrid::flat(16, 16, Meters::ZERO),
        );
        let large = DemTile::new(
            TileId::new(2, 0, 0).bounds(),
            HeightGrid::flat(64, 64, Meters::ZERO),
        );

        assert!(large.memory_footprint() > small.memory_footprint() * 10);
    }

    #[test]
    #[should_panic(expected = "at least 2×2 samples")]
    fn degenerate_grids_are_rejected() {
        let _ = HeightGrid::new(1, 4, vec![0.0; 4]);
    }

    #[test]
    #[should_panic(expected = "expected 16 samples")]
    fn mismatched_sample_counts_are_rejected() {
        let _ = HeightGrid::new(4, 4, vec![0.0; 15]);
    }
}
