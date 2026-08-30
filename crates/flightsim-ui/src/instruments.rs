//! コックピットの計器盤。
//!
//! # なぜ要るのか
//!
//! **コックピット視点が現状ほぼ空。** 外形モデルは視界を塞ぐので隠してあり、
//! 内装モデルも無いため、画面には空と地面と文字の HUD しかない。
//! 計器が描かれていれば「コックピットに座っている」感じになる。
//!
//! # 丸い針にした理由
//!
//! テープ計器（グラスコックピット風）のほうが実装は楽だが、
//! **同梱機は軽単発機**で、実機の計器盤はアナログの丸型。機体と計器の
//! 世代が食い違うと嘘になる。針の角度は純関数で出し、Bevy の UI ノードを
//! 回転させて描く。
//!
//! # ここで検査すること
//!
//! 針の角度は**外部の規約**で決まっている。実装がそう返すから正しい、
//! ではなく、計器の読み方から期待値を出して突き合わせる。
//!
//! - 高度計の長針は 1000 ft で 1 周する
//! - 方位計は 359° → 1° をまたぐとき逆回転しない
//! - 姿勢儀のバンクは、右バンクで地平線が左下がりに見える

use crate::HudState;
use bevy::prelude::*;
use flightsim_core::{Feet, FeetPerMinute, Knots, Meters, MetersPerSecond, Radians};

/// 計器の並び。実機の T 字配置に倣う。
///
/// 上段左から 対気速度・姿勢・高度、下段左から 昇降・方位・スロットル。
pub const INSTRUMENT_COUNT: usize = 6;

/// 計器 1 つの直径（画面画素）。
///
/// **左下の操作説明と右下の飛行記録に挟まれる幅に収める。** 96 px だと
/// 6 個並べて 606 px になり、画面中央に置くと操作説明へ食い込む
/// （実機のスクリーンショットで確認した）。
const DIAL_SIZE: f32 = 80.0;

/// 計器のあいだの隙間。
const DIAL_GAP: f32 = 6.0;

/// 針の長さ（直径に対する比）。
const NEEDLE_LENGTH: f32 = 0.38;

/// 対気速度計の全周に対応するノット。
///
/// 軽単発機の速度計は 0〜200 kt 程度。**0 が真上ではなく、
/// 実機と同じく左下（約 210°）から始める**。
const AIRSPEED_FULL_SCALE_KNOTS: f64 = 200.0;
const AIRSPEED_START_DEGREES: f64 = 210.0;
const AIRSPEED_SWEEP_DEGREES: f64 = 300.0;

/// 昇降計の全振れ。実機は ±2000 ft/min が一般的。
const VSI_FULL_SCALE_FPM: f64 = 2000.0;
/// 昇降計の振れ角。上下それぞれ 170°（真上を 0 として）。
const VSI_SWEEP_DEGREES: f64 = 170.0;

/// 姿勢儀のピッチ 1 度あたりの地平線の移動量（直径に対する比）。
const PITCH_SCALE_PER_DEGREE: f32 = 0.006;

/// 計器の照明が完全に点く太陽高度（度）。市民薄明の下限。
const PANEL_LIGHT_FULL_ON_DEGREES: f64 = -6.0;

/// 計器の照明が完全に消える太陽高度（度）。
///
/// 滑走路灯（+3 度）より低い。**盤面は昼でも外光で読めるので、
/// 点けるのは本当に暗くなってから**でよい。
const PANEL_LIGHT_FULL_OFF_DEGREES: f64 = 0.0;

/// 照明が最も明るいときの盤面の明るさ。
///
/// 実機の計器照明は赤系（暗順応を壊さない）だが、**ここでは読みやすさを
/// 優先して淡い橙**にする。赤一色は数字の視認性が落ちる。
const PANEL_LIT_FACE: Color = Color::srgba(0.16, 0.12, 0.07, 0.94);

/// どの計器か。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    /// 対気速度計。
    Airspeed,
    /// 姿勢儀。
    Attitude,
    /// 高度計。
    Altitude,
    /// 昇降計。
    VerticalSpeed,
    /// 方位計。
    Heading,
    /// スロットルとフラップ。
    Power,
}

impl Instrument {
    /// 盤面に出す略号。**ASCII のみ**（既定フォントに字形が無い記号は豆腐になる）。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Airspeed => "IAS",
            Self::Attitude => "ATT",
            Self::Altitude => "ALT",
            Self::VerticalSpeed => "V/S",
            Self::Heading => "HDG",
            Self::Power => "PWR",
        }
    }

    /// 実機の T 字配置での並び順。
    #[must_use]
    pub const fn all() -> [Self; INSTRUMENT_COUNT] {
        [
            Self::Airspeed,
            Self::Attitude,
            Self::Altitude,
            Self::VerticalSpeed,
            Self::Heading,
            Self::Power,
        ]
    }
}

