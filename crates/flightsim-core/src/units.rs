//! 単位付き newtype。
//!
//! 内部表現は常に SI（m, kg, s, rad, K, Pa, N）。ノット・フィート・度は
//! **UI・入力・外部データ読込の境界でのみ** 変換する。
//!
//! # なぜ newtype にするのか
//!
//! フライトシミュレータで最も多く、かつ最も見つけにくいバグが単位の取り違えである。
//! ft と m、kt と m/s、deg と rad は数値としては同種なので、コンパイラも人間のレビューも
//! 取り違えを検出できない。症状は「高度がおよそ 3.28 倍おかしい」という形で現れ、
//! 原因箇所から遠く離れた場所で顕在化する。
//!
//! 型で分けておけば、変換を書き忘れた時点でビルドが通らなくなる。

use core::fmt;

// ---------------------------------------------------------------------------
// 換算係数
// ---------------------------------------------------------------------------

/// 国際フィート。定義上の厳密値。
pub const METERS_PER_FOOT: f64 = 0.3048;

/// 国際海里。定義上の厳密値。
pub const METERS_PER_NAUTICAL_MILE: f64 = 1852.0;

/// 1 ノット = 1 海里/時。
pub const METERS_PER_SECOND_PER_KNOT: f64 = METERS_PER_NAUTICAL_MILE / 3600.0;

/// 1 ft/min。昇降計の単位。
pub const METERS_PER_SECOND_PER_FOOT_PER_MINUTE: f64 = METERS_PER_FOOT / 60.0;

/// 摂氏零度に対応する絶対温度。
pub const KELVIN_AT_ZERO_CELSIUS: f64 = 273.15;

// ---------------------------------------------------------------------------
// newtype 定義マクロ
// ---------------------------------------------------------------------------

macro_rules! define_unit {
    ($(#[$attr:meta])* $name:ident, $symbol:literal) => {
        $(#[$attr])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
        #[repr(transparent)]
        pub struct $name(pub f64);

        impl $name {
            /// 零値。
            pub const ZERO: Self = Self(0.0);

            /// 単位記号（表示用）。
            pub const SYMBOL: &'static str = $symbol;

            #[inline]
            #[must_use]
            pub const fn new(value: f64) -> Self {
                Self(value)
            }

            /// 内部の生値を取り出す。**呼び出し側で単位を保証すること。**
            #[inline]
            #[must_use]
            pub const fn get(self) -> f64 {
                self.0
            }

            #[inline]
            #[must_use]
            pub fn abs(self) -> Self {
                Self(self.0.abs())
            }

            #[inline]
            #[must_use]
            pub fn min(self, other: Self) -> Self {
                Self(self.0.min(other.0))
            }

            #[inline]
            #[must_use]
            pub fn max(self, other: Self) -> Self {
                Self(self.0.max(other.0))
            }

            #[inline]
            #[must_use]
            pub fn clamp(self, lo: Self, hi: Self) -> Self {
                Self(self.0.clamp(lo.0, hi.0))
            }

            /// 数値シミュレーションでは NaN が全状態に伝播するため、
            /// 主要な状態量はこれで検査できるようにしておく。
            #[inline]
            #[must_use]
            pub fn is_finite(self) -> bool {
                self.0.is_finite()
            }
        }

        impl core::ops::Add for $name {
            type Output = Self;
            #[inline]
            fn add(self, rhs: Self) -> Self { Self(self.0 + rhs.0) }
        }

        impl core::ops::Sub for $name {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: Self) -> Self { Self(self.0 - rhs.0) }
        }

        impl core::ops::Neg for $name {
            type Output = Self;
            #[inline]
            fn neg(self) -> Self { Self(-self.0) }
        }

        impl core::ops::Mul<f64> for $name {
            type Output = Self;
            #[inline]
            fn mul(self, rhs: f64) -> Self { Self(self.0 * rhs) }
        }

        impl core::ops::Mul<$name> for f64 {
            type Output = $name;
            #[inline]
            fn mul(self, rhs: $name) -> $name { $name(self * rhs.0) }
        }

        impl core::ops::Div<f64> for $name {
            type Output = Self;
            #[inline]
            fn div(self, rhs: f64) -> Self { Self(self.0 / rhs) }
        }

        /// 同種の量どうしの除算は無次元量になる。
        impl core::ops::Div for $name {
            type Output = f64;
            #[inline]
            fn div(self, rhs: Self) -> f64 { self.0 / rhs.0 }
        }

        impl core::ops::AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, rhs: Self) { self.0 += rhs.0; }
        }

        impl core::ops::SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, rhs: Self) { self.0 -= rhs.0; }
        }

        impl core::iter::Sum for $name {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                Self(iter.map(|v| v.0).sum())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{} {}", self.0, $symbol)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// 長さ
// ---------------------------------------------------------------------------

define_unit!(
    /// メートル。**長さの内部標準。**
    Meters,
    "m"
);

define_unit!(
    /// フィート。高度の表示に使う慣習単位。境界でのみ用いる。
    Feet,
    "ft"
);

define_unit!(
    /// 平方メートル（翼面積など）。
    SquareMeters,
    "m^2"
);

impl Meters {
    #[inline]
    #[must_use]
    pub fn to_feet(self) -> Feet {
        Feet(self.0 / METERS_PER_FOOT)
    }
}

impl Feet {
    #[inline]
    #[must_use]
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * METERS_PER_FOOT)
    }
}

