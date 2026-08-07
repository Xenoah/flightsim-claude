//! 視点モードとカメラ姿勢。
//!
//! # カメラは観測者に徹する
//!
//! カメラの平滑化を物理状態へ影響させないこと。カメラは純粋に観測者であり、
//! 追従の遅れが機体の運動を変えてはならない。

use bevy::prelude::*;
use flightsim_core::{Meters, Radians, Seconds};

/// 視点モード。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// 機体固定。首を振れる。
    #[default]
    Cockpit,
    /// 後方から追従。バネ的な遅れが入る。
    Chase,
    /// 機体から切り離した自由カメラ。
    Free,
    /// 地上の固定点から機体を追う。
    Tower,
}

impl ViewMode {
    /// 次のモード。`C` キーで巡回する。
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Cockpit => Self::Chase,
            Self::Chase => Self::Free,
            Self::Free => Self::Tower,
            Self::Tower => Self::Cockpit,
        }
    }

    /// HUD に出す短い名前。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cockpit => "COCKPIT",
            Self::Chase => "CHASE",
            Self::Free => "FREE",
            Self::Tower => "TOWER",
        }
    }
}

/// 追従カメラの状態。
///
/// **物理には一切影響しない。** ここが遅れても機体の運動は変わらない。
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraRig {
    /// チェイス視点で機体の後方どれだけ離れるか。
    pub chase_distance: Meters,
    /// チェイス視点で機体のどれだけ上に置くか。
    pub chase_height: Meters,
    /// 追従の時定数。小さいほど機敏に追う。
    pub follow_time_constant: Seconds,
    /// コックピット視点での目の位置（機体軸、前・右・下）。
    pub eye_offset: [Meters; 3],
    /// 現在の平滑化された位置（描画座標）。
    smoothed: Vec3,
    initialised: bool,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            chase_distance: Meters(35.0),
            chase_height: Meters(10.0),
            follow_time_constant: Seconds(0.25),
            // 軽single機の目線。重心より少し前、やや上。
            eye_offset: [Meters(0.6), Meters::ZERO, Meters(-0.9)],
            smoothed: Vec3::ZERO,
            initialised: false,
        }
    }
}

impl CameraRig {
    /// 目標位置へ指数的に近づける。
    ///
    /// フレームレートに依存しない形にしてある。`1 - exp(-dt/τ)` を使うことで、
    /// 60 Hz でも 144 Hz でも同じ速さで追従する。**単純な線形補間
    /// （`lerp(a, b, 0.1)`）はフレームレートで挙動が変わる。**
    pub fn follow(&mut self, target: Vec3, dt: Seconds) -> Vec3 {
        if !self.initialised {
            self.smoothed = target;
            self.initialised = true;
            return target;
        }

        let tau = self.follow_time_constant.get().max(1e-6);
        let alpha = 1.0 - (-dt.get().max(0.0) / tau).exp();
        #[allow(
            clippy::cast_possible_truncation,
            reason = "補間係数は [0, 1]。f32 で十分"
        )]
        let alpha = alpha as f32;

        self.smoothed = self.smoothed.lerp(target, alpha);
        if !self.smoothed.is_finite() {
            // NaN が入ったらカメラごと迷子になる。目標へ直接飛ばして復帰する。
            self.smoothed = target;
        }
        self.smoothed
    }

    /// 視点を切り替えたときに追従を初期化する。
    ///
    /// これをしないと、コックピットからタワーへ切り替えた瞬間に
    /// カメラが数キロを数秒かけて飛んでいく。
    pub const fn reset(&mut self) {
        self.initialised = false;
    }

    #[must_use]
    pub const fn is_initialised(&self) -> bool {
        self.initialised
    }
}

/// 見回しの角度を制限する。
///
/// 首は真後ろまでは回らない。制限が無いと、コックピット視点で
/// 際限なく回って方向感覚が失われる。
#[must_use]
pub fn clamp_look_around(yaw: Radians, pitch: Radians) -> (Radians, Radians) {
    let limit_yaw = 150.0_f64.to_radians();
    let limit_pitch = 80.0_f64.to_radians();
    (
        Radians(clamp_finite(yaw.get(), limit_yaw)),
        Radians(clamp_finite(pitch.get(), limit_pitch)),
    )
}

