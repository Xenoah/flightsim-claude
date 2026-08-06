//! 地形から接地平面を作る。
//!
//! # ここが `fdm` と `world` の接点
//!
//! `flightsim-fdm` は `flightsim-world` を参照できない（ARCHITECTURE.md §2）。
//! FDM は地形標高を**引数として**受け取る契約になっており、その引数を組み立てるのが
//! このモジュール（[ADR-0006](../../../../docs/adr/0006-simulation-integration-layer.md)）。
//!
//! # 勾配は有限差分で求める
//!
//! FDM の接地平面は「基準点の標高 + 北・東方向の勾配」で表される。
//! 単一の標高だけでは傾斜地で機体が傾かないため、基準点から一定距離だけ離れた
//! 2 点の標高差から勾配を出す。

use flightsim_core::{Geodetic, LocalFrame, Meters, Ned};
use flightsim_fdm::GroundSlope;
use flightsim_world::{Terrain, TileSource};

/// FDM へ渡す接地平面。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundPlane {
    /// 平面の基準となる測地座標（高度成分は使わない）。
    pub reference: Geodetic,
    /// 基準点の地面標高（楕円体高）。
    pub elevation: Meters,
    /// 基準点まわりの北・東方向の勾配。
    pub slope: GroundSlope,
    /// 地形データから得た値かどうか。
    ///
    /// `false` なら `fallback` を使っている。**海上を「標高 0 m の地面」として
    /// 扱っているのか、データが無いだけなのかを区別できるようにするため**、
    /// この情報を落とさずに持ち回る。
    pub from_terrain: bool,
}

/// 勾配の上限（正接）。
///
/// `tan(60°) ≒ 1.73`。これを超える斜面は着陸できる面ではないので、
/// 物理を発散させないためだけにクランプする。DEM のノイズや被覆境界の段差で
/// 非現実的な値が出ることがある。
const MAX_SLOPE_TANGENT: f64 = 1.732_050_8;

/// 地形から接地平面を組み立てる。
#[derive(Debug, Clone, Copy)]
pub struct GroundSampler {
    probe_distance: Meters,
    fallback_elevation: Meters,
}

impl Default for GroundSampler {
    fn default() -> Self {
        Self {
            // 機体の脚間距離と同程度にする。大きくすると滑走路の起伏が均され、
            // 小さくすると DEM の補間ノイズを拾う（ADR-0006）。
            probe_distance: Meters(10.0),
            // 地形が無い場所は楕円体高 0 m。海上を飛べるようにするため。
            fallback_elevation: Meters::ZERO,
        }
    }
}

impl GroundSampler {
    /// # Panics
    ///
    /// 探査距離が正の有限値でない場合。ゼロ除算で勾配が無限大になるため。
    #[must_use]
    pub fn new(probe_distance: Meters, fallback_elevation: Meters) -> Self {
        assert!(
            probe_distance.get().is_finite() && probe_distance.get() > 0.0,
            "probe distance must be positive and finite, got {probe_distance}"
        );
        Self {
            probe_distance,
            fallback_elevation,
        }
    }

    #[must_use]
    pub const fn probe_distance(self) -> Meters {
        self.probe_distance
    }

    #[must_use]
    pub const fn fallback_elevation(self) -> Meters {
        self.fallback_elevation
    }

    /// 指定位置の直下に接地平面を作る。
    ///
    /// 地形が引けない場合は `fallback_elevation` の水平面を返す
    /// （`from_terrain` が `false` になる）。
    pub fn sample<S: TileSource>(
        &self,
        terrain: &mut Terrain<S>,
        position: Geodetic,
    ) -> GroundPlane {
        // 基準点は機体の真下の地表。高度成分は接地平面の定義に関係しない。
        let reference = Geodetic::new(position.latitude, position.longitude, Meters::ZERO);

        let Some(centre) = terrain.elevation_at(reference) else {
            return GroundPlane {
                reference,
                elevation: self.fallback_elevation,
                slope: GroundSlope::LEVEL,
                from_terrain: false,
            };
        };

        let frame = LocalFrame::new(reference);
        let distance = self.probe_distance.get();

        // 探査点が地形の外に出た場合は中心の値で代用する。
        // そこで勾配を諦めると、被覆の縁で平面が突然水平になる。
        let mut probe = |north: f64, east: f64| -> f64 {
            let offset = frame
                .ned_to_ecef_position(Ned::new(north, east, 0.0))
                .to_geodetic();
            terrain.elevation_at(offset).unwrap_or(centre).get()
        };

        // 借用を分けるため、先に 4 点を集める。
        let north = probe(distance, 0.0);
        let south = probe(-distance, 0.0);
        let east = probe(0.0, distance);
        let west = probe(0.0, -distance);

        let slope = GroundSlope::new(
            clamp_slope((north - south) / (2.0 * distance)),
            clamp_slope((east - west) / (2.0 * distance)),
        );

        GroundPlane {
            reference,
            elevation: centre,
            slope,
            from_terrain: true,
        }
    }
}

