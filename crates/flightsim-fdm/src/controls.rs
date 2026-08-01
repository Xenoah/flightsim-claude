//! 操縦入力。
//!
//! # 符号の規約
//!
//! **入力は「操縦士が指示した向き」で表す。**空力の教科書で使われる舵面の変位角
//! （δe が正 = 昇降舵後縁下げ）とは符号が異なるので注意。
//!
//! | 入力 | 正の意味 | 生じるモーメント（機体軸） |
//! |---|---|---|
//! | `aileron` | 右ロール指示 | +X 軸まわり（右翼下げ） |
//! | `elevator` | 機首上げ指示 | +Y 軸まわり（機首上げ） |
//! | `rudder` | 右ヨー指示 | +Z 軸まわり（機首右） |
//!
//! この規約により、[`crate::AeroCoefficients`] の操縦舵効きは全て正の値になる。
//! 教科書の値を写す際は符号を反転させること。
//!
//! # 平滑化はここでは行わない
//!
//! キーボード入力の中立復帰や感度カーブは `flightsim-input` の責務。
//! FDM は与えられた舵角をそのまま使う。

/// 正規化された操縦入力。
///
/// 全ての値は構築時にクランプされ、NaN は 0 に潰される。
/// 不正な入力が物理状態へ伝播しないよう、境界で止める設計。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ControlInputs {
    aileron: f64,
    elevator: f64,
    rudder: f64,
    throttle: f64,
    flaps: f64,
}

/// NaN を 0 に潰したうえで範囲内へクランプする。
///
/// `f64::clamp` は NaN 入力に対して NaN を返すため、これだけでは守れない。
fn sanitize(value: f64, min: f64, max: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(min, max)
    }
}

impl ControlInputs {
    /// 全舵中立・スロットル全閉。
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            aileron: 0.0,
            elevator: 0.0,
            rudder: 0.0,
            throttle: 0.0,
            flaps: 0.0,
        }
    }

    /// 各入力を指定して構築する。範囲外の値はクランプされる。
    ///
    /// - `aileron` / `elevator` / `rudder`: `[-1, 1]`
    /// - `throttle` / `flaps`: `[0, 1]`
    #[must_use]
    pub fn new(aileron: f64, elevator: f64, rudder: f64, throttle: f64, flaps: f64) -> Self {
        Self {
            aileron: sanitize(aileron, -1.0, 1.0),
            elevator: sanitize(elevator, -1.0, 1.0),
            rudder: sanitize(rudder, -1.0, 1.0),
            throttle: sanitize(throttle, 0.0, 1.0),
            flaps: sanitize(flaps, 0.0, 1.0),
        }
    }

    #[must_use]
    pub fn with_aileron(mut self, value: f64) -> Self {
        self.aileron = sanitize(value, -1.0, 1.0);
        self
    }

    #[must_use]
    pub fn with_elevator(mut self, value: f64) -> Self {
        self.elevator = sanitize(value, -1.0, 1.0);
        self
    }

    #[must_use]
    pub fn with_rudder(mut self, value: f64) -> Self {
        self.rudder = sanitize(value, -1.0, 1.0);
        self
    }

    #[must_use]
    pub fn with_throttle(mut self, value: f64) -> Self {
        self.throttle = sanitize(value, 0.0, 1.0);
        self
    }

    #[must_use]
    pub fn with_flaps(mut self, value: f64) -> Self {
        self.flaps = sanitize(value, 0.0, 1.0);
        self
    }

    /// 右ロール指示。`[-1, 1]`
    #[must_use]
    pub const fn aileron(self) -> f64 {
        self.aileron
    }

    /// 機首上げ指示。`[-1, 1]`
    #[must_use]
    pub const fn elevator(self) -> f64 {
        self.elevator
    }

    /// 右ヨー指示。`[-1, 1]`
    #[must_use]
    pub const fn rudder(self) -> f64 {
        self.rudder
    }

    /// 出力指示。`[0, 1]`
    #[must_use]
    pub const fn throttle(self) -> f64 {
        self.throttle
    }

    /// フラップ展開量。`[0, 1]`
    #[must_use]
    pub const fn flaps(self) -> f64 {
        self.flaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_inputs_are_clamped() {
        let c = ControlInputs::new(5.0, -5.0, 100.0, 3.0, -1.0);
        assert!((c.aileron() - 1.0).abs() < f64::EPSILON);
        assert!((c.elevator() + 1.0).abs() < f64::EPSILON);
        assert!((c.rudder() - 1.0).abs() < f64::EPSILON);
        assert!((c.throttle() - 1.0).abs() < f64::EPSILON);
        assert!(c.flaps().abs() < f64::EPSILON);
    }

    #[test]
    fn nan_inputs_collapse_to_neutral() {
        // 入力デバイスの不調や設定ミスで NaN が来ても、物理状態を汚染させない。
        // NaN は一度入ると全状態に伝播し、原因特定が極めて困難になる。
        let c = ControlInputs::new(f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        for value in [
            c.aileron(),
            c.elevator(),
            c.rudder(),
            c.throttle(),
            c.flaps(),
        ] {
            assert!(value.is_finite(), "NaN leaked through sanitisation");
            assert!(value.abs() < f64::EPSILON);
        }
    }

    #[test]
    fn infinities_are_clamped_not_propagated() {
        let c = ControlInputs::new(f64::INFINITY, f64::NEG_INFINITY, 0.0, f64::INFINITY, 0.0);
        assert!((c.aileron() - 1.0).abs() < f64::EPSILON);
        assert!((c.elevator() + 1.0).abs() < f64::EPSILON);
        assert!((c.throttle() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn builders_only_change_the_named_input() {
        let base = ControlInputs::new(0.1, 0.2, 0.3, 0.4, 0.5);
        let modified = base.with_elevator(-0.9);

        assert!((modified.elevator() + 0.9).abs() < 1e-12);
        assert!((modified.aileron() - base.aileron()).abs() < f64::EPSILON);
        assert!((modified.rudder() - base.rudder()).abs() < f64::EPSILON);
        assert!((modified.throttle() - base.throttle()).abs() < f64::EPSILON);
        assert!((modified.flaps() - base.flaps()).abs() < f64::EPSILON);
    }

    #[test]
    fn neutral_is_all_zero() {
        let c = ControlInputs::neutral();
        assert_eq!(c, ControlInputs::default());
        assert!(c.throttle().abs() < f64::EPSILON);
    }
}
