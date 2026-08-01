//! 幾何誤差ベースの LOD 選択。
//!
//! # なぜ距離ベースではないのか
//!
//! 距離だけで細分化レベルを決めると、**平野を過剰に細分化し、山岳で不足する。**
//! 同じ距離でも、必要なポリゴン数は地形の起伏によって桁で違う。
//!
//! ここでは各タイルが持つ幾何誤差（そのタイルを親で代用したときに生じる最大標高誤差）を
//! 画面上のピクセル数へ換算し、閾値を超えたら細分化する。
//!
//! ```text
//! sse = (geometric_error × viewport_height) / (distance × 2 × tan(fov / 2))
//! ```
//!
//! 幾何誤差はタイル生成時に実データから算出して埋め込む（[`crate::HeightGrid::geometric_error`]）。
//! **定数にしてはならない。**

use crate::tile::{GeoBounds, TileId};
use flightsim_core::{Ecef, Geodetic, Meters, Radians};

/// 1 回の選択で返すタイル数の上限。
///
/// 設定を誤ると細分化が爆発してフレームが止まる。上限に達した場合は
/// [`LodSelection::truncated`] で明示的に報告し、黙って切り捨てない。
pub const DEFAULT_MAX_TILES: usize = 4_096;

/// LOD 選択の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LodSelection {
    /// 描画すべきタイル。互いに重ならず、全体で地球を覆う。
    pub tiles: Vec<TileId>,
    /// タイル数の上限に達して細分化を打ち切ったか。
    ///
    /// `true` のとき、返されたタイルは要求された精度を満たしていない。
    /// **呼び出し側はこれを無視しないこと。**「全部入っている」という誤解を生む。
    pub truncated: bool,
}

/// LOD 選択器。
#[derive(Debug, Clone, Copy)]
pub struct LodSelector {
    max_screen_space_error: f64,
    viewport_height: f64,
    vertical_fov: Radians,
    max_level: u8,
    root_geometric_error: Meters,
    max_tiles: usize,
}

impl LodSelector {
    /// # Panics
    ///
    /// 各パラメータが正でない場合にパニックする。ゼロや負値は細分化の暴走か
    /// ゼロ除算を引き起こすため、設定ミスとして即座に落とす。
    #[must_use]
    pub fn new(
        max_screen_space_error: f64,
        viewport_height: f64,
        vertical_fov: Radians,
        max_level: u8,
        root_geometric_error: Meters,
    ) -> Self {
        assert!(
            max_screen_space_error > 0.0,
            "max screen-space error must be positive, got {max_screen_space_error}"
        );
        assert!(
            viewport_height > 0.0,
            "viewport height must be positive, got {viewport_height}"
        );
        assert!(
            vertical_fov.get() > 0.0 && vertical_fov.get() < core::f64::consts::PI,
            "vertical field of view must be in (0, π), got {vertical_fov}"
        );
        assert!(
            root_geometric_error.get() > 0.0,
            "root geometric error must be positive, got {root_geometric_error}"
        );
        assert!(
            max_level <= crate::tile::MAX_LEVEL,
            "max level {max_level} exceeds the tiling scheme limit"
        );

        Self {
            max_screen_space_error,
            viewport_height,
            vertical_fov,
            max_level,
            root_geometric_error,
            max_tiles: DEFAULT_MAX_TILES,
        }
    }

    #[must_use]
    pub const fn with_max_tiles(mut self, max_tiles: usize) -> Self {
        self.max_tiles = max_tiles;
        self
    }

    /// 幾何誤差を画面上のピクセル数へ換算する。
    ///
    /// 距離ゼロでの発散を避けるため、分母に下限を設けている。
    #[must_use]
    pub fn screen_space_error(&self, geometric_error: Meters, distance: Meters) -> f64 {
        let half_fov_tangent = (self.vertical_fov.get() * 0.5).tan();
        // カメラが地表に完全に重なる状況（距離ゼロ）でも無限大を返さない。
        let denominator = (distance.get().max(1.0e-3)) * 2.0 * half_fov_tangent;
        geometric_error.get() * self.viewport_height / denominator
    }

