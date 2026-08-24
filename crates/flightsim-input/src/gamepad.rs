//! ゲームパッドの生入力から操縦入力へ変える純粋なロジック。
//!
//! # なぜ Bevy に依存しない関数として書くか
//!
//! デッドゾーンと感度カーブは既知の入力に対する期待値で検証したい。
//! Bevy の `Gamepad` コンポーネントを介さず `f64` を直接渡してテストできる
//! ようにすることで、GUI を立ち上げずに境界値（デッドゾーンちょうど、`±1.0`、NaN）
//! を検証できる。Bevy の `Gamepad` を読んで [`PilotGamepad`] を組み立てる側は
//! `crate::read_pilot_input` にある。
//!
//! # キーボードとの共存
//!
//! ジョイスティックの絶対位置とキーボードの on/off は同じ扱いにしない
//! （クレート直下のドキュメントを参照）。ここでは「ゲームパッドのその軸/
//! ボタンに触れているか」を [`is_axis_touched`] と各フィールドで判定し、
//! **触れていない軸だけ**キーボードの結果を残す。両方に触れたらゲームパッドを
//! 優先する。合成そのものは [`crate::PilotControls::update_with_gamepad`] が行う
//! （`PilotControls` の非公開フィールドに触れる必要があるため）。

/// 1 軸ぶんのデッドゾーンと感度カーブの設定。
///
/// 再バインド可能にするため、軸ごとに独立して持つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisCurve {
    /// この大きさ以下の入力は 0 として扱う。放置したスティックのドリフト対策。
    /// 目安は 0.1。
    pub deadzone: f64,
    /// デッドゾーンの外側を伸ばす指数。`1.0` は線形、大きいほど中央付近が鈍く
    /// なり、フルデフレクション近くで敏感になる（着陸時の当て舵をしやすくする）。
    pub response: f64,
    /// 軸の正負を反転する。
    pub invert: bool,
}

impl Default for AxisCurve {
    /// デッドゾーン 0.1、線形カーブ、非反転。
    fn default() -> Self {
        Self {
            deadzone: 0.1,
            response: 1.0,
            invert: false,
        }
    }
}

impl AxisCurve {
    /// # Panics
    ///
    /// `deadzone` が `[0, 1)` の範囲外、または `response` が有限の正値でない場合。
    #[must_use]
    pub fn new(deadzone: f64, response: f64, invert: bool) -> Self {
        assert!(
            deadzone.is_finite() && (0.0..1.0).contains(&deadzone),
            "deadzone must be within [0, 1), got {deadzone}"
        );
        assert!(
            response.is_finite() && response > 0.0,
            "response curve exponent must be finite and positive, got {response}"
        );
        Self {
            deadzone,
            response,
            invert,
        }
    }

    /// 生の軸値（想定範囲 `[-1, 1]`）を操縦入力へ変える。
    ///
    /// デッドゾーンの外は **連続的に 0 から立ち上がる**（境界で入力が跳ねない）。
    /// `deadzone` ちょうどは 0 を返す。範囲外の値と NaN は安全側に潰す。
    #[must_use]
    pub fn apply(self, raw: f64) -> f64 {
        let raw = if raw.is_nan() {
            0.0
        } else {
            raw.clamp(-1.0, 1.0)
        };
        let magnitude = raw.abs();
        let shaped = if magnitude <= self.deadzone {
            0.0
        } else {
            // デッドゾーンの外を [0, 1] へ再スケールしてからカーブを掛ける。
            // これにより deadzone のすぐ外側で shaped が 0 から連続的に
            // 立ち上がる。再スケールなしに `magnitude.powf(response)` を
            // 使うと、デッドゾーン境界で不連続に飛ぶ。
            let travel = (1.0 - self.deadzone).max(f64::EPSILON);
            let normalised = (magnitude - self.deadzone) / travel;
            normalised.powf(self.response)
        };
        let signed = if raw < 0.0 { -shaped } else { shaped };
        if self.invert { -signed } else { signed }
    }
}

/// 生の軸値がデッドゾーンを超えて動かされているか。
///
/// これで「その軸に触れているか」を判定し、キーボードとの共存を決める。
/// `deadzone` ちょうどは「触れていない」扱い（そこは [`AxisCurve::apply`] も
/// 0 を返すので、キーボード側にフォールバックしても出力は変わらない）。
#[must_use]
pub fn is_axis_touched(raw: f64, deadzone: f64) -> bool {
    raw.is_finite() && raw.abs() > deadzone
}

/// エルロン・エレベータ・ラダー・スロットルのデッドゾーンと感度カーブ。
///
/// 既定値はすべて [`AxisCurve::default`]（デッドゾーン 0.1、線形、非反転）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GamepadAxisMappings {
    /// 左スティック X。
    pub aileron: AxisCurve,
    /// 左スティック Y（符号は [`crate::PilotControls::update_with_gamepad`] が
    /// 反転して「手前に引く = 機首上げ」にする）。
    pub elevator: AxisCurve,
    /// 右スティック X。
    pub rudder: AxisCurve,
    /// 左右トリガー（スロットルの変化率として使う。§ crate ドキュメント参照）。
    pub throttle: AxisCurve,
}

