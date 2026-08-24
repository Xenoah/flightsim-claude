//! 太陽の位置。**Bevy に依存しない純粋な計算。**
//!
//! 入力は UTC のユリウス日と観測地点の測地座標、出力は観測地点の地平座標
//! （方位角・仰角）。描画の光源方向と、HUD やデバッグ表示の両方で使う。
//!
//! # 根拠にした式
//!
//! NOAA Solar Calculator が用いているのと同じ **低精度の太陽位置**。
//! 出典は Jean Meeus, *Astronomical Algorithms*, 2nd ed.:
//!
//! | 量 | 出典 |
//! |---|---|
//! | ユリウス日 ⇄ 暦日 | 第 7 章（グレゴリオ暦） |
//! | 太陽の平均黄経・平均近点角・中心差 | 第 25 章（低精度） |
//! | 章動と光行差を含む視黄経 | 第 25 章 |
//! | 平均黄道傾斜角 | 第 22 章 |
//! | グリニッジ平均恒星時 | 第 12 章 式 12.4 |
//! | 均時差 | 第 28 章（NOAA の実装と同形） |
//!
//! # 精度
//!
//! Meeus はこの近似の視黄経の誤差を **0.01° 程度**（1950〜2050 年）としている。
//! 本モジュールはさらに次を無視する。
//!
//! - **大気差**（地平線付近で約 0.57°、仰角 10° で約 0.09°）。返すのは
//!   幾何学的な仰角であって見かけの仰角ではない
//! - 太陽視差（最大 8.8 秒角）と光行差の高次項
//!
//! 合計しても地平線付近を除けば **1° より十分小さい**。日の出・日没の時刻を
//! 分単位で当てる用途には大気差の補正が要るが、空の色と陰の向きを決めるには
//! これで足りる。
//!
//! # 座標変換の担当
//!
//! ここで書く三角関数は **天球座標（赤経・赤緯）から地球固定系への変換** であって、
//! 測地変換ではない。楕円体に依存する変換（ECEF → NED）は
//! [`flightsim_core::LocalFrame`] に任せる。**測地変換をここに書かないこと**
//! （CLAUDE.md 規約 4）。

use core::f64::consts::FRAC_PI_2;
use flightsim_core::{Degrees, Geodetic, LocalFrame, Radians, Seconds};
use glam::DVec3;

/// 1 日の秒数。
const SECONDS_PER_DAY: f64 = 86_400.0;

/// 1 ユリウス世紀の日数。
const DAYS_PER_JULIAN_CENTURY: f64 = 36_525.0;

// ---------------------------------------------------------------------------
// 時刻
// ---------------------------------------------------------------------------

/// UTC のユリウス日。**単位は「日」**で、0.5 が正午の境目に来る。
///
/// `chrono` のような日付ライブラリを持ち込まずに時刻を表すための素の数値。
/// 天文計算がそのままこの量を要求するので、変換を挟まずに済む。
///
/// 日付として組み立てるには [`UtcDateTime::to_julian_date`] を使う。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct JulianDate(pub f64);

impl JulianDate {
    /// 元期 J2000.0。2000-01-01 12:00:00 UT。
    ///
    /// **この値は定義値**（外部の公表値）であり、実装から導いたものではない。
    pub const J2000: Self = Self(2_451_545.0);

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// J2000.0 からの経過日数。負なら過去。
    #[must_use]
    pub fn days_since_j2000(self) -> f64 {
        self.0 - Self::J2000.0
    }

    /// J2000.0 からの経過ユリウス世紀。天文計算の多項式の引数。
    #[must_use]
    pub fn centuries_since_j2000(self) -> f64 {
        self.days_since_j2000() / DAYS_PER_JULIAN_CENTURY
    }

    /// 指定した秒数だけ進めた時刻。
    #[must_use]
    pub fn advanced_by(self, elapsed: Seconds) -> Self {
        Self(self.0 + elapsed.get() / SECONDS_PER_DAY)
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// 暦日へ戻す。表示用。
    #[must_use]
    pub fn to_utc_date_time(self) -> UtcDateTime {
        UtcDateTime::from_julian_date(self)
    }

    /// 指定した経度で **地方平均太陽時** が `civil` の時刻になる UTC。
    ///
    /// 「どこで始めても朝 9 時」を作るための入口。経度 15° につき 1 時間の
    /// 単純な換算で、時間帯（タイムゾーン）も夏時間も見ない。
    /// 均時差のぶん（最大 ±16 分）は真太陽時とずれる。
    #[must_use]
    pub fn from_local_mean_solar_time(civil: UtcDateTime, longitude: Radians) -> Self {
        // 地方平均太陽時 = UT + 経度/15h。したがって UT = LMT − 経度/15h。
        Self(civil.to_julian_date().0 - longitude.to_degrees().get() / 360.0)
    }
}

/// UTC の暦日と時刻。**グレゴリオ暦**（1583 年以降）を前提とする。
///
/// 範囲外の値（`month = 13`、`day = 32` など）は連続的に外挿される。
/// 1 月 32 日は 2 月 1 日として扱われ、panic はしない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UtcDateTime {
    pub year: i32,
    /// 1〜12。
    pub month: u8,
    /// 1〜31。
    pub day: u8,
    /// 0〜23。
    pub hour: u8,
    /// 0〜59。
    pub minute: u8,
    /// 0〜60 未満。
    pub second: f64,
}

