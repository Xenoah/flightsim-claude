//! ローカル接平面（NED）と機体姿勢。
//!
//! # なぜ NED を使うのか
//!
//! ECEF は世界座標としては正しいが、「機首が 5 度上を向いている」「北北東に向かっている」
//! といった量を直接表現できない。姿勢・風・航法計器はローカル水平面基準でないと意味を持たない。
//!
//! 航空分野の標準である **NED（North-East-Down）** を採用する。ENU（East-North-Up）ではない。
//! NED では正のピッチが機首上げ、Z 軸が下向きになり、空力の教科書および実機の計器と一致する。
//!
//! # 機体座標系（Body）
//!
//! | 軸 | 向き |
//! |---|---|
//! | X | 機首方向（前） |
//! | Y | 右翼方向（右） |
//! | Z | 機体下方（下） |
//!
//! 姿勢は NED に対する 3-2-1 オイラー角（ヨー → ピッチ → ロールの順に内因性回転）で表す。

use crate::geodetic::{Ecef, Geodetic};
use crate::units::Radians;
use glam::{DMat3, DQuat, DVec3};

/// ローカル接平面上のベクトル `m` または `m/s`。成分は `(north, east, down)`。
///
/// 位置として使う場合は [`LocalFrame`] の原点からの相対位置を意味する。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(transparent)]
pub struct Ned(pub DVec3);

impl Ned {
    #[must_use]
    pub const fn new(north: f64, east: f64, down: f64) -> Self {
        Self(DVec3::new(north, east, down))
    }

    #[must_use]
    pub const fn north(self) -> f64 {
        self.0.x
    }

    #[must_use]
    pub const fn east(self) -> f64 {
        self.0.y
    }

    #[must_use]
    pub const fn down(self) -> f64 {
        self.0.z
    }

    /// 上向き成分。`down` の符号反転。昇降率の表示で使う。
    #[must_use]
    pub fn up(self) -> f64 {
        -self.0.z
    }

    /// 水平面内の大きさ。対地速度の水平成分など。
    #[must_use]
    pub fn horizontal_magnitude(self) -> f64 {
        self.0.truncate().length()
    }

    /// 水平成分の方位。`[0, 2π)` に正規化済み（北が 0、東が π/2）。
    ///
    /// 水平成分がゼロの場合は 0 を返す。
    #[must_use]
    pub fn bearing(self) -> Radians {
        if self.horizontal_magnitude() < f64::EPSILON {
            return Radians::ZERO;
        }
        Radians(self.0.y.atan2(self.0.x)).wrap_positive()
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }
}

/// 測地座標上の一点に固定されたローカル NED 座標系。
///
/// 三角関数の評価を構築時の 1 回にまとめるため、基底行列をキャッシュしている。
/// 毎フレーム同じ地点で使い回すこと。
#[derive(Debug, Clone, Copy)]
pub struct LocalFrame {
    origin: Geodetic,
    origin_ecef: Ecef,
    /// 列が順に ECEF で表した north / east / down 単位ベクトル。
    /// 直交行列なので逆変換は転置で済む。
    ned_to_ecef: DMat3,
}

impl LocalFrame {
    /// 指定した測地座標を原点とするローカル系を作る。
    #[must_use]
    pub fn new(origin: Geodetic) -> Self {
        let (sin_lat, cos_lat) = origin.latitude.get().sin_cos();
        let (sin_lon, cos_lon) = origin.longitude.get().sin_cos();

        // 楕円体法線（幾何学的な「上」）。
        let up = DVec3::new(cos_lat * cos_lon, cos_lat * sin_lon, sin_lat);
        let east = DVec3::new(-sin_lon, cos_lon, 0.0);
        let north = DVec3::new(-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat);

        Self {
            origin,
            origin_ecef: origin.to_ecef(),
            ned_to_ecef: DMat3::from_cols(north, east, -up),
        }
    }

    #[must_use]
    pub const fn origin(&self) -> Geodetic {
        self.origin
    }

    #[must_use]
    pub const fn origin_ecef(&self) -> Ecef {
        self.origin_ecef
    }

    /// NED → ECEF の回転を quaternion で得る。姿勢の合成に使う。
    #[must_use]
    pub fn ned_to_ecef_rotation(&self) -> DQuat {
        DQuat::from_mat3(&self.ned_to_ecef)
    }

    /// ローカル系での「上」方向（楕円体法線）を ECEF で返す。
    #[must_use]
    pub fn up_ecef(&self) -> DVec3 {
        -self.ned_to_ecef.z_axis
    }

    // --- ベクトル（速度・力・風）の変換。平行移動を伴わない。 ---

    #[must_use]
    pub fn ned_to_ecef_vector(&self, v: Ned) -> DVec3 {
        self.ned_to_ecef * v.0
    }

