//! 焼き込み対象の地理的範囲と、それを覆うタイルの列挙。
//!
//! # 日付変更線
//!
//! 西端が東端より大きい範囲（例 `170°..-170°`）は**日付変更線をまたぐ**と解釈する。
//! これを弾いてしまうと太平洋を挟む領域が焼けない。地形コードのバグはこの手の
//! 境界に集中するので、ここは明示的に扱う。
//!
//! # 極
//!
//! 緯度 ±90° はタイル索引の端に写る。極をまたぐ「隣接」は存在しないため
//! （[`TileId::neighbour`] も北南では `None` を返す）、範囲は緯度でクランプする。

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use flightsim_core::{Degrees, Radians};
use flightsim_world::TileId;

/// 範囲指定のエラー。
#[derive(Debug, Clone, PartialEq)]
pub enum RegionError {
    NonFinite,
    /// 緯度が ±90° の外。
    LatitudeOutOfRange(f64),
    /// 経度が ±180° の外。
    LongitudeOutOfRange(f64),
    /// 南端が北端より上。
    InvertedLatitudes {
        south: f64,
        north: f64,
    },
}

impl core::fmt::Display for RegionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonFinite => write!(formatter, "bounds contain a non-finite value"),
            Self::LatitudeOutOfRange(value) => {
                write!(formatter, "latitude {value}° is outside ±90°")
            }
            Self::LongitudeOutOfRange(value) => {
                write!(formatter, "longitude {value}° is outside ±180°")
            }
            Self::InvertedLatitudes { south, north } => write!(
                formatter,
                "south ({south}°) is north of north ({north}°); \
                 latitudes cannot wrap the way longitudes do"
            ),
        }
    }
}

impl std::error::Error for RegionError {}

/// 焼き込み対象の地理的範囲。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    west: Radians,
    south: Radians,
    east: Radians,
    north: Radians,
    crosses_dateline: bool,
}

impl Region {
    /// 度で範囲を指定する。
    ///
    /// `west > east` は日付変更線をまたぐ範囲として扱う。緯度には同じ規則は無い
    /// （極をまたぐ「連続」は存在しないため）。
    ///
    /// # Errors
    ///
    /// 値が非有限、緯度が ±90° の外、経度が ±180° の外、南端が北端より上の場合。
    pub fn from_degrees(west: f64, south: f64, east: f64, north: f64) -> Result<Self, RegionError> {
        if ![west, south, east, north]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(RegionError::NonFinite);
        }
        for latitude in [south, north] {
            if !(-90.0..=90.0).contains(&latitude) {
                return Err(RegionError::LatitudeOutOfRange(latitude));
            }
        }
        for longitude in [west, east] {
            if !(-180.0..=180.0).contains(&longitude) {
                return Err(RegionError::LongitudeOutOfRange(longitude));
            }
        }
        if south > north {
            return Err(RegionError::InvertedLatitudes { south, north });
        }

