//! WGS84 測地系と ECEF（地心地球固定）直交座標。
//!
//! # 世界座標が `f64` である理由
//!
//! 地球半径は約 6.378e6 m。`f32` の仮数部は 24 bit なので、この距離での分解能は
//! `6.378e6 / 2^23 ≒ 0.76 m` しかない。`f32` で ECEF を保持すると機体は地表で
//! 76cm 格子にスナップし、可視の振動として現れる。滑走路上の接地判定も高度表示も成立しない。
//!
//! `f64` なら同じ距離で約 2.8 nm（ナノメートル）の分解能。十分すぎる余裕がある。
//!
//! 描画時は [`crate::FloatingOrigin`] でカメラ近傍を原点とした `f32` へ落とす。

use crate::units::{Degrees, Meters, Radians};
use glam::DVec3;

/// WGS84 楕円体の定義定数。
///
/// [`SEMI_MAJOR_AXIS`] と [`INVERSE_FLATTENING`] は WGS84 の**定義値**（測定値ではない）。
/// 他は全てここから導出される。
///
/// [`SEMI_MAJOR_AXIS`]: wgs84::SEMI_MAJOR_AXIS
/// [`INVERSE_FLATTENING`]: wgs84::INVERSE_FLATTENING
pub mod wgs84 {
    /// 長半径 a `m`。WGS84 の定義値。
    pub const SEMI_MAJOR_AXIS: f64 = 6_378_137.0;

    /// 逆扁平率 1/f。WGS84 の定義値。
    pub const INVERSE_FLATTENING: f64 = 298.257_223_563;

    /// 扁平率 f。
    pub const FLATTENING: f64 = 1.0 / INVERSE_FLATTENING;

    /// 短半径 b = a(1 - f) `m`。
    pub const SEMI_MINOR_AXIS: f64 = SEMI_MAJOR_AXIS * (1.0 - FLATTENING);

    /// 第一離心率の二乗 e² = f(2 - f)。
    pub const ECCENTRICITY_SQ: f64 = FLATTENING * (2.0 - FLATTENING);

    /// 第二離心率の二乗 e'² = e² / (1 - e²)。Bowring 法で使う。
    pub const SECOND_ECCENTRICITY_SQ: f64 = ECCENTRICITY_SQ / (1.0 - ECCENTRICITY_SQ);

    /// 地心重力定数 GM `m³/s²`。
    pub const GRAVITATIONAL_PARAMETER: f64 = 3.986_004_418e14;

    /// 地球自転角速度 `rad/s`。
    ///
    /// 現時点ではコリオリ・遠心力を無視しているため未使用（ADR-0002）。
    /// 将来 FDM に補正項を足す際の受け皿として置いてある。
    pub const ANGULAR_VELOCITY: f64 = 7.292_115e-5;

    /// 平均半径 R₁ = (2a + b) / 3 `m`。球近似の距離計算に使う。
    pub const MEAN_RADIUS: f64 = (2.0 * SEMI_MAJOR_AXIS + SEMI_MINOR_AXIS) / 3.0;
}

/// 測地座標（緯度・経度・楕円体高）。
///
/// 高度は**楕円体高**であってジオイド高（平均海面からの高さ）ではない。
/// 両者は場所により最大 100m 程度ずれる。海面高度が必要になった時点で
/// ジオイドモデル（EGM96 等）を別途導入すること。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geodetic {
    pub latitude: Radians,
    pub longitude: Radians,
    /// 楕円体面からの高さ。
    pub altitude: Meters,
}

impl Geodetic {
    #[must_use]
    pub const fn new(latitude: Radians, longitude: Radians, altitude: Meters) -> Self {
        Self {
            latitude,
            longitude,
            altitude,
        }
    }

    /// 度で指定する。設定ファイル・外部データからの読み込み用。
    #[must_use]
    pub fn from_degrees(latitude_deg: f64, longitude_deg: f64, altitude_m: f64) -> Self {
        Self {
            latitude: Degrees(latitude_deg).to_radians(),
            longitude: Degrees(longitude_deg).to_radians(),
            altitude: Meters(altitude_m),
        }
    }

    #[must_use]
    pub fn latitude_degrees(self) -> f64 {
        self.latitude.to_degrees().get()
    }

    #[must_use]
    pub fn longitude_degrees(self) -> f64 {
        self.longitude.to_degrees().get()
    }

    /// 卯酉線曲率半径 N `m`。緯度における東西方向の曲率。
    #[must_use]
    pub fn prime_vertical_radius(self) -> f64 {
        let sin_lat = self.latitude.sin();
        wgs84::SEMI_MAJOR_AXIS / (1.0 - wgs84::ECCENTRICITY_SQ * sin_lat * sin_lat).sqrt()
    }