    /// そのレベルのタイルが持つ幾何誤差の見積り。
    ///
    /// 1 レベル細分化するごとに半分になるという前提を置く。実測値が手に入る
    /// （タイルが読み込み済みの）場合は、そちらを優先して
    /// [`Self::should_refine_with_error`] を使うこと。
    #[must_use]
    pub fn estimated_geometric_error(&self, level: u8) -> Meters {
        Meters(self.root_geometric_error.get() / f64::from(1u32 << level))
    }

    /// 実測の幾何誤差を用いた細分化判定。
    #[must_use]
    pub fn should_refine_with_error(&self, geometric_error: Meters, distance: Meters) -> bool {
        self.screen_space_error(geometric_error, distance) > self.max_screen_space_error
    }

    /// レベルからの見積りを用いた細分化判定。
    #[must_use]
    pub fn should_refine(&self, level: u8, distance: Meters) -> bool {
        self.should_refine_with_error(self.estimated_geometric_error(level), distance)
    }

    /// カメラ位置から描画すべきタイル集合を選ぶ。
    #[must_use]
    pub fn select(&self, camera: Ecef) -> LodSelection {
        let camera_position = camera.to_geodetic();
        let mut selection = LodSelection {
            tiles: Vec::new(),
            truncated: false,
        };

        for root in TileId::roots() {
            self.refine(root, camera, camera_position, &mut selection);
        }

        selection
    }

    fn refine(
        &self,
        tile: TileId,
        camera: Ecef,
        camera_position: Geodetic,
        selection: &mut LodSelection,
    ) {
        if selection.tiles.len() >= self.max_tiles {
            selection.truncated = true;
            return;
        }

        let distance = distance_to_bounds(camera, camera_position, tile.bounds());

        if tile.level >= self.max_level || !self.should_refine(tile.level, distance) {
            selection.tiles.push(tile);
            return;
        }

        match tile.children() {
            Some(children) => {
                for child in children {
                    self.refine(child, camera, camera_position, selection);
                }
            }
            None => selection.tiles.push(tile),
        }
    }
}

