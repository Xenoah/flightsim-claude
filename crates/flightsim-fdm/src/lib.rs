//! # flightsim-fdm
//!
//! 6 自由度の飛行力学モデル。
//!
//! ## 設計上の制約
//!
//! - **Bevy に依存しない。** `cargo test -p flightsim-fdm` が GUI もアセットもなしに回る。
//! - **`flightsim-world` に依存しない。** 地形が必要になったら引数で受け取る。
//! - **決定論的である。** 壁時計時間・乱数・グローバル可変状態を参照しない。
//!   同じ初期状態と入力列からは常に同じ軌跡が出る（[ADR-0004]）。
//!
//! 3 番目はリプレイとネットワーク同期の前提条件でもある。要件に含まれているので、
//! 後付けにするより最初から守るほうが安い。
//!
//! ## 使い方
//!
//! ```
//! use flightsim_core::{Attitude, Geodetic, Ned, Seconds};
//! use flightsim_fdm::{AircraftConfig, ControlInputs, Environment, FlightDynamics, RigidBodyState};
//!
//! let state = RigidBodyState::from_geodetic(
//!     Geodetic::from_degrees(35.55, 139.78, 1_000.0),
//!     Attitude::from_degrees(0.0, 2.0, 90.0),
//!     Ned::new(0.0, 55.0, 0.0),
//! );
//!
//! let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), state);
//! let controls = ControlInputs::neutral().with_throttle(0.7);
//! let environment = Environment::still_air();
//!
//! for _ in 0..120 {
//!     fdm.step(Seconds(1.0 / 120.0), controls, &environment);
//! }
//!
//! assert!(fdm.state().is_finite());
//! ```
//!
//! [ADR-0004]: https://github.com/../docs/adr/0004-simulation-loop.md

pub mod aero;
pub mod aircraft;
pub mod atmosphere;
pub mod controls;
pub mod gravity;
pub mod state;

pub use aero::{AeroAngles, AeroCoefficientSet, aero_angles};
pub use aircraft::{AeroCoefficients, AircraftConfig, EngineConfig, Geometry, MassProperties};
pub use atmosphere::{Atmosphere, AtmosphereSample};
pub use controls::ControlInputs;
pub use state::{RigidBodyState, StateDerivative};

use flightsim_core::{LocalFrame, Seconds};
use glam::DVec3;

/// 推奨する物理ステップ幅。60Hz 描画で 2 ステップ、144Hz で 1〜2 ステップ。
pub const RECOMMENDED_FIXED_DT: Seconds = Seconds(1.0 / 120.0);

/// 1 サブステップあたりに許す回転量 `rad`。約 2.9°。
///
/// これを超えると quaternion の線形積分の誤差が無視できなくなる。
const MAX_ROTATION_PER_SUBSTEP: f64 = 0.05;

/// サブステップ数の上限。
///
/// 上限が無いと、発散しかけた機体（角速度が極端に大きい状態）が
/// 大量のサブステップを誘発し、フレーム時間を破壊する。
const MAX_SUBSTEPS: u32 = 8;

/// 機体を取り巻く環境。
///
/// 天候システムはこの構造体を組み立てて FDM に渡す。
/// **見た目の情報（雲の形など）をここに混ぜないこと。** FDM が必要とするのは
/// 風と大気状態だけであり、それ以外は責務の汚染になる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Environment {
    pub atmosphere: Atmosphere,
    /// ECEF 系での風速 `m/s`。地面に対する空気の動き。
    pub wind_ecef: DVec3,
}

impl Default for Environment {
    fn default() -> Self {
        Self::still_air()
    }
}

impl Environment {
    /// 標準大気・無風。
    #[must_use]
    pub const fn still_air() -> Self {
        Self {
            atmosphere: Atmosphere::standard(),
            wind_ecef: DVec3::ZERO,
        }
    }

    /// ローカル NED 系で風を指定する。
    ///
    /// 気象データは「北風 10 m/s」のようにローカル基準で来るため、
    /// ECEF への変換をここで引き受ける。
    #[must_use]
    pub fn with_wind_ned(
        atmosphere: Atmosphere,
        position: flightsim_core::Geodetic,
        wind_ned: flightsim_core::Ned,
    ) -> Self {
        Self {
            atmosphere,
            wind_ecef: LocalFrame::new(position).ned_to_ecef_vector(wind_ned),
        }
    }
}

/// 飛行力学モデル本体。
#[derive(Debug, Clone)]
pub struct FlightDynamics {
    config: AircraftConfig,
    state: RigidBodyState,
}

