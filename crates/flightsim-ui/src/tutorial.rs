//! チュートリアル導線。
//!
//! **初見のプレイヤーは離陸できない。** これがこのジャンル最大の離脱要因。
//! 左下に固定の操作説明を出すだけでは「今なにをすべきか」が伝わらないので、
//! 現在の飛行状況を見て「次にやる操作」を画面中央上に指し示す。
//!
//! 状態機械（[`TutorialStage`] / [`TutorialProgress`]）は Bevy に一切依存しない
//! 純粋なデータと関数にしてあり、`HudState` だけを見て単体テストできる。
//! Bevy 配線（[`spawn_tutorial_prompt`] / [`update_tutorial_prompt`]）はその
//! 薄いラッパー。
//!
//! ## 設計方針
//!
//! - **段階は `HudState` だけから決める。** 滑走路位置のような追加情報には
//!   依存しない。`flightsim-ui` は `flightsim-sim` に依存しない（依存は一方向）
//!   ので、この境界を守ると後で配線を足すだけで済む。
//! - **ヒステリシスを入れる。** 閾値ちょうどで数値が揺れても段階がバタつかない
//!   よう、進む方向と戻る方向で別の閾値を使う（Schmitt トリガ）。
//! - **戻る遷移がある。** 場周高度に達する前に沈み始めたら（＝離陸直後の
//!   トラブル）、「上昇を続けろ」ではなく降下段階の案内へ直接切り替える。
//! - **完了したら二度と出ない。** 一度でも接地（`log.landings > 0`）したら
//!   [`TutorialStage::Complete`] に固定する。以後どんな `HudState` が来ても
//!   後戻りしない。**上級者の邪魔をしないのが最優先。**

use bevy::prelude::*;
use flightsim_core::{Feet, Knots, Meters, MetersPerSecond};

use crate::HudState;

// ---------------------------------------------------------------------------
// 段階
// ---------------------------------------------------------------------------

/// チュートリアルの段階。
///
/// 想定する流れ: `Parked` → `Accelerate` → `Rotate` → `Climb` → `Circuit` →
/// `Descend` → `Approach` → `Complete`。ただし [`TutorialProgress::update`] は
/// 現在の `HudState` から直接段階を導くので、この順序を飛ばしたり戻ったり
/// しても（旋回を早めに切り上げた、着陸復行した、等）詰まらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TutorialStage {
    /// 滑走路上で停止中。スロットルを上げるよう促す。
    Parked,
    /// 滑走路を加速中。まだ浮くほどの速度ではない。
    Accelerate,
    /// 離陸速度に達した。機首を上げて離陸するよう促す。
    Rotate,
    /// 上昇中。まだ場周高度に達していない。
    Climb,
    /// 場周高度に達した。旋回して戻るよう促す。
    Circuit,
    /// 滑走路へ戻る降下中。
    Descend,
    /// 最終進入。接地間近。
    Approach,
    /// 一度着陸した。以後は何も表示しない。
    Complete,
}

impl TutorialStage {
    /// 画面中央上に出す文言。見出し行 + 具体的な操作の 1 行。
    ///
    /// [`Self::Complete`] は表示しない段階なので空文字列を返す
    /// （呼び出し側は表示前に `!= Complete` を見る契約）。
    #[must_use]
    pub const fn prompt(self) -> &'static str {
        match self {
            Self::Parked => "TAKE OFF\nPress PageUp to open the throttle.",
            Self::Accelerate => "ACCELERATING\nHold the runway heading straight ahead.",
            Self::Rotate => "ROTATE\nHold S to raise the nose and lift off.",
            Self::Climb => "CLIMBING\nHold a steady climb with S.",
            Self::Circuit => "PATTERN ALTITUDE\nTurn back toward the runway with A/D.",
            Self::Descend => "HEADING BACK\nEase off S and reduce throttle with PageDown.",
            Self::Approach => "FINAL APPROACH\nLine up on the runway and ease the throttle back.",
            Self::Complete => "",
        }
    }
}

// ---------------------------------------------------------------------------
// 閾値（ヒステリシス: 進む方向と戻る方向で別の値を使う）
// ---------------------------------------------------------------------------

