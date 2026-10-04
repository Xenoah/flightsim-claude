//! # flightsim-ui
//!
//! HUD と計器。
//!
//! ## 単位変換はここでのみ行う
//!
//! 内部は SI（m, m/s, rad）だが、航空計器は慣習的に別単位を使う。
//! 変換は必ず `flightsim_core::units` を通す。ここで `* 1.94384` のような
//! マジックナンバーを書かないこと。**係数が散ると片方だけ直されて表示がずれる。**
//!
//! | 表示 | 慣習単位 | 内部単位 |
//! |---|---|---|
//! | 対気速度 | ノット | m/s |
//! | 高度 | フィート | m |
//! | 昇降率 | ft/min | m/s |
//! | 姿勢・方位 | 度 | rad |
//!
//! ## 数値がちらつくと読めない
//!
//! 描画フレームレートで数値を更新すると、下 1 桁が高速に入れ替わって
//! 読み取れなくなる。[`HudSmoothing`] で更新間隔を落とす。
//!
//! ## 着陸評価
//!
//! ゲームループを閉じる仕上げ。接地の評価は
//! [`LandingReport`] / [`evaluate_landing`] / [`format_landing_report`] にある。
//!
//! ## チュートリアル導線
//!
//! 「初見のプレイヤーは離陸できない」が最大の離脱要因。今なにをすべきかを
//! `HudState` から判定して画面中央上に指し示す。状態機械は
//! [`TutorialStage`] / [`TutorialProgress`] にあり、Bevy に依存しない。

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::prelude::*;
use flightsim_core::{Feet, FeetPerMinute, Knots, Meters, MetersPerSecond, Radians, Seconds};

pub mod attitude;
pub mod crash;
pub mod input_diagnostics;
pub mod instruments;
mod landing;
pub mod pause;
pub mod replay;
pub mod traffic;
mod tutorial;
pub mod world_map;

pub use crash::{CrashNotice, CrashOverlay, crash_text};
pub use input_diagnostics::InputDiagnosticsPanel;
pub use landing::{
    LANDING_REPORT_DISPLAY_DURATION, LandingEvaluation, LandingGrade, LandingReport,
    LandingReportDisplay, LandingReportState, LandingReportTimer, evaluate_landing,
    format_landing_report, grade_for_sink_rate, spawn_landing_report_display,
    update_landing_report_display,
};
pub use pause::{PauseOverlay, Paused, pause_text};
pub use replay::{ReplayBanner, ReplayStatus, format_replay_banner};
pub use traffic::TrafficPanel;
pub use tutorial::{
    TutorialProgress, TutorialPrompt, TutorialStage, TutorialState, TutorialVisibility,
    spawn_tutorial_prompt, update_tutorial_prompt,
};
pub use world_map::{
    WorldMapActions, WorldMapLayer, WorldMapRaster, WorldMapStart, WorldMapState, WorldMapSystems,
};

/// HUD に出す値。アプリ側が毎フレーム詰める。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct HudState {
    /// True airspeed through the air mass. Takeoff cues use `equivalent_airspeed`.
    pub airspeed: MetersPerSecond,
    /// Density-corrected equivalent airspeed, supplied by the simulation boundary.
    /// This is not a calibrated pitot/static IAS instrument.
    pub equivalent_airspeed: MetersPerSecond,
    /// AoA-based warning supplied by the app; never inferred from a fixed speed.
    pub stall_warning: bool,
    /// True when the aircraft lift curve cannot supply a valid warning threshold.
    /// The display must not present missing warning data as evidence of safety.
    pub stall_warning_unavailable: bool,
    pub altitude: Meters,
    pub agl: Meters,
    pub vertical_speed: MetersPerSecond,
    pub heading: Radians,
    pub pitch: Radians,
    pub roll: Radians,
    pub throttle: f64,
    pub flaps: f64,
    /// 昇降舵トリム `[-1, 1]`。正で機首上げ＝遅い速度で釣り合う。
    ///
    /// キーを離しても保持する。姿勢・高度を固定する自動操縦ではなく、
    /// 過渡応答や出力・速度・姿勢にも手放し時の動きは左右される。
    pub trim: f64,
    pub on_ground: bool,
    pub terrain_available: bool,
    pub view_mode: &'static str,
    /// Display-only label supplied by app; UI does not depend on the renderer.
    pub graphics_quality: &'static str,
    /// Requested cloud rendering tier and provenance, supplied by app.
    pub cloud_quality: &'static str,
    pub cloud_source: &'static str,
    /// 風がどちら**から**吹くか（真方位）。航空の慣習に合わせる。
    pub wind_from: Radians,
    /// 風速。0 なら `calm` と表示する。
    pub wind_speed: MetersPerSecond,
    /// この飛行の積み上げ。
    pub log: FlightSummary,
    /// 太陽の仰角。**計器の照明を決めるのに使う。**
    ///
    /// `flightsim-ui` は `flightsim-render` に依存できない（同階層の横断は
    /// 規約 2 違反）ので、app が詰め替える。既定の 0 は「地平線」で、
    /// 照明が中途半端に点く。値を渡し忘れても破綻はしない。
    pub sun_elevation: Radians,
}

