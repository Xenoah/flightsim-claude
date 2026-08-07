//! # flightsim-input
//!
//! 入力マッピングと視点制御。
//!
//! ## キーボードとアナログ軸は別物
//!
//! ジョイスティックは軸の絶対位置がそのまま舵角になるが、**キーボードは
//! on/off しかない**。押している間だけ舵を切り、離したら中立へ戻す処理を
//! 挟まないと、キーボード操縦は制御不能になる。
//!
//! その処理は [`AxisState`] にあり、**Bevy に依存しない純粋なロジック**として
//! 書いてある。舵の効き方の検証に GUI を立ち上げなくて済むようにするため。
//!
//! ## 平滑化はここで行う
//!
//! FDM は与えられた舵角をそのまま使う。感度カーブも中立復帰もこちらの責務。

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::prelude::*;
use flightsim_core::{Radians, Seconds};
use flightsim_fdm::ControlInputs;

pub mod camera;

pub use camera::{CameraRig, ViewMode};

/// 1 本の操縦軸。キーボード入力を連続量へ変える。
///
/// ジョイスティックを繋いだ場合は [`Self::set_absolute`] で直接値を入れる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisState {
    value: f64,
    /// 押している間に 1 秒あたり動く量。
    rate: f64,
    /// 離している間に 1 秒あたり中立へ戻る量。
    centering_rate: f64,
}

impl AxisState {
    /// # Panics
    ///
    /// レートが有限の正値でない場合。0 だと舵が永久に動かない。
    #[must_use]
    pub fn new(rate: f64, centering_rate: f64) -> Self {
        assert!(
            rate.is_finite() && rate > 0.0,
            "control rate must be positive and finite, got {rate}"
        );
        assert!(
            centering_rate.is_finite() && centering_rate >= 0.0,
            "centering rate must be finite and non-negative, got {centering_rate}"
        );
        Self {
            value: 0.0,
            rate,
            centering_rate,
        }
    }

    /// 舵面の既定値。切るのは速く、戻るのはやや遅い。
    #[must_use]
    pub fn control_surface() -> Self {
        Self::new(2.5, 1.8)
    }

    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// アナログ入力を直接入れる。ジョイスティック用。
    pub fn set_absolute(&mut self, value: f64) {
        self.value = sanitise(value);
    }

    /// 1 フレーム進める。
    ///
    /// `positive` / `negative` はキーが押されているか。両方押されていれば
    /// 打ち消し合って中立へ戻る。
    pub fn update(&mut self, dt: Seconds, positive: bool, negative: bool) {
        let step = self.rate * dt.get();
        let direction = f64::from(i8::from(positive) - i8::from(negative));

        if direction.abs() > 0.0 {
            self.value = sanitise(self.value + direction * step);
            return;
        }

        // 離したら中立へ。行き過ぎて逆側へ振れないようクランプする。
        let centering = self.centering_rate * dt.get();
        self.value = if self.value.abs() <= centering {
            0.0
        } else {
            self.value - self.value.signum() * centering
        };
    }
}

/// 単調に増減する軸（スロットル・フラップ）。中立へは戻らない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampAxis {
    value: f64,
    rate: f64,
}

impl RampAxis {
    /// # Panics
    ///
    /// レートが有限の正値でない場合。
    #[must_use]
    pub fn new(initial: f64, rate: f64) -> Self {
        assert!(
            rate.is_finite() && rate > 0.0,
            "ramp rate must be positive and finite, got {rate}"
        );
        Self {
            value: initial.clamp(0.0, 1.0),
            rate,
        }
    }

    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    pub fn set_absolute(&mut self, value: f64) {
        self.value = if value.is_nan() {
            0.0
        } else {
            value.clamp(0.0, 1.0)
        };
    }

    pub fn update(&mut self, dt: Seconds, increase: bool, decrease: bool) {
        let step = self.rate * dt.get();
        let direction = f64::from(i8::from(increase) - i8::from(decrease));
        let next = self.value + direction * step;
        self.value = if next.is_nan() {
            0.0
        } else {
            next.clamp(0.0, 1.0)
        };
    }
}

/// `[-1, 1]` に収め、NaN を 0 に潰す。
///
/// `f64::clamp` は NaN を素通りさせるので、これだけでは守れない。
fn sanitise(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-1.0, 1.0)
    }
}

/// 押されているキーの集合。Bevy から切り離してテストするための中間表現。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PilotKeys {
    pub roll_right: bool,
    pub roll_left: bool,
    pub pitch_up: bool,
    pub pitch_down: bool,
    pub yaw_right: bool,
    pub yaw_left: bool,
    pub throttle_up: bool,
    pub throttle_down: bool,
    pub flaps_extend: bool,
    pub flaps_retract: bool,
    pub brakes: bool,
}

