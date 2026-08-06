//! 決定論的なフライトディレクタ。
//!
//! # これは操縦支援の実装ではない
//!
//! 「離陸 → 旋回 → 着陸」を開ループの舵角時系列で実現するのは非現実的で、
//! 機体は本質的に不安定なため同じ舵角列が同じ結果にならない。
//! ここにあるのは **回帰テストのための駆動装置** であり、実機の自動操縦の
//! 再現ではない（[ADR-0006](../../../../docs/adr/0006-simulation-integration-layer.md)）。
//!
//! 壁時計時間・乱数・内部の積分状態を一切持たない純粋関数なので、FDM の決定論
//! （ADR-0004）は保たれる。
//!
//! # 制御の構造
//!
//! ```text
//!   高度誤差 ──> 目標ピッチ ──┐
//!   降下率誤差 ─> 目標ピッチ ──┼─> ピッチ誤差 + ピッチレート ──> エレベータ
//!   直接指定 ────────────────┘
//!
//!   方位誤差 ──> 目標バンク ──> ロール誤差 + ロールレート ──> エルロン
//!   ヨーレート ────────────────────────────────────────> ラダー（ダンパ）
//!   対気速度誤差 ──────────────────────────────────────> スロットル
//! ```
//!
//! 積分項を持たない（PD のみ）。定常偏差は残るが、**積分項は状態を持つため
//! 巻き戻しやリプレイで再現性を損なう**。テスト駆動装置には不要な複雑さ。

use flightsim_core::{Meters, MetersPerSecond, Radians};
use flightsim_fdm::{ControlInputs, RigidBodyState};

/// 縦方向の目標。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalTarget {
    /// ピッチ角を直接指定する。離陸の引き起こしとフレアで使う。
    Pitch(Radians),
    /// 対地高度を保持する。
    AltitudeAgl(Meters),
    /// 降下率を保持する。**正が降下**。
    DescentRate(MetersPerSecond),
}

/// ディレクタへの指示。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectorTargets {
    pub vertical: VerticalTarget,
    /// 目標方位（真方位）。
    pub heading: Radians,
    pub airspeed: MetersPerSecond,
    pub flaps: f64,
    pub brakes: f64,
    /// スロットルを固定する場合に指定する。離陸時の全開、着陸後のアイドル。
    pub throttle_override: Option<f64>,
    /// バンクを禁止する。地上滑走中に翼端を擦らないため。
    pub wings_level: bool,
}

/// 制御ゲイン。
///
/// **機体ごとに調整が要る。** 既定値は `AircraftConfig::light_single` 向け。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectorGains {
    /// 高度誤差 `m` あたりの目標ピッチ `rad`。
    pub altitude_to_pitch: f64,
    /// 降下率誤差 `m/s` あたりの目標ピッチ `rad`。
    pub descent_rate_to_pitch: f64,
    /// 目標ピッチの上限（絶対値）。
    pub max_pitch: Radians,
    /// ピッチ誤差 `rad` あたりのエレベータ。
    pub pitch_proportional: f64,
    /// ピッチレート `rad/s` あたりのエレベータ（減衰）。
    pub pitch_damping: f64,
    /// 方位誤差 `rad` あたりの目標バンク `rad`。
    pub heading_to_bank: f64,
    /// 目標バンクの上限（絶対値）。
    pub max_bank: Radians,
    /// ロール誤差 `rad` あたりのエルロン。
    pub roll_proportional: f64,
    /// ロールレート `rad/s` あたりのエルロン（減衰）。
    pub roll_damping: f64,
    /// ヨーレート `rad/s` あたりのラダー（減衰）。
    pub yaw_damping: f64,
    /// 対気速度誤差 `m/s` あたりのスロットル。
    pub speed_to_throttle: f64,
    /// 巡航時のスロットル基準値。
    pub trim_throttle: f64,
}

impl Default for DirectorGains {
    fn default() -> Self {
        Self {
            altitude_to_pitch: 0.004,
            descent_rate_to_pitch: 0.04,
            max_pitch: Radians(12.0_f64.to_radians()),
            pitch_proportional: 3.0,
            pitch_damping: 1.5,
            heading_to_bank: 1.5,
            max_bank: Radians(25.0_f64.to_radians()),
            roll_proportional: 2.0,
            roll_damping: 0.5,
            yaw_damping: 0.3,
            speed_to_throttle: 0.05,
            trim_throttle: 0.55,
        }
    }
}

/// PD 制御のフライトディレクタ。内部状態を持たない。
#[derive(Debug, Clone, Copy, Default)]
pub struct FlightDirector {
    gains: DirectorGains,
}

impl FlightDirector {
    #[must_use]
    pub const fn new(gains: DirectorGains) -> Self {
        Self { gains }
    }

    #[must_use]
    pub const fn gains(&self) -> DirectorGains {
        self.gains
    }