/// 勾配を有限かつ現実的な範囲に収める。
///
/// NaN は 0（水平）に倒す。**NaN を通すと接地反力から全状態へ伝播する。**
fn clamp_slope(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(-MAX_SLOPE_TANGENT, MAX_SLOPE_TANGENT)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の標高データ生成。f32 の精度で十分"
    )]

    use super::*;
    use flightsim_world::dem::HeightGrid;
    use flightsim_world::{DemTile, MemoryTileSource, TileId};

    fn terrain_with(tiles: Vec<(TileId, DemTile)>) -> Terrain<MemoryTileSource> {
        let mut source = MemoryTileSource::new();
        for (id, tile) in tiles {
            source.insert(id, tile);
        }
        Terrain::new(source, 8 * 1024 * 1024, 0..=14)
    }

    /// 北へ向かって一定勾配で上る地形のタイル。
    ///
    /// 格子の先頭行が最北端なので、**行番号が増えるほど低く**すれば北上がりになる。
    fn north_rising_tile(id: TileId, size: u32, total_rise: f32) -> DemTile {
        let samples: Vec<f32> = (0..size)
            .flat_map(|row| {
                (0..size).map(move |_| total_rise * (1.0 - row as f32 / (size as f32 - 1.0)))
            })
            .collect();
        DemTile::new(id.bounds(), HeightGrid::new(size, size, samples))
    }

    #[test]
    fn flat_terrain_produces_a_level_plane_at_its_elevation() {
        let id = TileId::new(12, 3_000, 1_500);
        let mut terrain = terrain_with(vec![(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(512.0))),
        )]);

        let plane = GroundSampler::default().sample(&mut terrain, id.center());

        assert!(plane.from_terrain);
        assert!((plane.elevation.get() - 512.0).abs() < 1e-3);
        assert!(plane.slope.north().abs() < 1e-6, "slope {:?}", plane.slope);
        assert!(plane.slope.east().abs() < 1e-6);
    }

    #[test]
    fn a_north_rising_slope_is_reported_as_positive_north() {
        // 符号を取り違えると機体が逆向きに傾く。
        let id = TileId::new(12, 3_000, 1_500);
        let mut terrain = terrain_with(vec![(id, north_rising_tile(id, 65, 1_000.0))]);

        let plane = GroundSampler::default().sample(&mut terrain, id.center());

        assert!(plane.from_terrain);
        assert!(
            plane.slope.north() > 0.0,
            "north-rising terrain reported slope {:?}",
            plane.slope
        );
        assert!(
            plane.slope.east().abs() < 1e-6,
            "no east slope was expected"
        );
    }

    #[test]
    fn the_reported_slope_matches_the_terrain_geometry() {
        // 数値が合っていることまで見る。「符号が合っている」だけでは足りない。
        let id = TileId::new(12, 3_000, 1_500);
        let rise = 500.0_f32;
        let mut terrain = terrain_with(vec![(id, north_rising_tile(id, 129, rise))]);

        let centre = id.center();
        let plane = GroundSampler::default().sample(&mut terrain, centre);

        // タイルの南北方向の実距離から理論勾配を出す。
        let bounds = id.bounds();
        let north_edge = Geodetic::new(bounds.north, centre.longitude, Meters::ZERO);
        let south_edge = Geodetic::new(bounds.south, centre.longitude, Meters::ZERO);
        let span = north_edge.great_circle_distance(south_edge).get();
        let expected = f64::from(rise) / span;

        assert!(
            (plane.slope.north() - expected).abs() < expected * 0.05,
            "slope was {} but the terrain rises {expected} per metre",
            plane.slope.north()
        );
    }

    #[test]
    fn an_east_rising_slope_is_reported_as_positive_east() {
        let id = TileId::new(12, 3_000, 1_500);
        let size = 65_u32;
        let samples: Vec<f32> = (0..size)
            .flat_map(|_| {
                (0..size).map(move |column| 1_000.0 * column as f32 / (size as f32 - 1.0))
            })
            .collect();
        let mut terrain = terrain_with(vec![(
            id,
            DemTile::new(id.bounds(), HeightGrid::new(size, size, samples)),
        )]);

        let plane = GroundSampler::default().sample(&mut terrain, id.center());
        assert!(plane.slope.east() > 0.0, "slope {:?}", plane.slope);
        assert!(plane.slope.north().abs() < 1e-6);
    }

    // --- 地形が無い場合 ---

    #[test]
    fn missing_terrain_falls_back_to_a_level_plane_and_says_so() {
        // 海上を飛べること。かつ「データが無い」ことが呼び出し側に伝わること。
        let mut terrain = terrain_with(vec![]);
        let plane =
            GroundSampler::default().sample(&mut terrain, Geodetic::from_degrees(0.0, -140.0, 0.0));

        assert!(
            !plane.from_terrain,
            "missing data must be visible to callers"
        );
        assert!(plane.elevation.get().abs() < 1e-9);
        assert_eq!(plane.slope, GroundSlope::LEVEL);
    }

    #[test]
    fn a_custom_fallback_elevation_is_used() {
        let mut terrain = terrain_with(vec![]);
        let sampler = GroundSampler::new(Meters(10.0), Meters(-30.0));
        let plane = sampler.sample(&mut terrain, Geodetic::from_degrees(0.0, 0.0, 0.0));

        assert!(!plane.from_terrain);
        assert!((plane.elevation.get() + 30.0).abs() < 1e-9);
    }

    #[test]
    fn probes_that_leave_the_data_do_not_flatten_the_plane_abruptly() {
        // 被覆の縁で探査点が外へ出ても、中心の値で代用して破綻させない。
        let id = TileId::new(12, 3_000, 1_500);
        let mut terrain = terrain_with(vec![(id, north_rising_tile(id, 65, 1_000.0))]);

        // タイルの北端ぎりぎり。北側の探査点は隣のタイル（存在しない）へ出る。
        let bounds = id.bounds();
        let near_edge = Geodetic::new(
            flightsim_core::Radians(bounds.north.get() - bounds.height().get() * 1e-4),
            bounds.center().longitude,
            Meters::ZERO,
        );

        let plane = GroundSampler::default().sample(&mut terrain, near_edge);
        assert!(plane.from_terrain);
        assert!(plane.slope.is_finite(), "slope {:?}", plane.slope);
    }

    // --- 数値の健全性 ---

    #[test]
    fn the_slope_is_always_finite_and_bounded() {
        // 断崖のような極端な地形でも接地反力を発散させない。
        let id = TileId::new(14, 12_000, 6_000);
        let size = 33_u32;
        let samples: Vec<f32> = (0..size)
            .flat_map(|_| {
                (0..size).map(move |column| if column < size / 2 { 0.0 } else { 8_000.0 })
            })
            .collect();
        let mut terrain = terrain_with(vec![(
            id,
            DemTile::new(id.bounds(), HeightGrid::new(size, size, samples)),
        )]);

        let plane = GroundSampler::default().sample(&mut terrain, id.center());
        assert!(plane.slope.is_finite());
        assert!(plane.slope.north().abs() <= MAX_SLOPE_TANGENT + 1e-9);
        assert!(plane.slope.east().abs() <= MAX_SLOPE_TANGENT + 1e-9);
    }

    #[test]
    fn nan_slopes_are_turned_into_level_ground() {
        // NaN を通すと接地反力から全状態へ伝播する。
        assert!((clamp_slope(f64::NAN) - 0.0).abs() < 1e-12);
        assert!((clamp_slope(f64::INFINITY) - MAX_SLOPE_TANGENT).abs() < 1e-9);
        assert!((clamp_slope(f64::NEG_INFINITY) + MAX_SLOPE_TANGENT).abs() < 1e-9);
    }

    #[test]
    fn the_reference_point_sits_on_the_ellipsoid_below_the_aircraft() {
        let id = TileId::new(12, 3_000, 1_500);
        let mut terrain = terrain_with(vec![(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(100.0))),
        )]);

        let aircraft = Geodetic::new(id.center().latitude, id.center().longitude, Meters(3_000.0));
        let plane = GroundSampler::default().sample(&mut terrain, aircraft);

        assert!((plane.reference.altitude.get()).abs() < 1e-12);
        assert!((plane.reference.latitude.get() - aircraft.latitude.get()).abs() < 1e-12);
        assert!((plane.reference.longitude.get() - aircraft.longitude.get()).abs() < 1e-12);
    }

    #[test]
    fn sampling_is_repeatable() {
        // 決定論の前提。キャッシュ状態によらず同じ答えを返すこと。
        let id = TileId::new(12, 3_000, 1_500);
        let mut terrain = terrain_with(vec![(id, north_rising_tile(id, 65, 700.0))]);
        let sampler = GroundSampler::default();

        let first = sampler.sample(&mut terrain, id.center());
        for _ in 0..25 {
            assert_eq!(sampler.sample(&mut terrain, id.center()), first);
        }
    }

    #[test]
    #[should_panic(expected = "probe distance must be positive")]
    fn a_zero_probe_distance_is_rejected() {
        let _ = GroundSampler::new(Meters::ZERO, Meters::ZERO);
    }
}
