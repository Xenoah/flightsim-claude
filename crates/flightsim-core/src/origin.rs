//! Floating origin — `f64` 世界座標から `f32` 描画座標への変換。
//!
//! # 解く問題
//!
//! GPU と描画エンジンは `f32` を前提とする。しかし地球半径 6.378e6 m の位置を `f32` で
//! 表すと分解能は約 0.76 m しかなく、機体は地表で 76cm 格子にスナップして振動する。
//!
//! # 解法
//!
//! カメラ近傍に `f64` のアンカーを置き、描画対象はアンカーからの**相対位置**を `f32` で表す。
//! 相対距離が高々数十 km に収まるなら、`f32` の分解能は
//!
//! ```text
//! 4_000 m / 2^23 ≒ 0.48 mm
//! ```
//!
//! となり、視覚的に完全に十分。カメラがアンカーから一定距離離れたら打ち直す。
//!
//! # 注意
//!
//! アンカーの打ち直しは、`f32` 側の全オブジェクト位置の一括更新を伴う。
//! これはフレームスパイクの候補なので、`flightsim-render` 側でベンチ対象に含めること。

use crate::geodetic::Ecef;
use crate::units::Meters;
use glam::{DVec3, Vec3};

/// アンカー打ち直しの既定閾値。
///
/// 4 km なら `f32` の分解能は 0.5 mm 以下。これより大きくすると精度が落ち、
/// 小さくすると打ち直しが頻発してフレームスパイクの原因になる。
pub const DEFAULT_REBASE_THRESHOLD: Meters = Meters(4_000.0);

/// `f64` ECEF 世界座標を、カメラ近傍を原点とする `f32` 描画座標へ写す。
#[derive(Debug, Clone, Copy)]
pub struct FloatingOrigin {
    anchor: Ecef,
    rebase_threshold: Meters,
}

impl FloatingOrigin {
    /// 既定の閾値（[`DEFAULT_REBASE_THRESHOLD`]）で作る。
    #[must_use]
    pub const fn new(anchor: Ecef) -> Self {
        Self {
            anchor,
            rebase_threshold: DEFAULT_REBASE_THRESHOLD,
        }
    }

    /// 閾値を指定して作る。
    ///
    /// # Panics
    ///
    /// 閾値が正でない場合パニックする。ゼロや負値を許すと毎フレーム打ち直しが発生し、
    /// 静かに性能が崩壊するため、設定ミスとして即座に落とす。
    #[must_use]
    pub fn with_threshold(anchor: Ecef, rebase_threshold: Meters) -> Self {
        assert!(
            rebase_threshold.get() > 0.0,
            "rebase threshold must be positive, got {rebase_threshold}"
        );
        Self {
            anchor,
            rebase_threshold,
        }
    }

    #[must_use]
    pub const fn anchor(&self) -> Ecef {
        self.anchor
    }

    #[must_use]
    pub const fn rebase_threshold(&self) -> Meters {
        self.rebase_threshold
    }

    /// アンカーからの相対位置を `f64` のまま返す。
    ///
    /// `f32` へ落とす前に更に計算を重ねる場合はこちらを使う。
    #[must_use]
    pub fn to_render_precise(&self, position: Ecef) -> DVec3 {
        position.0 - self.anchor.0
    }

    /// 描画用の `f32` 相対座標へ変換する。
    #[must_use]
    pub fn to_render(&self, position: Ecef) -> Vec3 {
        self.to_render_precise(position).as_vec3()
    }

    /// 描画座標から世界座標へ戻す。ピッキングやレイキャストの結果に使う。
    #[must_use]
    pub fn to_world(&self, render_position: Vec3) -> Ecef {
        Ecef(self.anchor.0 + render_position.as_dvec3())
    }

    /// アンカーからの距離。
    #[must_use]
    pub fn distance_from_anchor(&self, position: Ecef) -> Meters {
        self.anchor.distance_to(position)
    }

    /// 打ち直しが必要かどうか。
    #[must_use]
    pub fn needs_rebase(&self, camera: Ecef) -> bool {
        self.distance_from_anchor(camera) > self.rebase_threshold
    }

    /// カメラ位置を与え、閾値を超えていればアンカーを打ち直す。
    ///
    /// 打ち直した場合、**既存の描画座標に加算すべき補正量**を返す。
    /// 呼び出し側は全ての `f32` 位置にこれを足すこと。打ち直しが不要なら `None`。
    ///
    /// ```text
    /// new_render_pos = old_render_pos + shift
    /// ```
    #[must_use = "打ち直しの補正量を無視すると、全オブジェクトの描画位置がずれる"]
    pub fn rebase_if_needed(&mut self, camera: Ecef) -> Option<Vec3> {
        if !self.needs_rebase(camera) {
            return None;
        }

        // 旧アンカー基準の座標を新アンカー基準へ移すための補正量。
        // 打ち直し幅は閾値程度（数 km）なので f32 で表して精度上の問題はない。
        let shift = (self.anchor.0 - camera.0).as_vec3();
        self.anchor = camera;
        Some(shift)
    }

