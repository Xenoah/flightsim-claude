//! 地理座標系クアッドツリーによるタイル分割。
//!
//! # なぜ Web メルカトルではないのか
//!
//! Web メルカトル（地図タイルの標準）は緯度 ±85.05° で打ち切られる。極が表現できない。
//! 「地球全体を飛べる」という要件と両立しないため、地理座標系（等緯度経度）を採用する。
//!
//! # スキーム
//!
//! Cesium の geographic tiling scheme と同一。将来、商用の 3D Tiles を併用したくなった
//! 際にタイル索引を作り直さずに済むよう、意図的に互換を保っている（ADR-0003）。
//!
//! ```text
//! level 0:  2 × 1 タイル
//!   (0,0) → 経度 [-180°, 0°]   緯度 [-90°, 90°]
//!   (1,0) → 経度 [   0°, 180°] 緯度 [-90°, 90°]
//!
//! level n:  2^(n+1) × 2^n タイル
//! ```
//!
//! `x` は西から東へ、`y` は**北から南へ**増える（`y = 0` が最北端の行）。

use core::f64::consts::{PI, TAU};
use flightsim_core::{Geodetic, Meters, Radians};

/// 扱える最大レベル。
///
/// level 24 で 1 タイルの経度幅は約 0.0000107°（赤道で約 1.2 m）。
/// `u32` の座標に収まる範囲でもあり、実用上これ以上細かくする必要はない。
pub const MAX_LEVEL: u8 = 24;

/// タイルの識別子。
///
/// `Ord` を導出しているのは、優先度キューでの同順位の決着とテストの再現性のため。
/// 順序自体に地理的な意味はない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    pub level: u8,
    /// 西から東へ増える。`0 <= x < 2^(level+1)`
    pub x: u32,
    /// **北から南へ**増える。`0 <= y < 2^level`
    pub y: u32,
}

/// 隣接タイルの方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl TileId {
    /// # Panics
    ///
    /// `level` が [`MAX_LEVEL`] を超える場合、または `x` / `y` が
    /// そのレベルの範囲外の場合にパニックする。範囲外のタイル ID は
    /// 静かに誤った地形を読み込む原因になるため、構築時に落とす。
    #[must_use]
    pub fn new(level: u8, x: u32, y: u32) -> Self {
        assert!(
            level <= MAX_LEVEL,
            "level {level} exceeds MAX_LEVEL ({MAX_LEVEL})"
        );
        let (columns, rows) = (Self::columns(level), Self::rows(level));
        assert!(
            x < columns,
            "x {x} is out of range for level {level} (max {})",
            columns - 1
        );
        assert!(
            y < rows,
            "y {y} is out of range for level {level} (max {})",
            rows - 1
        );
        Self { level, x, y }
    }

    /// そのレベルの東西方向のタイル数 `2^(level+1)`。
    #[must_use]
    pub fn columns(level: u8) -> u32 {
        1u32 << (level + 1)
    }

    /// そのレベルの南北方向のタイル数 `2^level`。
    #[must_use]
    pub fn rows(level: u8) -> u32 {
        1u32 << level
    }

    /// level 0 の 2 タイル。走査の起点。
    #[must_use]
    pub fn roots() -> [Self; 2] {
        [Self::new(0, 0, 0), Self::new(0, 1, 0)]
    }

    /// タイルの地理的範囲。
    #[must_use]
    pub fn bounds(self) -> GeoBounds {
        let width = TAU / f64::from(Self::columns(self.level));
        let height = PI / f64::from(Self::rows(self.level));

        let west = -PI + f64::from(self.x) * width;
        let north = core::f64::consts::FRAC_PI_2 - f64::from(self.y) * height;

        GeoBounds {
            west: Radians(west),
            south: Radians(north - height),
            east: Radians(west + width),
            north: Radians(north),
        }
    }

    /// 親タイル。level 0 では `None`。
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        if self.level == 0 {
            return None;
        }
        Some(Self {
            level: self.level - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// 4 つの子タイル。順序は北西・北東・南西・南東。
    ///
    /// [`MAX_LEVEL`] のタイルでは `None`。
    #[must_use]
    pub fn children(self) -> Option<[Self; 4]> {
        if self.level >= MAX_LEVEL {
            return None;
        }
        let level = self.level + 1;
        let (x, y) = (self.x * 2, self.y * 2);
        Some([
            Self { level, x, y },
            Self { level, x: x + 1, y },
            Self { level, x, y: y + 1 },
            Self {
                level,
                x: x + 1,
                y: y + 1,
            },
        ])
    }

    /// 隣接タイル。
    ///
    /// **東西は日付変更線をまたいで循環する。** 南北は極で打ち切られ `None` を返す
    /// （極を越えた先は経度が 180° 反転するため、単純な隣接では表現できない）。
    #[must_use]
    pub fn neighbour(self, direction: Direction) -> Option<Self> {
        let columns = Self::columns(self.level);
        let rows = Self::rows(self.level);

        match direction {
            // 剰余により経度 ±180° をまたいで繋がる。
            Direction::East => Some(Self {
                x: (self.x + 1) % columns,
                ..self
            }),
            Direction::West => Some(Self {
                x: (self.x + columns - 1) % columns,
                ..self
            }),
            Direction::North => (self.y > 0).then(|| Self {
                y: self.y - 1,
                ..self
            }),
            Direction::South => (self.y + 1 < rows).then(|| Self {
                y: self.y + 1,
                ..self
            }),
        }
    }

    /// 指定した測地座標を含むタイル。
    #[must_use]
    pub fn containing(level: u8, position: Geodetic) -> Self {
        let columns = Self::columns(level);
        let rows = Self::rows(level);

        let normalised_longitude = position.longitude.wrap_signed().get();
        let latitude = position
            .latitude
            .get()
            .clamp(-core::f64::consts::FRAC_PI_2, core::f64::consts::FRAC_PI_2);

        let x_fraction = (normalised_longitude + PI) / TAU * f64::from(columns);
        let y_fraction = (core::f64::consts::FRAC_PI_2 - latitude) / PI * f64::from(rows);

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamp により 0..columns / 0..rows の有限値であることが保証されている"
        )]
        let x = x_fraction.floor().clamp(0.0, f64::from(columns - 1)) as u32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamp により 0..columns / 0..rows の有限値であることが保証されている"
        )]
        let y = y_fraction.floor().clamp(0.0, f64::from(rows - 1)) as u32;

        Self { level, x, y }
    }

    /// タイル中心の測地座標（高度ゼロ）。
    #[must_use]
    pub fn center(self) -> Geodetic {
        self.bounds().center()
    }
}

