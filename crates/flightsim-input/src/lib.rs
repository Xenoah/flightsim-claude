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
pub mod gamepad;

pub use camera::{CameraRig, ViewMode};
pub use gamepad::{AxisCurve, GamepadAxisMappings, PilotGamepad};

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

    /// アナログ入力で 1 フレーム進める。実機のスロットルレバーと同じ操作感。
    ///
    /// `rate_fraction` は `[-1, 1]`（正で増加、負で減少、0 で保持)。トリガーを
    /// 離すと `rate_fraction` は 0 になり、**現在値をそのまま保持する**。
    /// [`Self::set_absolute`] のように瞬時値を書き込むわけではない —
    /// トリガーの押し込み量を「動かす速さ」として扱う。
    pub fn update_analog(&mut self, dt: Seconds, rate_fraction: f64) {
        let rate_fraction = if rate_fraction.is_nan() {
            0.0
        } else {
            rate_fraction.clamp(-1.0, 1.0)
        };
        let next = self.value + self.rate * dt.get() * rate_fraction;
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
    /// トリムを機首上げ側へ（遅い速度で釣り合う）。
    pub trim_up: bool,
    /// トリムを機首下げ側へ（速い速度で釣り合う）。
    pub trim_down: bool,
}

/// 昇降舵トリム。
///
/// # なぜ要るのか
///
/// **舵から手を離すと機体は「舵中立で釣り合う速度」へ向かう。** この機体では
/// それが約 107 kt で、離陸直後の 75 kt からは大きく機首を下げて加速しようと
/// する。高度があれば長周期振動が減衰して落ち着くが、**離陸直後は最初の
/// 一振りで地面に届く**（実測: 60 m で手を離すと 8.8 秒後に接地）。
///
/// 実機に必ずトリムが付いているのはこのためで、無いほうが不自然だった。
///
/// # 何をするものか
///
/// 昇降舵に足し込まれる、中立へ戻らない量。**これで釣り合う速度が決まる。**
/// 正で機首上げ＝遅い速度で釣り合う。
///
/// | トリム | 釣り合う迎角 | おおよその速度 |
/// |---|---|---|
/// | 0.00 | 0.97° | 107 kt |
/// | 0.09 | 4.3° | 75 kt |
/// | 0.20 | 8.4° | 58 kt |
///
/// 値は `AircraftConfig::light_single` の係数から解いたもの
/// （`Cm = pitch_zero + pitch_alpha·α + pitch_elevator·δe = 0`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevatorTrim {
    value: f64,
    /// キー 1 秒あたりの変化量。**速すぎると狙った速度に置けない。**
    rate: f64,
}

impl Default for ElevatorTrim {
    fn default() -> Self {
        // 0.09 で約 75 kt。**離陸直後に手を離しても落ちない**ところに置く。
        // 0 のままだと 107 kt へ向かって機首を下げる。
        Self::new(0.09, 0.12)
    }
}

impl ElevatorTrim {
    /// 初期値と変化率から作る。
    ///
    /// # Panics
    ///
    /// `rate` が有限の非負値でない場合。
    #[must_use]
    pub fn new(initial: f64, rate: f64) -> Self {
        assert!(
            rate.is_finite() && rate >= 0.0,
            "trim rate must be finite and non-negative, got {rate}"
        );
        Self {
            value: sanitise(initial),
            rate,
        }
    }

    /// 現在のトリム。
    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// 直接置く。**やり直しと自動トリムが使う。**
    pub fn set(&mut self, value: f64) {
        self.value = sanitise(value);
    }

    /// キー入力で 1 フレームぶん動かす。
    pub fn update(&mut self, dt: Seconds, nose_up: bool, nose_down: bool) {
        let direction = f64::from(i8::from(nose_up) - i8::from(nose_down));
        if direction.abs() > 0.0 {
            self.value = sanitise(self.value + direction * self.rate * dt.get());
        }
    }
}