/// 操縦入力の現在値。
#[derive(Resource, Debug, Clone, Copy)]
pub struct PilotControls {
    pub aileron: AxisState,
    pub elevator: AxisState,
    pub rudder: AxisState,
    pub throttle: RampAxis,
    pub flaps: RampAxis,
    brakes: f64,
}

impl Default for PilotControls {
    fn default() -> Self {
        Self {
            aileron: AxisState::control_surface(),
            elevator: AxisState::control_surface(),
            rudder: AxisState::control_surface(),
            // スロットルは 4 秒で全開まで。急に全開にすると機体が跳ねる。
            throttle: RampAxis::new(0.0, 0.25),
            // フラップは 5 秒で全展開。
            flaps: RampAxis::new(0.0, 0.2),
            brakes: 0.0,
        }
    }
}

impl PilotControls {
    /// キー状態から 1 フレームぶん進める。
    pub fn update(&mut self, dt: Seconds, keys: PilotKeys) {
        self.aileron.update(dt, keys.roll_right, keys.roll_left);
        self.elevator.update(dt, keys.pitch_up, keys.pitch_down);
        self.rudder.update(dt, keys.yaw_right, keys.yaw_left);
        self.throttle
            .update(dt, keys.throttle_up, keys.throttle_down);
        self.flaps.update(dt, keys.flaps_extend, keys.flaps_retract);
        // ブレーキは踏んでいる間だけ。中間状態を持たせても操作感が悪くなるだけ。
        self.brakes = f64::from(u8::from(keys.brakes));
    }

    /// FDM へ渡す形にする。
    #[must_use]
    pub fn to_control_inputs(self) -> ControlInputs {
        ControlInputs::new(
            self.aileron.value(),
            self.elevator.value(),
            self.rudder.value(),
            self.throttle.value(),
            self.flaps.value(),
        )
        .with_brakes(self.brakes)
    }
}

/// 視点の見回し量（コックピット視点で首を振る）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LookAround {
    pub yaw: Radians,
    pub pitch: Radians,
}

/// 入力層のプラグイン。
#[derive(Debug, Default)]
pub struct FlightsimInputPlugin;

impl Plugin for FlightsimInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PilotControls>()
            .init_resource::<LookAround>()
            .init_resource::<ViewMode>()
            .add_systems(Update, (read_pilot_keys, cycle_view_mode));
    }
}

/// キーボードを読んで [`PilotControls`] を更新する。
pub fn read_pilot_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controls: ResMut<PilotControls>,
) {
    let keys = PilotKeys {
        roll_right: keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD),
        roll_left: keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA),
        // 実機の操縦桿と同じで、引く（下キー）と機首が上がる。
        pitch_up: keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS),
        pitch_down: keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW),
        yaw_right: keyboard.pressed(KeyCode::KeyE),
        yaw_left: keyboard.pressed(KeyCode::KeyQ),
        throttle_up: keyboard.pressed(KeyCode::PageUp) || keyboard.pressed(KeyCode::Equal),
        throttle_down: keyboard.pressed(KeyCode::PageDown) || keyboard.pressed(KeyCode::Minus),
        flaps_extend: keyboard.pressed(KeyCode::KeyF),
        flaps_retract: keyboard.pressed(KeyCode::KeyG),
        brakes: keyboard.pressed(KeyCode::Space),
    };
    controls.update(Seconds(f64::from(time.delta_secs())), keys);
}