impl FlightDynamics {
    #[must_use]
    pub fn new(config: AircraftConfig, state: RigidBodyState) -> Self {
        Self { config, state }
    }

    #[must_use]
    pub const fn state(&self) -> &RigidBodyState {
        &self.state
    }

    #[must_use]
    pub const fn config(&self) -> &AircraftConfig {
        &self.config
    }

    /// 状態を直接置き換える。シナリオの読み込みやリプレイの巻き戻しに使う。
    pub fn set_state(&mut self, state: RigidBodyState) {
        self.state = state;
    }

    /// 固定幅 `dt` だけ時間を進める。
    ///
    /// **`dt` は毎回同じ値であることを前提としている**（[`RECOMMENDED_FIXED_DT`]）。
    /// 描画フレーム時間を直接渡さないこと。アキュムレータには
    /// `flightsim_core::FixedStep` を使う。
    ///
    /// 内部では状態に応じてサブステップに分割する。外部契約（`dt` の意味）は変わらない。
    pub fn step(&mut self, dt: Seconds, controls: ControlInputs, environment: &Environment) {
        let substeps = self.required_substeps(dt);
        let h = dt.get() / f64::from(substeps);

        for _ in 0..substeps {
            self.state = self.integrate_rk4(h, controls, environment);
        }
    }