/// 画面に出す飛行の積み上げ。
///
/// `flightsim-sim` の記録をそのまま持たないのは、**ui が sim に依存しない**
/// ため（依存は一方向。CLAUDE.md 規約 2）。app が詰め替える。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlightSummary {
    /// 空中にいた時間の合計。
    pub airborne_time: Seconds,
    /// 飛んだ距離の累積。
    pub distance: Meters,
    /// 到達した最高の対地高度。
    pub peak_agl: Meters,
    /// 接地の回数。
    pub landings: u32,
}

/// 表示の平滑化。
///
/// 針は滑らかに、数値はゆっくり。両方を同じ頻度で動かすと読めない。
#[derive(Resource, Debug, Clone, Copy)]
pub struct HudSmoothing {
    /// 数値の更新間隔。
    pub refresh_interval: Seconds,
    /// 昇降率の平滑化時定数。生の値は接地時に激しく暴れる。
    pub vertical_speed_time_constant: Seconds,
    elapsed: f64,
    smoothed_vertical_speed: f64,
    displayed: DisplayedValues,
}

impl Default for HudSmoothing {
    fn default() -> Self {
        Self {
            // 秒 10 回。これ以上速いと下 1 桁が読めない。
            refresh_interval: Seconds(0.1),
            vertical_speed_time_constant: Seconds(0.8),
            elapsed: 0.0,
            smoothed_vertical_speed: 0.0,
            displayed: DisplayedValues::default(),
        }
    }
}

/// 実際に画面へ出す値。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DisplayedValues {
    /// Equivalent airspeed for the primary speed display.
    pub airspeed: Knots,
    pub altitude: Feet,
    pub agl: Feet,
    pub vertical_speed: FeetPerMinute,
    pub heading_degrees: f64,
    pub pitch_degrees: f64,
    pub roll_degrees: f64,
}

impl HudSmoothing {
    /// 1 フレーム進めて、表示すべき値を返す。
    pub fn update(&mut self, dt: Seconds, state: &HudState) -> DisplayedValues {
        // 昇降率だけは連続的に均す。接地の瞬間に ±50 m/s を往復するため。
        let tau = self.vertical_speed_time_constant.get().max(1e-6);
        let alpha = 1.0 - (-dt.get().max(0.0) / tau).exp();
        let raw = if state.vertical_speed.get().is_finite() {
            state.vertical_speed.get()
        } else {
            0.0
        };
        self.smoothed_vertical_speed += (raw - self.smoothed_vertical_speed) * alpha;

        self.elapsed += dt.get().max(0.0);
        if self.elapsed >= self.refresh_interval.get() {
            self.elapsed = 0.0;
            self.displayed = DisplayedValues {
                airspeed: state.equivalent_airspeed.to_knots(),
                altitude: state.altitude.to_feet(),
                agl: state.agl.to_feet(),
                vertical_speed: MetersPerSecond(self.smoothed_vertical_speed).to_feet_per_minute(),
                // 方位は 0〜360 に正規化する。-10° を 350° と出す。
                heading_degrees: state.heading.wrap_positive().to_degrees().get(),
                pitch_degrees: state.pitch.to_degrees().get(),
                roll_degrees: state.roll.to_degrees().get(),
            };
        }
        self.displayed
    }

    #[must_use]
    pub const fn displayed(&self) -> DisplayedValues {
        self.displayed
    }
}

/// 画面右下に常時表示するデータ帰属表記。
///
/// 空文字列なら表示しない。Bevy の既定フォントで確実に読めるよう、設定できるのは
/// ASCII の単一行だけである。アプリ側は [`DataAttribution::new`] で初期値を挿入するか、
/// [`DataAttribution::set`] で内容を更新する。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct DataAttribution(String);

impl DataAttribution {
    /// 表示する帰属表記を作る。空文字列なら画面には出ない。
    ///
    /// # Panics
    ///
    /// `text` が非 ASCII または複数行ならパニックする。既定フォントに無い字形が
    /// 豆腐として表示されたり、右下の予約領域からはみ出したりするのを防ぐためである。
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let mut attribution = Self::default();
        attribution.set(text);
        attribution
    }

    /// 表示する帰属表記を置き換える。空文字列を渡すと非表示になる。
    ///
    /// # Panics
    ///
    /// `text` が非 ASCII または複数行ならパニックする。
    pub fn set(&mut self, text: impl Into<String>) {
        let text = text.into();
        assert!(
            text.is_ascii(),
            "data attribution must be ASCII for Bevy's default font"
        );
        assert!(
            !text.contains('\n') && !text.contains('\r'),
            "data attribution must be a single line"
        );
        self.0 = text;
    }

    /// 現在の帰属表記。空なら画面には出ない。
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }

    /// 帰属表記を消して表示を隠す。
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// 表示する帰属表記が無いか。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// HUD のテキスト要素につける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudText;

