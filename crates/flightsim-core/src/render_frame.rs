//! 描画エンジンへ渡すローカル接平面。
//!
//! # [`FloatingOrigin`] との違い
//!
//! [`FloatingOrigin`] は ECEF の軸を保ったまま原点だけを移す。精度の問題は解けるが、
//! **軸の向きは地球の自転軸基準のまま**で、「上」がどの方向かは場所によって変わる。
//!
//! これは描画エンジンと噛み合わない。Bevy を含む多くのエンジンは
//! **Y 軸が上・y = 0 が地表**という前提で書かれている。とくに Bevy 0.18 の
//! 大気散乱は、シェーダ内で高度をこう求めている。
//!
//! ```wgsl
//! var world_pos = view.world_position * settings.scene_units_to_m
//!               + vec3(0.0, atmosphere.bottom_radius, 0.0);
//! ```
//!
//! つまり **`world_position.y` を海抜高度として解釈する**。ECEF 相対の座標を
//! そのまま渡すと、空の色が緯度経度によって出鱈目になる。
//!
//! そこで描画用には、アンカー地点の**局所水平面**を基準にした座標系を使う。
//!
//! ```text
//!   X = 東
//!   Y = 上（アンカー地点の楕円体法線）
//!   Z = 南   （X × Y = Z となる右手系。東 × 上 = -北 = 南）
//! ```
//!
//! アンカーは**楕円体面上（高度 0）**に置く。こうすると `y` がそのまま
//! 楕円体高になり、大気散乱の前提と一致する。
//!
//! # 平面近似の誤差
//!
//! 局所水平面は当然ながら球面の近似で、アンカーから離れるほど「地面が下がる」。
//! 距離 `d` での落差はおよそ `d² / (2R)`。
//!
//! | アンカーからの距離 | 落差 |
//! |---:|---:|
//! | 4 km | 1.3 m |
//! | 50 km | 196 m |
//! | 200 km | 3.1 km |
//!
//! **打ち直し閾値の内側（既定 4 km）では 1.3 m。** 遠景の地形はこの誤差を持つが、
//! 地平線の見え方としてはむしろ自然になる。**この座標系は描画専用であり、
//! 物理や地形の判定に使ってはならない。**

use crate::frames::{LocalFrame, Ned};
use crate::geodetic::{Ecef, Geodetic};
use crate::origin::DEFAULT_REBASE_THRESHOLD;
use crate::units::Meters;
use glam::{DQuat, Quat, Vec3};

/// アンカー地点の局所水平面を基準にした描画座標系。
#[derive(Debug, Clone, Copy)]
pub struct RenderFrame {
    /// 楕円体面上のアンカー（高度は常に 0）。
    anchor: Geodetic,
    frame: LocalFrame,
    rebase_threshold: Meters,
}

impl RenderFrame {
    /// カメラ位置の真下（楕円体面上）をアンカーにして作る。
    #[must_use]
    pub fn new(camera: Geodetic) -> Self {
        Self::with_threshold(camera, DEFAULT_REBASE_THRESHOLD)
    }

    /// # Panics
    ///
    /// 閾値が正の有限値でない場合。打ち直しが起きないか、毎フレーム起きる。
    #[must_use]
    pub fn with_threshold(camera: Geodetic, rebase_threshold: Meters) -> Self {
        assert!(
            rebase_threshold.get().is_finite() && rebase_threshold.get() > 0.0,
            "rebase threshold must be positive and finite, got {rebase_threshold}"
        );
        // 高度成分は落とす。y = 0 を楕円体面に一致させるため。
        let anchor = Geodetic::new(camera.latitude, camera.longitude, Meters::ZERO);
        Self {
            anchor,
            frame: LocalFrame::new(anchor),
            rebase_threshold,
        }
    }

    #[must_use]
    pub const fn anchor(&self) -> Geodetic {
        self.anchor
    }

    #[must_use]
    pub const fn rebase_threshold(&self) -> Meters {
        self.rebase_threshold
    }

    /// 世界座標を描画座標へ写す。
    #[must_use]
    pub fn to_render(&self, position: Ecef) -> Vec3 {
        Self::ned_to_render(self.frame.ecef_to_ned_position(position))
    }

    /// 描画座標を世界座標へ戻す。
    #[must_use]
    pub fn to_world(&self, render: Vec3) -> Ecef {
        self.frame.ned_to_ecef_position(Self::render_to_ned(render))
    }

