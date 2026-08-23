//! 着陸評価。
//!
//! ゲームループを閉じる仕上げ。**接地して何の反応も無ければゲームにならない。**
//! `flightsim-sim` の `Touchdown` から app が [`LandingReport`] を詰め、ここで
//! 評価してしばらく表示し、時間が経てば消える。
//!
//! 評価ロジック（沈下率 → 段階、文言選び）は Bevy の ECS 型（`Query` / `Commands` /
//! `System`）を一切使わない純関数にしてある。[`evaluate_landing`] と
//! [`format_landing_report`] は Bevy なしで単体テストできる。
//!
//! 単位変換は表示の直前でのみ、`flightsim_core::units` の変換を通して行う。

use bevy::prelude::*;
use flightsim_core::{MetersPerSecond, Radians, Seconds};

// ---------------------------------------------------------------------------
// app との契約（変えないこと）
// ---------------------------------------------------------------------------

/// 着陸の評価に使う生データ。app が `Touchdown` から詰める。
///
/// app は接地のたびに `commands.insert_resource(LandingReport { .. })` する。
/// ui 側はこのリソースの追加・変更を [`Res::is_added`] / [`Res::is_changed`] で
/// 拾って表示を開始する。**プラグインはこのリソースを `init_resource` しない**
/// （着陸前は存在しない、が正しい状態のため）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct LandingReport {
    /// 接地時の降下率。降下が正。
    pub sink_rate: MetersPerSecond,
    /// 接地時の対地速度（水平）。
    pub ground_speed: MetersPerSecond,
    /// 接地時のバンク角。
    pub bank: Radians,
    /// 滑走路上に接地したか。`None` = 滑走路情報なし（地形なしで飛んでいる等）。
    pub on_runway: Option<bool>,
    /// 滑走路方位との差。`None` = 滑走路情報なし。
    pub heading_error: Option<Radians>,
}

// ---------------------------------------------------------------------------
// 評価の段階
// ---------------------------------------------------------------------------

/// 着陸の評価段階。
///
/// 沈下率だけで見た目安（段階数と文言は ux 担当の裁量）:
///
/// | 段階 | 沈下率 | ft/min 換算（目安） |
/// |---|---|---|
/// | [`LandingGrade::Greased`] | 〜0.3 m/s | 〜60 |
/// | [`LandingGrade::Smooth`] | 〜1.0 m/s | 〜200 |
/// | [`LandingGrade::Normal`] | 〜2.0 m/s | 〜390 |
/// | [`LandingGrade::Firm`] | 〜3.0 m/s | 〜590 |
/// | [`LandingGrade::Dangerous`] | それ以上 | それ以上 |
///
/// `Greased` は基本 4 段階（滑らか・普通・硬い・危険）の上にもう一段設けた、
/// 特に穏やかな接地への賛辞専用の段階。**褒めるときははっきり褒める。**
///
/// 滑走路外への接地・過大なバンクは、沈下率が良くてもこの段階を下げる
/// （[`evaluate_landing`] 参照）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingGrade {
    /// 危険な接地。機体の損傷を疑うレベル。
    Dangerous,
    /// 硬い接地。
    Firm,
    /// 普通の接地。
    Normal,
    /// 滑らかな接地。
    Smooth,
    /// 極上の接地。ほぼ無衝撃。
    Greased,
}

impl LandingGrade {
    /// 段階の並び順を整数にする。大きいほど良い接地。
    const fn rank(self) -> u8 {
        match self {
            Self::Dangerous => 0,
            Self::Firm => 1,
            Self::Normal => 2,
            Self::Smooth => 3,
            Self::Greased => 4,
        }
    }

    const fn from_rank(rank: u8) -> Self {
        match rank {
            0 => Self::Dangerous,
            1 => Self::Firm,
            2 => Self::Normal,
            3 => Self::Smooth,
            _ => Self::Greased,
        }
    }

    /// この段階を超えないように切り詰める。滑走路外への着陸を
    /// 「沈下率が良くても最高評価にしない」ために使う。
    #[must_use]
    const fn capped_at(self, max: Self) -> Self {
        Self::from_rank(if self.rank() < max.rank() {
            self.rank()
        } else {
            max.rank()
        })
    }