    /// 現在の状態に必要なサブステップ数。
    ///
    /// 角速度が大きいほど細かく刻む。接地反力を実装したら、その剛性も判定に加えること。
    fn required_substeps(&self, dt: Seconds) -> u32 {
        let rotation = self.state.angular_velocity.length() * dt.get();

        if !rotation.is_finite() || rotation <= MAX_ROTATION_PER_SUBSTEP {
            return 1;
        }

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamp により 1..=MAX_SUBSTEPS の有限値であることが保証されている"
        )]
        let substeps = (rotation / MAX_ROTATION_PER_SUBSTEP)
            .ceil()
            .clamp(1.0, f64::from(MAX_SUBSTEPS)) as u32;

        substeps
    }

    /// 古典的 4 次 Runge-Kutta による 1 ステップ。
    ///
    /// オイラー法（誤差 O(dt)）に対し RK4 は O(dt⁴)。導関数評価が 4 回になるが、
    /// 剛体 1 機ぶんの計算量は些少であり、失速・接地といった非線形領域での
    /// 安定性の見返りが遥かに大きい（ADR-0004）。
    fn integrate_rk4(
        &self,
        h: f64,
        controls: ControlInputs,
        environment: &Environment,
    ) -> RigidBodyState {
        let k1 = self.derivative(&self.state, controls, environment);

        let s2 = state::offset(&self.state, &k1, h * 0.5);
        let k2 = self.derivative(&s2, controls, environment);

        let s3 = state::offset(&self.state, &k2, h * 0.5);
        let k3 = self.derivative(&s3, controls, environment);

        let s4 = state::offset(&self.state, &k3, h);
        let k4 = self.derivative(&s4, controls, environment);

        let weighted = (k1 + k2 * 2.0 + k3 * 2.0 + k4) * (1.0 / 6.0);
        state::offset(&self.state, &weighted, h)
    }

    /// 状態の時間微分。**ここが物理の全て。**
    fn derivative(
        &self,
        state: &RigidBodyState,
        controls: ControlInputs,
        environment: &Environment,
    ) -> StateDerivative {
        let position = state.position.to_geodetic();
        let frame = LocalFrame::new(position);
        let air = environment.atmosphere.sample(position.altitude);

        // --- 対気速度 ---
        // 風は「空気の動き」なので、機体から見た相対風は速度から風を引いたもの。
        let relative_velocity_ecef = state.velocity - environment.wind_ecef;
        let body_airspeed = state.orientation.inverse() * relative_velocity_ecef;
        let angles = aero::aero_angles(body_airspeed);

        // --- 空気力 ---
        let (aero_force, aero_moment) = aero::body_force_and_moment(
            &self.config.aero,
            &self.config.geometry,
            angles,
            state.angular_velocity,
            controls,
            air.density,
        );

        // --- 推力 ---
        // 機体軸 X（機首方向）に働くものとして扱う。
        // 推力線のオフセットによるピッチモーメントは未実装（TODO: エンジン計器の実装時）。
        let thrust = self.config.engine.thrust(
            controls.throttle(),
            angles.true_airspeed.get(),
            air.density_ratio(),
        );
        let total_force_body = aero_force + DVec3::X * thrust.get();

        // --- 並進 ---
        let mass = self.config.mass_properties.mass().get();
        let acceleration = state.orientation * (total_force_body / mass)
            + gravity::acceleration_ecef(position, &frame);

        // --- 回転 ---
        // オイラーの運動方程式。ω × (Iω) はジャイロ効果の項で、
        // これを落とすと機体が回っている最中の挙動が明確におかしくなる。
        let inertia = self.config.mass_properties.inertia();
        let angular_momentum = inertia * state.angular_velocity;
        let angular_acceleration = self.config.mass_properties.inverse_inertia()
            * (aero_moment - state.angular_velocity.cross(angular_momentum));

        StateDerivative {
            velocity: state.velocity,
            acceleration,
            orientation_rate: state::orientation_rate(state.orientation, state.angular_velocity),
            angular_acceleration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Attitude, Geodetic, Ned};

    fn cruising() -> FlightDynamics {
        FlightDynamics::new(
            AircraftConfig::light_single(),
            RigidBodyState::from_geodetic(
                Geodetic::from_degrees(35.0, 139.0, 2_000.0),
                Attitude::default(),
                Ned::new(55.0, 0.0, 0.0),
            ),
        )
    }

    #[test]
    fn a_single_step_leaves_the_state_finite() {
        let mut fdm = cruising();
        fdm.step(
            RECOMMENDED_FIXED_DT,
            ControlInputs::neutral().with_throttle(0.6),
            &Environment::still_air(),
        );
        assert!(fdm.state().is_finite());
    }

    #[test]
    fn substep_count_grows_with_angular_velocity_and_stays_bounded() {
        let mut fdm = cruising();
        assert_eq!(fdm.required_substeps(RECOMMENDED_FIXED_DT), 1);

        // 1 ステップで 0.05 rad を超える角速度。
        let mut state = *fdm.state();
        state.angular_velocity = DVec3::new(12.0, 0.0, 0.0);
        fdm.set_state(state);
        assert!(fdm.required_substeps(RECOMMENDED_FIXED_DT) > 1);

        // 発散しかけた機体でも上限を超えない。無制限だとフレーム時間が破壊される。
        let mut state = *fdm.state();
        state.angular_velocity = DVec3::splat(1.0e6);
        fdm.set_state(state);
        assert_eq!(fdm.required_substeps(RECOMMENDED_FIXED_DT), MAX_SUBSTEPS);
    }

    #[test]
    fn substep_count_is_one_for_a_corrupted_angular_velocity() {
        // NaN が入った場合に無限ループやゼロ除算を起こさないこと。
        let mut fdm = cruising();
        let mut state = *fdm.state();
        state.angular_velocity = DVec3::new(f64::NAN, 0.0, 0.0);
        fdm.set_state(state);
        assert_eq!(fdm.required_substeps(RECOMMENDED_FIXED_DT), 1);
    }

    #[test]
    fn wind_changes_the_resulting_state() {
        // 風は対気速度に効くため、同じ操縦入力でも軌跡が変わる。
        let position = Geodetic::from_degrees(35.0, 139.0, 2_000.0);
        let headwind = Environment::with_wind_ned(
            Atmosphere::standard(),
            position,
            Ned::new(-20.0, 0.0, 0.0), // 南からの風 = 北へ飛ぶ機体には向かい風
        );

        let mut with_wind = cruising();
        let mut without_wind = cruising();

        let controls = ControlInputs::neutral().with_throttle(0.6);
        with_wind.step(RECOMMENDED_FIXED_DT, controls, &headwind);
        without_wind.step(RECOMMENDED_FIXED_DT, controls, &Environment::still_air());

        assert_ne!(with_wind.state().velocity, without_wind.state().velocity);
        assert!(with_wind.state().is_finite());
    }

    #[test]
    fn gravity_alone_produces_free_fall_acceleration() {
        // 静止状態では動圧がゼロなので空気力が働かず、自由落下する。
        // 重力と積分器の結線の検査。
        let position = Geodetic::from_degrees(0.0, 0.0, 10_000.0);
        let state = RigidBodyState::from_geodetic(position, Attitude::default(), Ned::default());
        let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), state);

        let dt = 1.0 / 120.0;
        fdm.step(
            Seconds(dt),
            ControlInputs::neutral(),
            &Environment::still_air(),
        );

        let descent_rate = -fdm.state().vertical_speed().get();
        let expected = gravity::magnitude(position) * dt;
        assert!(
            (descent_rate - expected).abs() < 1e-3,
            "descent rate after one step was {descent_rate} m/s, expected {expected}"
        );
    }
}