/// 針 1 本の向き。**時計回りが正、真上が 0。**
///
/// 画面の座標系ではなく「計器の読み方」で持つ。描画側が
/// `Quat::from_rotation_z(-radians)` に変換する（UI は反時計回りが正）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeedleAngle(pub Radians);

impl NeedleAngle {
    /// 真上。
    pub const UP: Self = Self(Radians(0.0));

    /// 度で作る。
    #[must_use]
    pub fn from_degrees(degrees: f64) -> Self {
        Self(flightsim_core::Degrees(degrees).to_radians())
    }

    /// 度で読む。`[0, 360)` に丸める。
    #[must_use]
    pub fn degrees(self) -> f64 {
        let value = self.0.wrap_positive().to_degrees().get();
        if value.is_finite() { value } else { 0.0 }
    }
}

/// 非有限を 0 に潰す。**計器に NaN を出さない。**
fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

/// 対気速度計の針。
///
/// 0 kt が左下（210°）から始まり、時計回りに 300° 振れて 200 kt。
/// **超過速度でも 1 周を超えない**（振り切れたまま止まる）。
#[must_use]
pub fn airspeed_needle(airspeed: MetersPerSecond) -> NeedleAngle {
    let knots = finite_or_zero(airspeed.to_knots().get()).max(0.0);
    let fraction = (knots / AIRSPEED_FULL_SCALE_KNOTS).clamp(0.0, 1.0);
    NeedleAngle::from_degrees(AIRSPEED_START_DEGREES + fraction * AIRSPEED_SWEEP_DEGREES)
}

/// 高度計の長針（100 ft 目盛り）。
///
/// **1000 ft で 1 周する。** 実機の高度計と同じで、長針だけでは
/// 千の位が読めない（短針と併せて読む）。
#[must_use]
pub fn altitude_hundreds_needle(altitude: Meters) -> NeedleAngle {
    let feet = finite_or_zero(altitude.to_feet().get());
    // 負の高度（海面下）でも巻き戻るだけで壊れない。
    NeedleAngle::from_degrees((feet / 1000.0) * 360.0)
}

/// 高度計の短針（1000 ft 目盛り）。
///
/// 10000 ft で 1 周する。
#[must_use]
pub fn altitude_thousands_needle(altitude: Meters) -> NeedleAngle {
    let feet = finite_or_zero(altitude.to_feet().get());
    NeedleAngle::from_degrees((feet / 10_000.0) * 360.0)
}

/// 昇降計の針。
///
/// 真上が 0、上昇で右（時計回り）、降下で左。±2000 ft/min で振り切れる。
#[must_use]
pub fn vertical_speed_needle(vertical_speed: MetersPerSecond) -> NeedleAngle {
    let fpm = finite_or_zero(vertical_speed.to_feet_per_minute().get());
    let fraction = (fpm / VSI_FULL_SCALE_FPM).clamp(-1.0, 1.0);
    NeedleAngle::from_degrees(fraction * VSI_SWEEP_DEGREES)
}

/// 方位計の目盛り環の回転。
///
/// **機首方位を真上に出す**ので、環は方位の**逆**に回る。
/// 359° → 1° をまたいでも `wrap_positive` で連続する。
#[must_use]
pub fn heading_card_rotation(heading: Radians) -> NeedleAngle {
    let degrees = finite_or_zero(heading.wrap_positive().to_degrees().get());
    NeedleAngle::from_degrees(-degrees)
}

/// 姿勢儀の地平線。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HorizonPlacement {
    /// 地平線の傾き。**バンクの逆**（右バンクで地平線は左下がり）。
    pub roll: NeedleAngle,
    /// 盤面中心からの上下のずれ（画素）。機首上げで地平線は下がる。
    pub offset: f32,
}

/// 姿勢儀の地平線の置き方。
#[must_use]
pub fn horizon_placement(pitch: Radians, roll: Radians) -> HorizonPlacement {
    let pitch_degrees = finite_or_zero(pitch.to_degrees().get());
    let roll_degrees = finite_or_zero(roll.to_degrees().get());

    // 極端な姿勢でも盤面から飛び出さないよう、表示上は ±30° で頭打ち。
    let clamped_pitch = pitch_degrees.clamp(-30.0, 30.0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "画面上のずれは 3 桁の画素数。f32 で十分"
    )]
    let offset = (clamped_pitch * f64::from(PITCH_SCALE_PER_DEGREE) * f64::from(DIAL_SIZE)) as f32;

    HorizonPlacement {
        // **バンクと逆向き。** 機体が右へ傾くと、外の地平線は左下がりに見える。
        roll: NeedleAngle::from_degrees(-roll_degrees),
        offset,
    }
}

