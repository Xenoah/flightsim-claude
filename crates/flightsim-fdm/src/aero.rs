//! 空力係数と、そこから得られる機体軸の力・モーメント。
//!
//! # 失速の扱い
//!
//! 線形モデル `C_L = C_L0 + C_Lα · α` を失速角を超えても使い続けると、
//! **迎角を上げるほど揚力が増え続ける**という実機と真逆の挙動になる。
//! 失速からの回復操作（機首を下げる）が逆効果になり、シミュレータとして成立しない。
//!
//! ここでは失速角の前後で線形モデルと平板理論をシグモイドでブレンドする
//! （Beard & McLain の手法）。失速後は揚力が落ち、抗力が急増する。
//!
//! # 数値的な安定性
//!
//! ブレンド関数の教科書表記は指数の比になっており、大きな迎角で `exp` が
//! オーバーフローして `inf / inf = NaN` を生む。ここではロジスティック関数の積へ
//! 変形した等価な式を使い、オーバーフローを構造的に排除している（[`stall_blend`]）。

use crate::aircraft::{AeroCoefficients, Geometry};
use crate::controls::ControlInputs;
use flightsim_core::{KilogramsPerCubicMeter, MetersPerSecond, Radians};
use glam::DVec3;

/// 対気速度がこれ未満のとき、迎角・横滑り角をゼロとみなす `m/s`。
///
/// 静止状態で `atan2(0, 0)` や `v / 0` が不定になるのを防ぐ。
/// この速度では動圧がほぼゼロなので、力への影響は無視できる。
const MIN_AIRSPEED: f64 = 1.0e-6;

/// 角速度の無次元化で分母に用いる対気速度の下限 `m/s`。
///
/// 無次元角速度は `p·b / (2V)` で、低速では発散する。ただし動圧が `V²` に比例して
/// 消えるため、モーメント自体は速度とともにゼロへ向かう。下限は 0 除算の回避が目的。
const MIN_AIRSPEED_FOR_RATE_NORMALISATION: f64 = 1.0;

/// 対気速度ベクトルから求まる空力角。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AeroAngles {
    /// 迎角 α。機体 X 軸と相対風のなす角。機首上げが正。
    pub angle_of_attack: Radians,
    /// 横滑り角 β。相対風が右から来るときが正。
    pub sideslip: Radians,
    /// 真対気速度。
    pub true_airspeed: MetersPerSecond,
}

impl AeroAngles {
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.angle_of_attack.is_finite()
            && self.sideslip.is_finite()
            && self.true_airspeed.is_finite()
    }
}

/// 機体軸で表した対気速度ベクトル `(u, v, w)` から空力角を求める。
///
/// 静止時（対気速度ゼロ）は全てゼロを返す。**NaN を返すことはない。**
#[must_use]
pub fn aero_angles(body_airspeed: DVec3) -> AeroAngles {
    let speed = body_airspeed.length();

    if !speed.is_finite() || speed < MIN_AIRSPEED {
        return AeroAngles {
            angle_of_attack: Radians::ZERO,
            sideslip: Radians::ZERO,
            true_airspeed: MetersPerSecond(if speed.is_finite() { speed } else { 0.0 }),
        };
    }

    AeroAngles {
        angle_of_attack: Radians(body_airspeed.z.atan2(body_airspeed.x)),
        // clamp は数値誤差で |v/V| がわずかに 1 を超えた場合の保険。
        sideslip: Radians((body_airspeed.y / speed).clamp(-1.0, 1.0).asin()),
        true_airspeed: MetersPerSecond(speed),
    }
}

/// 無次元空力係数の一式。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AeroCoefficientSet {
    /// 揚力係数 C_L。
    pub lift: f64,
    /// 抗力係数 C_D。
    pub drag: f64,
    /// 横力係数 C_Y。
    pub side: f64,
    /// ロールモーメント係数（機体 X 軸まわり）。
    pub roll: f64,
    /// ピッチモーメント係数（機体 Y 軸まわり）。
    pub pitch: f64,
    /// ヨーモーメント係数（機体 Z 軸まわり）。
    pub yaw: f64,
}

