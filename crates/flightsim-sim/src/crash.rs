//! 墜落の判定。
//!
//! # なぜ要るのか
//!
//! これが無いと、沈下率 20 m/s で地面に突っ込んでも「接地」として記録され、
//! 評価が出て、機体は無傷で滑走を続ける。**失敗に結果が無いと、うまく降りる
//! 理由が無くなる。**
//!
//! # 何をもって墜落とするか
//!
//! 接地の瞬間に、脚と機体が受け止められない条件を 1 つでも満たしたとき。
//! 判定は接地**直前**の空中の状態で行う（接地後は脚のばねが衝撃を吸収して
//! いて、実際より穏やかに見える）。
//!
//! ## 閾値の根拠と、その限界
//!
//! **どれも認証資料から引いた「壊れる値」ではない。** 壊れる点を決めるには
//! 機体構造の強度データが要り、このプロジェクトは持っていない。以下は
//! 「ここまでは明らかに超えている」と言える線を、根拠を添えて置いたもの。
//!
//! | 条件 | 値 | 置いた理由 |
//! |---|---|---|
//! | 沈下率 | 5 m/s | FAR 23.473(d) の限界降下速度は 7 ft/s = 2.13 m/s。落下試験（23.725）はその 1.5 倍のエネルギー、速度にして約 2.6 m/s で行う。5 m/s はそのエネルギーの約 3.7 倍 |
//! | バンク | 20° | 翼端接触。**翼の上下位置がモデルに無いので幾何からは出せない。** 主脚の横位置 1.3 m・翼半幅 5.5 m・脚高 1.0 m で、翼が脚の高さにあると仮定すれば約 14°。実機の翼はこれより高いので余裕を見て 20° |
//! | 機首下げ | 15° | プロペラ接触。前脚より前に出たプロペラが先に当たる姿勢 |
//!
//! **難易度では変えない。** 墜落は採点ではなく結果なので、初心者だけ地面を
//! 柔らかくすると「降りられた」の意味が人によって変わる。

use flightsim_core::{Geodetic, MetersPerSecond, Radians, Seconds};

/// 何が原因で墜落したか。
///
/// **「墜落した」だけでは次に何を直せばいいか分からない。** 沈下率なのか
/// バンクなのかが分かって初めて練習になる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrashCause {
    /// 沈下率が大きすぎた。脚が持たない。
    SinkRate {
        /// 接地直前の沈下率。降下が正。
        sink_rate: MetersPerSecond,
    },
    /// バンクが大きすぎた。翼端が当たる。
    Bank {
        /// 接地直前のバンク角。
        bank: Radians,
    },
    /// 機首が下がりすぎていた。プロペラが当たる。
    NoseDown {
        /// 接地直前のピッチ角。機首上げが正。
        pitch: Radians,
    },
}

impl CrashCause {
    /// 画面に出す一行。**ASCII のみ**（既定フォントの都合）。
    #[must_use]
    pub fn headline(self) -> String {
        match self {
            Self::SinkRate { sink_rate } => format!(
                "CRASHED: came down at {:.1} m/s; the gear cannot take it",
                sink_rate.get().max(0.0)
            ),
            Self::Bank { bank } => format!(
                "CRASHED: banked {:.0} deg at touchdown; the wingtip hit",
                bank.get().to_degrees().abs()
            ),
            Self::NoseDown { pitch } => format!(
                "CRASHED: nose down {:.0} deg at touchdown; the propeller hit",
                pitch.get().to_degrees().abs()
            ),
        }
    }
}

/// 墜落の記録。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crash {
    /// 原因。
    pub cause: CrashCause,
    /// 墜落した場所。
    pub position: Geodetic,
    /// 開始からの経過時間。
    pub elapsed: Seconds,
}

/// 墜落と判定する境界。
///
/// 値の根拠はモジュールの doc を参照。**外から差し替えられる**が、
/// 難易度で変えないこと（同じ理由でここに書いてある）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrashLimits {
    /// これを超える沈下率での接地は脚が持たない。
    pub sink_rate: MetersPerSecond,
    /// これを超えるバンクでの接地は翼端が当たる。
    pub bank: Radians,
    /// これを超える機首下げでの接地はプロペラが当たる。
    pub nose_down: Radians,
}

impl Default for CrashLimits {
    fn default() -> Self {
        Self {
            sink_rate: MetersPerSecond(5.0),
            bank: Radians(20.0_f64.to_radians()),
            nose_down: Radians(15.0_f64.to_radians()),
        }
    }
}

impl CrashLimits {
    /// 墜落しない設定。
    ///
    /// 回帰テストや、機体を壊さずに接地を繰り返したい検証で使う。
    /// **遊ぶときの設定ではない。**
    pub const NONE: Self = Self {
        sink_rate: MetersPerSecond(f64::INFINITY),
        bank: Radians(f64::INFINITY),
        nose_down: Radians(f64::INFINITY),
    };