/// 離陸速度の目安。既存の操作説明（[`crate::help_text`] の
/// "hold S at about 60 kt"）と一致させてある。二重に指定して片方だけ
/// 直される事故を防げないのが弱点だが、値そのものは合わせておく。
const ROTATE_SPEED_ENTER: Knots = Knots(60.0);

/// 離陸速度を割り込んだと判定する下限。ヒステリシス用に少し低く取る。
const ROTATE_SPEED_EXIT: Knots = Knots(50.0);

/// スロットルのヒステリシス閾値（`HudState::throttle` は 0..1 の正規化値）。
const THROTTLE_ENTER: f64 = 0.5;
const THROTTLE_EXIT: f64 = 0.1;

/// 場周高度。上がるときは 800 ft、そこから外れる基準は 600 ft。
const PATTERN_ALTITUDE_ENTER: Feet = Feet(800.0);
const PATTERN_ALTITUDE_EXIT: Feet = Feet(600.0);

/// 最終進入とみなす高度。入るときは 200 ft、抜けるときは 300 ft。
const APPROACH_ENTER: Feet = Feet(200.0);
const APPROACH_EXIT: Feet = Feet(300.0);

/// 場周高度に達する前に沈み始めたと判定する昇降率
/// （離陸直後のトラブルを「戻る遷移」で拾うため）。
const EARLY_SINK_ENTER: MetersPerSecond = MetersPerSecond(-1.0);
const EARLY_SINK_EXIT: MetersPerSecond = MetersPerSecond(0.5);

// ---------------------------------------------------------------------------
// 分類（純関数）
// ---------------------------------------------------------------------------

/// 地上にいる間の段階を決める。
///
/// `current` を見て閾値を切り替えるのがヒステリシスの本体。すでに
/// `Rotate` にいるなら離陸速度を大きく割り込むまで `Accelerate` へ戻さず、
/// すでに `Accelerate`（または `Rotate`）にいるならスロットルがほぼ
/// アイドルに戻るまで `Parked` へ戻さない。
fn classify_on_ground(
    current: TutorialStage,
    throttle: f64,
    airspeed: MetersPerSecond,
) -> TutorialStage {
    let in_rotate = current == TutorialStage::Rotate;
    let rotate_bound = if in_rotate {
        ROTATE_SPEED_EXIT
    } else {
        ROTATE_SPEED_ENTER
    }
    .to_meters_per_second();

    if airspeed >= rotate_bound {
        return TutorialStage::Rotate;
    }

    let in_accelerate_or_above =
        matches!(current, TutorialStage::Accelerate | TutorialStage::Rotate);
    let throttle_bound = if in_accelerate_or_above {
        THROTTLE_EXIT
    } else {
        THROTTLE_ENTER
    };

    if throttle >= throttle_bound {
        TutorialStage::Accelerate
    } else {
        TutorialStage::Parked
    }
}

/// 直近の段階を踏まえた最終進入の判定境界。
fn approach_bound(current: TutorialStage) -> Meters {
    if current == TutorialStage::Approach {
        APPROACH_EXIT
    } else {
        APPROACH_ENTER
    }
    .to_meters()
}

/// 直近の段階を踏まえた場周高度の判定境界。
fn circuit_bound(current: TutorialStage) -> Meters {
    if current == TutorialStage::Circuit {
        PATTERN_ALTITUDE_EXIT
    } else {
        PATTERN_ALTITUDE_ENTER
    }
    .to_meters()
}