/// 数値的に安定なロジスティック関数 `1 / (1 + e^-x)`。
///
/// 正負で式を切り替えることで `exp` のオーバーフローを避ける。
fn logistic(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// 失速ブレンド係数 σ。線形域で 0、完全失速域で 1、失速角でちょうど 0.5。
///
/// # 教科書の式との関係
///
/// 一般的な表記は
///
/// ```text
/// σ = (1 + e^{-M(α-α₀)} + e^{M(α+α₀)}) / ((1 + e^{-M(α-α₀)})(1 + e^{M(α+α₀))})
/// ```
///
/// だが、これは `α` が大きいと分子分母がともに `inf` になり `NaN` を返す。
/// 分母を展開すると `(1+A)(1+B) = (1+A+B) + AB` なので、
///
/// ```text
/// σ = 1 - AB / ((1+A)(1+B)) = 1 - [A/(1+A)] · [B/(1+B)]
/// ```
///
/// と変形できる。`A/(1+A)` はロジスティック関数そのものなので、
/// 指数を直接評価せずに済み、オーバーフローが構造的に起きない。
/// # ブレンド率がゼロのとき
///
/// `blend_rate <= 0` は「失速モデルを無効化する」意味として 0 を返す。
///
/// 式にそのまま 0 を入れると `logistic(0) · logistic(0) = 0.25` となり σ = 0.75、
/// つまり **常時 75% 失速している** という直感に反する状態になる。
/// テスト用に空力を無効化した設定でこれを踏んだため、明示的に潰してある。
#[must_use]
pub fn stall_blend(angle_of_attack: Radians, stall_angle: Radians, blend_rate: f64) -> f64 {
    // NaN と非正値はどちらも「失速モデルを使わない」として扱う。
    if blend_rate.is_nan() || blend_rate <= 0.0 {
        return 0.0;
    }

    let alpha = angle_of_attack.get();
    let stall = stall_angle.get().abs();

    1.0 - logistic(-blend_rate * (alpha - stall)) * logistic(blend_rate * (alpha + stall))
}

/// 空力係数を求める。
#[must_use]
pub fn coefficients(
    aero: &AeroCoefficients,
    geometry: &Geometry,
    angles: AeroAngles,
    angular_velocity_body: DVec3,
    controls: ControlInputs,
) -> AeroCoefficientSet {
    let alpha = angles.angle_of_attack.get();
    let beta = angles.sideslip.get();
    let flaps = controls.flaps();

    // --- 角速度の無次元化 ---
    let normaliser = 1.0
        / (2.0
            * angles
                .true_airspeed
                .get()
                .max(MIN_AIRSPEED_FOR_RATE_NORMALISATION));
    let roll_rate = angular_velocity_body.x * geometry.wing_span.get() * normaliser;
    let pitch_rate = angular_velocity_body.y * geometry.mean_chord.get() * normaliser;
    let yaw_rate = angular_velocity_body.z * geometry.wing_span.get() * normaliser;

    // --- 揚力・抗力 ---
    let sigma = stall_blend(
        angles.angle_of_attack,
        aero.stall_angle,
        aero.stall_blend_rate,
    );

    let lift_linear = aero.lift_zero + aero.lift_alpha * alpha + aero.lift_flaps * flaps;

    // 平板理論。失速後の翼はほぼ平板として振る舞う。
    let (sin_alpha, cos_alpha) = alpha.sin_cos();
    let lift_flat_plate = 2.0 * alpha.signum() * sin_alpha * sin_alpha * cos_alpha;
    let drag_flat_plate = 2.0 * sin_alpha * sin_alpha;

    let induced_drag = lift_linear * lift_linear
        / (core::f64::consts::PI * aero.oswald_efficiency * geometry.aspect_ratio());
    let drag_linear = aero.drag_min + induced_drag + aero.drag_flaps * flaps;

    let lift = (1.0 - sigma) * lift_linear + sigma * lift_flat_plate;
    let drag = (1.0 - sigma) * drag_linear + sigma * drag_flat_plate;

    AeroCoefficientSet {
        lift,
        drag,
        side: aero.side_beta * beta + aero.side_rudder * controls.rudder(),
        roll: aero.roll_beta * beta
            + aero.roll_rate_p * roll_rate
            + aero.roll_rate_r * yaw_rate
            + aero.roll_aileron * controls.aileron()
            + aero.roll_rudder * controls.rudder(),
        pitch: aero.pitch_zero
            + aero.pitch_alpha * alpha
            + aero.pitch_rate_q * pitch_rate
            + aero.pitch_elevator * controls.elevator()
            + aero.pitch_flaps * flaps,
        yaw: aero.yaw_beta * beta
            + aero.yaw_rate_p * roll_rate
            + aero.yaw_rate_r * yaw_rate
            + aero.yaw_aileron * controls.aileron()
            + aero.yaw_rudder * controls.rudder(),
    }
}

/// 機体軸での空気力 `N` とモーメント `N·m` を返す。
///
/// 揚力・抗力は風軸で定義されるため、迎角による回転で機体軸へ移している。
#[must_use]
pub fn body_force_and_moment(
    aero: &AeroCoefficients,
    geometry: &Geometry,
    angles: AeroAngles,
    angular_velocity_body: DVec3,
    controls: ControlInputs,
    density: KilogramsPerCubicMeter,
) -> (DVec3, DVec3) {
    let coefficients = coefficients(aero, geometry, angles, angular_velocity_body, controls);

    let speed = angles.true_airspeed.get();
    let dynamic_pressure = 0.5 * density.get() * speed * speed;
    let reference = dynamic_pressure * geometry.wing_area.get();

    let lift = reference * coefficients.lift;
    let drag = reference * coefficients.drag;

    // 風軸 → 機体軸。α = 0 のとき X = -抗力、Z = -揚力（Z 下向きなので揚力は上向き）。
    let (sin_alpha, cos_alpha) = angles.angle_of_attack.get().sin_cos();
    let force = DVec3::new(
        lift * sin_alpha - drag * cos_alpha,
        reference * coefficients.side,
        -lift * cos_alpha - drag * sin_alpha,
    );

    let moment = DVec3::new(
        reference * geometry.wing_span.get() * coefficients.roll,
        reference * geometry.mean_chord.get() * coefficients.pitch,
        reference * geometry.wing_span.get() * coefficients.yaw,
    );

    (force, moment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::AircraftConfig;
    use flightsim_core::Degrees;

    fn config() -> AircraftConfig {
        AircraftConfig::light_single()
    }

    fn coefficients_at(alpha_deg: f64) -> AeroCoefficientSet {
        let c = config();
        coefficients(
            &c.aero,
            &c.geometry,
            AeroAngles {
                angle_of_attack: Degrees(alpha_deg).to_radians(),
                sideslip: Radians::ZERO,
                true_airspeed: MetersPerSecond(50.0),
            },
            DVec3::ZERO,
            ControlInputs::neutral(),
        )
    }

    // --- 空力角 ---

    #[test]
    fn angle_of_attack_is_positive_when_the_relative_wind_comes_from_below() {
        // 機体軸で w > 0（下向き成分）は、相対風が下から来ることを意味し、迎角は正。
        let angles = aero_angles(DVec3::new(50.0, 0.0, 5.0));
        assert!(angles.angle_of_attack.get() > 0.0);
        assert!((angles.angle_of_attack.to_degrees().get() - 5.71).abs() < 0.01);
    }

    #[test]
    fn sideslip_is_positive_when_the_relative_wind_comes_from_the_right() {
        let angles = aero_angles(DVec3::new(50.0, 5.0, 0.0));
        assert!(angles.sideslip.get() > 0.0);
    }

    #[test]
    fn true_airspeed_is_the_vector_magnitude() {
        let angles = aero_angles(DVec3::new(30.0, 40.0, 0.0));
        assert!((angles.true_airspeed.get() - 50.0).abs() < 1e-12);
    }

    #[test]
    fn zero_airspeed_produces_zero_angles_not_nan() {
        // 駐機中や失速からの垂直落下で対気速度がゼロに近づく。
        // ここで NaN が出ると以後の全状態が汚染される。
        for v in [
            DVec3::ZERO,
            DVec3::new(1e-12, 0.0, 0.0),
            DVec3::splat(1e-15),
        ] {
            let angles = aero_angles(v);
            assert!(angles.is_finite(), "aero angles went non-finite for {v}");
        }
    }

    #[test]
    fn non_finite_velocity_does_not_produce_nan_angles() {
        let angles = aero_angles(DVec3::new(f64::NAN, 0.0, 0.0));
        assert!(
            angles.is_finite(),
            "a NaN velocity leaked into the aero angles"
        );
    }

    // --- 失速 ---

    #[test]
    fn stall_blend_is_zero_below_stall_and_one_above() {
        let stall = Degrees(16.0).to_radians();

        assert!(stall_blend(Degrees(0.0).to_radians(), stall, 50.0) < 1e-3);
        assert!(stall_blend(Degrees(5.0).to_radians(), stall, 50.0) < 1e-2);
        assert!(stall_blend(Degrees(40.0).to_radians(), stall, 50.0) > 0.99);
        // 負の失速も対称に扱う。
        assert!(stall_blend(Degrees(-40.0).to_radians(), stall, 50.0) > 0.99);
    }

    #[test]
    fn stall_blend_is_one_half_at_the_stall_angle() {
        let stall = Degrees(16.0).to_radians();
        let sigma = stall_blend(stall, stall, 50.0);
        assert!(
            (sigma - 0.5).abs() < 1e-6,
            "blend at the stall angle was {sigma}"
        );
    }

    #[test]
    fn zero_blend_rate_disables_the_stall_model() {
        // 回帰テスト。式にそのまま 0 を入れると logistic(0)² = 0.25 となり σ = 0.75、
        // つまり常時 75% 失速している状態になる。空力を無効化した設定で踏んだ欠陥。
        for alpha_deg in [-90.0, -20.0, 0.0, 20.0, 90.0] {
            for rate in [0.0, -1.0, -100.0] {
                let sigma = stall_blend(
                    Degrees(alpha_deg).to_radians(),
                    Degrees(16.0).to_radians(),
                    rate,
                );
                assert!(
                    sigma.abs() < f64::EPSILON,
                    "blend rate {rate} should disable the stall model, but σ was {sigma} at α={alpha_deg}°"
                );
            }
        }
        // NaN も無効化として扱う（比較が全て false になるため）。
        assert!(
            stall_blend(Radians(0.2), Radians(0.28), f64::NAN).abs() < f64::EPSILON,
            "a NaN blend rate must not leak into the coefficients"
        );
    }

    #[test]
    fn stall_blend_never_overflows() {
        // 教科書どおりの指数比の式は、ここで inf/inf = NaN を返して壊れる。
        // 回帰テスト。
        for blend_rate in [1.0, 50.0, 500.0, 5_000.0] {
            for alpha_deg in [-180.0, -90.0, 0.0, 90.0, 180.0] {
                let sigma = stall_blend(
                    Degrees(alpha_deg).to_radians(),
                    Degrees(16.0).to_radians(),
                    blend_rate,
                );
                assert!(
                    sigma.is_finite(),
                    "stall blend returned {sigma} for α={alpha_deg}°, rate={blend_rate}"
                );
                assert!(
                    (0.0..=1.0).contains(&sigma),
                    "stall blend {sigma} left [0, 1] for α={alpha_deg}°, rate={blend_rate}"
                );
            }
        }
    }

    #[test]
    fn lift_peaks_near_the_stall_angle_then_falls() {
        // 失速の定義そのもの。線形モデルのままだとこのテストは通らない。
        let mut peak_lift = f64::NEG_INFINITY;
        let mut peak_alpha = 0.0;

        for tenth_degree in 0..=400 {
            let alpha = f64::from(tenth_degree) * 0.1;
            let lift = coefficients_at(alpha).lift;
            if lift > peak_lift {
                peak_lift = lift;
                peak_alpha = alpha;
            }
        }

        assert!(
            (10.0..20.0).contains(&peak_alpha),
            "maximum lift occurred at {peak_alpha}°, expected near the 16° stall angle"
        );
        // 失速後は明確に揚力が落ちること。
        let post_stall = coefficients_at(30.0).lift;
        assert!(
            post_stall < peak_lift * 0.9,
            "lift barely dropped after the stall: peak {peak_lift}, at 30° {post_stall}"
        );
    }

    #[test]
    fn drag_rises_sharply_after_the_stall() {
        let cruise = coefficients_at(3.0).drag;
        let stalled = coefficients_at(35.0).drag;
        assert!(
            stalled > cruise * 5.0,
            "post-stall drag ({stalled}) should be far above cruise drag ({cruise})"
        );
    }

    #[test]
    fn coefficients_stay_finite_across_the_whole_angle_range() {
        for alpha_deg in -180..=180 {
            let c = coefficients_at(f64::from(alpha_deg));
            for (name, value) in [
                ("lift", c.lift),
                ("drag", c.drag),
                ("side", c.side),
                ("roll", c.roll),
                ("pitch", c.pitch),
                ("yaw", c.yaw),
            ] {
                assert!(
                    value.is_finite(),
                    "{name} coefficient was {value} at α={alpha_deg}°"
                );
            }
        }
    }

    #[test]
    fn drag_is_never_negative() {
        // 負の抗力は機体を加速させる。エネルギーが湧く。
        for alpha_deg in -180..=180 {
            let drag = coefficients_at(f64::from(alpha_deg)).drag;
            assert!(drag >= 0.0, "drag was {drag} at α={alpha_deg}°");
        }
    }

    // --- 力とモーメント ---

    #[test]
    fn lift_acts_upward_in_body_axes_at_zero_alpha() {
        let c = config();
        let (force, _) = body_force_and_moment(
            &c.aero,
            &c.geometry,
            AeroAngles {
                angle_of_attack: Radians::ZERO,
                sideslip: Radians::ZERO,
                true_airspeed: MetersPerSecond(50.0),
            },
            DVec3::ZERO,
            ControlInputs::neutral(),
            KilogramsPerCubicMeter(1.225),
        );

        // 機体 Z は下向きなので、揚力は負の Z 成分として現れる。
        assert!(
            force.z < 0.0,
            "lift must produce a negative body-Z force, got {}",
            force.z
        );
        // 抗力は機体を後ろへ押す。
        assert!(
            force.x < 0.0,
            "drag must produce a negative body-X force, got {}",
            force.x
        );
    }

    #[test]
    fn dynamic_pressure_scales_forces_with_the_square_of_speed() {
        let c = config();
        let force_at = |speed: f64| {
            body_force_and_moment(
                &c.aero,
                &c.geometry,
                AeroAngles {
                    angle_of_attack: Degrees(2.0).to_radians(),
                    sideslip: Radians::ZERO,
                    true_airspeed: MetersPerSecond(speed),
                },
                DVec3::ZERO,
                ControlInputs::neutral(),
                KilogramsPerCubicMeter(1.225),
            )
            .0
            .z
        };

        // 速度 2 倍で力は 4 倍。
        let ratio = force_at(80.0) / force_at(40.0);
        assert!(
            (ratio - 4.0).abs() < 1e-9,
            "force ratio was {ratio}, expected 4"
        );
    }

    #[test]
    fn elevator_input_produces_a_nose_up_moment() {
        let c = config();
        let moment_for = |elevator: f64| {
            body_force_and_moment(
                &c.aero,
                &c.geometry,
                AeroAngles {
                    angle_of_attack: Radians::ZERO,
                    sideslip: Radians::ZERO,
                    true_airspeed: MetersPerSecond(50.0),
                },
                DVec3::ZERO,
                ControlInputs::neutral().with_elevator(elevator),
                KilogramsPerCubicMeter(1.225),
            )
            .1
            .y
        };

        // 操縦指示の規約: 正のエレベータ入力 = 機首上げ = 正の Y 軸モーメント。
        assert!(moment_for(1.0) > moment_for(0.0));
        assert!(moment_for(-1.0) < moment_for(0.0));
    }

    #[test]
    fn aileron_input_produces_a_right_roll_moment() {
        let c = config();
        let moment_for = |aileron: f64| {
            body_force_and_moment(
                &c.aero,
                &c.geometry,
                AeroAngles {
                    angle_of_attack: Radians::ZERO,
                    sideslip: Radians::ZERO,
                    true_airspeed: MetersPerSecond(50.0),
                },
                DVec3::ZERO,
                ControlInputs::neutral().with_aileron(aileron),
                KilogramsPerCubicMeter(1.225),
            )
            .1
        };

        assert!(moment_for(1.0).x > 0.0, "positive aileron must roll right");
        // 逆ヨー: 右ロール操作は左（負）のヨーモーメントを生む。
        assert!(
            moment_for(1.0).z < 0.0,
            "adverse yaw should oppose the turn"
        );
    }

    #[test]
    fn angular_rates_produce_damping_moments() {
        let c = config();
        let moment_with_rate = |rate: DVec3| {
            body_force_and_moment(
                &c.aero,
                &c.geometry,
                AeroAngles {
                    angle_of_attack: Radians::ZERO,
                    sideslip: Radians::ZERO,
                    true_airspeed: MetersPerSecond(50.0),
                },
                rate,
                ControlInputs::neutral(),
                KilogramsPerCubicMeter(1.225),
            )
            .1
        };

        // 各軸の回転は、それを打ち消す向きのモーメントを生む。
        // これが無いと機体は一度回り始めたら止まらない。
        assert!(
            moment_with_rate(DVec3::new(1.0, 0.0, 0.0)).x < 0.0,
            "roll damping missing"
        );
        assert!(
            moment_with_rate(DVec3::new(0.0, 1.0, 0.0)).y < 0.0,
            "pitch damping missing"
        );
        assert!(
            moment_with_rate(DVec3::new(0.0, 0.0, 1.0)).z < 0.0,
            "yaw damping missing"
        );
    }

    #[test]
    fn zero_airspeed_produces_zero_aerodynamic_force() {
        let c = config();
        let (force, moment) = body_force_and_moment(
            &c.aero,
            &c.geometry,
            aero_angles(DVec3::ZERO),
            DVec3::new(1.0, 1.0, 1.0),
            ControlInputs::new(1.0, 1.0, 1.0, 1.0, 1.0),
            KilogramsPerCubicMeter(1.225),
        );

        assert!(
            force.length() < 1e-9,
            "stationary aircraft had aerodynamic force {force}"
        );
        assert!(
            moment.length() < 1e-9,
            "stationary aircraft had aerodynamic moment {moment}"
        );
    }

    #[test]
    fn static_stability_opposes_an_increase_in_angle_of_attack() {
        // 迎角が増えると機首下げモーメントが出て、元に戻ろうとする。
        // これが縦静安定であり、無いと機体は操縦不能になる。
        let low = coefficients_at(2.0).pitch;
        let high = coefficients_at(8.0).pitch;
        assert!(
            high < low,
            "pitching moment did not become more nose-down as α increased"
        );
    }
}