impl UtcDateTime {
    #[must_use]
    pub const fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: f64) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// その日の 0 時 0 分 0 秒。
    #[must_use]
    pub const fn midnight(year: i32, month: u8, day: u8) -> Self {
        Self::new(year, month, day, 0, 0, 0.0)
    }

    /// ユリウス日へ。Meeus 第 7 章の式（グレゴリオ暦）。
    #[must_use]
    pub fn to_julian_date(self) -> JulianDate {
        let (year, month) = if self.month <= 2 {
            (f64::from(self.year - 1), f64::from(self.month) + 12.0)
        } else {
            (f64::from(self.year), f64::from(self.month))
        };

        // グレゴリオ暦の補正項。ユリウス暦との差はここだけに現れる。
        let century = (year / 100.0).floor();
        let gregorian = 2.0 - century + (century / 4.0).floor();

        let day_fraction =
            (f64::from(self.hour) * 3600.0 + f64::from(self.minute) * 60.0 + self.second)
                / SECONDS_PER_DAY;

        JulianDate(
            (365.25 * (year + 4716.0)).floor() + (30.6001 * (month + 1.0)).floor() - 1524.5
                + f64::from(self.day)
                + gregorian
                + day_fraction,
        )
    }

    /// ユリウス日から暦日へ。Meeus 第 7 章の逆変換。
    ///
    /// **ミリ秒に丸めてから分解する。** 丸めずに分解すると、ちょうど 0 時の
    /// 時刻が `23:59:59.9999999` として出て、表示が 1 日ずれる。
    #[must_use]
    pub fn from_julian_date(time: JulianDate) -> Self {
        if !time.is_finite() {
            // 非有限な時刻に対応する暦日は無い。表示が壊れるだけで済むよう
            // 定義済みの値を返す（panic も NaN の伝播もさせない）。
            return Self::midnight(0, 1, 1);
        }

        const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;
        let rounded = (time.0 * MILLISECONDS_PER_DAY).round() / MILLISECONDS_PER_DAY;

        let shifted = rounded + 0.5;
        let integral = shifted.floor();
        let fraction = shifted - integral;

        let alpha = ((integral - 1_867_216.25) / 36_524.25).floor();
        let a = integral + 1.0 + alpha - (alpha / 4.0).floor();
        let b = a + 1524.0;
        let c = ((b - 122.1) / 365.25).floor();
        let d = (365.25 * c).floor();
        let e = ((b - d) / 30.6001).floor();

        let day = b - d - (30.6001 * e).floor();
        let month = if e < 14.0 { e - 1.0 } else { e - 13.0 };
        let year = if month > 2.0 { c - 4716.0 } else { c - 4715.0 };

        let milliseconds = (fraction * MILLISECONDS_PER_DAY).round();
        let hour = (milliseconds / 3_600_000.0).floor();
        let minute = ((milliseconds - hour * 3_600_000.0) / 60_000.0).floor();
        let second = (milliseconds - hour * 3_600_000.0 - minute * 60_000.0) / 1000.0;

        #[allow(
            clippy::cast_possible_truncation,
            reason = "暦日の各成分は上の式で範囲が保証されている。year は i32 に収まる"
        )]
        Self {
            year: year as i32,
            month: month as u8,
            day: day as u8,
            hour: hour as u8,
            minute: minute as u8,
            second,
        }
    }
}

// ---------------------------------------------------------------------------
// 太陽の位置
// ---------------------------------------------------------------------------

/// 観測地点から見た太陽の方向（地平座標）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarPosition {
    /// 真方位。北が 0、東が π/2。`[0, 2π)`。
    pub azimuth: Radians,
    /// 仰角。水平が 0、天頂が π/2。**大気差を含まない幾何学的な値。**
    pub elevation: Radians,
}