/// 空中にいる間の段階を決める。
///
/// `reached_pattern_altitude` は [`TutorialProgress`] が持つ一方向のラッチ。
///
/// **場周高度に達する前は、高度だけで `Approach` を判定しない。** 離陸直後は
/// 高度が低くて当然で、それは最終進入ではない。上昇中はそのまま `Climb`。
/// 沈み始めたら（`vertical_speed` が十分に負）`Circuit` を経由せず直接
/// `Descend`／`Approach` へ案内する。**「上昇を続けろ」と言い続けるより、
/// 実際に起きていること（降りている）に合わせた案内のほうが正しい。**
fn classify_airborne(
    current: TutorialStage,
    agl: Meters,
    vertical_speed: MetersPerSecond,
    reached_pattern_altitude: bool,
) -> TutorialStage {
    if !reached_pattern_altitude {
        let was_descending = matches!(current, TutorialStage::Descend | TutorialStage::Approach);
        if was_descending {
            if vertical_speed >= EARLY_SINK_EXIT {
                return TutorialStage::Climb;
            }
            return if agl <= approach_bound(current) {
                TutorialStage::Approach
            } else {
                TutorialStage::Descend
            };
        }
        return if vertical_speed <= EARLY_SINK_ENTER {
            TutorialStage::Descend
        } else {
            TutorialStage::Climb
        };
    }

    if agl <= approach_bound(current) {
        return TutorialStage::Approach;
    }

    if agl >= circuit_bound(current) {
        TutorialStage::Circuit
    } else {
        TutorialStage::Descend
    }
}

// ---------------------------------------------------------------------------
// 進行状態
// ---------------------------------------------------------------------------

/// チュートリアルの進行状態。フレームをまたいで保持する必要がある分だけ持つ。
///
/// Bevy に依存しない。`Default` は「駐機中から開始」を表す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TutorialProgress {
    stage: TutorialStage,
    /// 一度でも場周高度に達したか。**一方向のラッチ。** 立った後は
    /// 高度が落ちても `Climb` へは戻らない（[`classify_airborne`] 参照）。
    reached_pattern_altitude: bool,
    /// 一度でも接地したか。**一方向のラッチ。** 立ったら以後どんな
    /// `HudState` が来ても [`TutorialStage::Complete`] のまま。
    completed: bool,
}

impl Default for TutorialProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl TutorialProgress {
    /// 駐機中から開始する初期状態。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stage: TutorialStage::Parked,
            reached_pattern_altitude: false,
            completed: false,
        }
    }

    /// 現在の段階。
    #[must_use]
    pub const fn stage(&self) -> TutorialStage {
        self.stage
    }

    /// 1 フレーム分、`HudState` を見て段階を更新する。戻り値は更新後の段階。
    ///
    /// **NaN/Inf が混ざったフレームは段階を変えない。** 変なフレーム 1 枚の
    /// ために案内がおかしな段階へ飛ぶより、直前の段階のまま次のフレームへ
    /// 流すほうが安全（`f64::clamp` が NaN を素通りさせる、という
    /// CLAUDE.md の既知の地雷と同じ理由でガードしてある）。
    pub fn update(&mut self, hud: &HudState) -> TutorialStage {
        if self.completed {
            return TutorialStage::Complete;
        }
        if hud.log.landings > 0 {
            self.completed = true;
            self.stage = TutorialStage::Complete;
            return self.stage;
        }

        if hud.on_ground && hud.throttle.is_finite() && hud.airspeed.is_finite() {
            self.stage = classify_on_ground(self.stage, hud.throttle, hud.airspeed);
        } else if !hud.on_ground && hud.agl.is_finite() && hud.vertical_speed.is_finite() {
            if hud.agl >= PATTERN_ALTITUDE_ENTER.to_meters() {
                self.reached_pattern_altitude = true;
            }
            self.stage = classify_airborne(
                self.stage,
                hud.agl,
                hud.vertical_speed,
                self.reached_pattern_altitude,
            );
        }

        self.stage
    }
}

// ---------------------------------------------------------------------------
// Bevy 配線
// ---------------------------------------------------------------------------

/// チュートリアル表示の on/off フラグ。
///
/// **実際のキー割り当ては input 担当の仕事。** ここではリソースだけを
/// 公開しておき、`H` などのキー入力が来たら [`TutorialVisibility::toggle`]
/// を呼べば繋がるようにしてある。既定は表示（`true`）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TutorialVisibility(pub bool);

impl Default for TutorialVisibility {
    fn default() -> Self {
        Self(true)
    }
}

impl TutorialVisibility {
    /// 表示・非表示を反転する。
    pub fn toggle(&mut self) {
        self.0 = !self.0;
    }
}