    /// 子午線曲率半径 M `m`。緯度における南北方向の曲率。
    #[must_use]
    pub fn meridional_radius(self) -> f64 {
        let sin_lat = self.latitude.sin();
        let w = 1.0 - wgs84::ECCENTRICITY_SQ * sin_lat * sin_lat;
        wgs84::SEMI_MAJOR_AXIS * (1.0 - wgs84::ECCENTRICITY_SQ) / (w * w.sqrt())
    }

    /// 北・東へメートルでずらした点を返す。高度は変えない。
    ///
    /// # 近似の範囲
    ///
    /// 曲率半径（子午線 M・卯酉線 N）による局所近似。**数 km までの近距離用**で、
    /// 滑走路・空港・タイル内の配置に使う。100 km を超える測地線問題には使わない
    /// （その用途が出たら Vincenty 等を別途入れる）。
    ///
    /// 極のごく近傍では東西の曲率半径の分母 cos(緯度) が 0 に近づく。
    /// **NaN を作らないため**、緯度 ±89.99° を超える点では東方向のずれを
    /// 無視する（滑走路を極点に置く用途は当面ない）。
    #[must_use]
    pub fn offset_by(self, north: Meters, east: Meters) -> Self {
        let d_lat = north.get() / self.meridional_radius();

        let cos_lat = self.latitude.cos();
        // ±89.99° で cos ≈ 1.7e-4。これ未満は東西のずれを捨てて NaN を避ける。
        let d_lon = if cos_lat.abs() > 1.7e-4 {
            east.get() / (self.prime_vertical_radius() * cos_lat)
        } else {
            0.0
        };

        Self {
            latitude: Radians(self.latitude.get() + d_lat),
            longitude: Radians(self.longitude.get() + d_lon),
            altitude: self.altitude,
        }
    }

    /// ECEF へ変換する（閉形式・厳密）。
    #[must_use]
    pub fn to_ecef(self) -> Ecef {
        let (sin_lat, cos_lat) = self.latitude.get().sin_cos();
        let (sin_lon, cos_lon) = self.longitude.get().sin_cos();

        let n = self.prime_vertical_radius();
        let h = self.altitude.get();

        Ecef(DVec3::new(
            (n + h) * cos_lat * cos_lon,
            (n + h) * cos_lat * sin_lon,
            (n * (1.0 - wgs84::ECCENTRICITY_SQ) + h) * sin_lat,
        ))
    }

    /// 2 点間の大圏距離（球近似）。
    ///
    /// **楕円体上の厳密な測地線距離ではない。** 平均半径による球近似のため、
    /// 最大で 0.5% 程度の誤差がある。タイル選択やおおまかな距離判定には十分だが、
    /// 航法計算に使わないこと。
    #[must_use]
    pub fn great_circle_distance(self, other: Self) -> Meters {
        let (lat1, lat2) = (self.latitude.get(), other.latitude.get());
        let d_lat = lat2 - lat1;
        let d_lon = other.longitude.get() - self.longitude.get();

        // haversine。両点が近接した場合でも桁落ちしない形。
        let a = (d_lat * 0.5).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon * 0.5).sin().powi(2);
        let c = 2.0 * a.sqrt().clamp(0.0, 1.0).asin();

        Meters(wgs84::MEAN_RADIUS * c)
    }
}

/// 地心地球固定直交座標 `m`。**このプロジェクトにおける世界の正準座標。**
///
/// 原点は地球重心、Z 軸が自転軸（北が正）、X 軸が本初子午線と赤道の交点方向、
/// Y 軸が右手系を成す（東経 90°方向）。
///
/// ECEF は地球と共に回転する非慣性系だが、現時点ではコリオリ力・遠心力を無視して
/// 慣性系として扱っている。巡航速度 250 m/s・緯度 45° でのコリオリ加速度は
/// 約 0.026 m/s²（重力の 0.26%）。詳細と将来の対応方針は ADR-0002 を参照。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(transparent)]
pub struct Ecef(pub DVec3);