/// 操縦入力の現在値。
#[derive(Resource, Debug, Clone, Copy)]
pub struct PilotControls {
    pub aileron: AxisState,
    pub elevator: AxisState,
    pub rudder: AxisState,
    pub throttle: RampAxis,
    pub flaps: RampAxis,
    /// 昇降舵トリム。**中立へ戻らない。**
    pub trim: ElevatorTrim,
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
            trim: ElevatorTrim::default(),
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
        self.trim.update(dt, keys.trim_up, keys.trim_down);
        // ブレーキは踏んでいる間だけ。中間状態を持たせても操作感が悪くなるだけ。
        self.brakes = f64::from(u8::from(keys.brakes));
    }

    /// キー状態とゲームパッドの両方から 1 フレームぶん進める。
    ///
    /// # キーボードとの共存
    ///
    /// **軸ごとに**「ゲームパッドのその軸に触れているか」を見る。触れていれば
    /// ゲームパッドの値をそのまま使い(ジョイスティックは絶対位置がそのまま
    /// 舵角)、触れていなければキーボードのレート制御に委ねる。これにより、
    /// 「ゲームパッドを繋いだらキーボードが死ぬ」も「一方のチャンネルを
    /// 動かしたらもう一方が巻き込まれる」も起きない。ブレーキだけは論理和で
    /// 合成する(踏んでいる間だけ有効という on/off な性質上、競合しないため)。
    ///
    /// `gamepad` が `None`(未接続)なら [`Self::update`] と完全に同じ結果になる。
    pub fn update_with_gamepad(
        &mut self,
        dt: Seconds,
        keys: PilotKeys,
        gamepad: Option<PilotGamepad>,
        mappings: &GamepadAxisMappings,
    ) {
        let gp = gamepad.unwrap_or_default();

        if gamepad.is_some() && gamepad::is_axis_touched(gp.left_stick_x, mappings.aileron.deadzone)
        {
            self.aileron
                .set_absolute(mappings.aileron.apply(gp.left_stick_x));
        } else {
            self.aileron.update(dt, keys.roll_right, keys.roll_left);
        }

        // スティック手前(奥へ倒すほど正という Bevy の軸規約で `y < 0`)に引く
        // と機首が上がる、という航空機の慣習をここで固定する。符号のテストは
        // `elevator_sign_matches_pulling_the_stick_back` にある。
        if gamepad.is_some()
            && gamepad::is_axis_touched(gp.left_stick_y, mappings.elevator.deadzone)
        {
            self.elevator
                .set_absolute(mappings.elevator.apply(-gp.left_stick_y));
        } else {
            self.elevator.update(dt, keys.pitch_up, keys.pitch_down);
        }

        if gamepad.is_some() && gamepad::is_axis_touched(gp.right_stick_x, mappings.rudder.deadzone)
        {
            self.rudder
                .set_absolute(mappings.rudder.apply(gp.right_stick_x));
        } else {
            self.rudder.update(dt, keys.yaw_right, keys.yaw_left);
        }

        // スロットルは瞬時値ではなく保持値。トリガーの押し込み量を
        // 「動かす速さ」として使う(実機のスロットルレバーと同じ操作感)。
        let trigger_touched = gamepad.is_some()
            && (gamepad::is_axis_touched(gp.right_trigger, mappings.throttle.deadzone)
                || gamepad::is_axis_touched(gp.left_trigger, mappings.throttle.deadzone));
        if trigger_touched {
            let increase = mappings.throttle.apply(gp.right_trigger).max(0.0);
            let decrease = mappings.throttle.apply(gp.left_trigger).max(0.0);
            self.throttle.update_analog(dt, increase - decrease);
        } else {
            self.throttle
                .update(dt, keys.throttle_up, keys.throttle_down);
        }

        // フラップはどちらのボタンかを押していたらゲームパッドを優先する。
        if gamepad.is_some() && (gp.flaps_extend || gp.flaps_retract) {
            self.flaps.update(dt, gp.flaps_extend, gp.flaps_retract);
        } else {
            self.flaps.update(dt, keys.flaps_extend, keys.flaps_retract);
        }

        // ブレーキは on/off なので、どちらかが踏んでいれば効く。
        self.brakes = f64::from(u8::from(keys.brakes || gp.brakes));
    }

    /// FDM へ渡す形にする。
    #[must_use]
    pub fn to_control_inputs(self) -> ControlInputs {
        ControlInputs::new(
            self.aileron.value(),
            self.effective_elevator(),
            self.rudder.value(),
            self.throttle.value(),
            self.flaps.value(),
        )
        .with_brakes(self.brakes)
    }

    /// 実際に舵面へ行く昇降舵。**操縦桿 + トリム。**
    ///
    /// 範囲外は `ControlInputs` 側で丸められるが、ここでも丸めておく。
    /// **足した結果を見たいのは操縦する側**（HUD と自動トリム）なので、
    /// 丸めた値を返す。
    #[must_use]
    pub fn effective_elevator(self) -> f64 {
        sanitise(self.elevator.value() + self.trim.value())
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
            .init_resource::<GamepadSettings>()
            .add_systems(Update, (read_pilot_keys, cycle_view_mode));
    }
}