/// 進行状態のリソース版。
///
/// フィールドを非公開にして [`TutorialProgress`] のカプセル化を保ちつつ、
/// `ResMut<TutorialState>` として system の引数に取れるよう型だけ公開する
/// （`LandingReportState` と同じ考え方）。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct TutorialState(TutorialProgress);

impl TutorialState {
    /// 現在の段階。
    #[must_use]
    pub const fn stage(&self) -> TutorialStage {
        self.0.stage()
    }
}

/// チュートリアル案内テキストにつける印。
#[derive(Component, Debug, Clone, Copy)]
pub struct TutorialPrompt;

/// チュートリアル表示欄を作る。
///
/// 既存の HUD（左上の計器・左下の操作説明・右上の着陸評価・右下の飛行記録）
/// のどれとも重ならないよう、画面上部に横幅いっぱいの透明なコンテナを置き、
/// その中でテキストだけを中央寄せする。左右の端に置く既存要素とは
/// 水平方向で重ならず、`top` も左上計器列の開始位置より下げてあるので
/// 縦方向でも視線の導線が別れる。
pub fn spawn_tutorial_prompt(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(56.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                Text::new(""),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.95, 0.55)),
                TextLayout::new_with_justify(Justify::Center),
                Visibility::Hidden,
                TutorialPrompt,
            ));
        });
}