        Ok(Self {
            west: Degrees(west).to_radians(),
            south: Degrees(south).to_radians(),
            east: Degrees(east).to_radians(),
            north: Degrees(north).to_radians(),
            crosses_dateline: west > east,
        })
    }

    /// ラジアンの矩形から作る。日付変更線の判定は呼び出し側の値をそのまま使う。
    #[must_use]
    pub fn from_radians(west: Radians, south: Radians, east: Radians, north: Radians) -> Self {
        Self {
            west,
            south,
            east,
            north,
            crosses_dateline: west.get() > east.get(),
        }
    }

    #[must_use]
    pub const fn west(self) -> Radians {
        self.west
    }

    #[must_use]
    pub const fn south(self) -> Radians {
        self.south
    }

    #[must_use]
    pub const fn east(self) -> Radians {
        self.east
    }

    #[must_use]
    pub const fn north(self) -> Radians {
        self.north
    }

    #[must_use]
    pub const fn crosses_dateline(self) -> bool {
        self.crosses_dateline
    }

    /// 2 つの範囲を包含する矩形。日付変更線をまたぐ場合は扱わず、単純に広げる。
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self::from_radians(
            Radians(self.west.get().min(other.west.get())),
            Radians(self.south.get().min(other.south.get())),
            Radians(self.east.get().max(other.east.get())),
            Radians(self.north.get().max(other.north.get())),
        )
    }

    /// この範囲に重なるタイルを列挙する。
    ///
    /// 境界にちょうど乗る端は**含まない側**に倒す（`ceil - 1`）。
    /// そうしないと、タイル境界に揃った範囲を指定したときに東と南へ 1 列ぶん
    /// 余計なタイルが生える。
    #[must_use]
    pub fn tiles(self, level: u8) -> Vec<TileId> {
        let columns = TileId::columns(level);
        let rows = TileId::rows(level);

        let (first_row, last_row) = {
            let start = index_floor((FRAC_PI_2 - self.north.get()) / PI, rows);
            let end = index_ceil_inclusive((FRAC_PI_2 - self.south.get()) / PI, rows);
            (start, end.max(start))
        };

        let column_ranges: Vec<(u32, u32)> = if self.crosses_dateline {
            // 西端から東の果てまでと、西の果てから東端まで。
            let first = index_floor((self.west.get() + PI) / TAU, columns);
            let last = index_ceil_inclusive((self.east.get() + PI) / TAU, columns);
            vec![(first, columns - 1), (0, last)]
        } else {
            let first = index_floor((self.west.get() + PI) / TAU, columns);
            let last = index_ceil_inclusive((self.east.get() + PI) / TAU, columns);
            vec![(first, last.max(first))]
        };

        let mut tiles = Vec::new();
        for row in first_row..=last_row {
            for &(first_column, last_column) in &column_ranges {
                for column in first_column..=last_column {
                    tiles.push(TileId::new(level, column, row));
                }
            }
        }
        tiles.sort_unstable();
        tiles.dedup();
        tiles
    }
}

/// 正規化位置 `[0, 1]` を格子添字へ。下端側。
fn index_floor(fraction: f64, count: u32) -> u32 {
    let scaled = (fraction * f64::from(count)).floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamp により 0..=count-1 の有限値であることが保証されている"
    )]
    let index = scaled.clamp(0.0, f64::from(count - 1)) as u32;
    index
}

