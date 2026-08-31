//! 再生中であることの表示。
//!
//! **今見ているものが記録の再生なのか、自分が飛んでいるのかが
//! 分からない状態を作らない。** 操縦桿を動かしても機体が言うことを
//! 聞かないとき、それが不具合なのか再生中なのかは、画面に出ていなければ
//! 区別が付かない。

use bevy::prelude::*;

use flightsim_core::Seconds;

/// 再生の表示に必要な情報。app が毎フレーム埋める。
///
/// **再生していないときは [`Self::active`] が偽**で、表示は消える。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ReplayStatus {
    /// 再生中か。偽なら表示しない。
    pub active: bool,
    /// 一時停止しているか。
    pub paused: bool,
    /// 再生速度の倍率。
    pub speed: f64,
    /// 再生済みの時間。
    pub elapsed: Seconds,
    /// 記録全体の長さ。
    pub total: Seconds,
}

/// 再生表示の印。
#[derive(Component, Debug)]
pub struct ReplayBanner;

/// 画面上部中央に再生の状態を出す。
///
/// 上部中央はチュートリアル導線と同じ帯。**再生中はチュートリアルを
/// 出さない**（記録の再生に「今すぐ離陸しろ」と指示しても意味がない）ので
/// 重ならない。app が `--replay` のときに黙らせる。
///
/// 幅は画面の中央 60% ではなく 76% を取る。**30% ずつ空けると 1 行に
/// 収まらず折り返した**（実機のスクリーンショットで発覚）。
pub fn spawn_replay_banner(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.9, 0.6)),
        TextLayout::new_with_justify(Justify::Center),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Percent(12.0),
            right: Val::Percent(12.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        Visibility::Hidden,
        ReplayBanner,
    ));
}

/// 表示を状態に合わせる。
pub fn update_replay_banner(
    status: Res<ReplayStatus>,
    mut query: Query<(&mut Text, &mut Visibility), With<ReplayBanner>>,
) {
    for (mut text, mut visibility) in &mut query {
        *visibility = if status.active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !status.active {
            continue;
        }
        let line = format_replay_banner(&status);
        if text.as_str() != line {
            **text = line;
        }
    }
}

/// 再生表示の 1 行。
///
/// **ASCII のみ。** 既定フォントに字形が無い記号は豆腐になる。
#[must_use]
pub fn format_replay_banner(status: &ReplayStatus) -> String {
    let state = if status.paused { "PAUSED" } else { "REPLAY" };
    let speed = if status.speed.is_finite() {
        status.speed.clamp(0.0, 99.0)
    } else {
        1.0
    };
    format!(
        "{state}  x{speed:.1}  {} / {}   F5 pause   F6/F7 speed   F8 back 10s",
        clock(status.elapsed),
        clock(status.total)
    )
}

/// 秒を `m:ss` にする。負や非有限は 0 に倒す。
fn clock(seconds: Seconds) -> String {
    let total = if seconds.get().is_finite() {
        seconds.get().max(0.0)
    } else {
        0.0
    };
    // 表示できる上限で頭打ちにする。記録の上限は約 4.6 時間なので
    // ここに当たるのは値が壊れているときだけ。**当たっても数字は出す**
    // （表示が消えるより、頭打ちの数字が出ている方が原因を追える）。
    const LONGEST_DISPLAYABLE: f64 = 359_999.0;
    // 小数を切り捨てる。表示が 1 秒だけ進んで戻るのを避ける。
    #[expect(
        clippy::cast_possible_truncation,
        reason = "非有限・負・上限の 3 方向を潰してあるので u64 に収まる"
    )]
    let whole = total.min(LONGEST_DISPLAYABLE) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> ReplayStatus {
        ReplayStatus {
            active: true,
            paused: false,
            speed: 1.0,
            elapsed: Seconds(65.0),
            total: Seconds(195.0),
        }
    }

    #[test]
    fn the_banner_says_what_is_happening_and_how_to_control_it() {
        let line = format_replay_banner(&status());
        assert!(line.starts_with("REPLAY"));
        assert!(line.contains("1:05 / 3:15"), "got: {line}");
        assert!(line.contains("F5"), "the pause key must be discoverable");
    }

    #[test]
    fn a_paused_replay_says_paused() {
        // **止まっているのに「再生中」と出したら、固まったように見える。**
        let mut status = status();
        status.paused = true;
        assert!(format_replay_banner(&status).starts_with("PAUSED"));
    }

    #[test]
    fn the_banner_is_ascii_at_every_speed() {
        for speed in [0.1, 1.0, 2.5, 8.0] {
            let mut status = status();
            status.speed = speed;
            let line = format_replay_banner(&status);
            assert!(line.is_ascii(), "{line}");
        }
    }

    #[test]
    fn non_finite_values_do_not_reach_the_screen() {
        // NaN の時計や速度が出ると、不具合が「変な表示」として埋もれる。
        let mut status = status();
        status.speed = f64::NAN;
        status.elapsed = Seconds(f64::NAN);
        status.total = Seconds(f64::INFINITY);
        let line = format_replay_banner(&status);
        assert!(!line.contains("NaN") && !line.contains("inf"), "{line}");
        assert!(line.contains("0:00"), "got: {line}");
    }

    #[test]
    fn the_clock_rolls_over_at_a_minute() {
        assert_eq!(clock(Seconds(0.0)), "0:00");
        assert_eq!(clock(Seconds(59.9)), "0:59");
        assert_eq!(clock(Seconds(60.0)), "1:00");
        assert_eq!(clock(Seconds(3_600.0)), "60:00");
        // 負の経過時間は起きないはずだが、出るなら 0 として出す。
        assert_eq!(clock(Seconds(-5.0)), "0:00");
    }
}