    /// 現在の状態と目標から操縦入力を作る。
    ///
    /// 対気速度は**無風を前提**に機体軸速度の大きさから求める。
    /// 風を入れる段階になったら、風ベクトルを引数に足すこと。
    #[must_use]
    pub fn control(
        &self,
        state: &RigidBodyState,
        agl: Meters,
        targets: DirectorTargets,
    ) -> ControlInputs {
        let attitude = state.attitude();
        let airspeed = state.body_velocity().length();

        // --- 縦 ---

        let target_pitch = match targets.vertical {
            VerticalTarget::Pitch(pitch) => pitch.get(),
            VerticalTarget::AltitudeAgl(target) => {
                let error = target.get() - agl.get();
                self.gains.altitude_to_pitch * error
            }
            VerticalTarget::DescentRate(target) => {
                // vertical_speed は上向きが正。降下率は符号を反転したもの。
                let current_descent = -state.vertical_speed().get();
                self.gains.descent_rate_to_pitch * (current_descent - target.get())
            }
        };
        let target_pitch = clamp_symmetric(target_pitch, self.gains.max_pitch.get());

        // 機体軸角速度 (p, q, r)。
        let (roll_rate, pitch_rate, yaw_rate) = (
            state.angular_velocity.x,
            state.angular_velocity.y,
            state.angular_velocity.z,
        );

        let elevator = self.gains.pitch_proportional * (target_pitch - attitude.pitch.get())
            - self.gains.pitch_damping * pitch_rate;

        // --- 横・方向 ---

        let target_bank = if targets.wings_level {
            0.0
        } else {
            let heading_error = attitude.yaw.shortest_difference_to(targets.heading).get();
            clamp_symmetric(
                self.gains.heading_to_bank * heading_error,
                self.gains.max_bank.get(),
            )
        };

        let aileron = self.gains.roll_proportional * (target_bank - attitude.roll.get())
            - self.gains.roll_damping * roll_rate;

        // ラダーはヨーダンパのみ。旋回の協調は行わない（テスト駆動装置には過剰）。
        let rudder = -self.gains.yaw_damping * yaw_rate;

        // --- 推力 ---

        let throttle = targets.throttle_override.unwrap_or_else(|| {
            self.gains.trim_throttle
                + self.gains.speed_to_throttle * (targets.airspeed.get() - airspeed)
        });

        // ControlInputs::new が NaN を 0 に潰し、範囲へクランプする。
        ControlInputs::new(aileron, elevator, rudder, throttle, targets.flaps)
            .with_brakes(targets.brakes)
    }
}