    #[must_use]
    pub fn ecef_to_ned_vector(&self, v: DVec3) -> Ned {
        // 直交行列なので転置が逆行列。
        Ned(self.ned_to_ecef.transpose() * v)
    }

    // --- 位置の変換。原点からの平行移動を含む。 ---

    #[must_use]
    pub fn ned_to_ecef_position(&self, p: Ned) -> Ecef {
        Ecef(self.origin_ecef.0 + self.ned_to_ecef * p.0)
    }

    #[must_use]
    pub fn ecef_to_ned_position(&self, p: Ecef) -> Ned {
        Ned(self.ned_to_ecef.transpose() * (p.0 - self.origin_ecef.0))
    }
}

/// NED に対する機体姿勢（3-2-1 オイラー角）。
///
/// 回転の適用順はヨー → ピッチ → ロール（内因性）。すなわち機体→NED の回転は
/// `Rz(yaw) · Ry(pitch) · Rx(roll)`。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Attitude {
    /// バンク角。右翼下げが正。`[-π, π]`
    pub roll: Radians,
    /// 縦傾斜。機首上げが正。`[-π/2, π/2]`
    pub pitch: Radians,
    /// 方位（真方位）。北から東回りが正。`[0, 2π)`
    pub yaw: Radians,
}

impl Attitude {
    #[must_use]
    pub const fn new(roll: Radians, pitch: Radians, yaw: Radians) -> Self {
        Self { roll, pitch, yaw }
    }

    #[must_use]
    pub fn from_degrees(roll_deg: f64, pitch_deg: f64, yaw_deg: f64) -> Self {
        use crate::units::Degrees;
        Self {
            roll: Degrees(roll_deg).to_radians(),
            pitch: Degrees(pitch_deg).to_radians(),
            yaw: Degrees(yaw_deg).to_radians(),
        }
    }

    /// 機体 → NED の回転 quaternion。
    ///
    /// `glam` の `from_euler` は版によって内因性／外因性の解釈が異なるため、
    /// 曖昧さを避けて明示的に合成している。
    #[must_use]
    pub fn to_quaternion(self) -> DQuat {
        DQuat::from_rotation_z(self.yaw.get())
            * DQuat::from_rotation_y(self.pitch.get())
            * DQuat::from_rotation_x(self.roll.get())
    }