impl SolarPosition {
    /// 真下（仰角 −90°）。**非有限な入力に対して返す値。**
    ///
    /// NaN をそのまま返すと `Transform` ごと壊れて描画が止まる。
    /// 夜として扱えば、絵は暗くなるだけで飛行は続く。
    pub const NIGHT: Self = Self {
        azimuth: Radians(0.0),
        elevation: Radians(-FRAC_PI_2),
    };

    /// 幾何学的な地平線より上にあるか。
    #[must_use]
    pub fn is_above_horizon(self) -> bool {
        self.elevation.get() > 0.0
    }
}

/// 太陽の軌道要素。多項式の評価を 1 箇所にまとめる。
#[derive(Debug, Clone, Copy)]
struct SolarElements {
    /// 平均黄経 rad。
    mean_longitude: f64,
    /// 平均近点角 rad。
    mean_anomaly: f64,
    /// 地球軌道の離心率（無次元）。
    eccentricity: f64,
    /// 章動と光行差を含む視黄経 rad。
    apparent_longitude: f64,
    /// 章動を含む黄道傾斜角 rad。
    obliquity: f64,
}

/// Meeus 第 25 章の低精度の太陽位置。引数は J2000.0 からのユリウス世紀。
fn solar_elements(centuries: f64) -> SolarElements {
    let t = centuries;

    // 平均黄経（幾何学的）。
    let mean_longitude = 280.466_46 + 36_000.769_83 * t + 0.000_303_2 * t * t;
    // 平均近点角。
    let mean_anomaly = 357.529_11 + 35_999.050_29 * t - 0.000_153_7 * t * t;
    // 地球軌道の離心率。
    let eccentricity = 0.016_708_634 - 0.000_042_037 * t - 0.000_000_126_7 * t * t;

    let m = Degrees(mean_anomaly).to_radians().get();
    // 中心差（真近点角 − 平均近点角）。
    let center = (1.914_602 - 0.004_817 * t - 0.000_014 * t * t) * m.sin()
        + (0.019_993 - 0.000_101 * t) * (2.0 * m).sin()
        + 0.000_289 * (3.0 * m).sin();

    // 月の昇交点。章動の主項に使う。
    let omega = Degrees(125.04 - 1934.136 * t).to_radians().get();

    // 視黄経 = 真黄経 − 光行差(20.5") − 章動。
    let apparent_longitude =
        Degrees(mean_longitude + center - 0.005_69 - 0.004_78 * omega.sin()).to_radians();

    // 平均黄道傾斜角（Meeus 22.2）に章動の主項を足す。
    let mean_obliquity = 23.0
        + (26.0 + (21.448 - 46.815 * t - 0.000_59 * t * t + 0.001_813 * t * t * t) / 60.0) / 60.0;
    let obliquity = Degrees(mean_obliquity + 0.002_56 * omega.cos()).to_radians();

    SolarElements {
        mean_longitude: Degrees(mean_longitude).to_radians().get(),
        mean_anomaly: m,
        eccentricity,
        apparent_longitude: apparent_longitude.get(),
        obliquity: obliquity.get(),
    }
}

/// 太陽の赤緯。天の赤道より北が正。
///
/// 至点で ±23.44°、分点で 0 になる。
#[must_use]
pub fn solar_declination(time: JulianDate) -> Radians {
    if !time.is_finite() {
        return Radians::ZERO;
    }
    let elements = solar_elements(time.centuries_since_j2000());
    Radians(
        (elements.obliquity.sin() * elements.apparent_longitude.sin())
            .clamp(-1.0, 1.0)
            .asin(),
    )
}

/// 太陽の視赤経。`[0, 2π)`。
fn right_ascension(elements: &SolarElements) -> Radians {
    let (sin_lambda, cos_lambda) = elements.apparent_longitude.sin_cos();
    Radians((elements.obliquity.cos() * sin_lambda).atan2(cos_lambda)).wrap_positive()
}

/// グリニッジ平均恒星時。Meeus 式 12.4。`[0, 2π)`。
///
/// 地球の自転位相そのもの。赤経から地球固定系の経度へ移すのに使う。
#[must_use]
pub fn greenwich_mean_sidereal_time(time: JulianDate) -> Radians {
    if !time.is_finite() {
        return Radians::ZERO;
    }
    let days = time.days_since_j2000();
    let t = time.centuries_since_j2000();
    let degrees = 280.460_618_37 + 360.985_647_366_29 * days + 0.000_387_933 * t * t
        - t * t * t / 38_710_000.0;
    // **度のまま畳んでからラジアンにする。** 何百万度をラジアンにしてから
    // 剰余を取ると、有効数字が落ちる。
    Degrees(degrees.rem_euclid(360.0))
        .to_radians()
        .wrap_positive()
}

