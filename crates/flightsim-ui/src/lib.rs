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

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::prelude::*;
use flightsim_core::{Feet, FeetPerMinute, Knots, Meters, MetersPerSecond, Radians, Seconds};

mod landing;

pub use landing::{
    LANDING_REPORT_DISPLAY_DURATION, LandingEvaluation, LandingGrade, LandingReport,
    LandingReportDisplay, LandingReportState, LandingReportTimer, evaluate_landing,
    format_landing_report, grade_for_sink_rate, spawn_landing_report_display,
    update_landing_report_display,
};

/// HUD に出す値。アプリ側が毎フレーム詰める。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct HudState {
    pub airspeed: MetersPerSecond,
    pub altitude: Meters,
    pub agl: Meters,
    pub vertical_speed: MetersPerSecond,
    pub heading: Radians,
    pub pitch: Radians,
    pub roll: Radians,
    pub throttle: f64,
    pub flaps: f64,
    pub on_ground: bool,
    pub terrain_available: bool,
    pub view_mode: &'static str,
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
                airspeed: state.airspeed.to_knots(),
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

/// HUD のテキスト要素につける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudText;

/// 操作説明のテキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudHelp;

/// HUD のプラグイン。
#[derive(Debug, Default)]
pub struct FlightsimUiPlugin;

impl Plugin for FlightsimUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .init_resource::<HudSmoothing>()
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud)
            // 着陸評価。`LandingReport` 自体は着陸するまで存在しないので、
            // ここでは `init_resource` しない（app が接地のたびに挿入する契約）。
            .init_resource::<LandingReportState>()
            .add_systems(Startup, spawn_landing_report_display)
            .add_systems(Update, update_landing_report_display);
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

    // どのキーが何に割り当てられているかを、いつでも見られるようにする。
    // フライトシムの最大の離脱要因は「初見で離陸できないこと」。
    commands.spawn((
        Text::new(help_text()),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgba(0.8, 0.85, 0.9, 0.75)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudHelp,
    ));
}

/// 操作説明。
#[must_use]
pub fn help_text() -> String {
    [
        "W/S or Up/Down .... pitch (S = nose up)",
        "A/D or Left/Right . roll",
        "Q/E ............... rudder",
        "PageUp/PageDown ... throttle",
        "F/G ............... flaps out / in",
        "Space ............. wheel brakes",
        "C ................. change view",
        "",
        "Takeoff: throttle to full, hold S at about 60 kt.",
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

    format!(
        "IAS  {:>5.0} kt\n\
         ALT  {:>5.0} ft\n\
         AGL  {:>5.0} ft{ground}\n\
         V/S  {:>5.0} ft/min\n\
         HDG  {:>5.0} deg\n\
         PIT  {:>5.1} deg\n\
         BNK  {:>5.1} deg\n\
         THR  {:>5.0} %\n\
         FLP  {:>5.0} %\n\
         VIEW {}{terrain}",
        values.airspeed.get(),
        values.altitude.get(),
        values.agl.get(),
        values.vertical_speed.get(),
        values.heading_degrees,
        values.pitch_degrees,
        values.roll_degrees,
        state.throttle * 100.0,
        state.flaps * 100.0,
        state.view_mode,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cruising() -> HudState {
        HudState {
            airspeed: MetersPerSecond(51.444),
            altitude: Meters(304.8),
            agl: Meters(304.8),
            vertical_speed: MetersPerSecond(2.54),
            heading: Radians(0.0),
            pitch: Radians(0.05),
            roll: Radians(-0.1),
            throttle: 0.75,
            flaps: 0.0,
            on_ground: false,
            terrain_available: true,
            view_mode: "COCKPIT",
        }
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
        state.airspeed = MetersPerSecond(80.0);
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
    fn the_help_lists_every_control() {
        // 初見で離陸できないのがこのジャンル最大の離脱要因。
        let help = help_text();
        for expected in [
            "pitch", "roll", "rudder", "throttle", "flaps", "brakes", "view",
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