/// チュートリアル表示を更新する。
///
/// 段階の判定は非表示中も進める。**そうしないと、非表示にしている間に
/// 完了しても、次に表示を戻したときに古い段階の案内が一瞬出てしまう。**
pub fn update_tutorial_prompt(
    hud: Res<HudState>,
    visibility: Res<TutorialVisibility>,
    mut state: ResMut<TutorialState>,
    mut query: Query<(&mut Text, &mut Visibility), With<TutorialPrompt>>,
) {
    let stage = state.0.update(&hud);
    let show = visibility.0 && stage != TutorialStage::Complete;

    for (mut text, mut node_visibility) in &mut query {
        if show {
            **text = stage.prompt().to_owned();
            *node_visibility = Visibility::Visible;
        } else {
            *node_visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Radians;

    fn hud(on_ground: bool, throttle: f64, airspeed_kt: f64, agl_ft: f64, vs_fpm: f64) -> HudState {
        HudState {
            airspeed: flightsim_core::Knots(airspeed_kt).to_meters_per_second(),
            altitude: Meters(0.0),
            agl: flightsim_core::Feet(agl_ft).to_meters(),
            vertical_speed: flightsim_core::FeetPerMinute(vs_fpm).to_meters_per_second(),
            heading: Radians(0.0),
            pitch: Radians(0.0),
            roll: Radians(0.0),
            throttle,
            flaps: 0.0,
            on_ground,
            terrain_available: true,
            view_mode: "CHASE",
            wind_from: Radians(0.0),
            wind_speed: MetersPerSecond(0.0),
            log: crate::FlightSummary::default(),
        }
    }

    fn landed(mut base: HudState) -> HudState {
        base.log.landings = 1;
        base
    }

    // --- 初期状態 ---

    #[test]
    fn a_fresh_flight_starts_parked() {
        let progress = TutorialProgress::new();
        assert_eq!(progress.stage(), TutorialStage::Parked);
    }

    #[test]
    fn idling_on_the_ground_stays_parked() {
        let mut progress = TutorialProgress::new();
        for _ in 0..10 {
            let stage = progress.update(&hud(true, 0.0, 0.0, 0.0, 0.0));
            assert_eq!(stage, TutorialStage::Parked);
        }
    }

    // --- 地上: スロットル ---

    #[test]
    fn opening_the_throttle_moves_past_parked() {
        let mut progress = TutorialProgress::new();
        let stage = progress.update(&hud(true, THROTTLE_ENTER, 0.0, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Accelerate);
    }

    #[test]
    fn a_throttle_just_under_the_threshold_stays_parked() {
        let mut progress = TutorialProgress::new();
        let stage = progress.update(&hud(true, THROTTLE_ENTER - 0.001, 0.0, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Parked);
    }

    #[test]
    fn throttle_does_not_flicker_between_parked_and_accelerate() {
        // 0.5 の付近を往復しても、一度 Accelerate に入ったら 0.1 を
        // 割り込むまでは Parked に戻らない（ヒステリシス）。
        let mut progress = TutorialProgress::new();
        assert_eq!(
            progress.update(&hud(true, 0.6, 0.0, 0.0, 0.0)),
            TutorialStage::Accelerate
        );
        for throttle in [0.45, 0.55, 0.3, 0.6, 0.2] {
            assert_eq!(
                progress.update(&hud(true, throttle, 0.0, 0.0, 0.0)),
                TutorialStage::Accelerate,
                "throttle {throttle} bounced the stage"
            );
        }
        // 実際にアイドルへ戻せば Parked に戻る。
        assert_eq!(
            progress.update(&hud(true, 0.05, 0.0, 0.0, 0.0)),
            TutorialStage::Parked
        );
    }

    // --- 地上: 離陸速度 ---

    #[test]
    fn reaching_rotate_speed_prompts_rotation() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(true, 1.0, 0.0, 0.0, 0.0));
        let stage = progress.update(&hud(true, 1.0, ROTATE_SPEED_ENTER.0, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Rotate);
    }

    #[test]
    fn a_speed_just_under_rotate_stays_accelerating() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(true, 1.0, 0.0, 0.0, 0.0));
        let stage = progress.update(&hud(true, 1.0, ROTATE_SPEED_ENTER.0 - 0.5, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Accelerate);
    }

    #[test]
    fn rotate_speed_does_not_flicker_near_the_threshold() {
        // 55〜65 kt を往復しても、一度 Rotate に入ったら 50 kt を
        // 割り込むまでは Accelerate に戻らない。
        let mut progress = TutorialProgress::new();
        progress.update(&hud(true, 1.0, 62.0, 0.0, 0.0));
        assert_eq!(progress.stage(), TutorialStage::Rotate);
        for speed in [58.0, 65.0, 55.0, 61.0] {
            assert_eq!(
                progress.update(&hud(true, 1.0, speed, 0.0, 0.0)),
                TutorialStage::Rotate,
                "speed {speed} kt bounced the stage"
            );
        }
        let stage = progress.update(&hud(true, 1.0, 45.0, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Accelerate);
    }

    // --- 離陸 ---

    #[test]
    fn lifting_off_moves_to_climb() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(true, 1.0, 65.0, 0.0, 0.0));
        let stage = progress.update(&hud(false, 1.0, 65.0, 20.0, 300.0));
        assert_eq!(stage, TutorialStage::Climb);
    }

    // --- 空中: 場周高度 ---

    #[test]
    fn reaching_pattern_altitude_prompts_the_turn() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, 100.0, 500.0));
        let stage = progress.update(&hud(false, 1.0, 65.0, PATTERN_ALTITUDE_ENTER.0, 0.0));
        assert_eq!(stage, TutorialStage::Circuit);
    }

    #[test]
    fn pattern_altitude_does_not_flicker_near_the_threshold() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, 900.0, 0.0));
        assert_eq!(progress.stage(), TutorialStage::Circuit);
        for altitude in [650.0, 750.0, 700.0, 620.0] {
            assert_eq!(
                progress.update(&hud(false, 1.0, 65.0, altitude, 0.0)),
                TutorialStage::Circuit,
                "altitude {altitude} ft bounced the stage"
            );
        }
        let stage = progress.update(&hud(false, 1.0, 65.0, 550.0, -400.0));
        assert_eq!(stage, TutorialStage::Descend);
    }

    // --- 戻る遷移: 場周高度に達する前に沈み始める ---

    #[test]
    fn sinking_before_reaching_pattern_altitude_goes_straight_to_descend() {
        // 離陸直後のトラブルを想定: 300 ft までしか上がらずに沈み始めた。
        // 「上昇を続けろ」ではなく降下段階の案内へ切り替わること。
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, 300.0, 200.0));
        assert_eq!(progress.stage(), TutorialStage::Climb);
        let stage = progress.update(&hud(false, 0.5, 60.0, 280.0, -250.0));
        assert_eq!(stage, TutorialStage::Descend);
    }

    #[test]
    fn recovering_climb_before_landing_returns_to_climb() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, 300.0, -250.0));
        assert_eq!(progress.stage(), TutorialStage::Descend);
        // 電源を入れ直して上昇に戻せば Climb に戻る。
        let stage = progress.update(&hud(false, 1.0, 65.0, 320.0, 150.0));
        assert_eq!(stage, TutorialStage::Climb);
    }

    // --- 最終進入 ---

    #[test]
    fn descending_below_the_approach_height_prompts_final() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, PATTERN_ALTITUDE_ENTER.0, 0.0));
        progress.update(&hud(false, 0.3, 60.0, 400.0, -300.0));
        let stage = progress.update(&hud(false, 0.2, 55.0, APPROACH_ENTER.0, -200.0));
        assert_eq!(stage, TutorialStage::Approach);
    }

    #[test]
    fn approach_does_not_flicker_near_the_threshold() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 1.0, 65.0, PATTERN_ALTITUDE_ENTER.0, 0.0));
        progress.update(&hud(false, 0.2, 55.0, 150.0, -200.0));
        assert_eq!(progress.stage(), TutorialStage::Approach);
        for altitude in [250.0, 280.0, 220.0, 260.0] {
            assert_eq!(
                progress.update(&hud(false, 0.2, 55.0, altitude, -200.0)),
                TutorialStage::Approach,
                "altitude {altitude} ft bounced the stage"
            );
        }
        // 実際に離脱すれば（300 ft を超えれば）Descend に戻る。
        let stage = progress.update(&hud(false, 0.2, 55.0, 350.0, -200.0));
        assert_eq!(stage, TutorialStage::Descend);
    }

    // --- 着陸で完了、以後は不変 ---

    #[test]
    fn touching_down_completes_the_tutorial() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(false, 0.2, 55.0, 50.0, -200.0));
        let stage = progress.update(&landed(hud(true, 0.0, 30.0, 0.0, 0.0)));
        assert_eq!(stage, TutorialStage::Complete);
    }

    #[test]
    fn completion_is_permanent_even_after_taking_off_again() {
        let mut progress = TutorialProgress::new();
        progress.update(&landed(hud(true, 0.0, 30.0, 0.0, 0.0)));
        assert_eq!(progress.stage(), TutorialStage::Complete);

        // タッチアンドゴーで再度上がっても、Complete のまま。
        let mut still_landed_state = hud(false, 1.0, 65.0, 500.0, 400.0);
        still_landed_state.log.landings = 1;
        let stage = progress.update(&still_landed_state);
        assert_eq!(stage, TutorialStage::Complete);
    }

    // --- 想定外の入力で詰まらない ---

    #[test]
    fn non_finite_hud_values_do_not_panic_or_change_the_stage() {
        let mut progress = TutorialProgress::new();
        progress.update(&hud(true, 0.6, 0.0, 0.0, 0.0));
        assert_eq!(progress.stage(), TutorialStage::Accelerate);

        let mut broken = hud(true, f64::NAN, 0.0, 0.0, 0.0);
        broken.airspeed = MetersPerSecond(f64::NAN);
        let stage = progress.update(&broken);
        assert_eq!(
            stage,
            TutorialStage::Accelerate,
            "a NaN frame moved the stage"
        );

        let mut broken_air = hud(false, 1.0, 65.0, f64::NAN, f64::INFINITY);
        broken_air.agl = Meters(f64::NAN);
        broken_air.vertical_speed = MetersPerSecond(f64::INFINITY);
        // 空中に切り替わった直後の壊れたフレームでも panic しない。
        let _ = progress.update(&broken_air);
    }

    #[test]
    fn negative_dt_equivalents_cannot_get_the_state_machine_stuck() {
        // タイマーではなく HudState 駆動なので「時間」という概念は無いが、
        // 想定外の値（負の高度・負のスロットル）でも詰まらないことを確認する。
        let mut progress = TutorialProgress::new();
        for _ in 0..1000 {
            progress.update(&hud(true, -1.0, -50.0, -10.0, -10.0));
        }
        assert_eq!(progress.stage(), TutorialStage::Parked);
        let stage = progress.update(&hud(true, 1.0, 65.0, 0.0, 0.0));
        assert_eq!(stage, TutorialStage::Rotate);
    }

    // --- 一気通貫の周回 ---

    #[test]
    fn a_full_circuit_visits_every_stage_in_order() {
        let mut progress = TutorialProgress::new();
        let mut seen = vec![progress.stage()];
        let push = |stage: TutorialStage, seen: &mut Vec<TutorialStage>| {
            if seen.last() != Some(&stage) {
                seen.push(stage);
            }
        };

        push(progress.update(&hud(true, 0.0, 0.0, 0.0, 0.0)), &mut seen);
        push(progress.update(&hud(true, 0.8, 10.0, 0.0, 0.0)), &mut seen);
        push(progress.update(&hud(true, 1.0, 65.0, 0.0, 0.0)), &mut seen);
        push(
            progress.update(&hud(false, 1.0, 65.0, 50.0, 500.0)),
            &mut seen,
        );
        push(
            progress.update(&hud(false, 1.0, 65.0, 850.0, 100.0)),
            &mut seen,
        );
        push(
            progress.update(&hud(false, 0.4, 60.0, 500.0, -400.0)),
            &mut seen,
        );
        push(
            progress.update(&hud(false, 0.2, 55.0, 100.0, -200.0)),
            &mut seen,
        );
        push(
            progress.update(&landed(hud(true, 0.0, 20.0, 0.0, 0.0))),
            &mut seen,
        );

        assert_eq!(
            seen,
            vec![
                TutorialStage::Parked,
                TutorialStage::Accelerate,
                TutorialStage::Rotate,
                TutorialStage::Climb,
                TutorialStage::Circuit,
                TutorialStage::Descend,
                TutorialStage::Approach,
                TutorialStage::Complete,
            ]
        );
    }

    // --- 表示の on/off ---

    #[test]
    fn visibility_defaults_to_shown() {
        assert_eq!(TutorialVisibility::default(), TutorialVisibility(true));
    }

    #[test]
    fn toggling_visibility_flips_it_back_and_forth() {
        let mut visibility = TutorialVisibility::default();
        visibility.toggle();
        assert_eq!(visibility, TutorialVisibility(false));
        visibility.toggle();
        assert_eq!(visibility, TutorialVisibility(true));
    }

    // --- 字形・文言 ---

    #[test]
    fn every_prompt_stays_ascii() {
        // 実機で `°` が豆腐になった前例がある（landing.rs）。同じ検査を通す。
        for stage in [
            TutorialStage::Parked,
            TutorialStage::Accelerate,
            TutorialStage::Rotate,
            TutorialStage::Climb,
            TutorialStage::Circuit,
            TutorialStage::Descend,
            TutorialStage::Approach,
            TutorialStage::Complete,
        ] {
            assert!(
                stage.prompt().is_ascii(),
                "a non-ASCII glyph reached the tutorial prompt for {stage:?}"
            );
        }
    }

    #[test]
    fn every_active_prompt_names_a_concrete_key() {
        // 「今なにをすべきか」だけでなく「どのキーか」を書くのが要件。
        let expectations: &[(TutorialStage, &str)] = &[
            (TutorialStage::Parked, "PageUp"),
            (TutorialStage::Rotate, "S"),
            (TutorialStage::Climb, "S"),
            (TutorialStage::Circuit, "A/D"),
            (TutorialStage::Descend, "PageDown"),
        ];
        for (stage, key) in expectations {
            assert!(
                stage.prompt().contains(key),
                "{stage:?} does not mention `{key}`: {}",
                stage.prompt()
            );
        }
    }

    #[test]
    fn active_prompts_are_at_most_two_lines() {
        for stage in [
            TutorialStage::Parked,
            TutorialStage::Accelerate,
            TutorialStage::Rotate,
            TutorialStage::Climb,
            TutorialStage::Circuit,
            TutorialStage::Descend,
            TutorialStage::Approach,
        ] {
            let lines = stage.prompt().lines().count();
            assert!(lines <= 2, "{stage:?} prompt has {lines} lines");
        }
    }

    #[test]
    fn the_completed_prompt_is_empty() {
        assert_eq!(TutorialStage::Complete.prompt(), "");
    }
}