    /// 機体 → NED の回転 quaternion からオイラー角を復元する。
    ///
    /// # ジンバルロック
    ///
    /// ピッチが ±90°（垂直上昇・垂直降下）に達するとロールとヨーが縮退し、
    /// 個別には決定できなくなる。**フライトシミュレータではこの姿勢に実際に到達する**ため、
    /// 無視できない。この実装ではロールを 0 に固定し、合成回転をヨーに寄せる。
    #[must_use]
    pub fn from_quaternion(q: DQuat) -> Self {
        let q = q.normalize();
        let (w, x, y, z) = (q.w, q.x, q.y, q.z);

        // 回転行列の R20 成分に相当する。sin(pitch) そのもの。
        let sin_pitch = 2.0 * (w * y - z * x);

        // 縮退の判定閾値。1 - 1e-12 は約 0.00008 度に相当し、
        // ここまで近づくと atan2 の引数がどちらも 0 に潰れて結果が不安定になる。
        const GIMBAL_LOCK_THRESHOLD: f64 = 1.0 - 1.0e-12;

        if sin_pitch.abs() >= GIMBAL_LOCK_THRESHOLD {
            let pitch = core::f64::consts::FRAC_PI_2.copysign(sin_pitch);
            // pitch = +π/2 のとき yaw = -2·atan2(x, w)、-π/2 のとき yaw = +2·atan2(x, w)。
            let yaw = -2.0 * x.atan2(w) * sin_pitch.signum();
            return Self {
                roll: Radians::ZERO,
                pitch: Radians(pitch),
                yaw: Radians(yaw).wrap_positive(),
            };
        }

        Self {
            roll: Radians((2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y))),
            pitch: Radians(sin_pitch.clamp(-1.0, 1.0).asin()),
            yaw: Radians((2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z)))
                .wrap_positive(),
        }
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.roll.is_finite() && self.pitch.is_finite() && self.yaw.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Degrees;
    use core::f64::consts::{FRAC_PI_2, PI};

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

    // --- ローカル系の基底 ---

    #[test]
    fn basis_is_orthonormal_everywhere() {
        for lat in (-90..=90).step_by(15) {
            for lon in (-180..=180).step_by(30) {
                let frame =
                    LocalFrame::new(Geodetic::from_degrees(f64::from(lat), f64::from(lon), 0.0));
                let m = frame.ned_to_ecef;

                for axis in [m.x_axis, m.y_axis, m.z_axis] {
                    assert_close!(axis.length(), 1.0, 1e-12);
                }
                assert_close!(m.x_axis.dot(m.y_axis), 0.0, 1e-12);
                assert_close!(m.y_axis.dot(m.z_axis), 0.0, 1e-12);
                assert_close!(m.z_axis.dot(m.x_axis), 0.0, 1e-12);

                // 右手系であること（行列式が +1）。-1 だと鏡像になり、
                // 方位が左右反転するという発見しにくい欠陥になる。
                assert_close!(m.determinant(), 1.0, 1e-12);
            }
        }
    }

    #[test]
    fn basis_points_in_the_expected_directions_at_the_equator() {
        // 緯度 0・経度 0 では、ECEF の X 軸が「上」、Z 軸が「北」、Y 軸が「東」。
        let frame = LocalFrame::new(Geodetic::from_degrees(0.0, 0.0, 0.0));

        let north = frame.ned_to_ecef_vector(Ned::new(1.0, 0.0, 0.0));
        assert_close!(north.z, 1.0, 1e-12);

        let east = frame.ned_to_ecef_vector(Ned::new(0.0, 1.0, 0.0));
        assert_close!(east.y, 1.0, 1e-12);

        let down = frame.ned_to_ecef_vector(Ned::new(0.0, 0.0, 1.0));
        assert_close!(down.x, -1.0, 1e-12);
    }

    #[test]
    fn up_is_the_ellipsoid_normal() {
        // 高度だけ上げた点は、ローカル系の「上」方向にちょうどその分だけ動く。
        for lat in [-80.0, -45.0, 0.0, 45.0, 80.0] {
            let frame = LocalFrame::new(Geodetic::from_degrees(lat, 33.0, 0.0));
            let higher = Geodetic::from_degrees(lat, 33.0, 250.0).to_ecef();
            let delta = higher.0 - frame.origin_ecef().0;

            assert_close!(delta.length(), 250.0, 1e-6);
            assert_close!(delta.normalize().dot(frame.up_ecef()), 1.0, 1e-12);
        }
    }

    #[test]
    fn position_round_trip_through_ned() {
        let frame = LocalFrame::new(Geodetic::from_degrees(35.55, 139.78, 40.0));
        for offset in [
            Ned::new(0.0, 0.0, 0.0),
            Ned::new(1000.0, -2500.0, -300.0),
            Ned::new(-50_000.0, 80_000.0, 12_000.0),
        ] {
            let round_tripped = frame.ecef_to_ned_position(frame.ned_to_ecef_position(offset));
            assert_close!(round_tripped.0.distance(offset.0), 0.0, 1e-6);
        }
    }

    #[test]
    fn vector_round_trip_preserves_magnitude() {
        let frame = LocalFrame::new(Geodetic::from_degrees(-33.87, 151.21, 0.0));
        let wind = Ned::new(12.0, -5.0, 0.5);
        let ecef = frame.ned_to_ecef_vector(wind);

        assert_close!(ecef.length(), wind.0.length(), 1e-12);
        assert_close!(
            frame.ecef_to_ned_vector(ecef).0.distance(wind.0),
            0.0,
            1e-12
        );
    }

    #[test]
    fn bearing_uses_compass_convention() {
        // 北が 0、東が 90、南が 180、西が 270。
        assert_close!(
            Ned::new(1.0, 0.0, 0.0).bearing().to_degrees().get(),
            0.0,
            1e-9
        );
        assert_close!(
            Ned::new(0.0, 1.0, 0.0).bearing().to_degrees().get(),
            90.0,
            1e-9
        );
        assert_close!(
            Ned::new(-1.0, 0.0, 0.0).bearing().to_degrees().get(),
            180.0,
            1e-9
        );
        assert_close!(
            Ned::new(0.0, -1.0, 0.0).bearing().to_degrees().get(),
            270.0,
            1e-9
        );
        // 水平成分ゼロで NaN を返さないこと。
        assert!(Ned::new(0.0, 0.0, 5.0).bearing().is_finite());
    }

    // --- 姿勢 ---

    #[test]
    fn attitude_round_trip_through_quaternion() {
        for roll in [-170.0, -90.0, -30.0, 0.0, 30.0, 90.0, 170.0] {
            for pitch in [-85.0, -45.0, 0.0, 45.0, 85.0] {
                for yaw in [0.0, 45.0, 179.0, 270.0, 359.0] {
                    let original = Attitude::from_degrees(roll, pitch, yaw);
                    let result = Attitude::from_quaternion(original.to_quaternion());

                    assert_close!(result.roll.get(), original.roll.get(), 1e-9);
                    assert_close!(result.pitch.get(), original.pitch.get(), 1e-9);
                    // 方位は単純な減算で比較してはならない。0° と 360° は同一方位だが
                    // 差を取ると 360° になる。最短角差で比較する。
                    assert_close!(
                        original.yaw.shortest_difference_to(result.yaw).get(),
                        0.0,
                        1e-9
                    );
                }
            }
        }
    }

    #[test]
    fn level_attitude_maps_body_axes_onto_ned_axes() {
        // 水平・機首北向きなら、機体 X（前）は北、Y（右翼）は東、Z（下）は下。
        let q = Attitude::default().to_quaternion();
        assert_close!((q * DVec3::X).distance(DVec3::X), 0.0, 1e-12);
        assert_close!((q * DVec3::Y).distance(DVec3::Y), 0.0, 1e-12);
        assert_close!((q * DVec3::Z).distance(DVec3::Z), 0.0, 1e-12);
    }

    #[test]
    fn positive_pitch_raises_the_nose() {
        // ピッチ +30° で機首（機体 X 軸）は上を向く。NED の Z は下向きなので
        // 北成分が正、下成分が負になるはず。
        let q = Attitude::from_degrees(0.0, 30.0, 0.0).to_quaternion();
        let nose = q * DVec3::X;
        assert!(nose.x > 0.0, "nose should still point north-ish");
        assert!(
            nose.z < 0.0,
            "positive pitch must raise the nose (down component negative)"
        );
        assert_close!(nose.z, -0.5, 1e-9); // -sin(30°)
    }

    #[test]
    fn positive_roll_lowers_the_right_wing() {
        // ロール +30° で右翼（機体 Y 軸）が下がる → NED の下成分が正。
        let q = Attitude::from_degrees(30.0, 0.0, 0.0).to_quaternion();
        let right_wing = q * DVec3::Y;
        assert!(
            right_wing.z > 0.0,
            "positive roll must lower the right wing"
        );
        assert_close!(right_wing.z, 0.5, 1e-9); // sin(30°)
    }

    #[test]
    fn positive_yaw_turns_toward_the_east() {
        // ヨー +90° で機首は東を向く。
        let q = Attitude::from_degrees(0.0, 0.0, 90.0).to_quaternion();
        let nose = q * DVec3::X;
        assert_close!(nose.distance(DVec3::Y), 0.0, 1e-12);
    }

    #[test]
    fn gimbal_lock_is_resolved_without_nan() {
        // 垂直上昇・垂直降下。フライトシムでは実際に到達する姿勢。
        for pitch_sign in [1.0, -1.0] {
            for yaw_deg in [0.0, 45.0, 123.0, 270.0] {
                let q = DQuat::from_rotation_z(Degrees(yaw_deg).to_radians().get())
                    * DQuat::from_rotation_y(FRAC_PI_2 * pitch_sign);

                let attitude = Attitude::from_quaternion(q);
                assert!(
                    attitude.is_finite(),
                    "gimbal lock produced a non-finite attitude"
                );
                assert_close!(attitude.pitch.get(), FRAC_PI_2 * pitch_sign, 1e-6);
                assert_close!(attitude.roll.get(), 0.0, 1e-12);
                assert_close!(attitude.yaw.to_degrees().get(), yaw_deg, 1e-6);

                // 復元した姿勢が元の回転を再現すること（縮退していても回転自体は一意）。
                let restored = attitude.to_quaternion();
                assert_close!((restored * DVec3::X).distance(q * DVec3::X), 0.0, 1e-9);
            }
        }
    }

    #[test]
    fn quaternion_stays_normalized_under_repeated_composition() {
        // 積分ループを模して回転を繰り返し合成する。正規化を怠ると
        // ノルムが 1 から漂い、姿勢がじわじわ歪む。
        let step = DQuat::from_rotation_x(0.001)
            * DQuat::from_rotation_y(0.0007)
            * DQuat::from_rotation_z(0.0013);

        let mut q = DQuat::IDENTITY;
        for _ in 0..100_000 {
            q = (q * step).normalize();
        }
        assert_close!(q.length(), 1.0, 1e-12);
        assert!(Attitude::from_quaternion(q).is_finite());
    }

    #[test]
    fn yaw_is_normalized_to_compass_range() {
        for yaw_deg in [-350.0, -10.0, 0.0, 10.0, 350.0, 720.0] {
            let a = Attitude::from_quaternion(
                Attitude::from_degrees(0.0, 0.0, yaw_deg).to_quaternion(),
            );
            let v = a.yaw.get();
            assert!(
                (0.0..2.0 * PI).contains(&v),
                "yaw {v} rad is outside [0, 2π) for input {yaw_deg}°"
            );
        }
    }
}
