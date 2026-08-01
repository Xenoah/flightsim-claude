//! 剛体の状態と、その時間微分。
//!
//! # 状態を ECEF で保持する理由
//!
//! 姿勢角や速度成分をローカル NED で保持すると、機体が移動するたびに基準面が回転し、
//! 「同じ姿勢のまま北へ飛ぶと勝手にピッチが変わる」という補正が必要になる。
//! ECEF は地球全体で一意なので、積分中に基準系が動かない。
//!
//! 表示や計器で必要になるローカル量（ピッチ・方位・昇降率）は、
//! そのつど [`RigidBodyState::attitude`] などで導出する。

use flightsim_core::{Attitude, Ecef, Geodetic, LocalFrame, Meters, MetersPerSecond, Ned};
use glam::{DQuat, DVec3, DVec4};

/// 6 自由度剛体の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidBodyState {
    /// 世界座標での位置。
    pub position: Ecef,
    /// 世界座標での速度 `m/s`。
    pub velocity: DVec3,
    /// 機体軸 → ECEF の回転。常に正規化されていること。
    pub orientation: DQuat,
    /// 機体軸での角速度 `(p, q, r)` `rad/s`。
    pub angular_velocity: DVec3,
}

impl RigidBodyState {
    /// 測地座標・NED 基準の姿勢・NED 速度から構築する。
    ///
    /// シナリオの初期化とテストの起点はこれを使う。ECEF を直接組み立てないこと。
    #[must_use]
    pub fn from_geodetic(position: Geodetic, attitude: Attitude, velocity_ned: Ned) -> Self {
        let frame = LocalFrame::new(position);
        Self {
            position: position.to_ecef(),
            velocity: frame.ned_to_ecef_vector(velocity_ned),
            orientation: (frame.ned_to_ecef_rotation() * attitude.to_quaternion()).normalize(),
            angular_velocity: DVec3::ZERO,
        }
    }

    /// 測地座標。
    #[must_use]
    pub fn geodetic(&self) -> Geodetic {
        self.position.to_geodetic()
    }

    /// 楕円体高。
    #[must_use]
    pub fn altitude(&self) -> Meters {
        self.geodetic().altitude
    }

    /// 現在位置におけるローカル NED 系。
    #[must_use]
    pub fn local_frame(&self) -> LocalFrame {
        LocalFrame::new(self.geodetic())
    }

    /// NED 基準の姿勢角（ロール・ピッチ・方位）。
    #[must_use]
    pub fn attitude(&self) -> Attitude {
        let frame = self.local_frame();
        Attitude::from_quaternion(frame.ned_to_ecef_rotation().inverse() * self.orientation)
    }

    /// NED 基準の速度。
    #[must_use]
    pub fn velocity_ned(&self) -> Ned {
        self.local_frame().ecef_to_ned_vector(self.velocity)
    }

    /// 機体軸での速度 `(u, v, w)`。対気速度の計算に使う（風を差し引く前）。
    #[must_use]
    pub fn body_velocity(&self) -> DVec3 {
        self.orientation.inverse() * self.velocity
    }

    /// 対地速度の水平成分。
    #[must_use]
    pub fn ground_speed(&self) -> MetersPerSecond {
        MetersPerSecond(self.velocity_ned().horizontal_magnitude())
    }

    /// 昇降率。**上昇が正**（NED の Down とは符号が逆）。
    #[must_use]
    pub fn vertical_speed(&self) -> MetersPerSecond {
        MetersPerSecond(self.velocity_ned().up())
    }

    /// 全ての状態量が有限か。
    ///
    /// 数値シミュレーションでは NaN が全状態に伝播するため、
    /// 積分ループの健全性検査としてこれを使う。
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.position.is_finite()
            && self.velocity.is_finite()
            && self.orientation.is_finite()
            && self.angular_velocity.is_finite()
    }
}

/// [`RigidBodyState`] の時間微分。
///
/// 姿勢の微分は quaternion の 4 成分をそのまま線形量として扱う。
/// これは厳密には多様体上の演算ではないが、各ステップで正規化する限り
/// 十分な精度が得られる（RK4 の標準的な扱い）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateDerivative {
    pub velocity: DVec3,
    pub acceleration: DVec3,
    /// quaternion の成分ごとの変化率 `(x, y, z, w)`。
    pub orientation_rate: DVec4,
    pub angular_acceleration: DVec3,
}

