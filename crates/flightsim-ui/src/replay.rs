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
/// Playback and persistent recording/compatibility notices share this banner.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ReplayStatus {
    /// Whether replay progress is active; a notice may also be visible in live flight.
    pub active: bool,
    /// 一時停止しているか。
    pub paused: bool,
    /// 再生速度の倍率。
    pub speed: f64,
    /// 再生済みの時間。
    pub elapsed: Seconds,
    /// 記録全体の長さ。
    pub total: Seconds,
    /// Exact rewind is rebuilding physics history in bounded batches.
    pub seeking: bool,
    /// A reproducibility or numerical failure stopped this replay.
    pub fault: Option<String>,
    /// The recorded input stream reached its end.
    pub finished: bool,
    /// Persistent compatibility or recording-availability explanation.
    pub notice: Option<String>,
}

/// 再生表示の印。
#[derive(Component, Debug)]
pub struct ReplayBanner;

/// Background and padding around the separately measured replay text.
#[derive(Component, Debug)]
pub struct ReplayBannerPanel;

/// 画面上部中央に再生の状態を出す。
///
/// 上部中央はチュートリアル導線と同じ帯。**再生中はチュートリアルを
/// 出さない**（記録の再生に「今すぐ離陸しろ」と指示しても意味がない）ので
/// 重ならない。app が `--replay` のときに黙らせる。
///
/// 幅は画面の中央 60% ではなく 76% を取る。**30% ずつ空けると 1 行に
/// 収まらず折り返した**（実機のスクリーンショットで発覚）。
pub fn spawn_replay_banner(mut commands: Commands) {
    commands
        .spawn((
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
            ReplayBannerPanel,
        ))
        .with_children(|parent| {
            // Text's final glyph bounds use its whole node width. Give padding
            // its own parent so measurement and rendered wrapping agree.
            parent.spawn((
                Text::new(""),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.9, 0.6)),
                TextLayout::new_with_justify(Justify::Center),
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Visibility::Hidden,
                ReplayBanner,
            ));
        });
}

/// 表示を状態に合わせる。
pub fn update_replay_banner(
    status: Res<ReplayStatus>,
    mut query: Query<(&mut Text, &mut Visibility), With<ReplayBanner>>,
    mut panels: Query<&mut Visibility, (With<ReplayBannerPanel>, Without<ReplayBanner>)>,
) {
    let shown = status.active || status.notice.is_some();
    for mut visibility in &mut panels {
        *visibility = if shown {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (mut text, mut visibility) in &mut query {
        *visibility = if shown {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !status.active && status.notice.is_none() {
            continue;
        }
        let line = format_replay_banner(&status);
        if text.as_str() != line {
            **text = line;
        }
    }
}

/// Playback progress and any persistent notice.
///
/// **ASCII のみ。** 既定フォントに字形が無い記号は豆腐になる。
#[must_use]
pub fn format_replay_banner(status: &ReplayStatus) -> String {
    let line = format_replay_progress(status);
    match status.notice.as_deref() {
        Some(notice) if status.active => format!("{line}\n{notice}"),
        Some(notice) => notice.to_owned(),
        None => line,
    }
}

fn format_replay_progress(status: &ReplayStatus) -> String {
    if let Some(fault) = &status.fault {
        return format!("{fault}   F8 rewind");
    }
    if status.finished && !status.seeking {
        return format!(
            "REPLAY COMPLETE  {} / {}   F8 back 10s",
            clock(status.elapsed),
            clock(status.total)
        );
    }
    let state = if status.seeking {
        "SEEKING"
    } else if status.paused {
        "PAUSED"
    } else {
        "REPLAY"
    };
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
    // 表示できる上限で頭打ちにする。記録時間はフレーム間隔にも依存するので、
    // これはファイル形式の最長時間ではなく UI の表示範囲。上限でも数字を残す。
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
            seeking: false,
            fault: None,
            finished: false,
            notice: None,
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
    fn partial_identity_notice_survives_pause_seek_completion_and_fault() {
        const NOTICE: &str =
            "LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified";
        for state in 0..5 {
            let mut status = status();
            status.notice = Some(NOTICE.into());
            status.paused = state == 1;
            status.seeking = state == 2;
            status.finished = state == 3;
            status.fault = (state == 4).then(|| "REPLAY STOPPED: test".into());
            let line = format_replay_banner(&status);
            assert!(line.ends_with(NOTICE));
            assert!(line.is_ascii());
        }
    }

    #[test]
    fn a_live_recording_notice_is_visible_and_clears_with_its_condition() {
        let mut app = App::new();
        app.insert_resource(ReplayStatus {
            notice: Some("RECORDING DISABLED: manual cloud weather; F9 unavailable".into()),
            ..default()
        })
        .add_systems(Update, update_replay_banner);
        let entity = app
            .world_mut()
            .spawn((Text::default(), Visibility::Hidden, ReplayBanner))
            .id();
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(entity).unwrap(),
            Visibility::Visible
        );
        assert!(
            app.world()
                .get::<Text>(entity)
                .unwrap()
                .contains("F9 unavailable")
        );
        app.world_mut().resource_mut::<ReplayStatus>().notice = None;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(entity).unwrap(),
            Visibility::Hidden
        );
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