    /// 1 段階下げる。`Dangerous` より下にはならない。
    #[must_use]
    const fn downgraded(self) -> Self {
        Self::from_rank(self.rank().saturating_sub(1))
    }

    /// 見出し用の文言。**褒めるときははっきり褒める。**
    #[must_use]
    pub const fn headline(self) -> &'static str {
        match self {
            Self::Greased => "GREASED IT!",
            Self::Smooth => "Smooth landing.",
            Self::Normal => "Solid landing.",
            Self::Firm => "Firm touchdown.",
            Self::Dangerous => "Dangerous landing! Check the aircraft.",
        }
    }
}

/// 沈下率だけを見た段階分け。
///
/// `if`/`else if` の連鎖にしてあるのは意図的。`f64::clamp` は NaN を
/// 素通りさせる（CLAUDE.md の既知の地雷）ので使わない。この連鎖なら
/// `NaN` はどの比較にも一致せず最後の分岐に落ち、安全側（`Dangerous`）に倒れる。
///
/// 負の沈下率（バウンド接地。接地の直前に上昇していた）は、衝撃としては
/// 実質ゼロなのでそのまま `Greased` 側に含める。パニックはしない。
#[must_use]
pub fn grade_for_sink_rate(sink_rate: MetersPerSecond) -> LandingGrade {
    let v = sink_rate.get();
    if v <= 0.3 {
        LandingGrade::Greased
    } else if v <= 1.0 {
        LandingGrade::Smooth
    } else if v <= 2.0 {
        LandingGrade::Normal
    } else if v <= 3.0 {
        LandingGrade::Firm
    } else {
        LandingGrade::Dangerous
    }
}

/// バンクがこれを超えたら減点する。
const BANK_PENALTY_THRESHOLD_DEGREES: f64 = 5.0;

/// 滑走路外への接地は、沈下率がどれほど良くてもこの段階が上限。
const OFF_RUNWAY_GRADE_CAP: LandingGrade = LandingGrade::Normal;

/// 着陸の評価結果。表示用に整形済み。
#[derive(Debug, Clone, PartialEq)]
pub struct LandingEvaluation {
    /// 最終的な段階（減点を反映済み）。
    pub grade: LandingGrade,
    /// 見出し（減点理由があれば併記した一行）。
    pub headline: String,
    /// 数値と補足の行。
    pub detail_lines: Vec<String>,
}

/// [`LandingReport`] を評価する。**Bevy に依存しない純関数。**
#[must_use]
pub fn evaluate_landing(report: LandingReport) -> LandingEvaluation {
    let mut grade = grade_for_sink_rate(report.sink_rate);

    let bank_degrees = report.bank.to_degrees().get();
    let hard_bank = bank_degrees.abs() > BANK_PENALTY_THRESHOLD_DEGREES;
    let off_runway = report.on_runway == Some(false);
    let bounced = report.sink_rate.get() < 0.0;

    if off_runway {
        grade = grade.capped_at(OFF_RUNWAY_GRADE_CAP);
    }
    if hard_bank {
        grade = grade.downgraded();
    }

    let mut notes = Vec::new();
    if off_runway {
        notes.push("off runway".to_string());
    }
    if hard_bank {
        notes.push(format!("banked {:.0}\u{b0}", bank_degrees.abs()));
    }
    let headline = if notes.is_empty() {
        grade.headline().to_string()
    } else {
        format!("{} ({})", grade.headline(), notes.join(", "))
    };

    let sink_fpm = report.sink_rate.to_feet_per_minute();
    let ground_kt = report.ground_speed.to_knots();

    let mut detail_lines = vec![
        format!("Sink rate  {:>6.0} ft/min", sink_fpm.get()),
        format!("Grd speed  {:>6.0} kt", ground_kt.get()),
        format!("Bank       {:>6.1} deg", bank_degrees),
    ];
    if bounced {
        detail_lines.push("(bounced — light contact)".to_string());
    }
    match report.on_runway {
        Some(true) => detail_lines.push("On runway".to_string()),
        Some(false) => detail_lines.push("Off runway".to_string()),
        None => {}
    }
    if let Some(heading_error) = report.heading_error {
        detail_lines.push(format!(
            "Hdg error  {:>6.1} deg",
            heading_error.to_degrees().get().abs()
        ));
    }

    LandingEvaluation {
        grade,
        headline,
        detail_lines,
    }
}

