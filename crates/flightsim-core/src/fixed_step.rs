//! 固定タイムステップのアキュムレータ（ADR-0004）。
//!
//! # なぜ固定ステップなのか
//!
//! 剛体の運動方程式を可変 dt（＝描画フレーム時間）でそのまま積分すると、以下で破綻する。
//!
//! - **失速時**: 迎角が急変する領域は非線形性が強い。dt が伸びるとオーバーシュートし、
//!   振動から発散に至る。
//! - **接地時**: 脚のばねダンパは剛性が高い。dt が大きいと反発が増幅し機体が跳ね飛ぶ。
//! - **再現性の喪失**: フレームレートが違うと結果が変わる。回帰テストが書けず、
//!   リプレイもネットワーク同期も成立しない。
//!
//! 3 番目が最も重い。FPS 依存の物理はテスト不能であり、このプロジェクトの開発体制と
//! 根本的に噛み合わない。
//!
//! # 使い方
//!
//! ```
//! use flightsim_core::{FixedStep, Seconds};
//!
//! let mut clock = FixedStep::new(Seconds(1.0 / 120.0));
//!
//! // 毎描画フレーム:
//! let steps = clock.advance(Seconds(1.0 / 60.0));
//! for _ in 0..steps {
//!     // fdm.step(clock.fixed_dt());
//! }
//! let alpha = clock.interpolation_alpha(); // 描画補間に使う
//! # let _ = alpha;
//! ```

use crate::units::Seconds;

/// 1 フレームで消費を許す最大の実時間。
///
/// これが無いと、重い 1 フレームが大量のステップを誘発し、それが次のフレームを更に
/// 重くする **death spiral** に入る。0.25 秒でクランプすることで、
/// 1 フレームあたりのステップ数に上限が生まれる（120Hz なら最大 30 ステップ）。
///
/// クランプによりシミュレーション時間は実時間より遅れるが、
/// **止まるより遅れるほうが遥かにましである。**
pub const DEFAULT_MAX_FRAME_TIME: Seconds = Seconds(0.25);

/// 固定 dt の消化を管理するアキュムレータ。
///
/// このクレートに置いているのは、FDM もワールド更新も同じ刻みを共有する必要があるため。
#[derive(Debug, Clone, Copy)]
pub struct FixedStep {
    fixed_dt: Seconds,
    max_frame_time: Seconds,
    accumulator: Seconds,
    elapsed: Seconds,
}

impl FixedStep {
    /// 固定刻みを指定して作る。
    ///
    /// # Panics
    ///
    /// `fixed_dt` が正でない場合パニックする。ゼロや負値は無限ループを生むため、
    /// 設定ミスとして即座に落とす。
    #[must_use]
    pub fn new(fixed_dt: Seconds) -> Self {
        Self::with_max_frame_time(fixed_dt, DEFAULT_MAX_FRAME_TIME)
    }

    /// スパイラル防止のクランプ値も指定して作る。
    ///
    /// # Panics
    ///
    /// `fixed_dt` または `max_frame_time` が正でない場合パニックする。
    #[must_use]
    pub fn with_max_frame_time(fixed_dt: Seconds, max_frame_time: Seconds) -> Self {
        assert!(
            fixed_dt.get() > 0.0,
            "fixed_dt must be positive, got {fixed_dt}"
        );
        assert!(
            max_frame_time.get() > 0.0,
            "max_frame_time must be positive, got {max_frame_time}"
        );
        Self {
            fixed_dt,
            max_frame_time,
            accumulator: Seconds::ZERO,
            elapsed: Seconds::ZERO,
        }
    }

    /// 物理ステップ 1 回分の刻み。**呼び出し側はこれを `step()` に渡すこと。**
    #[must_use]
    pub const fn fixed_dt(&self) -> Seconds {
        self.fixed_dt
    }

    /// シミュレーション開始からの経過時間。実時間ではなく、消化したステップ数 × `fixed_dt`。
    #[must_use]
    pub const fn elapsed(&self) -> Seconds {
        self.elapsed
    }

    /// 未消化の端数。
    #[must_use]
    pub const fn accumulated(&self) -> Seconds {
        self.accumulator
    }

    /// 描画補間の係数 `[0, 1)`。
    ///
    /// 前ステップの状態と現ステップの状態をこの比率で混ぜて描画する。
    /// **補間結果を物理状態へ書き戻さないこと。** 書き戻すと決定論が壊れ、
    /// リプレイとネットワーク同期の前提が崩れる。
    #[must_use]
    pub fn interpolation_alpha(&self) -> f64 {
        self.accumulator / self.fixed_dt
    }