/// ゲームパッドのデッドゾーンと感度カーブ。
///
/// [`GamepadAxisMappings`] は Bevy 非依存の純データなので、
/// リソースにするための包みだけをこちら側に置く。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GamepadSettings(pub GamepadAxisMappings);

/// 接続中のゲームパッドから 1 フレームぶんの生入力を読む。
///
/// 複数繋がっている場合は最初の 1 台。**未接続なら `None`** を返し、
/// 呼び出し側はキーボードだけで更新する。
fn read_gamepad(gamepads: &Query<&Gamepad>) -> Option<PilotGamepad> {
    let gamepad = gamepads.iter().next()?;
    let axis = |axis: GamepadAxis| f64::from(gamepad.get(axis).unwrap_or(0.0));
    let trigger = |button: GamepadButton| f64::from(gamepad.get(button).unwrap_or(0.0));

    Some(PilotGamepad {
        left_stick_x: axis(GamepadAxis::LeftStickX),
        left_stick_y: axis(GamepadAxis::LeftStickY),
        right_stick_x: axis(GamepadAxis::RightStickX),
        right_trigger: trigger(GamepadButton::RightTrigger2),
        left_trigger: trigger(GamepadButton::LeftTrigger2),
        // フラップは下げる方向が「出す」。十字キーの下 = 出す、が直感に合う。
        flaps_extend: gamepad.pressed(GamepadButton::DPadDown),
        flaps_retract: gamepad.pressed(GamepadButton::DPadUp),
        brakes: gamepad.pressed(GamepadButton::South),
    })
}

/// キーボードとゲームパッドを読んで [`PilotControls`] を更新する。
///
/// 軸ごとに「ゲームパッドに触れているか」で使い分ける
/// （[`PilotControls::update_with_gamepad`]）。
pub fn read_pilot_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    settings: Res<GamepadSettings>,
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
        // トリム。**手を離しても釣り合う速度を決める。**
        trim_up: keyboard.pressed(KeyCode::BracketRight),
        trim_down: keyboard.pressed(KeyCode::BracketLeft),
        flaps_extend: keyboard.pressed(KeyCode::KeyF),
        flaps_retract: keyboard.pressed(KeyCode::KeyG),
        brakes: keyboard.pressed(KeyCode::Space),
    };
    controls.update_with_gamepad(
        Seconds(f64::from(time.delta_secs())),
        keys,
        read_gamepad(&gamepads),
        &settings.0,
    );
}