// ---------------------------------------------------------------------------
// 速度
// ---------------------------------------------------------------------------

define_unit!(
    /// メートル毎秒。**速度の内部標準。**
    MetersPerSecond,
    "m/s"
);

define_unit!(
    /// ノット。対気速度・対地速度の表示に使う慣習単位。
    Knots,
    "kt"
);

define_unit!(
    /// フィート毎分。昇降計の慣習単位。
    FeetPerMinute,
    "ft/min"
);

impl MetersPerSecond {
    #[inline]
    #[must_use]
    pub fn to_knots(self) -> Knots {
        Knots(self.0 / METERS_PER_SECOND_PER_KNOT)
    }

    #[inline]
    #[must_use]
    pub fn to_feet_per_minute(self) -> FeetPerMinute {
        FeetPerMinute(self.0 / METERS_PER_SECOND_PER_FOOT_PER_MINUTE)
    }
}

impl Knots {
    #[inline]
    #[must_use]
    pub fn to_meters_per_second(self) -> MetersPerSecond {
        MetersPerSecond(self.0 * METERS_PER_SECOND_PER_KNOT)
    }
}

impl FeetPerMinute {
    #[inline]
    #[must_use]
    pub fn to_meters_per_second(self) -> MetersPerSecond {
        MetersPerSecond(self.0 * METERS_PER_SECOND_PER_FOOT_PER_MINUTE)
    }
}

// ---------------------------------------------------------------------------
// 角度
// ---------------------------------------------------------------------------

define_unit!(
    /// ラジアン。**角度の内部標準。**
    Radians,
    "rad"
);

define_unit!(
    /// 度。緯度経度・方位・姿勢角の表示に使う慣習単位。
    Degrees,
    "deg"
);

impl Radians {
    /// `[-π, π)` に正規化する。姿勢角の差分に使う。
    #[must_use]
    pub fn wrap_signed(self) -> Self {
        use core::f64::consts::PI;
        Self(Self(self.0 + PI).wrap_positive().0 - PI)
    }

    /// `[0, 2π)` に正規化する。**方位（heading）に使う。**
    ///
    /// 方位が 359° → 1° をまたぐ際の扱いは、この種のコードの定番の欠陥箇所。
    #[must_use]
    pub fn wrap_positive(self) -> Self {
        use core::f64::consts::TAU;
        let mut v = self.0 % TAU;
        if v < 0.0 {
            v += TAU;
            // `-1e-16` のような極小の負値に TAU を足すと、丸め誤差で TAU そのものに
            // なり得る（TAU 近傍の f64 の刻みは約 8.9e-16 なので、それ未満の差は消える）。
            // その結果は半開区間 [0, TAU) の上端を越えるため 0 に畳む。
            //
            // これを怠ると、ほぼ真北を向いた機体の方位が 0° ではなく 360° として
            // 出力される。表示上は等価に見えるが、範囲を前提にした下流の計算が壊れる。
            if v >= TAU {
                v = 0.0;
            }
        }
        Self(v)
    }