fn clamp_symmetric(value: f64, limit: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(-limit.abs(), limit.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Attitude, Geodetic, Ned};

    fn level_state(pitch_deg: f64, roll_deg: f64, yaw_deg: f64, speed: f64) -> RigidBodyState {
        RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.0, 139.0, 1_000.0),
            Attitude::from_degrees(roll_deg, pitch_deg, yaw_deg),
            Ned::new(speed, 0.0, 0.0),
        )
    }

    fn targets(vertical: VerticalTarget, heading_deg: f64) -> DirectorTargets {
        DirectorTargets {
            vertical,
            heading: Radians(heading_deg.to_radians()),
            airspeed: MetersPerSecond(50.0),
            flaps: 0.0,
            brakes: 0.0,
            throttle_override: None,
            wings_level: false,
        }
    }

    // --- 符号 ---

    #[test]
    fn a_nose_down_aircraft_told_to_pitch_up_gets_up_elevator() {
        // 正のエレベータ = 機首上げ指示（controls.rs の規約）。
        let state = level_state(-5.0, 0.0, 0.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(1_000.0),
            targets(VerticalTarget::Pitch(Radians(5.0_f64.to_radians())), 0.0),
        );
        assert!(
            controls.elevator() > 0.0,
            "elevator was {} for a 10° pitch-up demand",
            controls.elevator()
        );
    }

    #[test]
    fn a_right_turn_demand_produces_right_aileron() {
        // 正のエルロン = 右ロール指示。符号を取り違えると機体が逆へ回り続ける。
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(1_000.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 90.0),
        );
        assert!(
            controls.aileron() > 0.0,
            "aileron was {} when commanded to turn right",
            controls.aileron()
        );
    }

    #[test]
    fn a_left_turn_demand_produces_left_aileron() {
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(1_000.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 270.0),
        );
        assert!(
            controls.aileron() < 0.0,
            "aileron was {}",
            controls.aileron()
        );
    }

    #[test]
    fn the_shortest_way_round_is_chosen() {
        // 方位 350° から 10° へは右へ 20°。左へ 340° ではない。
        let state = level_state(0.0, 0.0, 350.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(1_000.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 10.0),
        );
        assert!(
            controls.aileron() > 0.0,
            "turning from 350° to 10° should bank right, aileron was {}",
            controls.aileron()
        );
    }

    #[test]
    fn flying_below_the_target_altitude_commands_a_climb() {
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(500.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 0.0),
        );
        assert!(
            controls.elevator() > 0.0,
            "elevator {}",
            controls.elevator()
        );
    }

    #[test]
    fn flying_above_the_target_altitude_commands_a_descent() {
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let controls = FlightDirector::default().control(
            &state,
            Meters(1_500.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 0.0),
        );
        assert!(
            controls.elevator() < 0.0,
            "elevator {}",
            controls.elevator()
        );
    }

    #[test]
    fn flying_slower_than_the_target_opens_the_throttle() {
        let slow = level_state(0.0, 0.0, 0.0, 30.0);
        let fast = level_state(0.0, 0.0, 0.0, 70.0);
        let director = FlightDirector::default();
        let demand = targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 0.0);

        assert!(
            director.control(&slow, Meters(1_000.0), demand).throttle()
                > director.control(&fast, Meters(1_000.0), demand).throttle()
        );
    }

    // --- 制限 ---

    #[test]
    fn the_bank_demand_is_limited() {
        // 180° の方位誤差でも背面にならないこと。
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let gains = DirectorGains::default();
        let controls = FlightDirector::new(gains).control(
            &state,
            Meters(1_000.0),
            targets(VerticalTarget::AltitudeAgl(Meters(1_000.0)), 179.0),
        );
        // 目標バンクが max_bank でクランプされていれば、エルロンは
        // roll_proportional * max_bank を超えない。
        assert!(
            controls.aileron() <= gains.roll_proportional * gains.max_bank.get() + 1e-9,
            "aileron {} exceeded the bank limit",
            controls.aileron()
        );
    }

    #[test]
    fn the_pitch_demand_is_limited() {
        // 10 km の高度誤差でも垂直上昇を指示しないこと。
        let state = level_state(0.0, 0.0, 0.0, 50.0);
        let gains = DirectorGains::default();
        let controls = FlightDirector::new(gains).control(
            &state,
            Meters(0.0),
            targets(VerticalTarget::AltitudeAgl(Meters(10_000.0)), 0.0),
        );
        assert!(controls.elevator() <= gains.pitch_proportional * gains.max_pitch.get() + 1e-9);
    }

    #[test]
    fn wings_level_mode_ignores_the_heading_demand() {
        // 地上滑走中に翼端を擦らないため。
        let state = level_state(0.0, 0.0, 0.0, 20.0);
        let mut demand = targets(VerticalTarget::Pitch(Radians::ZERO), 90.0);
        demand.wings_level = true;

        let controls = FlightDirector::default().control(&state, Meters(0.0), demand);
        assert!(
            controls.aileron().abs() < 1e-9,
            "aileron {} while wings-level was demanded",
            controls.aileron()
        );
    }

    #[test]
    fn a_throttle_override_wins_over_the_speed_loop() {
        let state = level_state(0.0, 0.0, 0.0, 10.0);
        let mut demand = targets(VerticalTarget::Pitch(Radians::ZERO), 0.0);
        demand.throttle_override = Some(1.0);

        let controls = FlightDirector::default().control(&state, Meters(0.0), demand);
        assert!((controls.throttle() - 1.0).abs() < 1e-12);
    }

    // --- 減衰 ---

    #[test]
    fn rotation_is_opposed_by_the_damping_terms() {
        let mut state = level_state(0.0, 0.0, 0.0, 50.0);
        state.angular_velocity = glam::DVec3::new(0.5, 0.5, 0.5);

        let demand = targets(VerticalTarget::Pitch(Radians::ZERO), 0.0);
        let controls = FlightDirector::default().control(&state, Meters(1_000.0), demand);

        assert!(controls.elevator() < 0.0, "pitch-up rate should be opposed");
        assert!(
            controls.aileron() < 0.0,
            "roll-right rate should be opposed"
        );
        assert!(controls.rudder() < 0.0, "yaw-right rate should be opposed");
    }

    // --- 健全性 ---

    #[test]
    fn the_director_is_a_pure_function() {
        // 同じ入力から常に同じ出力。内部状態を持たせるとリプレイが壊れる。
        let state = level_state(3.0, -7.0, 123.0, 44.0);
        let demand = targets(VerticalTarget::AltitudeAgl(Meters(800.0)), 200.0);
        let director = FlightDirector::default();

        let first = director.control(&state, Meters(600.0), demand);
        for _ in 0..100 {
            assert_eq!(director.control(&state, Meters(600.0), demand), first);
        }
    }

    #[test]
    fn non_finite_inputs_do_not_produce_non_finite_controls() {
        // NaN が舵角に漏れると全状態へ伝播する。
        let mut state = level_state(0.0, 0.0, 0.0, 50.0);
        state.angular_velocity = glam::DVec3::new(f64::NAN, f64::INFINITY, f64::NAN);

        let controls = FlightDirector::default().control(
            &state,
            Meters(f64::NAN),
            targets(VerticalTarget::AltitudeAgl(Meters(f64::NAN)), 0.0),
        );

        for value in [
            controls.aileron(),
            controls.elevator(),
            controls.rudder(),
            controls.throttle(),
            controls.flaps(),
            controls.brakes(),
        ] {
            assert!(value.is_finite(), "a control input was {value}");
        }
    }
}