    /// この接地が墜落か。墜落なら原因を返す。
    ///
    /// 引数は接地**直前**の値。接地後の値を渡すと、脚が吸収したぶん
    /// 穏やかに見えて墜落を見逃す。
    ///
    /// 非有限値は墜落にしない。**発散は墜落とは別の失敗**で、
    /// 混ぜると「操縦が下手だった」と「計算が壊れた」の区別が付かなくなる。
    #[must_use]
    pub fn evaluate(
        self,
        sink_rate: MetersPerSecond,
        bank: Radians,
        pitch: Radians,
    ) -> Option<CrashCause> {
        if !sink_rate.get().is_finite() || !bank.get().is_finite() || !pitch.get().is_finite() {
            return None;
        }
        // 沈下率を先に見る。**同時に複数該当するとき、一番効いたのは速度**。
        // 傾いていたから壊れたのではなく、速すぎて壊れた。
        if sink_rate.get() > self.sink_rate.get() {
            return Some(CrashCause::SinkRate { sink_rate });
        }
        if bank.get().abs() > self.bank.get() {
            return Some(CrashCause::Bank { bank });
        }
        if pitch.get() < -self.nose_down.get() {
            return Some(CrashCause::NoseDown { pitch });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> CrashLimits {
        CrashLimits::default()
    }

    #[test]
    fn a_normal_landing_is_not_a_crash() {
        // **普通に降りて壊れるようでは遊べない。** 評価表の「硬い接地」
        // （3.0 m/s まで）は生き残ること。
        assert_eq!(
            limits().evaluate(MetersPerSecond(3.0), Radians(0.05), Radians(0.05)),
            None
        );
    }

    #[test]
    fn slamming_into_the_ground_is_a_crash() {
        let cause = limits()
            .evaluate(MetersPerSecond(20.0), Radians(0.0), Radians(0.0))
            .expect("20 m/s into the ground must be a crash");
        assert!(matches!(cause, CrashCause::SinkRate { .. }));
    }

    #[test]
    fn the_boundary_itself_survives() {
        // 境界ちょうどは墜落にしない。**上下どちらに倒すかを決めておく。**
        assert_eq!(
            limits().evaluate(MetersPerSecond(5.0), Radians(0.0), Radians(0.0)),
            None
        );
        assert!(
            limits()
                .evaluate(MetersPerSecond(5.001), Radians(0.0), Radians(0.0))
                .is_some()
        );
    }

    #[test]
    fn a_wing_down_touchdown_is_a_crash_either_way() {
        // 左右どちらに傾いても翼端は当たる。**符号で片側だけ見逃さない。**
        for degrees in [25.0_f64, -25.0] {
            let cause = limits()
                .evaluate(
                    MetersPerSecond(0.5),
                    Radians(degrees.to_radians()),
                    Radians(0.0),
                )
                .unwrap_or_else(|| panic!("{degrees} deg of bank must be a crash"));
            assert!(matches!(cause, CrashCause::Bank { .. }));
        }
    }

    #[test]
    fn a_nose_high_touchdown_is_not_a_propeller_strike() {
        // 機首上げは尾部を擦ることはあってもプロペラは当たらない。
        // **符号を取り違えると、教科書どおりの引き起こしで墜落する。**
        assert_eq!(
            limits().evaluate(
                MetersPerSecond(1.0),
                Radians(0.0),
                Radians(20.0_f64.to_radians())
            ),
            None
        );
        let cause = limits()
            .evaluate(
                MetersPerSecond(1.0),
                Radians(0.0),
                Radians(-20.0_f64.to_radians()),
            )
            .expect("20 deg nose down must be a crash");
        assert!(matches!(cause, CrashCause::NoseDown { .. }));
    }

    #[test]
    fn sink_rate_wins_when_everything_is_wrong_at_once() {
        // 傾いていたから壊れたのではなく、速すぎて壊れた。
        let cause = limits()
            .evaluate(
                MetersPerSecond(30.0),
                Radians(40.0_f64.to_radians()),
                Radians(-40.0_f64.to_radians()),
            )
            .expect("this is a crash");
        assert!(matches!(cause, CrashCause::SinkRate { .. }), "{cause:?}");
    }

    #[test]
    fn a_diverged_state_is_not_reported_as_a_crash() {
        // **「操縦が下手だった」と「計算が壊れた」は別の失敗。**
        // 混ぜると原因の切り分けができなくなる。
        assert_eq!(
            limits().evaluate(MetersPerSecond(f64::NAN), Radians(0.0), Radians(0.0)),
            None
        );
        assert_eq!(
            limits().evaluate(MetersPerSecond(1.0), Radians(f64::INFINITY), Radians(0.0)),
            None
        );
    }

    #[test]
    fn the_none_limits_never_crash() {
        assert_eq!(
            CrashLimits::NONE.evaluate(MetersPerSecond(1_000.0), Radians(3.0), Radians(-3.0)),
            None
        );
    }

    #[test]
    fn every_headline_names_what_went_wrong_and_is_ascii() {
        // **「墜落した」だけでは次に何を直せばいいか分からない。**
        for cause in [
            CrashCause::SinkRate {
                sink_rate: MetersPerSecond(12.5),
            },
            CrashCause::Bank {
                bank: Radians(30.0_f64.to_radians()),
            },
            CrashCause::NoseDown {
                pitch: Radians(-25.0_f64.to_radians()),
            },
        ] {
            let line = cause.headline();
            assert!(line.is_ascii(), "{line}");
            assert!(line.starts_with("CRASHED"), "{line}");
            // 数字が入っていること。度合いが分からないと直しようがない。
            assert!(line.chars().any(|c| c.is_ascii_digit()), "{line}");
        }
    }

    #[test]
    fn the_bank_headline_reads_the_same_both_ways() {
        // 左右で「-30 deg」と出ると、負のバンクという概念を説明する羽目になる。
        let left = CrashCause::Bank {
            bank: Radians(-30.0_f64.to_radians()),
        }
        .headline();
        let right = CrashCause::Bank {
            bank: Radians(30.0_f64.to_radians()),
        }
        .headline();
        assert_eq!(left, right);
        // 負の角度が出ていないこと。区切りにハイフンを使うと、この検査が
        // 自分の区切り記号を拾って通らなくなる（一度やった）。
        assert!(!left.contains('-'), "{left}");
    }
}