fn clamp_finite(value: f64, limit: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-limit, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_modes_cycle_back_to_the_start() {
        let mut mode = ViewMode::Cockpit;
        let mut seen = vec![mode];
        for _ in 0..3 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(mode.next(), ViewMode::Cockpit, "the cycle does not close");
        assert_eq!(seen.len(), 4);
        // 全モードが 1 度ずつ出ること。
        seen.sort_by_key(|m| m.name());
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn following_is_frame_rate_independent() {
        // 単純な lerp(a, b, 0.1) だと 144 Hz で 2.4 倍速く追いつく。
        // 「なぜか PC によってカメラの追従が違う」という形で現れる。
        let target = Vec3::new(100.0, 0.0, 0.0);

        let mut slow = CameraRig::default();
        slow.follow(Vec3::ZERO, Seconds(0.0));
        for _ in 0..60 {
            slow.follow(target, Seconds(1.0 / 60.0));
        }

        let mut fast = CameraRig::default();
        fast.follow(Vec3::ZERO, Seconds(0.0));
        for _ in 0..144 {
            fast.follow(target, Seconds(1.0 / 144.0));
        }

        let difference = (slow.smoothed - fast.smoothed).length();
        assert!(
            difference < 1.0,
            "after one second 60 Hz reached {:?} but 144 Hz reached {:?}",
            slow.smoothed,
            fast.smoothed
        );
    }

    #[test]
    fn the_camera_converges_on_its_target() {
        let mut rig = CameraRig::default();
        rig.follow(Vec3::ZERO, Seconds(0.0));
        let target = Vec3::new(50.0, 20.0, -10.0);
        for _ in 0..300 {
            rig.follow(target, Seconds(1.0 / 60.0));
        }
        assert!(
            (rig.smoothed - target).length() < 0.01,
            "the camera settled at {:?} rather than {target:?}",
            rig.smoothed
        );
    }

    #[test]
    fn the_first_frame_snaps_rather_than_flying_in() {
        // 初期化しないと、起動直後にカメラが原点から機体まで飛んでくる。
        let mut rig = CameraRig::default();
        let target = Vec3::new(1_000.0, 500.0, 0.0);
        assert_eq!(rig.follow(target, Seconds(1.0 / 60.0)), target);
        assert!(rig.is_initialised());
    }

    #[test]
    fn resetting_makes_the_next_frame_snap_again() {
        // 視点切替でカメラが数キロ飛んでいくのを防ぐ。
        let mut rig = CameraRig::default();
        rig.follow(Vec3::ZERO, Seconds(0.0));
        rig.reset();
        assert!(!rig.is_initialised());

        let elsewhere = Vec3::new(5_000.0, 0.0, 0.0);
        assert_eq!(rig.follow(elsewhere, Seconds(1.0 / 60.0)), elsewhere);
    }

    #[test]
    fn a_non_finite_target_does_not_lose_the_camera() {
        let mut rig = CameraRig::default();
        rig.follow(Vec3::ZERO, Seconds(0.0));
        let result = rig.follow(Vec3::new(f32::NAN, 0.0, 0.0), Seconds(1.0 / 60.0));
        assert!(
            result.is_finite() || result.x.is_nan(),
            "the rig should either recover or expose the bad target, got {result:?}"
        );
        // 有限な目標へ戻せば復帰すること。
        let recovered = rig.follow(Vec3::new(10.0, 0.0, 0.0), Seconds(1.0 / 60.0));
        assert!(
            recovered.is_finite(),
            "the camera never recovered: {recovered:?}"
        );
    }

    #[test]
    fn looking_around_is_limited() {
        let (yaw, pitch) = clamp_look_around(Radians(10.0), Radians(-10.0));
        assert!(yaw.get() <= 150.0_f64.to_radians() + 1e-9);
        assert!(pitch.get() >= -80.0_f64.to_radians() - 1e-9);

        let (yaw, pitch) = clamp_look_around(Radians(f64::NAN), Radians(f64::NAN));
        assert!(yaw.get().abs() < 1e-12 && pitch.get().abs() < 1e-12);
    }
}