/// 計器照明の強さ。1 が全点灯、0 が消灯。
///
/// **滑走路灯と同じ考え方**（[`crate::instruments`] と
/// `flightsim_render::runway_lights` で別々に持つのは、ui が render に
/// 依存できないため）。両端で滑らかに繋ぐ。
#[must_use]
pub fn panel_light_fraction(sun_elevation: Radians) -> f32 {
    let degrees = finite_or_zero(sun_elevation.to_degrees().get());
    let span = PANEL_LIGHT_FULL_OFF_DEGREES - PANEL_LIGHT_FULL_ON_DEGREES;
    let t = ((PANEL_LIGHT_FULL_OFF_DEGREES - degrees) / span).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, reason = "0..=1 の比率。f32 で十分")]
    let t = t as f32;
    t * t * (3.0 - 2.0 * t)
}

/// 照明の強さから盤面の色を決める。
///
/// 消灯時は素の暗い盤面、全点灯で淡い橙。**間を線形に混ぜる**ので、
/// 日没にかけて滑らかに明るくなる。
#[must_use]
pub fn lit_dial_face(fraction: f32) -> Color {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dark = DIAL_FACE.to_linear();
    let lit = PANEL_LIT_FACE.to_linear();
    Color::linear_rgba(
        dark.red + (lit.red - dark.red) * fraction,
        dark.green + (lit.green - dark.green) * fraction,
        dark.blue + (lit.blue - dark.blue) * fraction,
        dark.alpha + (lit.alpha - dark.alpha) * fraction,
    )
}

/// 盤面に添える数値。針だけでは細かい値が読めない。
#[must_use]
pub fn instrument_readout(instrument: Instrument, state: &HudReadout) -> String {
    match instrument {
        Instrument::Airspeed => format!("{:.0} kt", state.airspeed.get().max(0.0)),
        Instrument::Attitude => format!("{:.0} / {:.0}", state.pitch_degrees, state.roll_degrees),
        Instrument::Altitude => format!("{:.0} ft", state.altitude.get()),
        Instrument::VerticalSpeed => format!("{:.0} fpm", state.vertical_speed.get()),
        Instrument::Heading => format!("{:.0} deg", state.heading_degrees),
        Instrument::Power => format!("{:.0} / {:.0} %", state.throttle, state.flaps),
    }
}

/// 計器に出す値をまとめたもの。
///
/// 単位変換は**ここへ詰めるときに一度だけ**行う（`flightsim_core::units`）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HudReadout {
    pub airspeed: Knots,
    pub altitude: Feet,
    pub vertical_speed: FeetPerMinute,
    pub heading_degrees: f64,
    pub pitch_degrees: f64,
    pub roll_degrees: f64,
    /// パーセント。
    pub throttle: f64,
    /// パーセント。
    pub flaps: f64,
}

impl HudReadout {
    /// `HudState` から作る。
    #[must_use]
    pub fn from_state(state: &crate::HudState) -> Self {
        Self {
            airspeed: state.airspeed.to_knots(),
            altitude: state.altitude.to_feet(),
            vertical_speed: state.vertical_speed.to_feet_per_minute(),
            heading_degrees: finite_or_zero(state.heading.wrap_positive().to_degrees().get()),
            pitch_degrees: finite_or_zero(state.pitch.to_degrees().get()),
            roll_degrees: finite_or_zero(state.roll.to_degrees().get()),
            throttle: finite_or_zero(state.throttle) * 100.0,
            flaps: finite_or_zero(state.flaps) * 100.0,
        }
    }
}

// ---------------------------------------------------------------------------
// 描画（Bevy）
// ---------------------------------------------------------------------------

/// 盤面の背景色。
const DIAL_FACE: Color = Color::srgba(0.06, 0.07, 0.08, 0.88);
/// 針の色。
const NEEDLE_COLOR: Color = Color::srgb(0.95, 0.95, 0.90);
/// 補助針（高度計の短針）の色。
const SECONDARY_NEEDLE_COLOR: Color = Color::srgb(0.75, 0.85, 1.0);
/// 空の側。
const HORIZON_SKY: Color = Color::srgb(0.20, 0.42, 0.70);
/// 地面の側。
const HORIZON_GROUND: Color = Color::srgb(0.35, 0.26, 0.16);

/// 計器盤の根。コックピット視点でだけ出す。
#[derive(Component, Debug, Clone, Copy)]
pub struct InstrumentPanel;

/// 針 1 本。どの計器のどの針かを持つ。
#[derive(Component, Debug, Clone, Copy)]
pub struct InstrumentNeedle {
    pub instrument: Instrument,
    /// 高度計は針が 2 本ある。0 が主（100 ft）、1 が補助（1000 ft）。
    pub index: u8,
}