/// 操作説明のテキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudHelp;

/// 飛行記録のテキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudLog;

/// データ帰属表記のテキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct DataAttributionDisplay;

/// Shared bottom layout reserves the measured attribution height automatically.
#[derive(Component)]
struct HudBottom;

#[derive(Component)]
struct HudBottomPanels;

/// HUD のプラグイン。
#[derive(Debug, Default)]
pub struct FlightsimUiPlugin;

impl Plugin for FlightsimUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(attitude::AttitudeIndicatorPlugin)
            .init_resource::<HudState>()
            .init_resource::<HudSmoothing>()
            .init_resource::<DataAttribution>()
            .init_resource::<InputDiagnosticsPanel>()
            .init_resource::<TrafficPanel>()
            .add_systems(Startup, traffic::spawn_traffic_panel)
            .add_systems(Update, traffic::update_traffic_panel)
            .add_systems(Startup, input_diagnostics::spawn_input_diagnostics)
            .add_systems(Update, input_diagnostics::update_input_diagnostics_panel)
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_help_for_replay)
            .add_systems(
                Update,
                (
                    update_hud,
                    update_flight_log_display,
                    update_data_attribution_display,
                ),
            )
            // 着陸評価。`LandingReport` 自体は着陸するまで存在しないので、
            // ここでは `init_resource` しない（app が接地のたびに挿入する契約）。
            .init_resource::<LandingReportState>()
            .add_systems(Startup, spawn_landing_report_display)
            .add_systems(Update, update_landing_report_display)
            // チュートリアル導線。既定は表示（`TutorialVisibility::default()`）。
            // 実際の `H` キー割り当ては input 担当が
            // `ResMut<TutorialVisibility>::toggle` を呼べば繋がる。
            .init_resource::<TutorialState>()
            .init_resource::<TutorialVisibility>()
            .add_systems(Startup, spawn_tutorial_prompt)
            .add_systems(Update, update_tutorial_prompt)
            // 再生中の表示。app が `ReplayStatus` を埋めなければ出ない。
            .init_resource::<replay::ReplayStatus>()
            .add_systems(Startup, replay::spawn_replay_banner)
            .add_systems(Update, replay::update_replay_banner)
            // 一時停止。app が `Esc` で `Paused` を切り替える。
            .init_resource::<pause::Paused>()
            .add_systems(Startup, pause::spawn_pause_overlay)
            .add_systems(Update, pause::update_pause_overlay)
            // 墜落。app が `CrashNotice` を埋めなければ出ない。
            .init_resource::<crash::CrashNotice>()
            .add_systems(Startup, crash::spawn_crash_overlay)
            .add_systems(Update, crash::update_crash_overlay)
            // 計器盤。コックピット視点のときだけ出る。
            .add_systems(Startup, instruments::spawn_instrument_panel)
            .add_systems(
                Update,
                (
                    instruments::update_instrument_visibility,
                    instruments::update_instruments,
                    instruments::update_panel_lighting,
                ),
            );
    }
}

/// HUD を組み立てる。
pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.85, 1.0, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));

    // Keep help and the flight log in one row above the footer. Flex layout
    // reserves its actual wrapped height in the same layout pass, including
    // narrow windows and attribution/replay changes. With no credit, the row
    // retains the legacy 12-pixel bottom/side margins.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Px(12.0),
                right: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            HudBottom,
        ))
        .with_children(|bottom| {
            bottom
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::FlexEnd,
                        column_gap: Val::Px(12.0),
                        row_gap: Val::Px(8.0),
                        ..default()
                    },
                    HudBottomPanels,
                ))
                .with_children(|panels| {
                    panels.spawn((
                        Text::new(help_text()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.8, 0.85, 0.9, 0.75)),
                        Node {
                            max_width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                        HudHelp,
                    ));
                    panels.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.85, 0.9, 0.85, 0.7)),
                        Node {
                            max_width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            margin: UiRect::left(Val::Auto),
                            ..default()
                        },
                        HudLog,
                    ));
                });
            bottom.spawn((
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgba(0.95, 0.95, 0.95, 0.9)),
                TextLayout::new_with_justify(Justify::Right)
                    .with_linebreak(bevy::text::LineBreak::WordOrCharacter),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Node {
                    display: Display::None,
                    max_width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    align_self: AlignSelf::FlexEnd,
                    flex_shrink: 0.0,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                Visibility::Hidden,
                DataAttributionDisplay,
            ));
        });
}