    /// `self` から `other` への最短角差。結果は `[-π, π)`。
    ///
    /// **角度の比較には必ずこれを使うこと。** 単純な減算では、
    /// 359° と 1° の差が 358° になってしまう（正しくは 2°）。
    #[must_use]
    pub fn shortest_difference_to(self, other: Self) -> Self {
        Self(other.0 - self.0).wrap_signed()
    }

    #[inline]
    #[must_use]
    pub fn to_degrees(self) -> Degrees {
        Degrees(self.0.to_degrees())
    }

    #[inline]
    #[must_use]
    pub fn sin(self) -> f64 {
        self.0.sin()
    }

    #[inline]
    #[must_use]
    pub fn cos(self) -> f64 {
        self.0.cos()
    }

    #[inline]
    #[must_use]
    pub fn tan(self) -> f64 {
        self.0.tan()
    }
}

impl Degrees {
    #[inline]
    #[must_use]
    pub fn to_radians(self) -> Radians {
        Radians(self.0.to_radians())
    }
}

// ---------------------------------------------------------------------------
// 大気・質量・力
// ---------------------------------------------------------------------------

define_unit!(
    /// 絶対温度。**温度の内部標準。**
    Kelvin,
    "K"
);

define_unit!(
    /// パスカル（気圧）。
    Pascals,
    "Pa"
);

define_unit!(
    /// キログラム。
    Kilograms,
    "kg"
);

define_unit!(
    /// 空気密度 `kg/m³`。動圧の計算に使う。
    KilogramsPerCubicMeter,
    "kg/m^3"
);

define_unit!(
    /// ニュートン（力・推力）。
    Newtons,
    "N"
);

define_unit!(
    /// 秒。**時間の内部標準。**
    ///
    /// シミュレーション内の経過時間を表す。壁時計時間ではない（ADR-0004）。
    Seconds,
    "s"
);

impl Kelvin {
    #[inline]
    #[must_use]
    pub fn from_celsius(c: f64) -> Self {
        Self(c + KELVIN_AT_ZERO_CELSIUS)
    }