    /// 世界座標系のベクトル（速度・向きなど）を描画座標へ写す。
    ///
    /// 位置と違って平行移動を伴わない。
    #[must_use]
    pub fn vector_to_render(&self, world: glam::DVec3) -> Vec3 {
        Self::ned_to_render(self.frame.ecef_to_ned_vector(world))
    }

    /// 機体軸 → ECEF の回転を、機体軸 → 描画座標の回転へ写す。
    #[must_use]
    pub fn rotation_to_render(&self, body_to_ecef: DQuat) -> Quat {
        // ECEF → NED → 描画 の順に合成する。
        let ecef_to_ned = self.frame.ned_to_ecef_rotation().inverse();
        let ned_to_render = DQuat::from_mat3(&glam::DMat3::from_cols(
            // NED の各軸が描画座標でどこを向くか。
            glam::DVec3::new(0.0, 0.0, -1.0), // 北 → -Z
            glam::DVec3::new(1.0, 0.0, 0.0),  // 東 → +X
            glam::DVec3::new(0.0, -1.0, 0.0), // 下 → -Y
        ));
        (ned_to_render * ecef_to_ned * body_to_ecef)
            .normalize()
            .as_quat()
    }

    /// アンカーからの水平距離。
    #[must_use]
    pub fn horizontal_distance(&self, position: Geodetic) -> Meters {
        self.anchor.great_circle_distance(position)
    }

    /// 打ち直しが要るか。
    #[must_use]
    pub fn needs_rebase(&self, camera: Geodetic) -> bool {
        self.horizontal_distance(camera).get() > self.rebase_threshold.get()
    }

    /// 必要ならアンカーを打ち直す。打ち直した場合は真。
    ///
    /// **打ち直すと全オブジェクトの描画座標が変わる。** 呼び出し側は
    /// `f32` 側の位置を作り直すこと。
    pub fn rebase_if_needed(&mut self, camera: Geodetic) -> bool {
        if !self.needs_rebase(camera) {
            return false;
        }
        *self = Self::with_threshold(camera, self.rebase_threshold);
        true
    }