/// 飛行記録の表示。
///
/// 時間は分秒、距離は海里（航空の慣習）、高度はフィート。
/// 単位変換は `flightsim_core::units` を通す。
#[must_use]
pub fn format_flight_log(log: FlightSummary) -> String {
    let seconds = if log.airborne_time.get().is_finite() {
        log.airborne_time.get().max(0.0)
    } else {
        0.0
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "1 回の飛行は現実的に u64 の秒数に収まる"
    )]
    let total = seconds as u64;

    let distance = if log.distance.get().is_finite() {
        log.distance.get().max(0.0)
    } else {
        0.0
    };
    let nautical_miles = Meters(distance).to_nautical_miles().get();

    let peak = if log.peak_agl.get().is_finite() {
        log.peak_agl.to_feet().get()
    } else {
        0.0
    };

    format!(
        "AIRBORNE {:02}:{:02}
DISTANCE {nautical_miles:>6.1} nm
PEAK AGL {peak:>6.0} ft
LANDINGS {:>6}",
        total / 60,
        total % 60,
        log.landings
    )
}

/// Keep live flight controls out of recorded playback guidance.
pub fn update_help_for_replay(
    status: Res<ReplayStatus>,
    mut help: Query<&mut Text, With<HudHelp>>,
) {
    let desired = if status.active {
        "REPLAY CONTROLS\nF5 ............. pause / resume\nF6 / F7 ........ playback speed\nF8 ............. back 10 seconds\nC .............. change view\nM .............. world map (preview only)\nF10 / F11 ...... input diagnostics\nF12 ............ leave LAN\n\nFlight controls are ignored in replay.".to_owned()
    } else {
        help_text()
    };
    for mut text in &mut help {
        if text.as_str() != desired {
            **text = desired.clone();
        }
    }
}

/// 操作説明。
#[must_use]
pub fn help_text() -> String {
    [
        "S/Down: pull up; W/Up: push down (pitch)",
        "A/D or Left/Right . roll",
        "Q/E ............... rudder",
        "PageUp/PageDown ... throttle (stays set)",
        "F/G ............... flaps out / in",
        "[ / ] ............. trim nose down/up (stays set)",
        "Space ............. wheel brakes",
        "C ................. change view",
        "H ................. hide / show the guide",
        "M ................. world map / new flight",
        ", / . ............. time faster / slower",
        "Esc ............... pause",
        "R ................. restart this flight",
        "F9 ................ save this flight as a replay",
        "F10 / F11 ......... input diagnostics / next page",
        "",
        "Takeoff: full power; near 75 kt EAS, gentle S/Down.",
        "Ease off at FIRST nose rise; then observe.",
        "Trim only once steady: ] if pulling, [ if pushing.",
        "Stall warning: W/Up, wings level, power as needed.",
    ]
    .join("\n")
}

/// HUD の文字列を作る。
///
/// Bevy から切り離してテストできるよう、書式化だけを純関数にしてある。
#[must_use]
pub fn format_hud(values: DisplayedValues, state: &HudState) -> String {
    let ground = if state.on_ground { "  GND" } else { "" };
    let terrain = if state.terrain_available {
        ""
    } else {
        "  [no terrain data]"
    };

    // トリムが中立から離れているときだけ向きを添える。**符号だけでは
    // どちらが機首上げか分からない。**
    let trim_hint = if state.trim > 0.02 {
        "  nose up"
    } else if state.trim < -0.02 {
        "  nose down"
    } else {
        ""
    };

    let warning = if state.stall_warning_unavailable {
        "  STALL WARN N/A"
    } else if state.stall_warning {
        "  STALL WARN"
    } else {
        ""
    };

    format!(
        "EAS  {:>5.0} kt{warning}\n\
         ALT  {:>5.0} ft\n\
         AGL  {:>5.0} ft{ground}\n\
         V/S  {:>5.0} ft/min\n\
         HDG  {:>5.0} deg\n\
         PIT  {:>5.1} deg\n\
         BNK  {:>5.1} deg\n\
         THR  {:>5.0} % set\n\
         FLP  {:>5.0} %\n\
         TRM  {:>5.2}{trim_hint}
\
         WND  {}\n\
         VIEW {}{terrain}\n\
         GFX  {} [F4]\n\
         CLD  {} [F3]\n\
         {}",
        values.airspeed.get(),
        values.altitude.get(),
        values.agl.get(),
        values.vertical_speed.get(),
        values.heading_degrees,
        values.pitch_degrees,
        values.roll_degrees,
        state.throttle * 100.0,
        state.flaps * 100.0,
        state.trim,
        format_wind(state.wind_from, state.wind_speed),
        state.view_mode,
        state.graphics_quality,
        state.cloud_quality,
        state.cloud_source,
    )
}