    #[inline]
    #[must_use]
    pub fn to_celsius(self) -> f64 {
        self.0 - KELVIN_AT_ZERO_CELSIUS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 浮動小数の比較。`clippy::float_cmp` を避けつつ意図を明示する。
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

    // 外部の公表値と突き合わせる。実装をなぞったテストは検証にならない。

    #[test]
    fn foot_conversion_matches_international_definition() {
        // 国際フィートは 0.3048 m ちょうどと定義されている。
        assert_close!(Feet(1.0).to_meters().get(), 0.3048, 0.0);
        assert_close!(Feet(1000.0).to_meters().get(), 304.8, 1e-12);
        assert_close!(Meters(304.8).to_feet().get(), 1000.0, 1e-9);
    }

    #[test]
    fn knot_conversion_matches_nautical_mile_definition() {
        // 1 kt = 1852 m/h = 0.514444... m/s
        assert_close!(
            Knots(1.0).to_meters_per_second().get(),
            0.5144444444444445,
            1e-15
        );
        // 100 kt = 51.4444 m/s（航空計器の典型値）
        assert_close!(
            Knots(100.0).to_meters_per_second().get(),
            51.44444444444444,
            1e-12
        );
    }

    #[test]
    fn feet_per_minute_conversion() {
        // 1000 ft/min ≒ 5.08 m/s（標準的な上昇率）
        assert_close!(
            FeetPerMinute(1000.0).to_meters_per_second().get(),
            5.08,
            1e-12
        );
        assert_close!(
            MetersPerSecond(5.08).to_feet_per_minute().get(),
            1000.0,
            1e-9
        );
    }

    #[test]
    fn round_trip_conversions_are_stable() {
        for v in [-12345.678, -1.0, 0.0, 1.0, 33_000.0] {
            assert_close!(Meters(v).to_feet().to_meters().get(), v, 1e-9);
            assert_close!(
                MetersPerSecond(v).to_knots().to_meters_per_second().get(),
                v,
                1e-9
            );
            assert_close!(Radians(v).to_degrees().to_radians().get(), v, 1e-9);
        }
    }

    #[test]
    fn celsius_kelvin_uses_standard_offset() {
        assert_close!(Kelvin::from_celsius(15.0).get(), 288.15, 1e-12);
        assert_close!(Kelvin::from_celsius(-56.5).get(), 216.65, 1e-12);
        assert_close!(Kelvin(288.15).to_celsius(), 15.0, 1e-12);
    }

    #[test]
    fn wrap_positive_handles_heading_wraparound() {
        use core::f64::consts::{PI, TAU};
        // 方位は [0, 2π) に収まること。-10° は 350° になる。
        assert_close!(
            Degrees(-10.0)
                .to_radians()
                .wrap_positive()
                .to_degrees()
                .get(),
            350.0,
            1e-9
        );
        assert_close!(Radians(TAU).wrap_positive().get(), 0.0, 1e-9);
        assert_close!(Radians(-PI).wrap_positive().get(), PI, 1e-9);
        // 何周しても範囲内。
        for k in -5..=5 {
            let v = Radians(f64::from(k) * TAU + 1.0).wrap_positive().get();
            assert!(
                (0.0..TAU).contains(&v),
                "wrap_positive left {v} out of range"
            );
        }
    }

    #[test]
    fn wrap_positive_never_returns_the_upper_bound() {
        // 回帰テスト。極小の負値に TAU を足すと丸めで TAU ちょうどになり、
        // 半開区間 [0, TAU) の上端を越えていた。
        // 症状: ほぼ真北を向いた機体の方位が 0° ではなく 360° になる。
        use core::f64::consts::TAU;
        for v in [-1e-16, -1e-18, -f64::MIN_POSITIVE, -0.0, 0.0] {
            let wrapped = Radians(v).wrap_positive().get();
            assert!(
                (0.0..TAU).contains(&wrapped),
                "wrap_positive({v}) returned {wrapped}, which is outside [0, TAU)"
            );
        }
        // 同じ理由で wrap_signed も上端 π を返してはならない。
        use core::f64::consts::PI;
        for v in [PI - 1e-16, PI, -PI - 1e-16] {
            let wrapped = Radians(v).wrap_signed().get();
            assert!(
                (-PI..PI).contains(&wrapped),
                "wrap_signed({v}) returned {wrapped}, which is outside [-PI, PI)"
            );
        }
    }

    #[test]
    fn shortest_difference_handles_compass_wraparound() {
        // 359° と 1° の差は 358° ではなく 2°。
        let a = Degrees(359.0).to_radians();
        let b = Degrees(1.0).to_radians();
        assert_close!(a.shortest_difference_to(b).to_degrees().get(), 2.0, 1e-9);
        assert_close!(b.shortest_difference_to(a).to_degrees().get(), -2.0, 1e-9);

        // 同一方位の差はゼロ（表現が 0° と 360° で異なっていても）。
        let north_a = Degrees(0.0).to_radians();
        let north_b = Degrees(360.0).to_radians();
        assert_close!(north_a.shortest_difference_to(north_b).get(), 0.0, 1e-12);

        // 結果は常に [-π, π)。
        for from in (0..360).step_by(7) {
            for to in (0..360).step_by(11) {
                let d = Degrees(f64::from(from))
                    .to_radians()
                    .shortest_difference_to(Degrees(f64::from(to)).to_radians())
                    .get();
                assert!(
                    (-core::f64::consts::PI..core::f64::consts::PI).contains(&d),
                    "difference {d} from {from}° to {to}° left [-PI, PI)"
                );
            }
        }
    }

    #[test]
    fn wrap_signed_handles_attitude_difference() {
        use core::f64::consts::{PI, TAU};
        assert_close!(Radians(PI + 0.1).wrap_signed().get(), -PI + 0.1, 1e-9);
        assert_close!(Radians(-PI - 0.1).wrap_signed().get(), PI - 0.1, 1e-9);
        for k in -5..=5 {
            let v = Radians(f64::from(k) * TAU + 0.3).wrap_signed().get();
            assert!((-PI..PI).contains(&v), "wrap_signed left {v} out of range");
        }
    }

    #[test]
    fn arithmetic_preserves_units() {
        let a = Meters(100.0);
        let b = Meters(25.0);
        assert_close!((a + b).get(), 125.0, 0.0);
        assert_close!((a - b).get(), 75.0, 0.0);
        assert_close!((a * 2.0).get(), 200.0, 0.0);
        assert_close!((2.0 * a).get(), 200.0, 0.0);
        // 同種の量どうしの除算は無次元。
        assert_close!(a / b, 4.0, 0.0);
        assert_close!([a, b].into_iter().sum::<Meters>().get(), 125.0, 0.0);
    }
}
