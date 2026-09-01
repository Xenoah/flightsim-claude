//! 一時停止の表示。
//!
//! **止まっていることと、そこから何ができるかを同時に出す。** 「PAUSED」
//! だけだと、再開の方法を探して結局ウィンドウを閉じることになる。

use bevy::prelude::*;

/// 一時停止しているか。app が `Esc` で切り替える。
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Paused(pub bool);

impl Paused {
    /// 切り替える。
    pub const fn toggle(&mut self) {
        self.0 = !self.0;
    }

    /// 止まっているか。
    #[must_use]
    pub const fn is_paused(self) -> bool {
        self.0
    }
}

/// 一時停止表示の印。
#[derive(Component, Debug)]
pub struct PauseOverlay;

/// 画面中央に一時停止を出す。
///
/// 中央に置くのは、**見落とされては困る**から。止まっているのに気付かず
/// 操縦桿を動かして「反応しない」と判断されるのが最悪の筋。
pub fn spawn_pause_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(pause_text()),
        TextFont {
            font_size: 22.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 1.0, 1.0)),
        TextLayout::new_with_justify(Justify::Center),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(40.0),
            left: Val::Percent(25.0),
            right: Val::Percent(25.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
            ..default()
        },
        Visibility::Hidden,
        PauseOverlay,
    ));
}

/// 表示を状態に合わせる。
pub fn update_pause_overlay(
    paused: Res<Paused>,
    mut query: Query<&mut Visibility, With<PauseOverlay>>,
) {
    for mut visibility in &mut query {
        *visibility = if paused.is_paused() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// 一時停止中に出す文言。
///
/// **ASCII のみ。** 既定フォントに字形が無い記号は豆腐になる。
#[must_use]
pub fn pause_text() -> String {
    [
        "PAUSED",
        "",
        "Esc ... resume",
        "R ..... restart this flight",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_says_how_to_get_out_of_it() {
        // **逃げ道を書かないと、閉じるしかなくなる。**
        let text = pause_text();
        assert!(text.contains("PAUSED"));
        assert!(text.contains("Esc"), "the way to resume must be on screen");
        assert!(text.contains('R'), "the way to restart must be on screen");
    }

    #[test]
    fn the_overlay_is_ascii() {
        assert!(pause_text().is_ascii());
    }

    #[test]
    fn pausing_toggles_both_ways() {
        let mut paused = Paused::default();
        assert!(!paused.is_paused(), "a flight does not start paused");
        paused.toggle();
        assert!(paused.is_paused());
        paused.toggle();
        assert!(!paused.is_paused());
    }
}