/// 風の表示。航空の慣習（from / 速度、例: `270 / 10 kt`）。
///
/// 無風は `calm`。METAR と同じ言い方にしておくと、後で実況気象を
/// 入れたときに表示を変えずに済む。
#[must_use]
pub fn format_wind(from: Radians, speed: MetersPerSecond) -> String {
    let knots = speed.to_knots().get();
    let bearing = from.wrap_positive().to_degrees().get();
    // **方位と速度の両方を検査する。** 速度だけ見ていると、方位が NaN の
    // ときに `NaN / 10 kt` が画面に出る（テストが実際に捕まえた）。
    if !knots.is_finite() || !bearing.is_finite() || knots < 1.0 {
        return "calm".to_owned();
    }
    format!("{bearing:03.0} / {knots:.0} kt")
}

/// HUD を更新する。
pub fn update_hud(
    time: Res<Time>,
    state: Res<HudState>,
    mut smoothing: ResMut<HudSmoothing>,
    mut query: Query<&mut Text, With<HudText>>,
) {
    let values = smoothing.update(Seconds(f64::from(time.delta_secs())), &state);
    for mut text in &mut query {
        **text = format_hud(values, &state);
    }
}

/// 飛行記録の表示を更新する。
pub fn update_flight_log_display(state: Res<HudState>, mut query: Query<&mut Text, With<HudLog>>) {
    for mut text in &mut query {
        **text = format_flight_log(state.log);
    }
}