    /// 1 描画フレーム分の実時間を投入し、**実行すべき物理ステップ数**を返す。
    ///
    /// 返り値は `max_frame_time / fixed_dt` で上限が付く（death spiral 防止）。
    /// 負の `frame_time` は 0 として扱う。
    pub fn advance(&mut self, frame_time: Seconds) -> u32 {
        let clamped = frame_time.clamp(Seconds::ZERO, self.max_frame_time);
        self.accumulator += clamped;

        let steps = (self.accumulator / self.fixed_dt).floor();

        // `max_frame_time` によるクランプで上限が保証されているため、
        // u32 への変換で切り捨てや溢れは起きない。念のため上限も明示しておく。
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "steps は max_frame_time / fixed_dt でクランプ済みの非負有限値"
        )]
        let steps = steps.clamp(0.0, f64::from(u32::MAX)) as u32;

        self.accumulator -= self.fixed_dt * f64::from(steps);
        self.elapsed += self.fixed_dt * f64::from(steps);

        steps
    }

    /// アキュムレータと経過時間を初期化する。シナリオの読み込み直しに使う。
    pub fn reset(&mut self) {
        self.accumulator = Seconds::ZERO;
        self.elapsed = Seconds::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    const HZ_120: Seconds = Seconds(1.0 / 120.0);

    #[test]
    fn exact_multiples_produce_exact_step_counts() {
        let mut clock = FixedStep::new(HZ_120);
        // 60fps の 1 フレーム = 120Hz の 2 ステップ。
        assert_eq!(clock.advance(Seconds(1.0 / 60.0)), 2);
        assert_close!(clock.interpolation_alpha(), 0.0, 1e-9);
    }

    #[test]
    fn leftover_time_carries_into_the_next_frame() {
        let mut clock = FixedStep::new(HZ_120);
        // 144fps では 1 フレームあたり 0.833 ステップ。
        // ステップ数は 0 と 1 を行き来し、端数は失われない。
        let mut total = 0;
        for _ in 0..144 {
            total += clock.advance(Seconds(1.0 / 144.0));
        }
        // 1 秒ぶん投入したので 120 ステップ前後になる。
        assert!(
            (119..=120).contains(&total),
            "got {total} steps in one second"
        );
        assert_close!(clock.elapsed().get(), f64::from(total) / 120.0, 1e-9);
    }

    #[test]
    fn simulation_time_tracks_real_time_without_drift() {
        // 不規則なフレーム時間でも、消化した時間の合計が投入量に追随すること。
        let mut clock = FixedStep::new(HZ_120);
        let frame_times = [0.016, 0.021, 0.008, 0.033, 0.012, 0.017, 0.009];

        let mut injected = 0.0;
        for _ in 0..500 {
            for &ft in &frame_times {
                clock.advance(Seconds(ft));
                injected += ft;
            }
        }

        // 遅れは常に 1 ステップ未満。
        let lag = injected - clock.elapsed().get();
        assert!(
            (0.0..HZ_120.get()).contains(&lag),
            "accumulated lag {lag} s should stay below one step"
        );
    }

    #[test]
    fn spike_is_clamped_to_prevent_death_spiral() {
        // 10 秒間のフリーズが起きても、消化ステップ数は上限で頭打ちになること。
        // ここで無制限にステップを実行すると、その処理自体が次のフレームを更に重くし、
        // 二度と復帰しなくなる。
        let mut clock = FixedStep::with_max_frame_time(HZ_120, Seconds(0.25));
        let steps = clock.advance(Seconds(10.0));

        assert_eq!(steps, 30, "0.25 s / (1/120 s) = 30 steps");
        assert!(clock.accumulated().get() < HZ_120.get());
    }

    #[test]
    fn interpolation_alpha_stays_in_unit_range() {
        let mut clock = FixedStep::new(HZ_120);
        for i in 0..1000 {
            // 意図的に固定刻みと約分できないフレーム時間を使う。
            clock.advance(Seconds(0.0001 * f64::from(i % 97) + 0.003));
            let alpha = clock.interpolation_alpha();
            assert!(
                (0.0..1.0).contains(&alpha),
                "interpolation alpha {alpha} left [0, 1)"
            );
        }
    }

    #[test]
    fn negative_and_zero_frame_times_are_harmless() {
        let mut clock = FixedStep::new(HZ_120);
        assert_eq!(clock.advance(Seconds(0.0)), 0);
        assert_eq!(clock.advance(Seconds(-1.0)), 0);
        assert_close!(clock.elapsed().get(), 0.0, 0.0);
        assert_close!(clock.accumulated().get(), 0.0, 0.0);
    }

    #[test]
    fn advance_is_deterministic() {
        // 同じ入力列からは常に同じステップ列が出ること。ADR-0004 の不変条件。
        let run = || {
            let mut clock = FixedStep::new(HZ_120);
            (0..200)
                .map(|i| clock.advance(Seconds(0.001 * f64::from(i % 41) + 0.004)))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn reset_clears_state() {
        let mut clock = FixedStep::new(HZ_120);
        clock.advance(Seconds(0.5));
        clock.reset();
        assert_close!(clock.elapsed().get(), 0.0, 0.0);
        assert_close!(clock.accumulated().get(), 0.0, 0.0);
    }

    #[test]
    #[should_panic(expected = "fixed_dt must be positive")]
    fn zero_fixed_dt_is_rejected() {
        // 刻み 0 は無限ループになる。設定ミスとして即座に落とす。
        let _ = FixedStep::new(Seconds(0.0));
    }
}