impl Ecef {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }

    #[must_use]
    pub const fn from_vec(v: DVec3) -> Self {
        Self(v)
    }

    #[must_use]
    pub const fn as_vec(self) -> DVec3 {
        self.0
    }

    #[must_use]
    pub fn distance_to(self, other: Self) -> Meters {
        Meters(self.0.distance(other.0))
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// 測地座標へ変換する。
    ///
    /// # アルゴリズム
    ///
    /// Bowring 法で初期値を得たあと、固定点反復を 2 回かけて精度を詰める。
    /// 閉形式（Ferrari）より高速で、地表付近では 1mm 以下の誤差に収まる。
    /// 高高度（数千 km）でも反復により収束する。
    ///
    /// 極近傍（`cos φ` が小さい領域）では `h = p / cos φ` が桁落ちするため、
    /// `h = z / sin φ - N(1 - e²)` に切り替える。
    #[must_use]
    pub fn to_geodetic(self) -> Geodetic {
        let (x, y, z) = (self.0.x, self.0.y, self.0.z);
        let p = (x * x + y * y).sqrt();

        // 極軸上（p ≈ 0）。経度が定義できないので 0 とする。
        // 閾値 1e-9 m は f64 の分解能から見て「厳密にゼロ」と等価な範囲。
        if p < 1.0e-9 {
            let latitude = Radians(core::f64::consts::FRAC_PI_2.copysign(z));
            let altitude = Meters(z.abs() - wgs84::SEMI_MINOR_AXIS);
            return Geodetic::new(latitude, Radians::ZERO, altitude);
        }

        let longitude = Radians(y.atan2(x));

        // --- Bowring の初期推定 ---
        let theta = (z * wgs84::SEMI_MAJOR_AXIS).atan2(p * wgs84::SEMI_MINOR_AXIS);
        let (sin_theta, cos_theta) = theta.sin_cos();
        let bowring_lat =
            (z + wgs84::SECOND_ECCENTRICITY_SQ * wgs84::SEMI_MINOR_AXIS * sin_theta.powi(3))
                .atan2(p - wgs84::ECCENTRICITY_SQ * wgs84::SEMI_MAJOR_AXIS * cos_theta.powi(3));

        // --- 固定点反復による精度の詰め ---
        let (latitude, altitude) = refine_latitude_and_height(bowring_lat, p, z);

        Geodetic::new(Radians(latitude), longitude, Meters(altitude))
    }
}