/// データ帰属表記を更新する。空文字列なら要素ごと隠す。
pub fn update_data_attribution_display(
    attribution: Res<DataAttribution>,
    mut display_query: Query<(&mut Text, &mut Visibility, &mut Node), With<DataAttributionDisplay>>,
) {
    for (mut text, mut visibility, mut node) in &mut display_query {
        if text.as_str() != attribution.text() {
            **text = attribution.text().to_owned();
        }
        let empty = attribution.is_empty();
        *visibility = if empty {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        // Hidden visibility alone still occupies layout space. Remove the empty
        // footer from flex layout so the legacy panel position is restored.
        node.display = if empty { Display::None } else { Display::Flex };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;

    fn cruising() -> HudState {
        HudState {
            airspeed: MetersPerSecond(51.444),
            equivalent_airspeed: MetersPerSecond(51.444),
            stall_warning: false,
            stall_warning_unavailable: false,
            altitude: Meters(304.8),
            agl: Meters(304.8),
            vertical_speed: MetersPerSecond(2.54),
            heading: Radians(0.0),
            pitch: Radians(0.05),
            roll: Radians(-0.1),
            throttle: 0.75,
            flaps: 0.0,
            trim: 0.0,
            on_ground: false,
            terrain_available: true,
            view_mode: "COCKPIT",
            graphics_quality: "LIGHT",
            cloud_quality: "LIGHT",
            cloud_source: "MONTHLY MODEL",
            wind_from: Radians(0.0),
            wind_speed: MetersPerSecond(0.0),
            log: FlightSummary::default(),
            sun_elevation: Radians(0.0),
        }
    }

    // --- 字形 ---

    #[test]
    fn nothing_on_screen_uses_glyphs_the_default_font_lacks() {
        // **実機で豆腐が出た。** 着陸評価の見出しが `banked 74°` を出し、
        // Bevy の既定フォントに `°` の字形が無くて □ になった。
        // 画面に出る文字列は ASCII に保つ（日本語 UI を入れるなら
        // フォントを同梱してから）。
        let mut state = cruising();
        state.wind_from = Degrees(270.0).to_radians();
        state.wind_speed = MetersPerSecond(5.144);
        let mut smoothing = HudSmoothing::default();
        let values = smoothing.update(Seconds(1.0), &state);

        let screens = [
            format_hud(values, &state),
            format_flight_log(FlightSummary::default()),
            help_text(),
            format_wind(Degrees(270.0).to_radians(), MetersPerSecond(5.144)),
            DataAttribution::new("Airport data: (c) OpenStreetMap contributors (ODbL)")
                .text()
                .to_owned(),
        ];
        for screen in screens {
            assert!(
                screen.is_ascii(),
                "a non-ASCII glyph reached the screen: {screen:?}"
            );
        }
    }

    fn attribution_harness(attribution: DataAttribution) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(attribution)
            .init_resource::<ReplayStatus>()
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_help_for_replay)
            .add_systems(Update, update_data_attribution_display);
        app.update();
        app
    }

    #[test]
    fn replay_help_switches_back_to_live_controls_without_losing_attribution() {
        let mut app = attribution_harness(DataAttribution::default());
        let help = |app: &mut App| {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Text, With<HudHelp>>();
            query
                .single(world)
                .expect("one help panel")
                .as_str()
                .to_owned()
        };
        assert!(help(&mut app).contains("restart this flight"));
        app.world_mut().resource_mut::<ReplayStatus>().active = true;
        app.update();
        let replay = help(&mut app);
        assert!(replay.is_ascii());
        assert!(replay.contains("F8 ............. back 10 seconds"));
        assert!(replay.contains("M .............. world map (preview only)"));
        assert!(!replay.contains("restart this flight"));
        app.world_mut().resource_mut::<ReplayStatus>().active = false;
        app.update();
        assert_eq!(help(&mut app), help_text());
    }

    #[test]
    fn an_empty_attribution_is_absent_from_the_screen() {
        let mut app = attribution_harness(DataAttribution::default());
        let world = app.world_mut();
        let mut query =
            world.query_filtered::<(&Text, &Visibility), With<DataAttributionDisplay>>();
        let (text, visibility) = query
            .single(world)
            .expect("spawn_hud should create one attribution display");

        assert!(text.is_empty());
        assert_eq!(*visibility, Visibility::Hidden);
    }

    #[test]
    fn a_non_empty_attribution_is_persistent_and_visible() {
        const OSM: &str = "Airport data: (c) OpenStreetMap contributors (ODbL)";
        let mut app = attribution_harness(DataAttribution::new(OSM));

        for _ in 0..2 {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<(&Text, &Visibility), With<DataAttributionDisplay>>();
            let (text, visibility) = query
                .single(world)
                .expect("spawn_hud should create one attribution display");
            assert_eq!(text.as_str(), OSM);
            assert_eq!(*visibility, Visibility::Visible);
            app.update();
        }
    }

    #[test]
    fn empty_attribution_removes_its_layout_reservation() {
        const OSM: &str = "Airport data: (c) OpenStreetMap contributors (ODbL)";
        let mut app = attribution_harness(DataAttribution::default());
        let footer_display = |app: &mut App| {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Node, With<DataAttributionDisplay>>();
            query.single(world).unwrap().display
        };
        assert_eq!(footer_display(&mut app), Display::None);
        app.world_mut().resource_mut::<DataAttribution>().set(OSM);
        app.update();
        assert_eq!(footer_display(&mut app), Display::Flex);
        app.world_mut().resource_mut::<DataAttribution>().clear();
        app.update();
        assert_eq!(footer_display(&mut app), Display::None);
    }

    #[test]
    #[should_panic(expected = "must be ASCII")]
    fn non_ascii_attribution_is_rejected_before_reaching_the_default_font() {
        let _ = DataAttribution::new("Airport data: OpenStreetMap contributors ©");
    }

    // --- 飛行記録 ---

    #[test]
    fn an_untouched_log_reads_as_zero() {
        let text = format_flight_log(FlightSummary::default());
        assert!(text.contains("00:00"), "{text}");
        assert!(text.contains("LANDINGS"), "{text}");
    }

    #[test]
    fn the_log_uses_aviation_units() {
        // 1 海里 = 1852 m、1000 ft = 304.8 m。既知の換算値と突き合わせる。
        let text = format_flight_log(FlightSummary {
            airborne_time: Seconds(125.0),
            distance: Meters(1852.0 * 12.5),
            peak_agl: Meters(304.8),
            landings: 3,
        });
        assert!(text.contains("02:05"), "125 s should read 02:05: {text}");
        assert!(text.contains("12.5 nm"), "{text}");
        assert!(text.contains("1000 ft"), "{text}");
        assert!(text.contains('3'), "{text}");
    }

    #[test]
    fn an_hour_long_flight_still_reads_as_minutes() {
        // 60 分を超えても壊れないこと（60:00 と出る。時間表記は要らない）。
        let text = format_flight_log(FlightSummary {
            airborne_time: Seconds(3661.0),
            ..FlightSummary::default()
        });
        assert!(text.contains("61:01"), "{text}");
    }

    #[test]
    fn non_finite_log_values_do_not_reach_the_screen() {
        // NaN が記録に混ざっても「NaN nm」を出さない。
        let text = format_flight_log(FlightSummary {
            airborne_time: Seconds(f64::NAN),
            distance: Meters(f64::INFINITY),
            peak_agl: Meters(f64::NAN),
            landings: 0,
        });
        assert!(!text.contains("NaN"), "{text}");
        assert!(!text.contains("inf"), "{text}");
    }

    // --- 風の表示 ---

    #[test]
    fn calm_air_is_written_as_calm() {
        // 「000 / 0 kt」より「calm」のほうが読み手に速い。METAR も同じ。
        assert_eq!(format_wind(Radians(0.0), MetersPerSecond(0.0)), "calm");
        // 1 kt 未満は無風扱い。0.4 kt を「0 kt」と出すと嘘になる。
        assert_eq!(format_wind(Radians(1.0), MetersPerSecond(0.2)), "calm");
    }

    #[test]
    fn wind_is_written_the_way_pilots_say_it() {
        // 270° から 10 kt（5.144 m/s）。3 桁ゼロ詰めは航空の慣習。
        let text = format_wind(Degrees(270.0).to_radians(), MetersPerSecond(5.144));
        assert_eq!(text, "270 / 10 kt");
    }

    #[test]
    fn a_northerly_wind_reads_as_360_not_000() {
        // 方位 0 は 360 と書くのが慣習だが、ここでは wrap_positive の
        // 結果をそのまま出す。**どちらにせよ 3 桁になること**を固定する。
        let text = format_wind(Degrees(5.0).to_radians(), MetersPerSecond(10.0));
        assert!(text.starts_with("005"), "{text}");
    }

    #[test]
    fn a_negative_bearing_is_normalised() {
        // -90° は 270°。生の負値を画面に出さない。
        let text = format_wind(Degrees(-90.0).to_radians(), MetersPerSecond(10.0));
        assert!(text.starts_with("270"), "{text}");
    }

    #[test]
    fn non_finite_wind_does_not_reach_the_screen() {
        assert_eq!(format_wind(Radians(f64::NAN), MetersPerSecond(5.0)), "calm");
        assert_eq!(format_wind(Radians(0.0), MetersPerSecond(f64::NAN)), "calm");
        assert_eq!(
            format_wind(Radians(0.0), MetersPerSecond(f64::INFINITY)),
            "calm"
        );
    }

    #[test]
    fn the_hud_shows_the_wind_line() {
        let mut state = cruising();
        state.wind_from = Degrees(270.0).to_radians();
        state.wind_speed = MetersPerSecond(5.144);
        let mut smoothing = HudSmoothing::default();
        let values = smoothing.update(Seconds(1.0), &state);
        let text = format_hud(values, &state);
        assert!(text.contains("WND"), "{text}");
        assert!(text.contains("270 / 10 kt"), "{text}");
    }

    // --- 単位 ---

    #[test]
    fn the_units_match_published_conversions() {
        // 100 kt = 51.444 m/s、1000 ft = 304.8 m。定義値との照合。
        let mut smoothing = HudSmoothing::default();
        let state = cruising();
        // 更新間隔を越えさせる。
        let values = smoothing.update(Seconds(1.0), &state);

        assert!(
            (values.airspeed.get() - 100.0).abs() < 0.1,
            "51.444 m/s became {} kt",
            values.airspeed
        );
        assert!(
            (values.altitude.get() - 1_000.0).abs() < 0.5,
            "304.8 m became {} ft",
            values.altitude
        );
        // 2.54 m/s = 500 ft/min（定義どおり）。平滑化があるので緩く見る。
        assert!(values.vertical_speed.get() > 0.0);
    }

    #[test]
    fn the_heading_is_normalised_to_a_compass_reading() {
        // -10° を 350° と出す。負の方位は計器として意味を成さない。
        let mut smoothing = HudSmoothing::default();
        let state = HudState {
            heading: Radians(-10.0_f64.to_radians()),
            ..cruising()
        };
        let values = smoothing.update(Seconds(1.0), &state);
        assert!(
            (values.heading_degrees - 350.0).abs() < 0.1,
            "-10° displayed as {}",
            values.heading_degrees
        );
    }

    // --- 平滑化 ---

    #[test]
    fn the_numbers_do_not_change_every_frame() {
        // 毎フレーム更新すると下 1 桁が読めなくなる。
        let mut smoothing = HudSmoothing::default();
        let mut state = cruising();
        smoothing.update(Seconds(1.0), &state);
        let first = smoothing.displayed();

        // 1 フレームぶんだけ進めて速度を変える。
        state.equivalent_airspeed = MetersPerSecond(80.0);
        let after_one_frame = smoothing.update(Seconds(1.0 / 60.0), &state);
        assert_eq!(
            after_one_frame, first,
            "the display changed within one frame of a 0.1 s refresh interval"
        );

        // 更新間隔を越えれば反映される。
        let after_refresh = smoothing.update(Seconds(0.2), &state);
        assert_ne!(after_refresh, first);
    }

    #[test]
    fn the_vertical_speed_is_smoothed() {
        // 接地の瞬間に ±50 m/s を往復する。生で出すと読めない。
        let mut smoothing = HudSmoothing::default();
        let calm = HudState {
            vertical_speed: MetersPerSecond(0.0),
            ..cruising()
        };
        for _ in 0..100 {
            smoothing.update(Seconds(0.1), &calm);
        }

        let spike = HudState {
            vertical_speed: MetersPerSecond(-50.0),
            ..cruising()
        };
        let immediate = smoothing.update(Seconds(1.0 / 60.0), &spike);
        assert!(
            immediate.vertical_speed.get() > -500.0,
            "a single-frame spike moved the display to {} ft/min",
            immediate.vertical_speed
        );
    }

    #[test]
    fn non_finite_inputs_do_not_reach_the_display() {
        let mut smoothing = HudSmoothing::default();
        let broken = HudState {
            vertical_speed: MetersPerSecond(f64::NAN),
            ..cruising()
        };
        let values = smoothing.update(Seconds(0.2), &broken);
        assert!(
            values.vertical_speed.get().is_finite(),
            "the display showed {}",
            values.vertical_speed
        );
    }

    // --- 書式 ---

    #[test]
    fn the_display_always_carries_its_units() {
        // "250" ではなく "250 kt"。単位の無い数字は読み手に推測を強いる。
        let mut smoothing = HudSmoothing::default();
        let state = cruising();
        let text = format_hud(smoothing.update(Seconds(1.0), &state), &state);

        for unit in ["kt", "ft", "ft/min", "deg", "%"] {
            assert!(text.contains(unit), "the HUD never shows `{unit}`:\n{text}");
        }
    }

    #[test]
    fn being_on_the_ground_is_visible() {
        let mut smoothing = HudSmoothing::default();
        let parked = HudState {
            on_ground: true,
            ..cruising()
        };
        let text = format_hud(smoothing.update(Seconds(1.0), &parked), &parked);
        assert!(text.contains("GND"), "the HUD does not show ground contact");
    }

    #[test]
    fn missing_terrain_is_visible() {
        // 「なぜ海の上を飛んでいるのか」が分からなくなるのを防ぐ。
        let mut smoothing = HudSmoothing::default();
        let state = HudState {
            terrain_available: false,
            ..cruising()
        };
        let text = format_hud(smoothing.update(Seconds(1.0), &state), &state);
        assert!(text.contains("no terrain data"), "{text}");
    }

    #[test]
    fn eas_and_persistent_controls_are_unambiguous_for_both_profiles() {
        for (profile, trim) in [("Light Single", 0.09), ("Swift Sport", 0.08)] {
            let mut state = cruising();
            state.airspeed = Knots(100.0).to_meters_per_second();
            state.equivalent_airspeed = Knots(75.0).to_meters_per_second();
            state.throttle = 1.0;
            state.trim = trim;
            let mut smoothing = HudSmoothing::default();
            for warning in [false, true, false, true] {
                state.stall_warning = warning;
                let text = format_hud(smoothing.update(Seconds(0.2), &state), &state);
                assert!(text.starts_with("EAS     75 kt"), "{profile}: {text}");
                assert!(!text.contains("TAS") && !text.contains("IAS"));
                assert!(text.contains("THR    100 % set"), "{profile}: {text}");
                assert!(
                    text.contains(&format!("TRM   {trim:.2}  nose up")),
                    "{profile}: {text}"
                );
                assert_eq!(text.contains("STALL WARN"), warning);
            }
        }
    }

    #[test]
    fn unavailable_warning_is_visible_even_with_a_stale_warning_value() {
        let mut state = cruising();
        let mut smoothing = HudSmoothing::default();
        for available in [false, true, false] {
            state.stall_warning_unavailable = !available;
            state.stall_warning = true;
            let text = format_hud(smoothing.update(Seconds(0.2), &state), &state);
            assert_eq!(text.contains("STALL WARN N/A"), !available);
            assert!(text.contains("\nVIEW COCKPIT\n"));
            assert!(text.contains("GFX  LIGHT [F4]"));
            assert!(text.ends_with("CLD  LIGHT [F3]\nMONTHLY MODEL"));
        }
    }

    #[test]
    fn help_teaches_small_inputs_and_conditional_trim_with_bounded_lines() {
        let help = help_text();
        for phrase in [
            "S/Down: pull up",
            "W/Up: push down",
            "stays set",
            "75 kt EAS",
            "FIRST nose rise",
            "then observe",
            "] if pulling",
            "[ if pushing",
            "Stall warning",
        ] {
            assert!(help.contains(phrase), "missing {phrase}: {help}");
        }
        assert!(!help.contains("hold S") && !help.contains("flies hands-off"));
        assert!(!help.contains("brief S") && !help.contains("65 kt EAS"));
        assert!(help.lines().count() <= 20);
        assert!(help.lines().all(|line| line.len() <= 52));
    }

    #[test]
    fn the_help_lists_every_control() {
        // 初見で離陸できないのがこのジャンル最大の離脱要因。
        let help = help_text();
        for expected in [
            "pitch",
            "roll",
            "rudder",
            "throttle",
            "flaps",
            "brakes",
            "view",
            "world map",
        ] {
            assert!(
                help.contains(expected),
                "the help never mentions `{expected}`"
            );
        }
        assert!(
            help.to_lowercase().contains("takeoff"),
            "the help does not say how to take off"
        );
    }
}

#[cfg(test)]
#[path = "attribution_layout_tests.rs"]
mod attribution_layout_tests;