/// `C` キーまたはゲームパッドの北ボタン（Y/△）で視点を切り替える。
pub fn cycle_view_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut mode: ResMut<ViewMode>,
) {
    let pad_pressed = gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::North));
    if keyboard.just_pressed(KeyCode::KeyC) || pad_pressed {
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

    // --- ゲームパッドとの合成 ---

    fn quiet_keys() -> PilotKeys {
        PilotKeys::default()
    }

    #[test]
    fn elevator_sign_matches_pulling_the_stick_back() {
        // スティックを手前（Bevy の軸規約で y < 0）に引くと機首上げ。
        // ここを逆にすると、着陸のフレアで機首が下がる。
        let mut controls = PilotControls::default();
        let pad = PilotGamepad {
            left_stick_y: -1.0,
            ..PilotGamepad::default()
        };
        controls.update_with_gamepad(
            FRAME,
            quiet_keys(),
            Some(pad),
            &GamepadAxisMappings::default(),
        );
        assert!(
            controls.to_control_inputs().elevator() > 0.9,
            "pulling the stick back must pitch the nose up"
        );
    }

    #[test]
    fn an_untouched_pad_leaves_the_keyboard_in_charge() {
        // 「ゲームパッドを繋いだらキーボードが死ぬ」を防ぐ。
        let mut with_pad = PilotControls::default();
        let mut without_pad = PilotControls::default();
        let keys = PilotKeys {
            roll_right: true,
            throttle_up: true,
            ..PilotKeys::default()
        };

        for _ in 0..30 {
            with_pad.update_with_gamepad(
                FRAME,
                keys,
                // 繋がってはいるが、どの軸もデッドゾーン内。
                Some(PilotGamepad {
                    left_stick_x: 0.05,
                    ..PilotGamepad::default()
                }),
                &GamepadAxisMappings::default(),
            );
            without_pad.update(FRAME, keys);
        }

        let a = with_pad.to_control_inputs();
        let b = without_pad.to_control_inputs();
        assert!(
            (a.aileron() - b.aileron()).abs() < 1e-12
                && (a.throttle() - b.throttle()).abs() < 1e-12,
            "an idle gamepad changed the keyboard behaviour"
        );
        assert!(a.aileron() > 0.0, "the keyboard input must still act");
    }

    #[test]
    fn touching_one_axis_does_not_capture_the_others() {
        // エルロンをスティックで取り、ラダーはキーボードのまま。
        let mut controls = PilotControls::default();
        let keys = PilotKeys {
            yaw_right: true,
            ..PilotKeys::default()
        };
        let pad = PilotGamepad {
            left_stick_x: 0.8,
            ..PilotGamepad::default()
        };
        for _ in 0..30 {
            controls.update_with_gamepad(FRAME, keys, Some(pad), &GamepadAxisMappings::default());
        }
        let inputs = controls.to_control_inputs();
        assert!(inputs.aileron() > 0.5, "the stick should drive the aileron");
        assert!(
            inputs.rudder() > 0.0,
            "the keyboard rudder must keep working while the stick rolls"
        );
    }

    #[test]
    fn releasing_the_trigger_holds_the_throttle() {
        // 実機のスロットルレバーと同じ。離しても戻らない。
        let mut controls = PilotControls::default();
        let mappings = GamepadAxisMappings::default();

        // 3 秒間 右トリガーを全開（レート 0.25/s なので 0.75 まで開く）。
        for _ in 0..180 {
            controls.update_with_gamepad(
                FRAME,
                quiet_keys(),
                Some(PilotGamepad {
                    right_trigger: 1.0,
                    ..PilotGamepad::default()
                }),
                &mappings,
            );
        }
        let held = controls.to_control_inputs().throttle();
        assert!(
            held > 0.6,
            "three seconds of full trigger should open the throttle"
        );

        // 離して 2 秒。値が保持されること。
        for _ in 0..120 {
            controls.update_with_gamepad(
                FRAME,
                quiet_keys(),
                Some(PilotGamepad::default()),
                &mappings,
            );
        }
        let after = controls.to_control_inputs().throttle();
        assert!(
            (after - held).abs() < 1e-9,
            "the throttle crept from {held} to {after} after releasing the trigger"
        );
    }

    #[test]
    fn brakes_from_either_source_work() {
        let mut controls = PilotControls::default();
        controls.update_with_gamepad(
            FRAME,
            quiet_keys(),
            Some(PilotGamepad {
                brakes: true,
                ..PilotGamepad::default()
            }),
            &GamepadAxisMappings::default(),
        );
        assert!(controls.to_control_inputs().brakes() > 0.5);

        let mut keyboard_only = PilotControls::default();
        keyboard_only.update_with_gamepad(
            FRAME,
            PilotKeys {
                brakes: true,
                ..PilotKeys::default()
            },
            Some(PilotGamepad::default()),
            &GamepadAxisMappings::default(),
        );
        assert!(keyboard_only.to_control_inputs().brakes() > 0.5);
    }

    #[test]
    fn no_gamepad_behaves_exactly_like_the_keyboard_path() {
        let keys = PilotKeys {
            pitch_up: true,
            throttle_up: true,
            ..PilotKeys::default()
        };
        let mut via_gamepad_api = PilotControls::default();
        let mut plain = PilotControls::default();
        for _ in 0..60 {
            via_gamepad_api.update_with_gamepad(FRAME, keys, None, &GamepadAxisMappings::default());
            plain.update(FRAME, keys);
        }
        assert_eq!(
            via_gamepad_api.to_control_inputs(),
            plain.to_control_inputs(),
            "with no gamepad connected the two paths must be identical"
        );
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

    // --- 昇降舵トリム ---

    #[test]
    fn the_trim_does_not_return_to_neutral() {
        // **中立へ戻ったらトリムではない。** 手を離しても効き続けることが要点。
        let mut trim = ElevatorTrim::new(0.0, 0.12);
        trim.update(Seconds(1.0), true, false);
        let set = trim.value();
        assert!(set > 0.0);
        for _ in 0..600 {
            trim.update(Seconds(1.0 / 60.0), false, false);
        }
        assert!(
            (trim.value() - set).abs() < 1e-12,
            "the trim drifted from {set} to {}",
            trim.value()
        );
    }

    #[test]
    fn the_trim_moves_both_ways_and_stays_in_range() {
        let mut trim = ElevatorTrim::new(0.0, 0.5);
        for _ in 0..600 {
            trim.update(Seconds(1.0 / 60.0), true, false);
        }
        assert!((trim.value() - 1.0).abs() < 1e-9, "got {}", trim.value());
        for _ in 0..1_200 {
            trim.update(Seconds(1.0 / 60.0), false, true);
        }
        assert!((trim.value() + 1.0).abs() < 1e-9, "got {}", trim.value());
    }

    #[test]
    fn pressing_both_trim_keys_cancels_out() {
        let mut trim = ElevatorTrim::new(0.2, 0.5);
        trim.update(Seconds(1.0), true, true);
        assert!((trim.value() - 0.2).abs() < 1e-12);
    }

    #[test]
    fn the_default_trim_is_nose_up_so_it_flies_hands_off() {
        // **0 のままだと、手を離した機体は 107 kt へ向けて機首を下げる。**
        // 離陸直後の高度ではそれが接地になる（`flightsim-sim` の
        // `tests/hands_off.rs` で測ってある）。
        let trim = ElevatorTrim::default();
        assert!(
            trim.value() > 0.0,
            "the default trim must hold the nose up, got {}",
            trim.value()
        );
        assert!(
            trim.value() < 0.2,
            "but not so much that it interferes with the take-off roll, got {}",
            trim.value()
        );
    }

    #[test]
    fn the_trim_is_added_to_the_stick() {
        let mut controls = PilotControls::default();
        controls.trim.set(0.1);
        controls.elevator.set_absolute(0.3);
        assert!((controls.effective_elevator() - 0.4).abs() < 1e-12);
        // 舵面へ渡る値にも入っていること。**ここが抜けるとトリックが効かない。**
        assert!((controls.to_control_inputs().elevator() - 0.4).abs() < 1e-12);
    }

    #[test]
    fn the_effective_elevator_never_leaves_the_surface_range() {
        // トリムと操縦桿を足すと 1 を超えうる。**舵は 1 までしか動かない。**
        let mut controls = PilotControls::default();
        controls.trim.set(0.8);
        controls.elevator.set_absolute(1.0);
        assert!((controls.effective_elevator() - 1.0).abs() < 1e-12);
        controls.trim.set(-0.8);
        controls.elevator.set_absolute(-1.0);
        assert!((controls.effective_elevator() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_broken_trim_value_is_refused() {
        let mut trim = ElevatorTrim::default();
        trim.set(f64::NAN);
        assert!(trim.value().is_finite(), "got {}", trim.value());
        trim.set(f64::INFINITY);
        assert!((-1.0..=1.0).contains(&trim.value()), "got {}", trim.value());
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