/// [`Ecef::to_geodetic`] の反復部分。`(緯度 `rad`, 楕円体高 `m`)` を返す。
///
/// Bowring の初期値からは 2 回の反復で `f64` の精度限界に達する。
/// 反復回数と極近傍の分岐をここ一箇所にまとめている。
///
/// - `p`: 自転軸からの距離 `sqrt(x² + y²)`
/// - `z`: ECEF の Z 成分
fn refine_latitude_and_height(initial_latitude: f64, p: f64, z: f64) -> (f64, f64) {
    const ITERATIONS: usize = 2;

    let mut lat = initial_latitude;
    let mut height = 0.0;

    for _ in 0..ITERATIONS {
        let (sin_lat, cos_lat) = lat.sin_cos();

        let n = wgs84::SEMI_MAJOR_AXIS / (1.0 - wgs84::ECCENTRICITY_SQ * sin_lat * sin_lat).sqrt();

        // 極近傍では p / cos φ が桁落ちする。cos と sin の絶対値が大きいほうを分母に使う。
        // 分岐点を 1/√2（緯度 45°）に置くことで、どちらの式でも分母が 0.707 以上になる。
        height = if cos_lat.abs() > core::f64::consts::FRAC_1_SQRT_2 {
            p / cos_lat - n
        } else {
            z / sin_lat - n * (1.0 - wgs84::ECCENTRICITY_SQ)
        };

        // 楕円体高を考慮した緯度の更新。
        lat = z.atan2(p * (1.0 - wgs84::ECCENTRICITY_SQ * n / (n + height)));
    }

    (lat, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_close {
        ($actual:expr, $expected:expr, $tol:expr) => {{
            let (a, e, t) = ($actual, $expected, $tol);
            assert!(
                (a - e).abs() <= t,
                "expected {a} ≈ {e} (tolerance {t}), difference was {}",
                (a - e).abs()
            );
        }};
    }

    // --- 定義値との照合 ---
    // これらは WGS84 の定義から一意に決まる値であり、実装から導いたものではない。

    #[test]
    fn wgs84_derived_constants_match_published_values() {
        assert_close!(wgs84::SEMI_MINOR_AXIS, 6_356_752.314_245_179, 1e-6);
        assert_close!(wgs84::ECCENTRICITY_SQ, 0.006_694_379_990_141_32, 1e-15);
        assert_close!(
            wgs84::SECOND_ECCENTRICITY_SQ,
            0.006_739_496_742_276_43,
            1e-15
        );
        // 短半径は長半径より小さい（扁平）。定数どうしなのでコンパイル時に検査する。
        const { assert!(wgs84::SEMI_MINOR_AXIS < wgs84::SEMI_MAJOR_AXIS) };
    }

    #[test]
    fn axis_intersections_are_exact() {
        // 赤道・本初子午線上の地表点は X 軸上、距離は長半径ちょうど。
        let p = Geodetic::from_degrees(0.0, 0.0, 0.0).to_ecef();
        assert_close!(p.0.x, wgs84::SEMI_MAJOR_AXIS, 1e-6);
        assert_close!(p.0.y, 0.0, 1e-6);
        assert_close!(p.0.z, 0.0, 1e-6);

        // 東経 90° は Y 軸上。
        let p = Geodetic::from_degrees(0.0, 90.0, 0.0).to_ecef();
        assert_close!(p.0.x, 0.0, 1e-6);
        assert_close!(p.0.y, wgs84::SEMI_MAJOR_AXIS, 1e-6);

        // 北極は Z 軸上、距離は短半径ちょうど。
        let p = Geodetic::from_degrees(90.0, 0.0, 0.0).to_ecef();
        assert_close!(p.0.x, 0.0, 1e-6);
        assert_close!(p.0.y, 0.0, 1e-6);
        assert_close!(p.0.z, wgs84::SEMI_MINOR_AXIS, 1e-6);
    }

    #[test]
    fn prime_vertical_radius_equals_semi_major_at_equator() {
        let g = Geodetic::from_degrees(0.0, 0.0, 0.0);
        assert_close!(g.prime_vertical_radius(), wgs84::SEMI_MAJOR_AXIS, 1e-9);
    }

    #[test]
    fn geocentric_radius_stays_between_the_two_axes() {
        // 楕円体上の全ての点は、地心からの距離が [b, a] に収まる。
        for lat in (-90..=90).step_by(5) {
            for lon in (-180..=180).step_by(15) {
                let r = Geodetic::from_degrees(f64::from(lat), f64::from(lon), 0.0)
                    .to_ecef()
                    .0
                    .length();
                assert!(
                    (wgs84::SEMI_MINOR_AXIS - 1e-6..=wgs84::SEMI_MAJOR_AXIS + 1e-6).contains(&r),
                    "radius {r} at lat {lat} lon {lon} is outside [b, a]"
                );
            }
        }
    }

    // --- 往復変換 ---

    #[test]
    fn round_trip_is_accurate_worldwide() {
        // 高度は地表から成層圏、さらに衛星軌道域まで試す。
        for lat in [-89.9, -60.0, -23.5, 0.0, 23.5, 45.0, 60.0, 89.9] {
            for lon in [-179.9, -120.0, -0.001, 0.0, 0.001, 120.0, 179.9] {
                for alt in [-400.0, 0.0, 1000.0, 12_000.0, 400_000.0] {
                    let original = Geodetic::from_degrees(lat, lon, alt);
                    let result = original.to_ecef().to_geodetic();

                    assert_close!(result.latitude_degrees(), lat, 1e-9);
                    assert_close!(result.longitude_degrees(), lon, 1e-9);
                    // 高度は 0.1mm 以内。
                    assert_close!(result.altitude.get(), alt, 1e-4);
                }
            }
        }
    }

    #[test]
    fn poles_are_handled_without_nan() {
        // 極では経度が定義できない。NaN を出さず、緯度と高度が正しいこと。
        for (lat, sign) in [(90.0, 1.0), (-90.0, -1.0)] {
            let g = Geodetic::from_degrees(lat, 0.0, 500.0)
                .to_ecef()
                .to_geodetic();
            assert!(g.latitude.is_finite() && g.longitude.is_finite() && g.altitude.is_finite());
            assert_close!(g.latitude_degrees(), lat, 1e-9);
            assert_close!(g.altitude.get(), 500.0, 1e-6);
            assert!(g.latitude.get() * sign > 0.0);
        }

        // 厳密に極軸上の ECEF 点。
        let g = Ecef::new(0.0, 0.0, wgs84::SEMI_MINOR_AXIS + 100.0).to_geodetic();
        assert_close!(g.latitude_degrees(), 90.0, 1e-12);
        assert_close!(g.altitude.get(), 100.0, 1e-6);
    }

    #[test]
    fn dateline_is_continuous() {
        // 経度 ±180° 近傍で位置が飛ばないこと。地形コードのバグの定番箇所。
        let west = Geodetic::from_degrees(35.0, -179.999_999, 0.0).to_ecef();
        let east = Geodetic::from_degrees(35.0, 179.999_999, 0.0).to_ecef();
        // 2e-6 度 ≒ 0.18 m（緯度 35° で）。
        assert!(
            west.distance_to(east).get() < 1.0,
            "dateline discontinuity: {} m apart",
            west.distance_to(east).get()
        );
    }

    #[test]
    fn altitude_change_moves_along_the_normal() {
        // 高度だけ変えた 2 点の距離は、その高度差にちょうど等しい。
        // （楕円体の法線方向に動くことの検証）
        for lat in [-75.0, -30.0, 0.0, 30.0, 75.0] {
            let low = Geodetic::from_degrees(lat, 42.0, 0.0).to_ecef();
            let high = Geodetic::from_degrees(lat, 42.0, 1000.0).to_ecef();
            assert_close!(low.distance_to(high).get(), 1000.0, 1e-6);
        }
    }

    // --- 距離 ---

    #[test]
    fn a_metre_offset_matches_surveyed_degree_lengths() {
        // 外部の既知値: 赤道での緯度 1° は 110 574 m、経度 1° は 111 320 m。
        // （測地学の標準値。実装がこう返すから、ではない）
        let equator = Geodetic::from_degrees(0.0, 0.0, 0.0);

        let north = equator.offset_by(Meters(110_574.0), Meters(0.0));
        assert!(
            (north.latitude_degrees() - 1.0).abs() < 0.002,
            "110 574 m north of the equator should be 1°, got {}",
            north.latitude_degrees()
        );

        let east = equator.offset_by(Meters(0.0), Meters(111_320.0));
        assert!(
            (east.longitude_degrees() - 1.0).abs() < 0.002,
            "111 320 m east on the equator should be 1°, got {}",
            east.longitude_degrees()
        );
    }

    #[test]
    fn a_short_offset_round_trips_through_the_distance() {
        // 比較先の great_circle_distance は平均半径の球面（haversine）。
        // offset_by は楕円体の曲率半径を使うので、緯度 35° では両者が
        // 約 0.06% 食い違う。**これは offset_by の誤差ではなく座標系の差**
        // なので、許容は 0.1%（2.5 km で 2.5 m）とする。
        let start = Geodetic::from_degrees(35.55, 139.78, 8.0);
        let moved = start.offset_by(Meters(2000.0), Meters(1500.0));
        let distance = start.great_circle_distance(moved);
        assert!(
            (distance.get() - 2500.0).abs() < 2.5,
            "a 2000/1500 offset should be 2500 m away, got {distance}"
        );
    }

    #[test]
    fn offsets_near_the_pole_do_not_produce_nan() {
        let pole = Geodetic::from_degrees(89.9999, 0.0, 0.0);
        let moved = pole.offset_by(Meters(100.0), Meters(100.0));
        assert!(
            moved.latitude.get().is_finite()
                && moved.longitude.get().is_finite()
                && moved.altitude.get().is_finite(),
            "an offset at the pole went non-finite"
        );
    }

    #[test]
    fn altitude_is_preserved_by_an_offset() {
        let start = Geodetic::from_degrees(10.0, 20.0, 123.0);
        let moved = start.offset_by(Meters(500.0), Meters(-300.0));
        assert!((moved.altitude.get() - 123.0).abs() < 1e-9);
    }

    #[test]
    fn great_circle_distance_matches_known_arc_lengths() {
        // 赤道 1/4 周 = 平均半径 × π/2。
        let a = Geodetic::from_degrees(0.0, 0.0, 0.0);
        let b = Geodetic::from_degrees(0.0, 90.0, 0.0);
        assert_close!(
            a.great_circle_distance(b).get(),
            wgs84::MEAN_RADIUS * core::f64::consts::FRAC_PI_2,
            1.0
        );

        // 緯度 1 度 ≒ 111 km。球近似なので 1 km の許容。
        let a = Geodetic::from_degrees(0.0, 0.0, 0.0);
        let b = Geodetic::from_degrees(1.0, 0.0, 0.0);
        assert_close!(a.great_circle_distance(b).get(), 111_000.0, 1_000.0);

        // 同一点の距離はゼロ（haversine の桁落ち検査）。
        assert_close!(a.great_circle_distance(a).get(), 0.0, 1e-6);
    }

    #[test]
    fn great_circle_distance_is_symmetric() {
        let a = Geodetic::from_degrees(35.68, 139.77, 0.0);
        let b = Geodetic::from_degrees(-33.87, 151.21, 0.0);
        assert_close!(
            a.great_circle_distance(b).get(),
            b.great_circle_distance(a).get(),
            1e-6
        );
    }
}