/// `C` キーで視点を切り替える。
pub fn cycle_view_mode(keyboard: Res<ButtonInput<KeyCode>>, mut mode: ResMut<ViewMode>) {
    if keyboard.just_pressed(KeyCode::KeyC) {
        *mode = mode.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Seconds = Seconds(1.0 / 60.0);

    fn hold(axis: &mut AxisState, frames: u32, positive: bool, negative: bool) {
        for _ in 0..frames {
            axis.update(FRAME, positive, negative);
        }
    }

    // --- キーボードの舵 ---

    #[test]
    fn holding_a_key_moves_the_surface_progressively() {
        // on/off をそのまま舵角にすると、キーボード操縦は制御不能になる。
        let mut axis = AxisState::control_surface();
        axis.update(FRAME, true, false);
        let after_one = axis.value();
        assert!(
            after_one > 0.0 && after_one < 0.2,
            "one frame moved the surface to {after_one}; that is not a rate"
        );

        hold(&mut axis, 60, true, false);
        assert!(axis.value() > after_one);
    }

    #[test]
    fn a_held_key_saturates_at_full_deflection() {
        let mut axis = AxisState::control_surface();
        hold(&mut axis, 600, true, false);
        assert!((axis.value() - 1.0).abs() < 1e-9, "value {}", axis.value());
    }

    #[test]
    fn releasing_returns_the_surface_to_neutral() {
        let mut axis = AxisState::control_surface();
        hold(&mut axis, 60, true, false);
        assert!(axis.value() > 0.1);

        hold(&mut axis, 600, false, false);
        assert!(
            axis.value().abs() < 1e-9,
            "the surface settled at {} instead of neutral",
            axis.value()
        );
    }

    #[test]
    fn centering_never_overshoots_past_neutral() {
        // 行き過ぎると舵が逆へ振れ、機体が勝手に振動する。
        let mut axis = AxisState::new(2.0, 100.0);
        axis.set_absolute(0.01);
        axis.update(FRAME, false, false);
        assert!(
            (axis.value() - 0.0).abs() < 1e-12,
            "centering overshot to {}",
            axis.value()
        );
    }

    #[test]
    fn pressing_both_directions_cancels_out() {
        let mut axis = AxisState::control_surface();
        axis.set_absolute(0.5);
        hold(&mut axis, 600, true, true);
        assert!(axis.value().abs() < 1e-9);
    }

    #[test]
    fn an_analog_axis_can_be_set_directly() {
        let mut axis = AxisState::control_surface();
        axis.set_absolute(-0.75);
        assert!((axis.value() + 0.75).abs() < 1e-12);
        // 範囲外と NaN は潰される。
        axis.set_absolute(5.0);
        assert!((axis.value() - 1.0).abs() < 1e-12);
        axis.set_absolute(f64::NAN);
        assert!((axis.value() - 0.0).abs() < 1e-12);
    }

    // --- スロットル ---

    #[test]
    fn the_throttle_holds_its_position_when_released() {
        // 舵と違って、離しても戻らないのが正しい。
        let mut throttle = RampAxis::new(0.0, 0.25);
        for _ in 0..120 {
            throttle.update(FRAME, true, false);
        }
        let held = throttle.value();
        assert!(held > 0.2);

        for _ in 0..600 {
            throttle.update(FRAME, false, false);
        }
        assert!(
            (throttle.value() - held).abs() < 1e-12,
            "the throttle drifted from {held} to {}",
            throttle.value()
        );
    }

    #[test]
    fn the_throttle_stays_inside_its_range() {
        let mut throttle = RampAxis::new(0.5, 1.0);
        for _ in 0..600 {
            throttle.update(FRAME, true, false);
        }
        assert!((throttle.value() - 1.0).abs() < 1e-12);
        for _ in 0..600 {
            throttle.update(FRAME, false, true);
        }
        assert!((throttle.value() - 0.0).abs() < 1e-12);
    }

    // --- 統合 ---

    #[test]
    fn the_pilot_controls_reach_the_fdm_in_the_right_places() {
        let mut controls = PilotControls::default();
        let keys = PilotKeys {
            roll_right: true,
            pitch_up: true,
            yaw_left: true,
            throttle_up: true,
            brakes: true,
            ..PilotKeys::default()
        };
        for _ in 0..60 {
            controls.update(FRAME, keys);
        }

        let inputs = controls.to_control_inputs();
        assert!(
            inputs.aileron() > 0.0,
            "roll right should be positive aileron"
        );
        assert!(
            inputs.elevator() > 0.0,
            "pitch up should be positive elevator"
        );
        assert!(inputs.rudder() < 0.0, "yaw left should be negative rudder");
        assert!(inputs.throttle() > 0.0);
        assert!((inputs.brakes() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn controls_never_produce_non_finite_values() {
        // NaN が舵角に漏れると全状態へ伝播する。
        let mut controls = PilotControls::default();
        controls.aileron.set_absolute(f64::NAN);
        controls.throttle.set_absolute(f64::INFINITY);
        controls.update(Seconds(f64::NAN), PilotKeys::default());

        let inputs = controls.to_control_inputs();
        for value in [
            inputs.aileron(),
            inputs.elevator(),
            inputs.rudder(),
            inputs.throttle(),
            inputs.flaps(),
            inputs.brakes(),
        ] {
            assert!(value.is_finite(), "a control input was {value}");
        }
    }

    #[test]
    #[should_panic(expected = "control rate must be positive")]
    fn a_zero_rate_is_rejected() {
        let _ = AxisState::new(0.0, 1.0);
    }
}