/// 正規化位置 `[0, 1]` を格子添字へ。上端側（境界に乗る場合は含まない側へ倒す）。
fn index_ceil_inclusive(fraction: f64, count: u32) -> u32 {
    let scaled = (fraction * f64::from(count)).ceil() - 1.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamp により 0..=count-1 の有限値であることが保証されている"
    )]
    let index = scaled.clamp(0.0, f64::from(count - 1)) as u32;
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_matching_one_tile_yields_exactly_that_tile() {
        // level 2 は 8×4 タイル、1 タイル 45°×45°。
        let bounds = TileId::new(2, 3, 1).bounds();
        let region = Region::from_radians(bounds.west, bounds.south, bounds.east, bounds.north);

        assert_eq!(region.tiles(2), vec![TileId::new(2, 3, 1)]);
    }

    #[test]
    fn tile_aligned_bounds_do_not_grow_an_extra_row_or_column() {
        // 境界にちょうど乗る端で 1 列余計に生えるのは、この種のコードの定番のバグ。
        let west = TileId::new(3, 4, 2).bounds();
        let east = TileId::new(3, 5, 2).bounds();
        let region = Region::from_radians(west.west, west.south, east.east, west.north);

        assert_eq!(
            region.tiles(3),
            vec![TileId::new(3, 4, 2), TileId::new(3, 5, 2)]
        );
    }

    #[test]
    fn the_whole_globe_yields_every_tile_at_a_level() {
        let region = Region::from_degrees(-180.0, -90.0, 180.0, 90.0).expect("valid");
        for level in 0..=4_u8 {
            let expected = (TileId::columns(level) as usize) * (TileId::rows(level) as usize);
            assert_eq!(
                region.tiles(level).len(),
                expected,
                "level {level} should yield every tile"
            );
        }
    }

    #[test]
    fn a_region_crossing_the_dateline_yields_tiles_on_both_sides() {
        // 170°E から 170°W。太平洋を挟む 20° の帯。
        let region = Region::from_degrees(170.0, -5.0, -170.0, 5.0).expect("valid");
        assert!(region.crosses_dateline());

        let tiles = region.tiles(4);
        assert!(!tiles.is_empty());

        let columns = TileId::columns(4);
        assert!(
            tiles.iter().any(|tile| tile.x == 0),
            "the eastern side of the dateline is missing"
        );
        assert!(
            tiles.iter().any(|tile| tile.x == columns - 1),
            "the western side of the dateline is missing"
        );
        // 経度 0° 付近（地球の反対側）は含まれてはならない。
        assert!(
            !tiles.iter().any(|tile| tile.x == columns / 2),
            "a dateline-crossing region wrapped the wrong way and covered the far side"
        );
    }

    #[test]
    fn a_dateline_region_covers_fewer_tiles_than_its_complement() {
        let narrow = Region::from_degrees(170.0, -5.0, -170.0, 5.0).expect("valid");
        let wide = Region::from_degrees(-170.0, -5.0, 170.0, 5.0).expect("valid");
        assert!(narrow.tiles(5).len() < wide.tiles(5).len());
    }

    #[test]
    fn polar_regions_reach_the_first_and_last_rows() {
        let north = Region::from_degrees(-10.0, 85.0, 10.0, 90.0).expect("valid");
        assert!(
            north.tiles(4).iter().all(|tile| tile.y == 0),
            "a region touching the north pole must sit in row 0"
        );

        let south = Region::from_degrees(-10.0, -90.0, 10.0, -85.0).expect("valid");
        let last_row = TileId::rows(4) - 1;
        assert!(
            south.tiles(4).iter().all(|tile| tile.y == last_row),
            "a region touching the south pole must sit in the last row"
        );
    }

    #[test]
    fn every_enumerated_tile_actually_overlaps_the_region() {
        let region = Region::from_degrees(139.0, 35.0, 140.5, 36.5).expect("valid");
        for tile in region.tiles(7) {
            let bounds = tile.bounds();
            let overlaps_longitude =
                bounds.east.get() > region.west().get() && bounds.west.get() < region.east().get();
            let overlaps_latitude = bounds.north.get() > region.south().get()
                && bounds.south.get() < region.north().get();
            assert!(
                overlaps_longitude && overlaps_latitude,
                "{tile:?} does not overlap the requested region"
            );
        }
    }

    #[test]
    fn a_degenerate_region_still_yields_one_tile() {
        // 幅ゼロ。切り捨てで空になってはいけない。
        let region = Region::from_degrees(139.7, 35.6, 139.7, 35.6).expect("valid");
        assert_eq!(region.tiles(8).len(), 1);
    }

    #[test]
    fn tiles_are_unique_and_sorted() {
        let region = Region::from_degrees(-20.0, -20.0, 20.0, 20.0).expect("valid");
        let tiles = region.tiles(5);
        let mut sorted = tiles.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(tiles, sorted);
    }

    // --- 入力の検査 ---

    #[test]
    fn out_of_range_and_non_finite_bounds_are_rejected() {
        assert_eq!(
            Region::from_degrees(0.0, -91.0, 1.0, 1.0),
            Err(RegionError::LatitudeOutOfRange(-91.0))
        );
        assert_eq!(
            Region::from_degrees(0.0, 0.0, 181.0, 1.0),
            Err(RegionError::LongitudeOutOfRange(181.0))
        );
        assert_eq!(
            Region::from_degrees(0.0, 10.0, 1.0, 5.0),
            Err(RegionError::InvertedLatitudes {
                south: 10.0,
                north: 5.0
            })
        );
        assert_eq!(
            Region::from_degrees(f64::NAN, 0.0, 1.0, 1.0),
            Err(RegionError::NonFinite)
        );
    }

    #[test]
    fn the_poles_and_the_dateline_are_accepted_as_exact_values() {
        assert!(Region::from_degrees(-180.0, -90.0, 180.0, 90.0).is_ok());
        assert!(Region::from_degrees(180.0, 0.0, -180.0, 1.0).is_ok());
    }

    #[test]
    fn union_widens_to_cover_both() {
        let a = Region::from_degrees(0.0, 0.0, 10.0, 10.0).expect("valid");
        let b = Region::from_degrees(5.0, -5.0, 20.0, 5.0).expect("valid");
        let union = a.union(b);

        assert!((union.west().to_degrees().get() - 0.0).abs() < 1e-12);
        assert!((union.south().to_degrees().get() + 5.0).abs() < 1e-12);
        assert!((union.east().to_degrees().get() - 20.0).abs() < 1e-12);
        assert!((union.north().to_degrees().get() - 10.0).abs() < 1e-12);
    }
}