    /// NED（北・東・下）を描画座標（東・上・南）へ。
    fn ned_to_render(ned: Ned) -> Vec3 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "アンカーからの相対。打ち直し閾値の内側では f32 の分解能が 1 mm 未満"
        )]
        Vec3::new(ned.east() as f32, ned.up() as f32, -ned.north() as f32)
    }

    /// 描画座標（東・上・南）を NED へ。
    fn render_to_ned(render: Vec3) -> Ned {
        Ned::new(
            f64::from(-render.z),
            f64::from(render.x),
            f64::from(-render.y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Degrees, Radians};

    fn tokyo() -> Geodetic {
        Geodetic::from_degrees(35.553, 139.781, 1_000.0)
    }

    // --- 軸の向き ---

    #[test]
    fn the_anchor_sits_on_the_ellipsoid_below_the_camera() {
        // y = 0 を海抜 0 m に一致させるのがこの座標系の要点。
        // ずれると大気散乱の空の色が高度によって狂う。
        let frame = RenderFrame::new(tokyo());
        assert!(frame.anchor().altitude.get().abs() < 1e-12);
        assert!((frame.anchor().latitude.get() - tokyo().latitude.get()).abs() < 1e-12);
    }

    #[test]
    fn altitude_becomes_the_y_coordinate() {
        // 大気散乱が world_position.y を海抜高度として読む（ADR-0007）。
        let frame = RenderFrame::new(tokyo());
        for altitude in [0.0, 100.0, 3_000.0, -400.0] {
            let position =
                Geodetic::new(tokyo().latitude, tokyo().longitude, Meters(altitude)).to_ecef();
            let rendered = frame.to_render(position);
            assert!(
                (f64::from(rendered.y) - altitude).abs() < 0.01,
                "{altitude} m of altitude became y = {}",
                rendered.y
            );
            assert!(rendered.x.abs() < 0.01 && rendered.z.abs() < 0.01);
        }
    }

    #[test]
    fn east_is_positive_x() {
        let frame = RenderFrame::new(tokyo());
        let east = Geodetic::new(
            tokyo().latitude,
            Radians(tokyo().longitude.get() + Degrees(0.01).to_radians().get()),
            Meters::ZERO,
        );
        let rendered = frame.to_render(east.to_ecef());
        assert!(rendered.x > 100.0, "east gave x = {}", rendered.x);
        assert!(
            rendered.z.abs() < 1.0,
            "east leaked into z = {}",
            rendered.z
        );
    }

    #[test]
    fn north_is_negative_z() {
        // Bevy は右手系で Z が手前。東 × 上 = 南 なので北は -Z。
        let frame = RenderFrame::new(tokyo());
        let north = Geodetic::new(
            Radians(tokyo().latitude.get() + Degrees(0.01).to_radians().get()),
            tokyo().longitude,
            Meters::ZERO,
        );
        let rendered = frame.to_render(north.to_ecef());
        assert!(rendered.z < -100.0, "north gave z = {}", rendered.z);
        assert!(
            rendered.x.abs() < 1.0,
            "north leaked into x = {}",
            rendered.x
        );
    }

    #[test]
    fn the_frame_is_right_handed() {
        // 左手系にすると、全てが鏡像になって「なぜか左右が逆」という
        // 極めて分かりにくい壊れ方をする。
        let frame = RenderFrame::new(tokyo());
        let step = Degrees(0.01).to_radians().get();

        let east = frame.to_render(
            Geodetic::new(
                tokyo().latitude,
                Radians(tokyo().longitude.get() + step),
                Meters::ZERO,
            )
            .to_ecef(),
        );
        let up = frame.to_render(
            Geodetic::new(tokyo().latitude, tokyo().longitude, Meters(1_000.0)).to_ecef(),
        );
        let north = frame.to_render(
            Geodetic::new(
                Radians(tokyo().latitude.get() + step),
                tokyo().longitude,
                Meters::ZERO,
            )
            .to_ecef(),
        );

        // X × Y は +Z（= 南 = -北）を向くはず。
        let cross = east.normalize().cross(up.normalize());
        assert!(
            cross.dot(-north.normalize()) > 0.99,
            "east × up did not point south; the frame is left-handed"
        );
    }

    // --- 往復 ---

    #[test]
    fn positions_round_trip_within_a_millimetre() {
        let frame = RenderFrame::new(tokyo());
        for (north, east, up) in [
            (0.0, 0.0, 0.0),
            (1_000.0, -2_000.0, 500.0),
            (-3_500.0, 3_500.0, -200.0),
            (3_900.0, 0.0, 12_000.0),
        ] {
            let world = frame.frame.ned_to_ecef_position(Ned::new(north, east, -up));
            let back = frame.to_world(frame.to_render(world));
            let error = back.distance_to(world).get();
            assert!(
                error < 0.001,
                "({north}, {east}, {up}) round-tripped with {error:.6} m of error"
            );
        }
    }

    #[test]
    fn a_rebase_preserves_relative_geometry() {
        // 打ち直しで見かけの位置関係が変わると、その瞬間に世界が飛ぶ。
        let mut frame = RenderFrame::new(tokyo());
        let landmarks: Vec<Ecef> = [(0.0, 0.0), (2_000.0, 0.0), (0.0, -1_500.0)]
            .iter()
            .map(|&(north, east)| {
                LocalFrame::new(tokyo()).ned_to_ecef_position(Ned::new(north, east, 0.0))
            })
            .collect();

        let before: Vec<Vec3> = landmarks.iter().map(|&p| frame.to_render(p)).collect();

        // 閾値を越えて移動させる。
        let moved = Geodetic::new(
            Radians(tokyo().latitude.get() + Degrees(0.1).to_radians().get()),
            tokyo().longitude,
            Meters(1_000.0),
        );
        assert!(frame.rebase_if_needed(moved), "the rebase did not happen");

        let after: Vec<Vec3> = landmarks.iter().map(|&p| frame.to_render(p)).collect();
        for i in 0..landmarks.len() {
            for j in (i + 1)..landmarks.len() {
                let was = (before[i] - before[j]).length();
                let now = (after[i] - after[j]).length();
                assert!(
                    (f64::from(was) - f64::from(now)).abs() < 0.05,
                    "the distance between landmarks {i} and {j} changed from {was} to {now}"
                );
            }
        }
    }

    #[test]
    fn a_rebase_only_happens_beyond_the_threshold() {
        let mut frame = RenderFrame::with_threshold(tokyo(), Meters(4_000.0));

        // 1 km 移動しても打ち直さない。
        let near = LocalFrame::new(tokyo())
            .ned_to_ecef_position(Ned::new(1_000.0, 0.0, 0.0))
            .to_geodetic();
        assert!(!frame.rebase_if_needed(near));

        // 10 km なら打ち直す。
        let far = LocalFrame::new(tokyo())
            .ned_to_ecef_position(Ned::new(10_000.0, 0.0, 0.0))
            .to_geodetic();
        assert!(frame.rebase_if_needed(far));
        assert!(frame.horizontal_distance(far).get() < 1.0);
    }

    // --- 姿勢 ---

    #[test]
    fn a_level_north_facing_aircraft_looks_along_negative_z() {
        use crate::frames::Attitude;

        let frame = RenderFrame::new(tokyo());
        let body_to_ecef = LocalFrame::new(tokyo()).ned_to_ecef_rotation()
            * Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO).to_quaternion();

        let rotation = frame.rotation_to_render(body_to_ecef);
        // 機体軸の前方は +X（機首方向）。北を向いているので描画座標では -Z。
        let forward = rotation * Vec3::X;
        assert!(
            forward.dot(Vec3::NEG_Z) > 0.99,
            "a north-facing aircraft points at {forward:?} instead of -Z"
        );

        // 機体軸の下方は +Z。描画座標では -Y。
        let down = rotation * Vec3::Z;
        assert!(
            down.dot(Vec3::NEG_Y) > 0.99,
            "the aircraft's belly points at {down:?} instead of -Y"
        );
    }

    #[test]
    fn an_east_facing_aircraft_looks_along_positive_x() {
        use crate::frames::Attitude;

        let frame = RenderFrame::new(tokyo());
        let body_to_ecef = LocalFrame::new(tokyo()).ned_to_ecef_rotation()
            * Attitude::from_degrees(0.0, 0.0, 90.0).to_quaternion();

        let forward = frame.rotation_to_render(body_to_ecef) * Vec3::X;
        assert!(
            forward.dot(Vec3::X) > 0.99,
            "an east-facing aircraft points at {forward:?} instead of +X"
        );
    }

    #[test]
    fn rotations_stay_normalised() {
        use crate::frames::Attitude;

        let frame = RenderFrame::new(tokyo());
        for (roll, pitch, yaw) in [(0.0, 0.0, 0.0), (35.0, -12.0, 200.0), (-89.0, 89.0, 359.0)] {
            let body_to_ecef = LocalFrame::new(tokyo()).ned_to_ecef_rotation()
                * Attitude::from_degrees(roll, pitch, yaw).to_quaternion();
            let rotation = frame.rotation_to_render(body_to_ecef);
            assert!(
                (rotation.length() - 1.0).abs() < 1e-5,
                "quaternion length {} for ({roll}, {pitch}, {yaw})",
                rotation.length()
            );
        }
    }

    // --- 数値 ---

    #[test]
    fn vectors_transform_without_translation() {
        // 速度ベクトルに平行移動が混ざると、静止した機体が動いて見える。
        let frame = RenderFrame::new(tokyo());
        let north_velocity = LocalFrame::new(tokyo()).ned_to_ecef_vector(Ned::new(50.0, 0.0, 0.0));
        let rendered = frame.vector_to_render(north_velocity);

        assert!((rendered.length() - 50.0).abs() < 0.01);
        assert!(rendered.z < -49.0, "north velocity gave {rendered:?}");
    }

    #[test]
    fn the_frame_works_at_the_poles_and_the_dateline() {
        for (latitude, longitude) in [(90.0, 0.0), (-90.0, 0.0), (0.0, 180.0), (0.0, -180.0)] {
            let camera = Geodetic::from_degrees(latitude, longitude, 500.0);
            let frame = RenderFrame::new(camera);
            let rendered = frame.to_render(camera.to_ecef());
            assert!(
                rendered.is_finite(),
                "({latitude}, {longitude}) rendered to {rendered:?}"
            );
            assert!(
                (f64::from(rendered.y) - 500.0).abs() < 0.01,
                "({latitude}, {longitude}) gave y = {}",
                rendered.y
            );
        }
    }

    #[test]
    #[should_panic(expected = "rebase threshold must be positive")]
    fn a_zero_threshold_is_rejected() {
        let _ = RenderFrame::with_threshold(tokyo(), Meters::ZERO);
    }
}