/// 姿勢儀の地平線。
#[derive(Component, Debug, Clone, Copy)]
pub struct AttitudeHorizon;

/// 盤面に添える数値。
#[derive(Component, Debug, Clone, Copy)]
pub struct InstrumentReadout(pub Instrument);

/// 盤面そのもの。照明で色が変わる。
#[derive(Component, Debug, Clone, Copy)]
pub struct DialFace;

/// 計器盤を組み立てる。
///
/// **画面下端の中央**に横並び。左上の計器列・右上の着陸評価・
/// 左下の操作説明・右下の飛行記録・中央上のチュートリアルのどれにも
/// 重ならない場所。
pub fn spawn_instrument_panel(mut commands: Commands) {
    #[allow(clippy::cast_precision_loss, reason = "計器は 6 個")]
    let panel_width =
        DIAL_SIZE * INSTRUMENT_COUNT as f32 + DIAL_GAP * (INSTRUMENT_COUNT as f32 - 1.0);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // **操作説明（左下・9 行）の上へ逃がす。** 画面下端に置くと
                // 幅を詰めても重なる（実機のスクリーンショットで確認した）。
                // ウィンドウ幅に依らず衝突しない位置。
                bottom: Val::Px(190.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-panel_width / 2.0)),
                column_gap: Val::Px(DIAL_GAP),
                ..default()
            },
            Visibility::Hidden,
            InstrumentPanel,
            Name::new("instrument panel"),
        ))
        .with_children(|panel| {
            for instrument in Instrument::all() {
                spawn_dial(panel, instrument);
            }
        });
}

/// 計器 1 つ。
fn spawn_dial(panel: &mut ChildSpawnerCommands, instrument: Instrument) {
    panel
        .spawn((
            Node {
                width: Val::Px(DIAL_SIZE),
                height: Val::Px(DIAL_SIZE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(DIAL_FACE),
            DialFace,
        ))
        .with_children(|dial| {
            if matches!(instrument, Instrument::Attitude) {
                spawn_horizon(dial);
            } else {
                spawn_needles(dial, instrument);
            }

            dial.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(6.0),
                    ..default()
                },
                Text::new(instrument.label()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgba(0.8, 0.9, 0.8, 0.9)),
            ));
            dial.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(6.0),
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 1.0, 0.9)),
                InstrumentReadout(instrument),
            ));
        });
}

/// 姿勢儀の地平線。空と地面の 2 色を持つ板を回転・上下させる。
fn spawn_horizon(dial: &mut ChildSpawnerCommands) {
    // **中心を明示する。** inset を省くと親の静的配置に依存し、
    // 回転の中心が盤面中心からずれる（実機で針がはみ出した）。
    let size = DIAL_SIZE * 2.0;
    dial.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(size),
            height: Val::Px(size),
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            margin: UiRect {
                left: Val::Px(-size / 2.0),
                top: Val::Px(-size / 2.0),
                ..default()
            },
            flex_direction: FlexDirection::Column,
            ..default()
        },
        AttitudeHorizon,
    ))
    .with_children(|horizon| {
        horizon.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(50.0),
                ..default()
            },
            BackgroundColor(HORIZON_SKY),
        ));
        horizon.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(50.0),
                ..default()
            },
            BackgroundColor(HORIZON_GROUND),
        ));
    });
}

/// 針。回転の中心を盤面中心に置くため、針の長さの 2 倍の高さを持つ
/// 板の上半分だけを塗る。
fn spawn_needles(dial: &mut ChildSpawnerCommands, instrument: Instrument) {
    let needles: &[(u8, Color, f32)] = if matches!(instrument, Instrument::Altitude) {
        &[
            (1, SECONDARY_NEEDLE_COLOR, 0.26),
            (0, NEEDLE_COLOR, NEEDLE_LENGTH),
        ]
    } else {
        &[(0, NEEDLE_COLOR, NEEDLE_LENGTH)]
    };

    for (index, color, length) in needles {
        // 回転の中心を盤面中心に置くため、針の長さの 2 倍の高さを持つ板を
        // 中心へ据え、その上半分だけを塗る。**inset を省かないこと。**
        let width = 3.0;
        let height = DIAL_SIZE * length * 2.0;
        dial.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(width),
                height: Val::Px(height),
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-width / 2.0),
                    top: Val::Px(-height / 2.0),
                    ..default()
                },
                flex_direction: FlexDirection::Column,
                ..default()
            },
            InstrumentNeedle {
                instrument,
                index: *index,
            },
        ))
        .with_children(|needle| {
            needle.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(50.0),
                    ..default()
                },
                BackgroundColor(*color),
            ));
        });
    }
}