/// 経度緯度で表した矩形範囲。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBounds {
    pub west: Radians,
    pub south: Radians,
    pub east: Radians,
    pub north: Radians,
}

impl GeoBounds {
    #[must_use]
    pub fn width(self) -> Radians {
        Radians(self.east.get() - self.west.get())
    }

    #[must_use]
    pub fn height(self) -> Radians {
        Radians(self.north.get() - self.south.get())
    }

    #[must_use]
    pub fn center(self) -> Geodetic {
        Geodetic::new(
            Radians((self.south.get() + self.north.get()) * 0.5),
            Radians((self.west.get() + self.east.get()) * 0.5),
            Meters::ZERO,
        )
    }

    /// 範囲内に含まれるか。西端・北端を含み、東端・南端を含まない半開区間。
    ///
    /// 半開にしているのは、隣接タイルの境界上の点が両方のタイルに属さないようにするため。
    #[must_use]
    pub fn contains(self, position: Geodetic) -> bool {
        let longitude = position.longitude.wrap_signed().get();
        let latitude = position.latitude.get();

        longitude >= self.west.get()
            && longitude < self.east.get()
            && latitude > self.south.get()
            && latitude <= self.north.get()
    }

    /// 範囲内の位置を `[0, 1]` の正規化座標へ写す。
    ///
    /// `u = 0` が西端、`v = 0` が**北端**（DEM の行順に合わせている）。
    #[must_use]
    pub fn normalise(self, position: Geodetic) -> (f64, f64) {
        let longitude = position.longitude.wrap_signed().get();
        let u = (longitude - self.west.get()) / self.width().get();
        let v = (self.north.get() - position.latitude.get()) / self.height().get();
        (u, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn degrees(value: f64) -> f64 {
        value.to_radians()
    }

    // --- レベル 0 ---

    #[test]
    fn level_zero_covers_the_whole_globe_in_two_tiles() {
        let [west_tile, east_tile] = TileId::roots();

        let west = west_tile.bounds();
        assert!((west.west.get() - degrees(-180.0)).abs() < 1e-12);
        assert!((west.east.get() - 0.0).abs() < 1e-12);
        assert!((west.south.get() - degrees(-90.0)).abs() < 1e-12);
        assert!((west.north.get() - degrees(90.0)).abs() < 1e-12);

        let east = east_tile.bounds();
        assert!((east.west.get() - 0.0).abs() < 1e-12);
        assert!((east.east.get() - degrees(180.0)).abs() < 1e-12);
    }

    #[test]
    fn tile_counts_follow_the_scheme() {
        for level in 0..=10 {
            assert_eq!(TileId::columns(level), 2u32.pow(u32::from(level) + 1));
            assert_eq!(TileId::rows(level), 2u32.pow(u32::from(level)));
            // 東西は南北の 2 倍。等緯度経度なのでタイルは正方形になる。
            assert_eq!(TileId::columns(level), TileId::rows(level) * 2);
        }
    }

    #[test]
    fn tiles_are_square_in_degrees() {
        for level in 0..=8 {
            let bounds = TileId::new(level, 0, 0).bounds();
            assert!(
                (bounds.width().get() - bounds.height().get()).abs() < 1e-12,
                "tiles at level {level} are not square: {} × {}",
                bounds.width().to_degrees(),
                bounds.height().to_degrees()
            );
        }
    }

    // --- 階層関係 ---

    #[test]
    fn children_tile_the_parent_exactly() {
        for tile in [
            TileId::new(0, 0, 0),
            TileId::new(3, 5, 2),
            TileId::new(7, 100, 60),
        ] {
            let parent_bounds = tile.bounds();
            let children = tile.children().expect("not at max level");

            // 4 つの子の面積の合計が親と一致する。
            let child_area: f64 = children
                .iter()
                .map(|c| c.bounds().width().get() * c.bounds().height().get())
                .sum();
            let parent_area = parent_bounds.width().get() * parent_bounds.height().get();
            assert!(
                (child_area - parent_area).abs() < 1e-12,
                "children of {tile:?} do not tile the parent"
            );

            // 全ての子が親に含まれる。
            for child in children {
                assert_eq!(
                    child.parent(),
                    Some(tile),
                    "{child:?} does not point back to {tile:?}"
                );
            }
        }
    }

    #[test]
    fn root_tiles_have_no_parent() {
        for tile in TileId::roots() {
            assert_eq!(tile.parent(), None);
        }
    }

    #[test]
    fn max_level_tiles_have_no_children() {
        assert_eq!(TileId::new(MAX_LEVEL, 0, 0).children(), None);
    }

    // --- 座標変換の往復 ---

    #[test]
    fn containing_and_bounds_are_consistent_worldwide() {
        for level in [0, 1, 4, 8, 12] {
            for latitude in [-89.9, -45.0, -0.001, 0.0, 0.001, 45.0, 89.9] {
                for longitude in [-179.9, -90.0, -0.001, 0.0, 0.001, 90.0, 179.9] {
                    let position = Geodetic::from_degrees(latitude, longitude, 0.0);
                    let tile = TileId::containing(level, position);

                    assert!(
                        tile.bounds().contains(position),
                        "tile {tile:?} at level {level} does not contain ({latitude}, {longitude})"
                    );
                }
            }
        }
    }

    #[test]
    fn tile_centres_map_back_to_the_same_tile() {
        for level in [0, 2, 5, 9] {
            let columns = TileId::columns(level);
            let rows = TileId::rows(level);
            for x in [0, columns / 3, columns - 1] {
                for y in [0, rows / 2, rows - 1] {
                    let tile = TileId::new(level, x, y);
                    assert_eq!(
                        TileId::containing(level, tile.center()),
                        tile,
                        "the centre of {tile:?} resolved to a different tile"
                    );
                }
            }
        }
    }

    // --- 特異点 ---

    #[test]
    fn the_dateline_wraps_around() {
        // 地形コードの定番の欠陥箇所。経度 ±180° をまたいでタイルが繋がること。
        for level in [0, 1, 5, 10] {
            let columns = TileId::columns(level);
            let easternmost = TileId::new(level, columns - 1, 0);
            let westernmost = TileId::new(level, 0, 0);

            assert_eq!(
                easternmost.neighbour(Direction::East),
                Some(westernmost),
                "the easternmost tile at level {level} does not wrap to the west"
            );
            assert_eq!(
                westernmost.neighbour(Direction::West),
                Some(easternmost),
                "the westernmost tile at level {level} does not wrap to the east"
            );
        }
    }

    #[test]
    fn longitudes_just_either_side_of_the_dateline_land_in_adjacent_tiles() {
        let level = 6;
        let east = TileId::containing(level, Geodetic::from_degrees(0.0, 179.99, 0.0));
        let west = TileId::containing(level, Geodetic::from_degrees(0.0, -179.99, 0.0));

        assert_eq!(east.neighbour(Direction::East), Some(west));
        assert_eq!(west.neighbour(Direction::West), Some(east));
    }

    #[test]
    fn the_poles_terminate_instead_of_wrapping() {
        // 極を越えた先は経度が 180° 反転するので、単純な隣接では表現できない。
        // 循環させてしまうと、北極の向こう側の地形が繋がって見える。
        for level in [1, 4, 9] {
            let rows = TileId::rows(level);
            assert_eq!(TileId::new(level, 0, 0).neighbour(Direction::North), None);
            assert_eq!(
                TileId::new(level, 0, rows - 1).neighbour(Direction::South),
                None
            );
        }
    }

    #[test]
    fn extreme_positions_resolve_without_panicking() {
        // 経度 ±180°、緯度 ±90° ちょうど。範囲外への添字アクセスを起こさないこと。
        for level in [0, 3, 12] {
            for (latitude, longitude) in [
                (90.0, 180.0),
                (90.0, -180.0),
                (-90.0, 180.0),
                (-90.0, -180.0),
                (0.0, 180.0),
            ] {
                let tile =
                    TileId::containing(level, Geodetic::from_degrees(latitude, longitude, 0.0));
                assert!(tile.x < TileId::columns(level));
                assert!(tile.y < TileId::rows(level));
            }
        }
    }

    #[test]
    fn out_of_range_longitudes_are_normalised() {
        // 何周した経度でも正しいタイルに解決すること。
        let reference = TileId::containing(8, Geodetic::from_degrees(35.0, 139.0, 0.0));
        for turns in -3..=3 {
            let longitude = 139.0 + f64::from(turns) * 360.0;
            assert_eq!(
                TileId::containing(8, Geodetic::from_degrees(35.0, longitude, 0.0)),
                reference,
                "longitude {longitude}° did not normalise to the same tile"
            );
        }
    }

    // --- 境界の扱い ---

    #[test]
    fn adjacent_tiles_do_not_both_claim_a_boundary_point() {
        // 半開区間になっていないと、境界上の点が 2 つのタイルに属し、
        // 地形が二重に読み込まれる。
        let level = 5;
        let tile = TileId::new(level, 10, 6);
        let east = tile.neighbour(Direction::East).unwrap();
        let bounds = tile.bounds();

        // 東端の点はこのタイルには属さず、東隣に属する。
        let on_eastern_edge = Geodetic::new(bounds.center().latitude, bounds.east, Meters::ZERO);
        assert!(!bounds.contains(on_eastern_edge));
        assert!(east.bounds().contains(on_eastern_edge));
    }

    #[test]
    fn normalise_maps_corners_to_the_unit_square() {
        let bounds = TileId::new(4, 3, 5).bounds();

        let north_west = Geodetic::new(bounds.north, bounds.west, Meters::ZERO);
        let (u, v) = bounds.normalise(north_west);
        assert!(
            u.abs() < 1e-12 && v.abs() < 1e-12,
            "north-west corner mapped to ({u}, {v})"
        );

        let south_east = Geodetic::new(bounds.south, bounds.east, Meters::ZERO);
        let (u, v) = bounds.normalise(south_east);
        assert!(
            (u - 1.0).abs() < 1e-12 && (v - 1.0).abs() < 1e-12,
            "south-east corner mapped to ({u}, {v})"
        );
    }

    #[test]
    #[should_panic(expected = "is out of range for level")]
    fn out_of_range_tile_coordinates_are_rejected() {
        // 範囲外の ID を許すと、静かに誤った地形を読み込む。
        let _ = TileId::new(2, TileId::columns(2), 0);
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_LEVEL")]
    fn excessive_levels_are_rejected() {
        let _ = TileId::new(MAX_LEVEL + 1, 0, 0);
    }
}