/// [`LandingEvaluation`] を画面に出す文字列にする。**Bevy に依存しない純関数。**
#[must_use]
pub fn format_landing_report(evaluation: &LandingEvaluation) -> String {
    let mut lines = Vec::with_capacity(1 + evaluation.detail_lines.len());
    lines.push(evaluation.headline.clone());
    lines.extend(evaluation.detail_lines.iter().cloned());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// 表示の寿命管理（Bevy に依存しない）
// ---------------------------------------------------------------------------

/// 着陸評価を表示しておく時間。8〜12 秒の目安の中央付近。
pub const LANDING_REPORT_DISPLAY_DURATION: Seconds = Seconds(10.0);

/// 「表示中か・あと何秒か」を管理する。Bevy の `Time` から切り離してテストできる。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LandingReportTimer {
    elapsed: Seconds,
    active: bool,
}

impl LandingReportTimer {
    /// 新しい着陸が来た。表示を最初から数え直す。
    ///
    /// **前の着陸の表示中に呼ばれても構わない。** 次の接地が来たら上書きする
    /// という仕様どおり、経過時間をゼロへ戻す。
    pub fn restart(&mut self) {
        self.elapsed = Seconds(0.0);
        self.active = true;
    }

    /// 時間を進める。戻り値は「まだ表示すべきか」。
    ///
    /// 負の `dt`（想定外の入力）は経過を進めない。詰まらせないための防御。
    pub fn tick(&mut self, dt: Seconds) -> bool {
        if self.active {
            self.elapsed += Seconds(dt.get().max(0.0));
            if self.elapsed.get() >= LANDING_REPORT_DISPLAY_DURATION.get() {
                self.active = false;
            }
        }
        self.active
    }

    /// 現在表示すべきか。
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }
}

// ---------------------------------------------------------------------------
// Bevy 配線
// ---------------------------------------------------------------------------

/// 着陸評価のテキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct LandingReportDisplay;

/// 直近の着陸評価と、その表示タイマー。
///
/// `pub` にしてあるのは `update_landing_report_display` の `ResMut<_>` 引数に
/// 出てくるため（`private_interfaces` lint）。`HudSmoothing` と同じ考え方で、
/// フィールドは非公開のまま型だけ公開する。
#[derive(Resource, Debug, Clone, Default)]
pub struct LandingReportState {
    evaluation: Option<LandingEvaluation>,
    timer: LandingReportTimer,
}

/// 着陸評価の表示欄を作る。
///
/// 既存 HUD の左上の計器列・下部の操作ヘルプと重ならないよう、右上に置く。
pub fn spawn_landing_report_display(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.85, 0.4)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0),
            ..default()
        },
        Visibility::Hidden,
        LandingReportDisplay,
    ));
}