/// カメラから範囲内の最近点までの距離。
///
/// 経度は日付変更線をまたいで循環するため、単純なクランプでは正しく求まらない。
/// 範囲の中心からの符号付き角差を `[-π, π]` に正規化してから判定している。
#[must_use]
pub fn distance_to_bounds(camera: Ecef, camera_position: Geodetic, bounds: GeoBounds) -> Meters {
    let latitude = camera_position
        .latitude
        .get()
        .clamp(bounds.south.get(), bounds.north.get());

    let center_longitude = (bounds.west.get() + bounds.east.get()) * 0.5;
    let half_width = bounds.width().get() * 0.5;

    // 中心からの符号付き角差。これを使えば ±180° の折り返しが自然に処理される。
    let offset = Radians(camera_position.longitude.get() - center_longitude)
        .wrap_signed()
        .get();
    let clamped_offset = offset.clamp(-half_width, half_width);
    let longitude = center_longitude + clamped_offset;

    let nearest = Geodetic::new(Radians(latitude), Radians(longitude), Meters::ZERO).to_ecef();
    camera.distance_to(nearest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;

    fn selector() -> LodSelector {
        LodSelector::new(
            16.0,                       // 許容 16 ピクセル
            1_080.0,                    // ビューポート高さ
            Degrees(60.0).to_radians(), // 垂直画角
            12,                         // 最大レベル
            Meters(20_000.0),           // level 0 の幾何誤差
        )
    }

    fn camera_over(latitude: f64, longitude: f64, altitude: f64) -> Ecef {
        Geodetic::from_degrees(latitude, longitude, altitude).to_ecef()
    }

    // --- screen-space error ---

    #[test]
    fn screen_space_error_falls_with_distance() {
        let selector = selector();
        let near = selector.screen_space_error(Meters(100.0), Meters(1_000.0));
        let far = selector.screen_space_error(Meters(100.0), Meters(10_000.0));

        assert!(far < near);
        // 距離 10 倍で誤差は 1/10。
        assert!((near / far - 10.0).abs() < 1e-9);
    }

    #[test]
    fn screen_space_error_scales_with_geometric_error() {
        let selector = selector();
        let small = selector.screen_space_error(Meters(10.0), Meters(5_000.0));
        let large = selector.screen_space_error(Meters(50.0), Meters(5_000.0));
        assert!((large / small - 5.0).abs() < 1e-9);
    }

    #[test]
    fn zero_distance_does_not_produce_infinity() {
        // カメラが地表に重なる状況でも無限大を返さないこと。
        let error = selector().screen_space_error(Meters(100.0), Meters(0.0));
        assert!(error.is_finite(), "zero distance produced {error}");
    }

    #[test]
    fn geometric_error_halves_with_each_level() {
        let selector = selector();
        for level in 0..10 {
            let coarse = selector.estimated_geometric_error(level).get();
            let fine = selector.estimated_geometric_error(level + 1).get();
            assert!((coarse / fine - 2.0).abs() < 1e-9);
        }
    }

    // --- 単調性 ---

    #[test]
    fn refinement_is_monotonic_in_distance() {
        // カメラが近づいて LOD が粗くなることがあってはならない。
        let selector = selector();
        for level in 0..12 {
            let mut previously_refined = true;
            for kilometres in 1..500 {
                let distance = Meters(f64::from(kilometres) * 1_000.0);
                let refined = selector.should_refine(level, distance);
                assert!(
                    previously_refined || !refined,
                    "at level {level}, refinement resumed at {kilometres} km after having stopped"
                );
                previously_refined = refined;
            }
        }
    }

    #[test]
    fn moving_closer_never_reduces_the_selected_level() {
        let selector = selector();
        let mut previous_max_level = 0;

        for altitude in [200_000.0, 100_000.0, 50_000.0, 20_000.0, 5_000.0, 1_000.0] {
            let selection = selector.select(camera_over(35.0, 139.0, altitude));
            let max_level = selection.tiles.iter().map(|t| t.level).max().unwrap_or(0);

            assert!(
                max_level >= previous_max_level,
                "descending to {altitude} m reduced the maximum level from \
                 {previous_max_level} to {max_level}"
            );
            previous_max_level = max_level;
        }
    }

    // --- 距離計算 ---

    #[test]
    fn a_camera_inside_the_bounds_measures_only_its_altitude() {
        let tile = TileId::new(6, 40, 20);
        let center = tile.center();
        let camera = Geodetic::new(center.latitude, center.longitude, Meters(3_000.0)).to_ecef();

        let distance = distance_to_bounds(camera, camera.to_geodetic(), tile.bounds());
        assert!(
            (distance.get() - 3_000.0).abs() < 1.0,
            "a camera directly above the tile measured {distance} instead of its altitude"
        );
    }

    #[test]
    fn distance_across_the_dateline_uses_the_short_way_round() {
        // 単純な経度クランプだとここで地球一周分の距離を返す。
        let level = 4;
        // 日付変更線のすぐ西のタイル。
        let tile = TileId::containing(level, Geodetic::from_degrees(0.0, -179.0, 0.0));
        // カメラは日付変更線のすぐ東。実距離は 100 km 程度。
        let camera = camera_over(0.0, 179.0, 1_000.0);

        let distance = distance_to_bounds(camera, camera.to_geodetic(), tile.bounds());
        assert!(
            distance.get() < 1_000_000.0,
            "the dateline made a nearby tile look {:.0} km away",
            distance.get() / 1_000.0
        );
    }

    #[test]
    fn distance_is_never_negative_or_nan() {
        for level in [0, 3, 8] {
            for (latitude, longitude) in [(0.0, 0.0), (89.9, 179.9), (-89.9, -179.9), (45.0, 90.0)]
            {
                let camera = camera_over(latitude, longitude, 10_000.0);
                for tile in [
                    TileId::new(level, 0, 0),
                    TileId::containing(level, camera.to_geodetic()),
                ] {
                    let distance = distance_to_bounds(camera, camera.to_geodetic(), tile.bounds());
                    assert!(
                        distance.is_finite() && distance.get() >= 0.0,
                        "distance was {distance} for {tile:?} from ({latitude}, {longitude})"
                    );
                }
            }
        }
    }

    // --- タイル集合の性質 ---

    #[test]
    fn the_selection_covers_the_globe_without_overlap() {
        // 選択されたタイルの面積の合計が地球全体（4π ステラジアン相当の
        // 経緯度矩形面積 = 2π × π）と一致すること。
        // 重なりや隙間があるとここでずれる。
        let selection = selector().select(camera_over(35.0, 139.0, 30_000.0));

        let total: f64 = selection
            .tiles
            .iter()
            .map(|t| {
                let b = t.bounds();
                b.width().get() * b.height().get()
            })
            .sum();

        let whole_globe = core::f64::consts::TAU * core::f64::consts::PI;
        assert!(
            (total - whole_globe).abs() < 1e-9,
            "selected tiles cover {total} of the expected {whole_globe}"
        );
        assert!(!selection.truncated);
    }

    #[test]
    fn tiles_near_the_camera_are_finer_than_tiles_on_the_far_side() {
        let camera_position = Geodetic::from_degrees(35.0, 139.0, 5_000.0);
        let selection = selector().select(camera_position.to_ecef());

        let nearest = selection
            .tiles
            .iter()
            .filter(|t| t.bounds().contains(camera_position))
            .map(|t| t.level)
            .max()
            .expect("the camera must be inside some selected tile");

        // 地球の反対側。
        let antipode = Geodetic::from_degrees(-35.0, -41.0, 0.0);
        let far = selection
            .tiles
            .iter()
            .filter(|t| t.bounds().contains(antipode))
            .map(|t| t.level)
            .max()
            .expect("the antipode must be inside some selected tile");

        assert!(
            nearest > far,
            "tiles under the camera (level {nearest}) should be finer than \
             tiles on the far side of the globe (level {far})"
        );
    }

    #[test]
    fn the_tile_budget_is_reported_when_reached() {
        // 上限に達したことを黙って隠さないこと。
        // 「全部入っている」という誤解は、原因の分かりにくい描画欠落を生む。
        let selection = selector()
            .with_max_tiles(16)
            .select(camera_over(35.0, 139.0, 500.0));

        assert!(
            selection.truncated,
            "a 16-tile budget at 500 m altitude should have been exhausted"
        );
        assert!(
            selection.tiles.len() <= 16 + 4,
            "budget overshoot was {}",
            selection.tiles.len()
        );
    }

    #[test]
    fn selection_respects_the_maximum_level() {
        let selector = LodSelector::new(
            1.0, // 極端に厳しい閾値
            2_160.0,
            Degrees(60.0).to_radians(),
            5, // ただし最大レベルは 5
            Meters(20_000.0),
        )
        .with_max_tiles(100_000);

        let selection = selector.select(camera_over(0.0, 0.0, 100.0));
        assert!(selection.tiles.iter().all(|t| t.level <= 5));
    }

    #[test]
    fn selection_works_at_the_poles() {
        // 極でも地球全体を覆えること。地理座標系タイルを選んだ理由そのもの。
        for latitude in [89.9, -89.9] {
            let selection = selector().select(camera_over(latitude, 0.0, 10_000.0));
            let total: f64 = selection
                .tiles
                .iter()
                .map(|t| t.bounds().width().get() * t.bounds().height().get())
                .sum();
            let whole_globe = core::f64::consts::TAU * core::f64::consts::PI;
            assert!(
                (total - whole_globe).abs() < 1e-9,
                "coverage broke down at latitude {latitude}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "max screen-space error must be positive")]
    fn zero_error_threshold_is_rejected() {
        // 閾値ゼロは常に細分化を要求し、最大レベルまで爆発する。
        let _ = LodSelector::new(
            0.0,
            1_080.0,
            Degrees(60.0).to_radians(),
            12,
            Meters(20_000.0),
        );
    }
}
