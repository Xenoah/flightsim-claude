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
            fixed_dt.is_finite() && fixed_dt.get() > 0.0,
            "fixed_dt must be positive and finite, got {fixed_dt}"
        );
        assert!(
            max_frame_time.is_finite() && max_frame_time.get() > 0.0,
            "max_frame_time must be positive and finite, got {max_frame_time}"
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
    /// Negative or non-finite frame times do not advance the clock.
    /// A boundary within 64 scaled f64 roundoff units is treated as exact,
    /// avoiding a whole missing tick from partitions such as 144 Hz into 120 Hz.
    pub fn advance(&mut self, frame_time: Seconds) -> u32 {
        if !frame_time.is_finite() || frame_time.get() <= 0.0 {
            return 0;
        }
        let clamped = frame_time.clamp(Seconds::ZERO, self.max_frame_time);
        self.accumulator += clamped;

        let ratio = self.accumulator / self.fixed_dt;
        let nearest = ratio.round();
        let roundoff = 64.0 * f64::EPSILON * ratio.abs().max(1.0);
        let steps = if (ratio - nearest).abs() <= roundoff {
            nearest
        } else {
            ratio.floor()
        };

        // `max_frame_time` によるクランプで上限が保証されているため、
        // u32 への変換で切り捨てや溢れは起きない。念のため上限も明示しておく。
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "steps は max_frame_time / fixed_dt でクランプ済みの非負有限値"
        )]
        let steps = steps.clamp(0.0, f64::from(u32::MAX)) as u32;

        let remainder = self.accumulator - self.fixed_dt * f64::from(steps);
        // A rounded-up boundary may leave a negative residual of a few ULPs.
        // Do not carry that artificial debt into the next otherwise exact tick.
        self.accumulator = Seconds(remainder.get().max(0.0));
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
        assert_eq!(total, 120, "144 Hz must not lose a boundary tick");
        assert_close!(clock.elapsed().get(), f64::from(total) / 120.0, 1e-9);
    }

    #[test]
    fn shared_frame_boundaries_produce_exact_tick_counts() {
        for fps in [30, 60, 144] {
            let mut clock = FixedStep::new(HZ_120);
            let frame = Seconds(1.0 / f64::from(fps));
            let first: u32 = (0..fps / 3).map(|_| clock.advance(frame)).sum();
            assert_eq!(first, 40, "one-third second at {fps} Hz");
            let rest: u32 = (fps / 3..fps).map(|_| clock.advance(frame)).sum();
            assert_eq!(first + rest, 120, "one second at {fps} Hz");
            for _ in 0..10 {
                assert_eq!(clock.advance(Seconds::ZERO), 0);
            }
            assert!((0.0..1.0).contains(&clock.interpolation_alpha()));
        }
    }

    #[test]
    fn an_hour_of_common_frame_rates_has_exact_tick_count() {
        for fps in [30, 60, 144] {
            let mut clock = FixedStep::new(HZ_120);
            let mut ticks = 0_u64;
            for _ in 0..fps * 3600 {
                ticks += u64::from(clock.advance(Seconds(1.0 / f64::from(fps))));
            }
            assert_eq!(ticks, 432_000, "one hour at {fps} Hz");
            assert!(clock.accumulated().get() < HZ_120.get() * 64.0 * f64::EPSILON);
            assert_close!(clock.elapsed().get(), 3600.0, 1e-7);
        }
    }

    #[test]
    fn nonfinite_frame_time_cannot_poison_a_valid_remainder() {
        let mut clock = FixedStep::new(HZ_120);
        clock.advance(HZ_120 / 2.0);
        let before = clock.accumulated().get().to_bits();
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(clock.advance(Seconds(value)), 0);
            assert_eq!(clock.accumulated().get().to_bits(), before);
            assert_eq!(clock.elapsed(), Seconds::ZERO);
        }
        assert_eq!(clock.advance(HZ_120 / 2.0), 1);
    }

    #[test]
    fn roundoff_policy_does_not_consume_a_real_subtick_gap() {
        let dt = HZ_120.get();
        let gap = dt * 1024.0 * f64::EPSILON;
        let mut below = FixedStep::new(HZ_120);
        assert_eq!(below.advance(Seconds(dt - gap)), 0);
        assert!(below.interpolation_alpha() < 1.0);
        assert_eq!(below.advance(Seconds(gap)), 1);
        let mut above = FixedStep::new(HZ_120);
        assert_eq!(above.advance(Seconds(dt + gap)), 1);
        assert!(above.accumulated().get() > 0.0);
        assert!(above.accumulated().get() < 2.0 * gap);
        let mut one_ulp_below = FixedStep::new(HZ_120);
        assert_eq!(
            one_ulp_below.advance(Seconds(f64::from_bits(dt.to_bits() - 1))),
            1
        );
        assert_eq!(one_ulp_below.accumulated(), Seconds::ZERO);
        assert_eq!(one_ulp_below.advance(Seconds::ZERO), 0);
    }

    #[test]
    fn jittered_partitions_preserve_total_ticks_and_reset() {
        let mut clock = FixedStep::new(HZ_120);
        let mut ticks = 0;
        for _ in 0..1200 {
            for part in [1, 2, 3, 4, 5, 6, 7] {
                ticks += clock.advance(Seconds(HZ_120.get() * f64::from(part) / 28.0));
                assert!((0.0..1.0).contains(&clock.interpolation_alpha()));
            }
        }
        assert_eq!(ticks, 1200);
        assert_close!(
            clock.elapsed().get() + clock.accumulated().get(),
            10.0,
            1e-11
        );
        clock.reset();
        assert_eq!(clock.elapsed(), Seconds::ZERO);
        assert_eq!(clock.advance(Seconds::ZERO), 0);
        assert_eq!(clock.advance(HZ_120), 1);
    }

    #[test]
    #[should_panic(expected = "fixed_dt must be positive and finite")]
    fn infinite_fixed_step_is_rejected() {
        let _ = FixedStep::new(Seconds(f64::INFINITY));
    }

    #[test]
    #[should_panic(expected = "max_frame_time must be positive and finite")]
    fn infinite_frame_limit_is_rejected() {
        let _ = FixedStep::with_max_frame_time(HZ_120, Seconds(f64::INFINITY));
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