/// 着陸評価の表示を更新する。
///
/// `LandingReport` は着陸するまで存在しない（プラグインが `init_resource` しない、
/// 契約どおりの状態）ので `Option<Res<_>>` で受ける。
pub fn update_landing_report_display(
    time: Res<Time>,
    report: Option<Res<LandingReport>>,
    mut state: ResMut<LandingReportState>,
    mut query: Query<(&mut Text, &mut Visibility), With<LandingReportDisplay>>,
) {
    if let Some(report) = report
        && (report.is_added() || report.is_changed())
    {
        state.evaluation = Some(evaluate_landing(*report));
        state.timer.restart();
    }

    let visible = state.timer.tick(Seconds(f64::from(time.delta_secs())));

    for (mut text, mut visibility) in &mut query {
        if visible {
            if let Some(evaluation) = &state.evaluation {
                **text = format_landing_report(evaluation);
            }
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn greased_on_runway() -> LandingReport {
        LandingReport {
            sink_rate: MetersPerSecond(0.1),
            ground_speed: MetersPerSecond(30.0),
            bank: Radians(0.0),
            on_runway: Some(true),
            heading_error: Some(Radians(0.0)),
        }
    }

    // --- 沈下率の段階分け ---

    #[test]
    fn sink_rate_thresholds_match_the_design_targets() {
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(0.0)),
            LandingGrade::Greased
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(0.3)),
            LandingGrade::Greased
        );
        // 境目のすぐ上。
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(0.300_001)),
            LandingGrade::Smooth
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(1.0)),
            LandingGrade::Smooth
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(1.000_001)),
            LandingGrade::Normal
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(2.0)),
            LandingGrade::Normal
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(2.000_001)),
            LandingGrade::Firm
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(3.0)),
            LandingGrade::Firm
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(3.000_001)),
            LandingGrade::Dangerous
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(100.0)),
            LandingGrade::Dangerous
        );
    }

    #[test]
    fn a_bounced_touchdown_does_not_panic_and_grades_as_gentle() {
        // 接地の直前に上昇していた = バウンド。沈下率は負になる。
        // 衝撃としては実質ゼロなので、褒める側に倒す。パニックしないことが本題。
        let grade = grade_for_sink_rate(MetersPerSecond(-3.0));
        assert_eq!(grade, LandingGrade::Greased);

        let report = LandingReport {
            sink_rate: MetersPerSecond(-3.0),
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_eq!(evaluation.grade, LandingGrade::Greased);
        assert!(
            evaluation
                .detail_lines
                .iter()
                .any(|line| line.contains("bounced")),
            "a bounce should be called out: {:?}",
            evaluation.detail_lines
        );
    }

    #[test]
    fn exactly_zero_sink_rate_does_not_panic() {
        let grade = grade_for_sink_rate(MetersPerSecond(0.0));
        assert_eq!(grade, LandingGrade::Greased);
    }

    #[test]
    fn non_finite_sink_rate_fails_safe_to_the_worst_grade() {
        // NaN はどの比較にも一致しない。`clamp` と違って安全側に倒れることを確認する。
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(f64::NAN)),
            LandingGrade::Dangerous
        );
        assert_eq!(
            grade_for_sink_rate(MetersPerSecond(f64::INFINITY)),
            LandingGrade::Dangerous
        );
        // 負の無限大は「バウンド」側の理屈だと最上位に転びうるので、実際に確認する。
        let grade = grade_for_sink_rate(MetersPerSecond(f64::NEG_INFINITY));
        assert!(
            grade == LandingGrade::Greased || grade == LandingGrade::Dangerous,
            "unexpected grade for -inf: {grade:?}"
        );
    }

    // --- 減点要素 ---

    #[test]
    fn landing_off_the_runway_is_never_top_grade_even_with_a_perfect_sink_rate() {
        let report = LandingReport {
            sink_rate: MetersPerSecond(0.1), // 完璧な沈下率
            on_runway: Some(false),
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_ne!(evaluation.grade, LandingGrade::Greased);
        assert_ne!(evaluation.grade, LandingGrade::Smooth);
        assert!(evaluation.headline.contains("off runway"));
        assert!(
            evaluation
                .detail_lines
                .iter()
                .any(|line| line == "Off runway")
        );
    }

    #[test]
    fn on_runway_true_does_not_cap_the_grade() {
        let report = LandingReport {
            sink_rate: MetersPerSecond(0.1),
            on_runway: Some(true),
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_eq!(evaluation.grade, LandingGrade::Greased);
    }

    #[test]
    fn missing_runway_information_does_not_cap_the_grade() {
        // None = 滑走路情報なし。ペナルティを課す理由が無い。
        let report = LandingReport {
            sink_rate: MetersPerSecond(0.1),
            on_runway: None,
            heading_error: None,
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_eq!(evaluation.grade, LandingGrade::Greased);
        assert!(
            !evaluation
                .detail_lines
                .iter()
                .any(|line| line.contains("runway"))
        );
    }

    #[test]
    fn a_hard_bank_downgrades_the_grade_and_is_named() {
        let level = evaluate_landing(LandingReport {
            bank: Radians(0.0),
            ..greased_on_runway()
        });
        let banked = evaluate_landing(LandingReport {
            bank: Radians(10.0_f64.to_radians()),
            ..greased_on_runway()
        });
        assert_eq!(level.grade, LandingGrade::Greased);
        assert_eq!(banked.grade, LandingGrade::Smooth);
        assert!(banked.headline.contains("banked"));
    }

    #[test]
    fn a_negative_bank_is_treated_the_same_as_a_positive_one() {
        // ロールの符号は左右のどちらかを表すだけで、危険度は絶対値で決まる。
        let banked_left = evaluate_landing(LandingReport {
            bank: Radians(-10.0_f64.to_radians()),
            ..greased_on_runway()
        });
        let banked_right = evaluate_landing(LandingReport {
            bank: Radians(10.0_f64.to_radians()),
            ..greased_on_runway()
        });
        assert_eq!(banked_left.grade, banked_right.grade);
    }

    #[test]
    fn a_small_bank_does_not_trigger_the_penalty() {
        // 目安 5° 超。ちょうど 5° はまだ許容範囲。
        let report = LandingReport {
            bank: Radians(5.0_f64.to_radians()),
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_eq!(evaluation.grade, LandingGrade::Greased);
        assert!(!evaluation.headline.contains("banked"));
    }

    #[test]
    fn a_dangerous_landing_cannot_be_downgraded_past_itself() {
        let report = LandingReport {
            sink_rate: MetersPerSecond(10.0),
            bank: Radians(30.0_f64.to_radians()),
            on_runway: Some(false),
            ..greased_on_runway()
        };
        let evaluation = evaluate_landing(report);
        assert_eq!(evaluation.grade, LandingGrade::Dangerous);
    }

    // --- 数値の併記 ---

    #[test]
    fn the_numbers_are_shown_in_conventional_units() {
        // 100 kt = 51.444 m/s、既知値との照合。
        let report = LandingReport {
            sink_rate: MetersPerSecond(1.0), // 1 m/s ≒ 197 ft/min
            ground_speed: MetersPerSecond(51.444_444_444_444_44), // ≒ 100 kt
            ..greased_on_runway()
        };
        let text = format_landing_report(&evaluate_landing(report));
        assert!(text.contains("ft/min"), "{text}");
        assert!(text.contains("kt"), "{text}");
        assert!(text.contains("100"), "expected ~100 kt in:\n{text}");
    }

    // --- 表示タイマー ---

    #[test]
    fn the_timer_is_inactive_until_started() {
        let timer = LandingReportTimer::default();
        assert!(!timer.is_active());
    }

    #[test]
    fn the_timer_stays_active_for_the_full_duration() {
        let mut timer = LandingReportTimer::default();
        timer.restart();
        // 1 秒刻みで規定時間の直前まで進めても、まだ表示されている。
        // 規定時間は 10 秒なので、9 回進めれば直前 (9 秒 < 10 秒) に収まる。
        const STEPS_BEFORE_EXPIRY: u32 = 9;
        for _ in 0..STEPS_BEFORE_EXPIRY {
            assert!(timer.tick(Seconds(1.0)));
        }
    }

    #[test]
    fn the_timer_expires_after_the_configured_duration() {
        let mut timer = LandingReportTimer::default();
        timer.restart();
        let still_active = timer.tick(Seconds(LANDING_REPORT_DISPLAY_DURATION.get() - 0.001));
        assert!(still_active, "expired one frame early");
        let expired = !timer.tick(Seconds(1.0));
        assert!(expired, "did not expire after the configured duration");
        assert!(!timer.is_active());
    }

    #[test]
    fn a_new_landing_overwrites_the_timer_even_mid_display() {
        let mut timer = LandingReportTimer::default();
        timer.restart();
        timer.tick(Seconds(LANDING_REPORT_DISPLAY_DURATION.get() - 0.1));
        // まだ表示中のはずが、次の接地が来て上書きされる。
        timer.restart();
        assert!(timer.is_active());
        // 上書き直後はほぼ規定時間ぶん残っている。
        assert!(timer.tick(Seconds(LANDING_REPORT_DISPLAY_DURATION.get() - 0.5)));
    }

    #[test]
    fn negative_dt_does_not_advance_or_get_stuck() {
        // 想定外の入力（負の dt）でも表示が詰まったり暴れたりしない。
        let mut timer = LandingReportTimer::default();
        timer.restart();
        for _ in 0..1000 {
            assert!(timer.tick(Seconds(-1.0)));
        }
        // 正の dt に戻れば普通に進み、いずれ期限切れになる。
        let mut ticks = 0;
        while timer.tick(Seconds(1.0)) {
            ticks += 1;
            assert!(ticks < 1_000_000, "the timer never expired");
        }
    }

    // --- 書式 ---

    #[test]
    fn the_headline_is_always_the_first_line() {
        let evaluation = evaluate_landing(greased_on_runway());
        let text = format_landing_report(&evaluation);
        assert_eq!(text.lines().next(), Some(evaluation.headline.as_str()));
    }
}
