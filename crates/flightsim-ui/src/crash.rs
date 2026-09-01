//! 墜落の表示。
//!
//! **何が起きたか、なぜか、次に何ができるかを同時に出す。**
//! 「CRASHED」だけだと、操縦が悪かったのか不具合なのかが分からないまま
//! 画面が固まったように見える。

use bevy::prelude::*;

/// 墜落の表示に必要な情報。app が埋める。
///
/// 原因の文言は `flightsim-sim` が作る。**ui は sim に依存しない**ので
/// （依存は一方向）、文字列にしてから渡してもらう。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct CrashNotice {
    /// 原因の一行。空なら墜落していない。
    headline: String,
}

impl CrashNotice {
    /// 原因を設定する。
    ///
    /// # Panics
    ///
    /// `headline` が非 ASCII または複数行ならパニックする。既定フォントに
    /// 字形が無い文字が豆腐になるのと、帯からはみ出すのを防ぐため。
    pub fn set(&mut self, headline: impl Into<String>) {
        let headline = headline.into();
        assert!(
            headline.is_ascii(),
            "the crash headline must be ASCII for Bevy's default font"
        );
        assert!(
            !headline.contains('\n') && !headline.contains('\r'),
            "the crash headline must be a single line"
        );
        self.headline = headline;
    }

    /// 表示を消す。
    pub fn clear(&mut self) {
        self.headline.clear();
    }

    /// 墜落しているか。
    #[must_use]
    pub fn is_crashed(&self) -> bool {
        !self.headline.is_empty()
    }

    /// 原因の一行。
    #[must_use]
    pub fn headline(&self) -> &str {
        &self.headline
    }
}

/// 墜落表示の印。
#[derive(Component, Debug)]
pub struct CrashOverlay;

/// 画面中央に墜落を出す。
pub fn spawn_crash_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        // 赤。**失敗であることが一目で分かる色**にする。
        TextColor(Color::srgb(1.0, 0.55, 0.5)),
        TextLayout::new_with_justify(Justify::Center),
        BackgroundColor(Color::srgba(0.15, 0.0, 0.0, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(38.0),
            left: Val::Percent(10.0),
            right: Val::Percent(10.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
            ..default()
        },
        Visibility::Hidden,
        CrashOverlay,
    ));
}

/// 表示を状態に合わせる。
pub fn update_crash_overlay(
    notice: Res<CrashNotice>,
    mut query: Query<(&mut Text, &mut Visibility), With<CrashOverlay>>,
) {
    for (mut text, mut visibility) in &mut query {
        *visibility = if notice.is_crashed() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !notice.is_crashed() {
            continue;
        }
        let body = crash_text(notice.headline());
        if text.as_str() != body {
            **text = body;
        }
    }
}

/// 墜落表示の本文。原因の下に逃げ道を置く。
///
/// **ASCII のみ。**
#[must_use]
pub fn crash_text(headline: &str) -> String {
    format!("{headline}\n\nR ..... try again")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_shows_the_cause_and_the_way_out() {
        let text = crash_text("CRASHED: came down at 13.3 m/s; the gear cannot take it");
        assert!(text.contains("13.3 m/s"), "the cause must survive");
        assert!(text.contains('R'), "the way to try again must be on screen");
        assert!(text.is_ascii());
    }

    #[test]
    fn an_empty_notice_is_not_a_crash() {
        // 起動直後に赤い帯が出ていては困る。
        let notice = CrashNotice::default();
        assert!(!notice.is_crashed());
        assert!(notice.headline().is_empty());
    }

    #[test]
    fn setting_and_clearing_a_notice_flips_the_display() {
        let mut notice = CrashNotice::default();
        notice.set("CRASHED: banked 30 deg at touchdown; the wingtip hit");
        assert!(notice.is_crashed());
        notice.clear();
        assert!(
            !notice.is_crashed(),
            "a restart must be able to take the notice away"
        );
    }

    #[test]
    #[should_panic(expected = "ASCII")]
    fn a_non_ascii_headline_is_rejected_at_the_boundary() {
        // 豆腐が出てから気付くのでは遅い。**入れる時点で落とす。**
        CrashNotice::default().set("墜落");
    }

    #[test]
    #[should_panic(expected = "single line")]
    fn a_multi_line_headline_is_rejected() {
        CrashNotice::default().set("CRASHED\nand again");
    }
}