/// ゲームパッドから読んだ生の値。1 フレーム分。
///
/// Bevy から切り離してテストするための中間表現（[`crate::PilotKeys`] と同じ設計）。
/// 軸は `[-1, 1]`、トリガーは `[0, 1]` を想定する。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PilotGamepad {
    /// 左スティック X。正で右。
    pub left_stick_x: f64,
    /// 左スティック Y。正でスティックを奥へ倒す（Bevy の軸規約）。
    pub left_stick_y: f64,
    /// 右スティック X。正で右。
    pub right_stick_x: f64,
    /// 右トリガー（スロットル増加）。
    pub right_trigger: f64,
    /// 左トリガー（スロットル減少）。
    pub left_trigger: f64,
    pub flaps_extend: bool,
    pub flaps_retract: bool,
    pub brakes: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- デッドゾーン ---

    #[test]
    fn inputs_at_or_inside_the_deadzone_are_silenced() {
        let curve = AxisCurve::new(0.1, 1.0, false);
        assert!(curve.apply(0.0).abs() < 1e-12);
        assert!(curve.apply(0.05).abs() < 1e-12);
        assert!(curve.apply(-0.05).abs() < 1e-12);
        // ちょうど境界も 0。
        assert!(curve.apply(0.1).abs() < 1e-12);
        assert!(curve.apply(-0.1).abs() < 1e-12);
    }

    #[test]
    fn the_deadzone_boundary_does_not_jump() {
        // 境界のすぐ外側は、境界のすぐ内側の 0 から連続的に立ち上がること。
        // 不連続だと、スティックをわずかに動かしただけで舵が跳ねる。
        let curve = AxisCurve::new(0.1, 1.0, false);
        let just_inside = curve.apply(0.1);
        let just_outside = curve.apply(0.1 + 1e-6);
        assert!(just_inside.abs() < 1e-12);
        assert!(
            just_outside < 1e-4,
            "output jumped to {just_outside} just past the deadzone"
        );
    }

    #[test]
    fn full_deflection_reaches_full_output() {
        let curve = AxisCurve::new(0.1, 1.0, false);
        assert!((curve.apply(1.0) - 1.0).abs() < 1e-12);
        assert!((curve.apply(-1.0) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_linear_curve_scales_the_travel_beyond_the_deadzone() {
        // deadzone 0.2、response 1.0 の線形カーブ。0.6 は残り travel の半分
        // (0.6 - 0.2) / (1.0 - 0.2) = 0.5 のはず。
        let curve = AxisCurve::new(0.2, 1.0, false);
        assert!((curve.apply(0.6) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_nonlinear_curve_softens_the_centre() {
        // response 2.0 は中央付近を鈍く、端で敏感にする。同じ入力に対して
        // 線形カーブより出力が小さいはず。
        let linear = AxisCurve::new(0.1, 1.0, false);
        let curved = AxisCurve::new(0.1, 2.0, false);
        let input = 0.5;
        assert!(curved.apply(input) < linear.apply(input));
        // 端は両方とも 1.0 に達する。
        assert!((curved.apply(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn inverting_flips_the_sign() {
        let normal = AxisCurve::new(0.1, 1.0, false);
        let inverted = AxisCurve::new(0.1, 1.0, true);
        assert!((normal.apply(0.5) + inverted.apply(0.5)).abs() < 1e-12);
    }

    #[test]
    fn out_of_range_and_non_finite_inputs_are_clamped() {
        let curve = AxisCurve::new(0.1, 1.0, false);
        assert!((curve.apply(5.0) - 1.0).abs() < 1e-12);
        assert!((curve.apply(-5.0) + 1.0).abs() < 1e-12);
        assert!(curve.apply(f64::NAN).abs() < 1e-12);
        assert!((curve.apply(f64::INFINITY) - 1.0).abs() < 1e-12);
        assert!((curve.apply(f64::NEG_INFINITY) + 1.0).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "deadzone must be within")]
    fn a_deadzone_of_one_is_rejected() {
        // deadzone 1.0 だと travel が 0 になり除算が壊れる。
        let _ = AxisCurve::new(1.0, 1.0, false);
    }

    #[test]
    #[should_panic(expected = "response curve exponent must be")]
    fn a_zero_response_is_rejected() {
        let _ = AxisCurve::new(0.1, 0.0, false);
    }

    // --- 「触れているか」判定 ---

    #[test]
    fn touch_detection_matches_the_deadzone() {
        assert!(!is_axis_touched(0.05, 0.1));
        assert!(
            !is_axis_touched(0.1, 0.1),
            "exactly at the boundary counts as untouched"
        );
        assert!(is_axis_touched(0.100_001, 0.1));
        assert!(is_axis_touched(-0.5, 0.1));
        assert!(!is_axis_touched(f64::NAN, 0.1));
    }
}