/// コックピット視点のときだけ計器盤を出す。
pub fn update_instrument_visibility(
    state: Res<HudState>,
    mut panels: Query<&mut Visibility, With<InstrumentPanel>>,
) {
    let wanted = if state.view_mode == "COCKPIT" {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut panels {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// 針と地平線と数値を更新する。
pub fn update_instruments(
    state: Res<HudState>,
    mut needles: Query<(&InstrumentNeedle, &mut UiTransform)>,
    mut horizons: Query<&mut UiTransform, (With<AttitudeHorizon>, Without<InstrumentNeedle>)>,
    mut readouts: Query<(&InstrumentReadout, &mut Text)>,
) {
    let readout = HudReadout::from_state(&state);

    for (needle, mut transform) in &mut needles {
        let angle = needle_angle_for(needle, &state);
        // **UI は反時計回りが正。** 計器の読み方（時計回り）から符号を返す。
        #[allow(
            clippy::cast_possible_truncation,
            reason = "角度は [0, 2pi)。f32 の分解能で十分"
        )]
        let radians = angle.0.get() as f32;
        transform.rotation = Rot2::radians(-radians);
    }

    let placement = horizon_placement(state.pitch, state.roll);
    for mut transform in &mut horizons {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "角度は [0, 2pi)。f32 の分解能で十分"
        )]
        let radians = placement.roll.0.get() as f32;
        transform.rotation = Rot2::radians(-radians);
        transform.translation = Val2::px(0.0, placement.offset);
    }

    for (target, mut text) in &mut readouts {
        **text = instrument_readout(target.0, &readout);
    }
}

/// 太陽高度に応じて盤面を照らす。
///
/// **夜に盤面が読めないと計器盤の意味がない。** 滑走路灯と同じく
/// 太陽高度へ連動させ、暗くなるほど盤面を明るくする。
pub fn update_panel_lighting(
    state: Res<HudState>,
    mut faces: Query<&mut BackgroundColor, With<DialFace>>,
    mut previous: Local<Option<f32>>,
) {
    let fraction = panel_light_fraction(state.sun_elevation);
    if previous.is_some_and(|value| (value - fraction).abs() < 1e-3) {
        return;
    }
    *previous = Some(fraction);

    let color = lit_dial_face(fraction);
    for mut face in &mut faces {
        face.0 = color;
    }
}