/// 均時差（真太陽時 − 平均太陽時）。正なら日時計が時計より進んでいる。
///
/// 2 月上旬に約 −14 分、11 月上旬に約 +16 分。NOAA と同じ式（Meeus 第 28 章）。
#[must_use]
pub fn equation_of_time(time: JulianDate) -> Seconds {
    if !time.is_finite() {
        return Seconds::ZERO;
    }
    let elements = solar_elements(time.centuries_since_j2000());
    let y = (elements.obliquity / 2.0).tan().powi(2);
    let l0 = elements.mean_longitude;
    let m = elements.mean_anomaly;
    let e = elements.eccentricity;

    let radians = y * (2.0 * l0).sin() - 2.0 * e * m.sin()
        + 4.0 * e * y * m.sin() * (2.0 * l0).cos()
        - 0.5 * y * y * (4.0 * l0).sin()
        - 1.25 * e * e * (2.0 * m).sin();

    // 1 分の自転は 0.25°。したがって「分 = 4 × 度」。
    Seconds(4.0 * radians.to_degrees() * 60.0)
}

/// 太陽の方向を **ECEF の単位ベクトル**で返す。
///
/// 太陽までの距離（1 天文単位）に対して地球の半径は 2.3 万分の 1 なので、
/// 観測地点によらず同じ向きとみなしてよい（視差は最大 8.8 秒角）。
#[must_use]
pub fn sun_direction_ecef(time: JulianDate) -> DVec3 {
    if !time.is_finite() {
        // 南極方向。仰角は必ず地平線下になり、夜として扱われる。
        return DVec3::new(0.0, 0.0, -1.0);
    }
    let elements = solar_elements(time.centuries_since_j2000());
    let declination = (elements.obliquity.sin() * elements.apparent_longitude.sin())
        .clamp(-1.0, 1.0)
        .asin();
    let right_ascension = right_ascension(&elements).get();

    // 地球固定系での経度 = 赤経 − グリニッジ恒星時。
    // これが直下点（太陽が天頂に来る点）の地心経度になる。
    let longitude = right_ascension - greenwich_mean_sidereal_time(time).get();
    let (sin_lon, cos_lon) = longitude.sin_cos();
    let (sin_dec, cos_dec) = declination.sin_cos();

    DVec3::new(cos_dec * cos_lon, cos_dec * sin_lon, sin_dec)
}