    /// 補正量を計算せず、無条件にアンカーを移す。
    ///
    /// シーンの初期化やテレポートなど、全オブジェクトをどのみち再構築する場面で使う。
    pub fn force_rebase(&mut self, anchor: Ecef) {
        self.anchor = anchor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geodetic::Geodetic;

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

    fn tokyo() -> Ecef {
        Geodetic::from_degrees(35.553_333, 139.781_111, 40.0).to_ecef()
    }

    #[test]
    fn anchor_maps_to_the_render_origin() {
        let origin = FloatingOrigin::new(tokyo());
        assert_close!(origin.to_render(tokyo()).length(), 0.0, 1e-6);
    }

    #[test]
    fn nearby_positions_keep_sub_millimeter_precision() {
        // これが floating origin を導入する理由そのもの。
        // ECEF を直接 f32 化した場合と比較して、精度が桁違いに良いことを示す。
        let anchor = tokyo();
        let origin = FloatingOrigin::new(anchor);

        // アンカーから 3 km の点を 1 mm 刻みで 2 つ用意する。
        let frame = crate::frames::LocalFrame::new(anchor.to_geodetic());
        let a = frame.ned_to_ecef_position(crate::frames::Ned::new(3000.0, 0.0, 0.0));
        let b = frame.ned_to_ecef_position(crate::frames::Ned::new(3000.001, 0.0, 0.0));

        // floating origin 経由なら 1mm の差が保たれる。
        let separation = origin.to_render(a).distance(origin.to_render(b));
        assert!(
            (0.0005..0.0015).contains(&separation),
            "1mm separation was rendered as {separation} m"
        );

        // 一方、ECEF をそのまま f32 化すると差が消し飛ぶ。
        let naive_a = a.0.as_vec3();
        let naive_b = b.0.as_vec3();
        assert!(
            naive_a.distance(naive_b) < 0.0005 || naive_a.distance(naive_b) > 0.0015,
            "naive f32 ECEF unexpectedly preserved 1mm precision; \
             the quantisation argument for floating origin needs revisiting"
        );
    }

    #[test]
    fn round_trip_through_render_space() {
        let origin = FloatingOrigin::new(tokyo());
        let frame = crate::frames::LocalFrame::new(tokyo().to_geodetic());

        for offset in [
            crate::frames::Ned::new(0.0, 0.0, 0.0),
            crate::frames::Ned::new(1500.0, -800.0, -200.0),
            crate::frames::Ned::new(-20_000.0, 15_000.0, -3_000.0),
        ] {
            let world = frame.ned_to_ecef_position(offset);
            let recovered = origin.to_world(origin.to_render(world));
            // f32 を経由するので、20km 先で数 mm の誤差は許容範囲。
            assert_close!(recovered.distance_to(world).get(), 0.0, 0.01);
        }
    }

    #[test]
    fn no_rebase_within_threshold() {
        let mut origin = FloatingOrigin::new(tokyo());
        let frame = crate::frames::LocalFrame::new(tokyo().to_geodetic());

        let near = frame.ned_to_ecef_position(crate::frames::Ned::new(1000.0, 0.0, 0.0));
        assert!(!origin.needs_rebase(near));
        assert!(origin.rebase_if_needed(near).is_none());
        assert_eq!(origin.anchor(), tokyo());
    }

    #[test]
    fn rebase_shift_keeps_objects_visually_stationary() {
        // 打ち直しの本質的な要件。旧補正を適用した位置と、
        // 新アンカーで計算し直した位置が一致しなければ、
        // オブジェクトが打ち直しの瞬間に飛ぶ。
        let mut origin = FloatingOrigin::new(tokyo());
        let frame = crate::frames::LocalFrame::new(tokyo().to_geodetic());

        let landmark = frame.ned_to_ecef_position(crate::frames::Ned::new(2000.0, 500.0, -100.0));
        let before = origin.to_render(landmark);

        // 閾値を超えてカメラを動かす。
        let camera = frame.ned_to_ecef_position(crate::frames::Ned::new(10_000.0, 0.0, 0.0));
        let shift = origin
            .rebase_if_needed(camera)
            .expect("moving 10 km must trigger a rebase");

        let patched = before + shift;
        let recomputed = origin.to_render(landmark);

        assert_close!(patched.distance(recomputed), 0.0, 0.01);
        assert_eq!(origin.anchor(), camera);
    }

    #[test]
    fn repeated_rebasing_does_not_accumulate_drift() {
        // 打ち直しを繰り返しても誤差が蓄積しないこと。
        // 蓄積すると長距離飛行の後に地形と機体がずれる。
        let mut origin = FloatingOrigin::new(tokyo());
        let frame = crate::frames::LocalFrame::new(tokyo().to_geodetic());
        let landmark = frame.ned_to_ecef_position(crate::frames::Ned::new(0.0, 0.0, -500.0));

        for i in 1..=50 {
            let camera = frame.ned_to_ecef_position(crate::frames::Ned::new(
                f64::from(i) * 5_000.0,
                0.0,
                0.0,
            ));
            let _ = origin.rebase_if_needed(camera);
        }

        // 最後のアンカーから見たランドマークの位置が、真値と一致すること。
        let recovered = origin.to_world(origin.to_render(landmark));
        assert_close!(recovered.distance_to(landmark).get(), 0.0, 0.05);
    }

    #[test]
    #[should_panic(expected = "rebase threshold must be positive")]
    fn zero_threshold_is_rejected() {
        // 閾値 0 を許すと毎フレーム打ち直しが起きて静かに性能が崩壊する。
        let _ = FloatingOrigin::with_threshold(tokyo(), Meters(0.0));
    }
}