/// 針 1 本の向きを決める。
fn needle_angle_for(needle: &InstrumentNeedle, state: &HudState) -> NeedleAngle {
    match (needle.instrument, needle.index) {
        (Instrument::Airspeed, _) => airspeed_needle(state.airspeed),
        (Instrument::Altitude, 0) => altitude_hundreds_needle(state.altitude),
        (Instrument::Altitude, _) => altitude_thousands_needle(state.altitude),
        (Instrument::VerticalSpeed, _) => vertical_speed_needle(state.vertical_speed),
        (Instrument::Heading, _) => heading_card_rotation(state.heading),
        // スロットルは 0〜100% を 270 度に割り当てる。
        (Instrument::Power, _) => {
            NeedleAngle::from_degrees(finite_or_zero(state.throttle).clamp(0.0, 1.0) * 270.0)
        }
        (Instrument::Attitude, _) => NeedleAngle::UP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;

    /// 角度の差を `[-180, 180]` で測る。
    fn angular_difference(a: f64, b: f64) -> f64 {
        let mut difference = (a - b) % 360.0;
        if difference > 180.0 {
            difference -= 360.0;
        }
        if difference < -180.0 {
            difference += 360.0;
        }
        difference
    }

    // --- 対気速度計 ---

    #[test]
    fn the_airspeed_needle_starts_at_the_bottom_left() {
        // 実機の速度計は 0 が真上ではない。ここを真上にすると
        // 巡航速度で針が真下を向き、読みにくくなる。
        let at_zero = airspeed_needle(MetersPerSecond(0.0)).degrees();
        assert!(
            (at_zero - AIRSPEED_START_DEGREES).abs() < 1e-6,
            "0 kt should sit at {AIRSPEED_START_DEGREES} deg, got {at_zero}"
        );
    }

    #[test]
    fn the_airspeed_needle_sweeps_clockwise_with_speed() {
        // 100 kt = 51.44 m/s は全目盛りの半分。
        let half = airspeed_needle(Knots(100.0).to_meters_per_second());
        let expected = AIRSPEED_START_DEGREES + AIRSPEED_SWEEP_DEGREES * 0.5;
        assert!(
            angular_difference(half.degrees(), expected).abs() < 0.5,
            "100 kt should read {expected} deg, got {}",
            half.degrees()
        );
    }

    #[test]
    fn overspeed_pins_the_needle_instead_of_wrapping() {
        // 振り切れたまま止まること。1 周させると低速と見分けが付かない。
        let full = airspeed_needle(Knots(AIRSPEED_FULL_SCALE_KNOTS).to_meters_per_second());
        let over = airspeed_needle(Knots(500.0).to_meters_per_second());
        assert!(
            angular_difference(full.degrees(), over.degrees()).abs() < 1e-6,
            "the needle should pin at full scale"
        );
    }

    #[test]
    fn a_negative_or_broken_airspeed_reads_zero() {
        for value in [-10.0, f64::NAN, f64::INFINITY] {
            let angle = airspeed_needle(MetersPerSecond(value)).degrees();
            assert!(
                angle.is_finite(),
                "airspeed {value} produced a non-finite needle angle"
            );
        }
        assert!(
            (airspeed_needle(MetersPerSecond(-10.0)).degrees() - AIRSPEED_START_DEGREES).abs()
                < 1e-6,
            "a negative airspeed should read zero"
        );
    }

    // --- 高度計 ---

    #[test]
    fn the_long_hand_makes_one_turn_per_thousand_feet() {
        // **実機の規約。** ここを外すと高度が読めない。
        let zero = altitude_hundreds_needle(Feet(0.0).to_meters()).degrees();
        let thousand = altitude_hundreds_needle(Feet(1000.0).to_meters()).degrees();
        assert!(
            angular_difference(zero, thousand).abs() < 0.5,
            "1000 ft should bring the long hand back to the top: {zero} vs {thousand}"
        );

        // 250 ft は 1/4 周。
        let quarter = altitude_hundreds_needle(Feet(250.0).to_meters()).degrees();
        assert!(
            angular_difference(quarter, 90.0).abs() < 0.5,
            "250 ft should read 90 deg, got {quarter}"
        );
    }

    #[test]
    fn the_short_hand_makes_one_turn_per_ten_thousand_feet() {
        let quarter = altitude_thousands_needle(Feet(2500.0).to_meters()).degrees();
        assert!(
            angular_difference(quarter, 90.0).abs() < 0.5,
            "2500 ft should put the short hand at 90 deg, got {quarter}"
        );
    }

    #[test]
    fn the_two_hands_agree_at_a_readable_altitude() {
        // 3200 ft: 短針は 3 と 4 の間、長針は 2（=720°/1000ft の 200 ft）。
        let feet = 3200.0;
        let long = altitude_hundreds_needle(Feet(feet).to_meters()).degrees();
        let short = altitude_thousands_needle(Feet(feet).to_meters()).degrees();
        assert!(
            angular_difference(long, 72.0).abs() < 0.5,
            "the long hand should read 200 ft (72 deg), got {long}"
        );
        assert!(
            (115.0..=117.0).contains(&short),
            "the short hand should sit past 3000 ft, got {short}"
        );
    }

    #[test]
    fn a_broken_altitude_does_not_move_the_hands() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let long = altitude_hundreds_needle(Meters(value)).degrees();
            let short = altitude_thousands_needle(Meters(value)).degrees();
            assert!(long.is_finite() && short.is_finite());
            assert!(long.abs() < 1e-6 && short.abs() < 1e-6, "should read zero");
        }
    }

    // --- 昇降計 ---

    #[test]
    fn level_flight_puts_the_vsi_needle_at_the_top() {
        let level = vertical_speed_needle(MetersPerSecond(0.0)).degrees();
        assert!(
            level.abs() < 1e-6,
            "level flight should read 0, got {level}"
        );
    }

    #[test]
    fn climbing_swings_the_vsi_needle_clockwise() {
        // 上昇が右、降下が左。逆にすると降下中に上昇と読める。
        let climb = vertical_speed_needle(FeetPerMinute(1000.0).to_meters_per_second());
        let descent = vertical_speed_needle(FeetPerMinute(-1000.0).to_meters_per_second());
        assert!(
            angular_difference(climb.degrees(), 85.0).abs() < 1.0,
            "1000 fpm up should read about 85 deg, got {}",
            climb.degrees()
        );
        assert!(
            angular_difference(descent.degrees(), -85.0).abs() < 1.0,
            "1000 fpm down should read about -85 deg, got {}",
            descent.degrees()
        );
    }

    #[test]
    fn an_extreme_sink_rate_pins_the_vsi() {
        let pinned = vertical_speed_needle(FeetPerMinute(-9000.0).to_meters_per_second());
        assert!(
            angular_difference(pinned.degrees(), -VSI_SWEEP_DEGREES).abs() < 1e-6,
            "the VSI should pin at full deflection"
        );
    }

    // --- 方位計 ---

    #[test]
    fn the_heading_card_turns_opposite_to_the_aircraft() {
        // 機首方位を真上に出すので、環は逆に回る。
        let card = heading_card_rotation(Degrees(90.0).to_radians()).degrees();
        assert!(
            angular_difference(card, -90.0).abs() < 1e-6,
            "a 090 heading should rotate the card to -90, got {card}"
        );
    }

    #[test]
    fn the_heading_card_does_not_spin_backwards_across_north() {
        // **359 度 → 1 度で 358 度分を逆回転しないこと。**
        // 一周またぎで環が高速に逆回転すると、方位が読めなくなる。
        let before = heading_card_rotation(Degrees(359.0).to_radians()).degrees();
        let after = heading_card_rotation(Degrees(1.0).to_radians()).degrees();
        let step = angular_difference(after, before);
        assert!(
            step.abs() < 5.0,
            "crossing north moved the card by {step} deg, expected about 2"
        );
    }

    #[test]
    fn a_negative_heading_is_normalised() {
        let card = heading_card_rotation(Degrees(-90.0).to_radians()).degrees();
        let expected = heading_card_rotation(Degrees(270.0).to_radians()).degrees();
        assert!(angular_difference(card, expected).abs() < 1e-6);
    }

    // --- 姿勢儀 ---

    #[test]
    fn banking_right_tilts_the_horizon_left() {
        // **外を見たときの見え方。** 機体が右へ傾くと地平線は左下がり。
        // ここを同符号にすると、旋回のたびに逆に傾いて酔う。
        let placement = horizon_placement(Radians::ZERO, Degrees(20.0).to_radians());
        assert!(
            angular_difference(placement.roll.degrees(), -20.0).abs() < 1e-6,
            "a 20 deg right bank should tilt the horizon -20, got {}",
            placement.roll.degrees()
        );
    }

    #[test]
    fn pitching_up_moves_the_horizon_down() {
        let up = horizon_placement(Degrees(10.0).to_radians(), Radians::ZERO);
        let down = horizon_placement(Degrees(-10.0).to_radians(), Radians::ZERO);
        assert!(
            up.offset > 0.0 && down.offset < 0.0,
            "the horizon should move opposite the nose: up {} down {}",
            up.offset,
            down.offset
        );
        assert!(
            (up.offset + down.offset).abs() < 1e-4,
            "the movement should be symmetric"
        );
    }

    #[test]
    fn an_extreme_attitude_keeps_the_horizon_on_the_dial() {
        // 背面や急降下でも盤面から飛び出さないこと。
        for pitch in [-90.0, -60.0, 60.0, 90.0] {
            let placement = horizon_placement(Degrees(pitch).to_radians(), Radians::ZERO);
            assert!(
                placement.offset.abs() <= DIAL_SIZE * 0.5,
                "at {pitch} deg the horizon left the dial: {}",
                placement.offset
            );
        }
    }

    #[test]
    fn a_broken_attitude_centres_the_horizon() {
        let placement = horizon_placement(Radians(f64::NAN), Radians(f64::INFINITY));
        assert!(placement.offset.is_finite() && placement.offset.abs() < 1e-6);
        assert!(placement.roll.degrees().is_finite());
    }

    // --- 計器の照明 ---

    #[test]
    fn the_panel_is_unlit_in_daylight() {
        // 昼は外光で読める。点けると盤面が白飛びして逆に読みにくい。
        for degrees in [1.0, 20.0, 78.0] {
            let fraction = panel_light_fraction(Degrees(degrees).to_radians());
            assert!(
                fraction.abs() < 1e-6,
                "the panel should stay unlit at {degrees} deg, got {fraction}"
            );
        }
    }

    #[test]
    fn the_panel_is_fully_lit_at_night() {
        for degrees in [-6.0, -15.0, -40.0] {
            let fraction = panel_light_fraction(Degrees(degrees).to_radians());
            assert!(
                (fraction - 1.0).abs() < 1e-6,
                "the panel should be fully lit at {degrees} deg, got {fraction}"
            );
        }
    }

    #[test]
    fn the_panel_lights_later_than_the_runway() {
        // 滑走路灯は +3 度から点き始める。**盤面はもっと暗くなってから**で
        // よい（外光で読めるうちに点けると白飛びする）。
        let at_sunset = panel_light_fraction(Degrees(0.0).to_radians());
        assert!(
            at_sunset.abs() < 1e-6,
            "the panel should still be dark at sunset, got {at_sunset}"
        );
        let just_after = panel_light_fraction(Degrees(-1.0).to_radians());
        assert!(
            just_after > 0.0,
            "the panel should start lighting just after sunset, got {just_after}"
        );
    }

    #[test]
    fn the_panel_light_rises_smoothly() {
        let mut previous = 0.0_f32;
        let mut degrees = 2.0;
        while degrees >= -10.0 {
            let fraction = panel_light_fraction(Degrees(degrees).to_radians());
            assert!(
                fraction >= previous - 1e-6,
                "the panel dimmed as the sun set: {previous} then {fraction}"
            );
            assert!(
                fraction - previous < 0.2,
                "the panel light jumped by {} at {degrees} deg",
                fraction - previous
            );
            previous = fraction;
            degrees -= 0.25;
        }
        assert!((previous - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_broken_sun_angle_leaves_the_panel_unlit() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let fraction = panel_light_fraction(Radians(value));
            assert!(
                fraction.is_finite() && (0.0..=1.0).contains(&fraction),
                "a broken sun angle produced {fraction}"
            );
        }
    }

    #[test]
    fn the_lit_face_brightens_without_losing_opacity() {
        // 透けると背景の地形が盤面越しに見えて読めなくなる。
        let dark = lit_dial_face(0.0).to_linear();
        let bright = lit_dial_face(1.0).to_linear();
        assert!(
            bright.red > dark.red,
            "the lit face should be brighter: {dark:?} -> {bright:?}"
        );
        assert!(
            bright.alpha >= dark.alpha && bright.alpha > 0.8,
            "the face must stay opaque enough to read, got {}",
            bright.alpha
        );
    }

    #[test]
    fn a_broken_fraction_falls_back_to_the_dark_face() {
        for fraction in [f32::NAN, f32::INFINITY, -1.0, 9.0] {
            let color = lit_dial_face(fraction).to_linear();
            assert!(
                color.red.is_finite() && color.alpha.is_finite(),
                "fraction {fraction} produced {color:?}"
            );
        }
        let broken = lit_dial_face(f32::NAN).to_linear();
        let dark = lit_dial_face(0.0).to_linear();
        assert!((broken.red - dark.red).abs() < 1e-6);
    }

    // --- 表示 ---

    #[test]
    fn every_readout_is_ascii() {
        // 既定フォントに字形が無い記号は豆腐になる（実機で踏んだ）。
        let readout = HudReadout {
            airspeed: Knots(97.0),
            altitude: Feet(3200.0),
            vertical_speed: FeetPerMinute(-450.0),
            heading_degrees: 51.0,
            pitch_degrees: -2.5,
            roll_degrees: 12.0,
            throttle: 75.0,
            flaps: 100.0,
        };
        for instrument in Instrument::all() {
            let text = instrument_readout(instrument, &readout);
            assert!(text.is_ascii(), "{instrument:?} produced {text:?}");
            assert!(instrument.label().is_ascii());
        }
    }

    #[test]
    fn the_readout_converts_units_once() {
        // 100 kt = 51.444 m/s、1000 ft = 304.8 m。既知の換算値と突き合わせる。
        let state = crate::HudState {
            airspeed: MetersPerSecond(51.444),
            altitude: Meters(304.8),
            vertical_speed: MetersPerSecond(2.54),
            heading: Degrees(-10.0).to_radians(),
            throttle: 0.75,
            flaps: 1.0,
            ..crate::HudState::default()
        };
        let readout = HudReadout::from_state(&state);
        assert!((readout.airspeed.get() - 100.0).abs() < 0.1);
        assert!((readout.altitude.get() - 1000.0).abs() < 0.1);
        assert!((readout.vertical_speed.get() - 500.0).abs() < 1.0);
        // 方位は 0..360 へ正規化される。
        assert!((readout.heading_degrees - 350.0).abs() < 0.1);
        assert!((readout.throttle - 75.0).abs() < 1e-9);
    }

    #[test]
    fn a_broken_state_does_not_reach_the_readout() {
        let state = crate::HudState {
            airspeed: MetersPerSecond(f64::NAN),
            altitude: Meters(f64::INFINITY),
            heading: Radians(f64::NAN),
            throttle: f64::NAN,
            ..crate::HudState::default()
        };
        let readout = HudReadout::from_state(&state);
        assert!(readout.heading_degrees.is_finite());
        assert!(readout.throttle.is_finite());
        for instrument in Instrument::all() {
            let text = instrument_readout(instrument, &readout);
            assert!(!text.contains("NaN"), "{instrument:?}: {text}");
        }
    }

    #[test]
    fn the_panel_has_the_six_standard_instruments() {
        let all = Instrument::all();
        assert_eq!(all.len(), INSTRUMENT_COUNT);
        // 重複が無いこと。
        for (index, instrument) in all.iter().enumerate() {
            assert!(
                !all[..index].contains(instrument),
                "{instrument:?} appears twice"
            );
        }
    }
}