/// 観測地点から見た太陽の方位角と仰角。
///
/// ECEF → NED の変換は [`flightsim_core::LocalFrame`] に任せる。
/// 楕円体法線を「上」とするので、仰角は測地緯度基準（地心緯度ではない）。
///
/// 時刻や観測地点が非有限なら [`SolarPosition::NIGHT`] を返す。
/// **NaN を下流へ流さない。**
#[must_use]
pub fn solar_position(time: JulianDate, observer: Geodetic) -> SolarPosition {
    if !time.is_finite()
        || !observer.latitude.get().is_finite()
        || !observer.longitude.get().is_finite()
    {
        return SolarPosition::NIGHT;
    }

    let frame = LocalFrame::new(observer);
    let ned = frame.ecef_to_ned_vector(sun_direction_ecef(time));

    SolarPosition {
        azimuth: ned.bearing(),
        elevation: Radians(ned.up().clamp(-1.0, 1.0).asin()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 東京（気象庁の観測点付近）。
    fn tokyo() -> Geodetic {
        Geodetic::from_degrees(35.6895, 139.6917, 0.0)
    }

    // -----------------------------------------------------------------------
    // ユリウス日 — Meeus 第 7 章の例と定義値に突き合わせる
    // -----------------------------------------------------------------------

    #[test]
    fn the_epoch_matches_the_published_value_of_j2000() {
        // J2000.0 = 2000-01-01 12:00 UT = JD 2451545.0（定義値）。
        let jd = UtcDateTime::new(2000, 1, 1, 12, 0, 0.0).to_julian_date();
        assert!(
            (jd.get() - 2_451_545.0).abs() < 1e-9,
            "2000-01-01 12:00 UT should be JD 2451545.0, got {}",
            jd.get()
        );
    }

    #[test]
    fn the_julian_date_matches_meeus_worked_examples() {
        // Meeus 第 7 章の例。**外部の公表値。**
        let cases = [
            // スプートニク 1 号の打ち上げ: 1957-10-04.81 → 2436116.31
            (UtcDateTime::new(1957, 10, 4, 19, 26, 24.0), 2_436_116.31),
            // 1987-01-27.0 → 2446822.5
            (UtcDateTime::midnight(1987, 1, 27), 2_446_822.5),
            // 1987-06-19.5 → 2446966.0
            (UtcDateTime::new(1987, 6, 19, 12, 0, 0.0), 2_446_966.0),
            // 1988-01-27.0 → 2447187.5（うるう年の 2 月をまたぐ前）
            (UtcDateTime::midnight(1988, 1, 27), 2_447_187.5),
            // 1600-01-01.0 → 2305447.5（グレゴリオ暦の補正が効く古い日付）
            (UtcDateTime::midnight(1600, 1, 1), 2_305_447.5),
            // 2000-01-01.5 の 1 年後 2100-01-01.0 → 2488069.5
            (UtcDateTime::midnight(2100, 1, 1), 2_488_069.5),
        ];
        for (civil, expected) in cases {
            let jd = civil.to_julian_date().get();
            assert!(
                (jd - expected).abs() < 1e-6,
                "{civil:?} should be JD {expected}, got {jd}"
            );
        }
    }

    #[test]
    fn the_calendar_survives_a_round_trip() {
        // 分解と組み立てが食い違うと、HUD の時刻だけが 1 日ずれる。
        let cases = [
            UtcDateTime::new(2026, 1, 1, 0, 0, 0.0),
            UtcDateTime::new(2026, 2, 28, 23, 59, 59.0),
            UtcDateTime::new(2028, 2, 29, 12, 34, 56.0),
            UtcDateTime::new(2026, 12, 31, 23, 0, 0.0),
            UtcDateTime::new(1999, 8, 11, 11, 3, 0.0),
        ];
        for civil in cases {
            let back = civil.to_julian_date().to_utc_date_time();
            assert_eq!(back.year, civil.year, "{civil:?} -> {back:?}");
            assert_eq!(back.month, civil.month, "{civil:?} -> {back:?}");
            assert_eq!(back.day, civil.day, "{civil:?} -> {back:?}");
            assert_eq!(back.hour, civil.hour, "{civil:?} -> {back:?}");
            assert_eq!(back.minute, civil.minute, "{civil:?} -> {back:?}");
            assert!(
                (back.second - civil.second).abs() < 1e-3,
                "{civil:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn midnight_does_not_come_out_as_the_previous_day() {
        // 丸めを忘れると 23:59:59.9999999 になる。**表示が 1 日ずれる。**
        let midnight = UtcDateTime::midnight(2026, 6, 21).to_julian_date();
        let back = midnight.to_utc_date_time();
        assert_eq!((back.day, back.hour, back.minute), (21, 0, 0), "{back:?}");
        assert!(back.second.abs() < 1e-6, "{back:?}");
    }

    #[test]
    fn advancing_by_a_day_lands_on_the_next_day() {
        let start = UtcDateTime::new(2026, 6, 21, 9, 0, 0.0).to_julian_date();
        let later = start.advanced_by(Seconds(SECONDS_PER_DAY));
        let back = later.to_utc_date_time();
        assert_eq!((back.month, back.day, back.hour), (6, 22, 9), "{back:?}");
    }

    // -----------------------------------------------------------------------
    // 赤緯 — 公表されている至点・分点の時刻に突き合わせる
    // -----------------------------------------------------------------------

    #[test]
    fn the_declination_vanishes_at_the_published_equinox() {
        // 2026 年の春分は 3 月 20 日 14:46 UTC、秋分は 9 月 23 日 00:05 UTC
        // （国立天文台 / timeanddate の公表値）。分点の定義は視黄経が 0°/180° に
        // なる瞬間なので、そのとき赤緯はほぼ 0 になる。
        for civil in [
            UtcDateTime::new(2026, 3, 20, 14, 46, 0.0),
            UtcDateTime::new(2026, 9, 23, 0, 5, 0.0),
        ] {
            let declination = solar_declination(civil.to_julian_date()).to_degrees().get();
            assert!(
                declination.abs() < 0.05,
                "{civil:?}: declination should be ~0°, got {declination:.4}°"
            );
        }
    }

    #[test]
    fn the_declination_reaches_the_obliquity_at_the_published_solstice() {
        // 2026 年の夏至は 6 月 21 日 08:24 UTC、冬至は 12 月 21 日 20:50 UTC。
        // 赤緯は黄道傾斜角 23.44° に達する。
        let summer = solar_declination(UtcDateTime::new(2026, 6, 21, 8, 24, 0.0).to_julian_date())
            .to_degrees()
            .get();
        assert!(
            (summer - 23.44).abs() < 0.05,
            "the June solstice declination should be +23.44°, got {summer:.4}°"
        );

        let winter =
            solar_declination(UtcDateTime::new(2026, 12, 21, 20, 50, 0.0).to_julian_date())
                .to_degrees()
                .get();
        assert!(
            (winter + 23.44).abs() < 0.05,
            "the December solstice declination should be −23.44°, got {winter:.4}°"
        );
    }

    // -----------------------------------------------------------------------
    // 均時差 — アナレンマの公表値に突き合わせる
    // -----------------------------------------------------------------------

    #[test]
    fn the_equation_of_time_matches_the_published_analemma() {
        // 公表されている極値と零点（年による変動は 1 分未満）。
        let cases = [
            // 2 月上旬の極小 −14.2 分
            (UtcDateTime::midnight(2026, 2, 11), -14.2, 0.5),
            // 5 月中旬の極大 +3.7 分
            (UtcDateTime::midnight(2026, 5, 14), 3.7, 0.5),
            // 7 月下旬の極小 −6.5 分
            (UtcDateTime::midnight(2026, 7, 26), -6.5, 0.5),
            // 11 月上旬の極大 +16.4 分
            (UtcDateTime::midnight(2026, 11, 3), 16.4, 0.5),
            // 零点（4 月中旬・6 月中旬・9 月上旬・12 月下旬）
            (UtcDateTime::midnight(2026, 4, 15), 0.0, 0.6),
            (UtcDateTime::midnight(2026, 6, 13), 0.0, 0.6),
            (UtcDateTime::midnight(2026, 9, 1), 0.0, 0.6),
            (UtcDateTime::midnight(2026, 12, 25), 0.0, 0.6),
        ];
        for (civil, expected_minutes, tolerance) in cases {
            let minutes = equation_of_time(civil.to_julian_date()).get() / 60.0;
            assert!(
                (minutes - expected_minutes).abs() < tolerance,
                "{civil:?}: equation of time should be {expected_minutes} min, got {minutes:.2} min"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 地平座標 — 教科書の値に突き合わせる
    // -----------------------------------------------------------------------

    /// 1 日を 1 分刻みで走査し、仰角が最大になる時刻と値を返す。
    fn culmination(date: UtcDateTime, observer: Geodetic) -> (JulianDate, SolarPosition) {
        let midnight = date.to_julian_date();
        let mut best = (midnight, solar_position(midnight, observer));
        for minute in 0..(24 * 60) {
            let time = midnight.advanced_by(Seconds(f64::from(minute) * 60.0));
            let position = solar_position(time, observer);
            if position.elevation.get() > best.1.elevation.get() {
                best = (time, position);
            }
        }
        best
    }

    #[test]
    fn tokyo_noon_altitude_matches_the_published_figures() {
        // 国立天文台が公表している東京の南中高度: 夏至 約 78°、冬至 約 31°。
        // 幾何学的には 90° − 緯度 ± 23.44°（35.69°N で 77.75° と 30.87°）。
        let (_, summer) = culmination(UtcDateTime::midnight(2026, 6, 21), tokyo());
        let summer_degrees = summer.elevation.to_degrees().get();
        assert!(
            (summer_degrees - 77.75).abs() < 0.3,
            "Tokyo's midsummer noon sun should be ~77.8° up, got {summer_degrees:.2}°"
        );

        let (_, winter) = culmination(UtcDateTime::midnight(2026, 12, 21), tokyo());
        let winter_degrees = winter.elevation.to_degrees().get();
        assert!(
            (winter_degrees - 30.87).abs() < 0.3,
            "Tokyo's midwinter noon sun should be ~30.9° up, got {winter_degrees:.2}°"
        );

        // 南中の名のとおり、北半球の中緯度では真南に来る。
        for (label, position) in [("summer", summer), ("winter", winter)] {
            let azimuth = position.azimuth.to_degrees().get();
            assert!(
                (azimuth - 180.0).abs() < 0.5,
                "the {label} culmination should be due south, got {azimuth:.2}°"
            );
        }
    }

    #[test]
    fn the_sun_is_overhead_at_the_equator_on_the_equinox() {
        // 分点には太陽が赤道の真上を通る。**天頂**（90°）に達すること。
        let (_, noon) = culmination(
            UtcDateTime::midnight(2026, 3, 20),
            Geodetic::from_degrees(0.0, 0.0, 0.0),
        );
        let elevation = noon.elevation.to_degrees().get();
        assert!(
            elevation > 89.5,
            "the equinox sun should pass within 0.5° of the equatorial zenith, got {elevation:.2}°"
        );
    }

    #[test]
    fn the_equinox_day_is_twelve_hours_long_at_the_equator() {
        // 分点の昼夜はどこでもほぼ 12 時間ずつ（大気差を無視した幾何学的な値）。
        let midnight = UtcDateTime::midnight(2026, 3, 20).to_julian_date();
        let equator = Geodetic::from_degrees(0.0, 0.0, 0.0);
        let daylight_minutes = (0..(24 * 60))
            .filter(|minute| {
                let time = midnight.advanced_by(Seconds(f64::from(*minute) * 60.0));
                solar_position(time, equator).is_above_horizon()
            })
            .count();
        #[allow(
            clippy::cast_precision_loss,
            reason = "1 日は 1440 分。f64 で厳密に表せる"
        )]
        let hours = daylight_minutes as f64 / 60.0;
        assert!(
            (hours - 12.0).abs() < 0.2,
            "the equinox day should be 12 h long at the equator, got {hours:.2} h"
        );
    }

    #[test]
    fn the_equinox_sun_rises_due_east() {
        // **分点には太陽はどこでも真東から昇る。** 教科書的な事実。
        let midnight = UtcDateTime::midnight(2026, 3, 20).to_julian_date();
        for observer in [
            tokyo(),
            Geodetic::from_degrees(0.0, 0.0, 0.0),
            Geodetic::from_degrees(-33.87, 151.21, 0.0), // シドニー
        ] {
            let mut sunrise = None;
            let mut previous = solar_position(midnight, observer);
            for minute in 1..(24 * 60) {
                let time = midnight.advanced_by(Seconds(f64::from(minute) * 60.0));
                let position = solar_position(time, observer);
                if !previous.is_above_horizon() && position.is_above_horizon() {
                    sunrise = Some(position);
                    break;
                }
                previous = position;
            }
            let sunrise = sunrise.expect("the sun should rise somewhere in the day");
            let azimuth = sunrise.azimuth.to_degrees().get();
            assert!(
                (azimuth - 90.0).abs() < 1.5,
                "at {:.1}°N the equinox sunrise should be due east, got {azimuth:.2}°",
                observer.latitude_degrees()
            );
        }
    }

    #[test]
    fn the_midnight_sun_stands_still_over_the_pole() {
        // **極は特異点。** 夏至の北極では太陽は 1 日中ほぼ同じ高さ（≒ 黄道傾斜角）を
        // 回り続ける。ここで NaN や不連続が出たら、緯度 ±90° の扱いが壊れている。
        let midnight = UtcDateTime::midnight(2026, 6, 21).to_julian_date();
        let pole = Geodetic::from_degrees(90.0, 0.0, 0.0);
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for minute in 0..(24 * 60) {
            let time = midnight.advanced_by(Seconds(f64::from(minute) * 60.0));
            let position = solar_position(time, pole);
            assert!(
                position.elevation.get().is_finite() && position.azimuth.get().is_finite(),
                "the pole produced {position:?}"
            );
            lowest = lowest.min(position.elevation.to_degrees().get());
            highest = highest.max(position.elevation.to_degrees().get());
        }
        assert!(
            (highest - 23.44).abs() < 0.2 && (highest - lowest) < 0.2,
            "the midnight sun should circle at ~23.4°, got {lowest:.2}°..{highest:.2}°"
        );
    }

    #[test]
    fn the_polar_night_keeps_the_sun_down_all_day() {
        // 冬至の北極は 1 日中夜。
        let midnight = UtcDateTime::midnight(2026, 12, 21).to_julian_date();
        let pole = Geodetic::from_degrees(90.0, 0.0, 0.0);
        for minute in 0..(24 * 60) {
            let time = midnight.advanced_by(Seconds(f64::from(minute) * 60.0));
            let position = solar_position(time, pole);
            assert!(
                !position.is_above_horizon(),
                "the polar night should keep the sun below the horizon, got {position:?}"
            );
        }
    }

    #[test]
    fn the_dateline_is_not_a_special_case() {
        // 経度 ±180° は同じ場所。**日付変更線で太陽が飛ばないこと。**
        let time = UtcDateTime::new(2026, 6, 21, 0, 0, 0.0).to_julian_date();
        let east = solar_position(time, Geodetic::from_degrees(10.0, 180.0, 0.0));
        let west = solar_position(time, Geodetic::from_degrees(10.0, -180.0, 0.0));
        assert!(
            (east.elevation.get() - west.elevation.get()).abs() < 1e-9,
            "+180° gave {east:?} but −180° gave {west:?}"
        );
        assert!(
            east.azimuth
                .shortest_difference_to(west.azimuth)
                .get()
                .abs()
                < 1e-9,
            "+180° gave {east:?} but −180° gave {west:?}"
        );
    }

    #[test]
    fn the_sun_moves_west_over_the_day() {
        // 方位が東 → 南 → 西と進むこと。**符号を間違えると太陽が逆走する。**
        let midnight = UtcDateTime::midnight(2026, 4, 10).to_julian_date();
        let observer = tokyo();
        let morning = midnight.advanced_by(Seconds(0.0 * 3600.0)); // 09:00 JST
        let noon = midnight.advanced_by(Seconds(3.0 * 3600.0)); // 12:00 JST
        let evening = midnight.advanced_by(Seconds(7.0 * 3600.0)); // 16:00 JST

        let morning = solar_position(morning, observer).azimuth.to_degrees().get();
        let noon = solar_position(noon, observer).azimuth.to_degrees().get();
        let evening = solar_position(evening, observer).azimuth.to_degrees().get();
        assert!(
            morning < noon && noon < evening,
            "the sun should sweep east→south→west, got {morning:.1}° {noon:.1}° {evening:.1}°"
        );
        assert!(
            (90.0..130.0).contains(&morning),
            "at 09:00 JST the sun should be in the east-southeast, got {morning:.1}°"
        );
    }

    #[test]
    fn local_mean_solar_noon_puts_the_sun_near_the_meridian() {
        // 地方平均太陽時の正午は、均時差（±16 分 = ±4°）のぶんだけ南中とずれる。
        for longitude_degrees in [-179.0, -75.0, 0.0, 45.0, 139.7, 180.0] {
            let observer = Geodetic::from_degrees(35.0, longitude_degrees, 0.0);
            let time = JulianDate::from_local_mean_solar_time(
                UtcDateTime::new(2026, 5, 5, 12, 0, 0.0),
                observer.longitude,
            );
            let azimuth = solar_position(time, observer).azimuth.to_degrees().get();
            assert!(
                (azimuth - 180.0).abs() < 5.0,
                "local mean noon at {longitude_degrees}° gave an azimuth of {azimuth:.2}°"
            );
        }
    }

    #[test]
    fn the_sun_direction_is_a_unit_vector_all_year() {
        let start = UtcDateTime::midnight(2026, 1, 1).to_julian_date();
        for day in 0..366 {
            let time = start.advanced_by(Seconds(f64::from(day) * SECONDS_PER_DAY));
            let direction = sun_direction_ecef(time);
            assert!(
                (direction.length() - 1.0).abs() < 1e-12,
                "day {day} gave a direction of length {}",
                direction.length()
            );
        }
    }

    #[test]
    fn nothing_produces_a_nan() {
        // **NaN は全状態に伝播する。** 時刻も座標も外から来る値なので、
        // 壊れた入力で描画が止まらないことを確かめる。
        let broken_times = [
            JulianDate(f64::NAN),
            JulianDate(f64::INFINITY),
            JulianDate(f64::NEG_INFINITY),
            JulianDate(0.0),
            JulianDate(-1e12),
        ];
        let observers = [
            tokyo(),
            Geodetic::from_degrees(90.0, 0.0, 0.0),
            Geodetic::from_degrees(-90.0, 180.0, 0.0),
            Geodetic::from_degrees(f64::NAN, f64::NAN, 0.0),
            Geodetic::from_degrees(0.0, 0.0, 40_000.0),
        ];
        for time in broken_times {
            assert!(sun_direction_ecef(time).is_finite());
            assert!(solar_declination(time).get().is_finite());
            assert!(equation_of_time(time).get().is_finite());
            assert!(greenwich_mean_sidereal_time(time).get().is_finite());
            for observer in observers {
                let position = solar_position(time, observer);
                assert!(
                    position.azimuth.get().is_finite() && position.elevation.get().is_finite(),
                    "time {time:?} at {observer:?} gave {position:?}"
                );
                assert!(
                    (0.0..core::f64::consts::TAU).contains(&position.azimuth.get()),
                    "azimuth out of range: {position:?}"
                );
                assert!(
                    position.elevation.get().abs() <= FRAC_PI_2 + 1e-12,
                    "elevation out of range: {position:?}"
                );
            }
        }
    }

    #[test]
    fn the_sidereal_time_advances_by_one_turn_per_day() {
        // 恒星日は 23h56m04s。1 太陽日で 360.9856° 進む。
        let start = UtcDateTime::midnight(2026, 3, 1).to_julian_date();
        let a = greenwich_mean_sidereal_time(start).to_degrees().get();
        let b = greenwich_mean_sidereal_time(start.advanced_by(Seconds(SECONDS_PER_DAY)))
            .to_degrees()
            .get();
        let advance = (b - a).rem_euclid(360.0);
        assert!(
            (advance - 0.9856).abs() < 0.001,
            "sidereal time should gain 0.9856°/day on solar time, got {advance:.4}°"
        );
    }
}