impl StateDerivative {
    pub const ZERO: Self = Self {
        velocity: DVec3::ZERO,
        acceleration: DVec3::ZERO,
        orientation_rate: DVec4::ZERO,
        angular_acceleration: DVec3::ZERO,
    };
}

impl core::ops::Add for StateDerivative {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            velocity: self.velocity + rhs.velocity,
            acceleration: self.acceleration + rhs.acceleration,
            orientation_rate: self.orientation_rate + rhs.orientation_rate,
            angular_acceleration: self.angular_acceleration + rhs.angular_acceleration,
        }
    }
}

impl core::ops::Mul<f64> for StateDerivative {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self {
            velocity: self.velocity * rhs,
            acceleration: self.acceleration * rhs,
            orientation_rate: self.orientation_rate * rhs,
            angular_acceleration: self.angular_acceleration * rhs,
        }
    }
}

/// 機体軸の角速度から quaternion の変化率を求める。
///
/// `q` が機体軸 → ECEF の回転で、`ω` が機体軸表現のとき `q̇ = ½ · q ⊗ (0, ω)`。
///
/// Hamilton 積を手で展開しているのは、glam の quaternion 積が
/// 正規化された quaternion を前提とする（デバッグ時に検証が入る）ためと、
/// 純虚 quaternion を作る手間を省くため。
#[must_use]
pub fn orientation_rate(orientation: DQuat, angular_velocity_body: DVec3) -> DVec4 {
    let (w, x, y, z) = (orientation.w, orientation.x, orientation.y, orientation.z);
    let (p, q, r) = (
        angular_velocity_body.x,
        angular_velocity_body.y,
        angular_velocity_body.z,
    );

    DVec4::new(
        0.5 * (w * p + y * r - z * q),
        0.5 * (w * q - x * r + z * p),
        0.5 * (w * r + x * q - y * p),
        0.5 * (-x * p - y * q - z * r),
    )
}

/// 状態を微分方向へ `h` だけ進めた新しい状態を返す。
///
/// quaternion は毎回正規化する。これを怠るとノルムが 1 から漂い、姿勢がじわじわ歪む。
#[must_use]
pub fn offset(state: &RigidBodyState, derivative: &StateDerivative, h: f64) -> RigidBodyState {
    let orientation = DVec4::new(
        state.orientation.x,
        state.orientation.y,
        state.orientation.z,
        state.orientation.w,
    ) + derivative.orientation_rate * h;

    RigidBodyState {
        position: Ecef(state.position.0 + derivative.velocity * h),
        velocity: state.velocity + derivative.acceleration * h,
        orientation: DQuat::from_xyzw(orientation.x, orientation.y, orientation.z, orientation.w)
            .normalize(),
        angular_velocity: state.angular_velocity + derivative.angular_acceleration * h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Degrees, Radians};

    fn level_north(altitude: f64, speed: f64) -> RigidBodyState {
        RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.0, 139.0, altitude),
            Attitude::default(),
            Ned::new(speed, 0.0, 0.0),
        )
    }

    #[test]
    fn construction_round_trips_through_local_quantities() {
        let attitude = Attitude::from_degrees(10.0, 5.0, 120.0);
        let velocity = Ned::new(60.0, 20.0, -3.0);
        let position = Geodetic::from_degrees(-12.5, -75.25, 3_400.0);

        let state = RigidBodyState::from_geodetic(position, attitude, velocity);

        let recovered_position = state.geodetic();
        assert!((recovered_position.latitude_degrees() - (-12.5)).abs() < 1e-9);
        assert!((recovered_position.longitude_degrees() - (-75.25)).abs() < 1e-9);
        assert!((recovered_position.altitude.get() - 3_400.0).abs() < 1e-4);

        let recovered_attitude = state.attitude();
        assert!((recovered_attitude.roll.get() - attitude.roll.get()).abs() < 1e-9);
        assert!((recovered_attitude.pitch.get() - attitude.pitch.get()).abs() < 1e-9);
        assert!(
            attitude
                .yaw
                .shortest_difference_to(recovered_attitude.yaw)
                .get()
                .abs()
                < 1e-9
        );

        assert!(state.velocity_ned().0.distance(velocity.0) < 1e-9);
    }

    #[test]
    fn body_velocity_equals_airspeed_axis_when_level_and_pointing_north() {
        let state = level_north(1_000.0, 55.0);
        let body = state.body_velocity();

        // 機首方向（機体 X）に全速度が乗り、横・上下成分はゼロ。
        assert!((body.x - 55.0).abs() < 1e-9, "forward speed was {}", body.x);
        assert!(body.y.abs() < 1e-9);
        assert!(body.z.abs() < 1e-9);
    }

    #[test]
    fn vertical_speed_is_positive_when_climbing() {
        // NED の Down が負 = 上昇。表示上は正の昇降率になること。
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, 500.0),
            Attitude::default(),
            Ned::new(50.0, 0.0, -5.0),
        );
        assert!((state.vertical_speed().get() - 5.0).abs() < 1e-9);
        assert!((state.ground_speed().get() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn orientation_rate_is_zero_when_not_rotating() {
        let state = level_north(1_000.0, 50.0);
        let rate = orientation_rate(state.orientation, DVec3::ZERO);
        assert!(rate.length() < 1e-15);
    }

    #[test]
    fn orientation_rate_integrates_into_the_expected_rotation() {
        // 機体 X 軸まわりに一定角速度で回すと、ロール角がその通り増えること。
        let mut state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, 3_000.0),
            Attitude::default(),
            Ned::new(50.0, 0.0, 0.0),
        );
        let roll_rate = Degrees(30.0).to_radians().get(); // 30 °/s
        state.angular_velocity = DVec3::new(roll_rate, 0.0, 0.0);

        let dt = 1.0 / 1_000.0;
        for _ in 0..1_000 {
            let derivative = StateDerivative {
                orientation_rate: orientation_rate(state.orientation, state.angular_velocity),
                ..StateDerivative::ZERO
            };
            state = offset(&state, &derivative, dt);
        }

        // 1 秒で 30 度ロールしている。
        let roll = state.attitude().roll.to_degrees().get();
        assert!(
            (roll - 30.0).abs() < 0.05,
            "roll after one second was {roll}°"
        );
    }

    #[test]
    fn offset_keeps_the_quaternion_normalised() {
        let mut state = level_north(2_000.0, 60.0);
        state.angular_velocity = DVec3::new(0.5, -0.3, 0.2);

        for _ in 0..100_000 {
            let derivative = StateDerivative {
                orientation_rate: orientation_rate(state.orientation, state.angular_velocity),
                ..StateDerivative::ZERO
            };
            state = offset(&state, &derivative, 1.0 / 120.0);
        }

        // ADR-0004 の不変条件: ノルムの誤差が 1e-9 を超えない。
        let norm = state.orientation.length();
        assert!(
            (norm - 1.0).abs() < 1e-9,
            "quaternion norm drifted to {norm} after 100k steps"
        );
        assert!(state.is_finite());
    }

    #[test]
    fn derivative_arithmetic_is_component_wise() {
        let a = StateDerivative {
            velocity: DVec3::new(1.0, 2.0, 3.0),
            acceleration: DVec3::splat(1.0),
            orientation_rate: DVec4::splat(0.5),
            angular_acceleration: DVec3::splat(2.0),
        };
        let sum = a + a;
        let scaled = a * 2.0;

        assert_eq!(sum, scaled);
        assert!((sum.velocity - DVec3::new(2.0, 4.0, 6.0)).length() < 1e-15);
    }

    #[test]
    fn is_finite_detects_corrupted_state() {
        let mut state = level_north(1_000.0, 50.0);
        assert!(state.is_finite());

        state.velocity = DVec3::new(f64::NAN, 0.0, 0.0);
        assert!(!state.is_finite(), "a NaN velocity must be detected");
    }

    #[test]
    fn attitude_is_relative_to_the_local_horizon_not_the_earth_axis() {
        // 同じ「水平・北向き」姿勢を地球上の別々の地点で作ると、
        // ECEF での quaternion は全く違うが、ローカル姿勢角は等しくなる。
        // これが ECEF で状態を持つ設計の要点。
        for (latitude, longitude) in [(0.0, 0.0), (45.0, 90.0), (-60.0, -170.0), (80.0, 30.0)] {
            let state = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(latitude, longitude, 5_000.0),
                Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
                Ned::new(70.0, 0.0, 0.0),
            );
            let attitude = state.attitude();

            assert!(
                attitude.roll.get().abs() < 1e-9 && attitude.pitch.get().abs() < 1e-9,
                "level attitude was not preserved at ({latitude}, {longitude}): {attitude:?}"
            );
            assert!(
                attitude.yaw.get().abs() < 1e-9,
                "heading drifted at ({latitude}, {longitude}): {}",
                attitude.yaw.to_degrees()
            );
        }
    }
}
