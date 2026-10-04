//! 一時停止の表示。
//!
//! **止まっていることと、そこから何ができるかを同時に出す。** 「PAUSED」
//! だけだと、再開の方法を探して結局ウィンドウを閉じることになる。

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
};

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
/// Schedule after `spawn_hud` to anchor inside its measured body, keeping even
/// wrapped attribution outside the opaque overlay in the same layout pass.
/// Standalone use without a HUD retains a viewport-relative panel.
pub fn spawn_pause_overlay(mut commands: Commands, body: Query<Entity, With<crate::HudBody>>) {
    let body = body.single().ok();
    let inset = if body.is_some() { 12.0 } else { 24.0 };
    let mut overlay = commands.spawn((
        BackgroundColor(Color::srgb(0.025, 0.03, 0.04)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(inset),
            bottom: Val::Px(inset),
            left: Val::Px(inset),
            right: Val::Px(inset),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        GlobalZIndex(50),
        Visibility::Hidden,
        PauseOverlay,
    ));
    if let Some(body) = body {
        overlay.insert(ChildOf(body));
    }
    overlay.with_children(|panel| {
        panel.spawn((
            Text::new("PAUSED - complete controls"),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        ));
        panel
            .spawn((
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
                PauseReferenceScroll,
            ))
            .with_children(|scroll| {
                scroll.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    PauseReference,
                ));
            });
        panel.spawn((
            Text::new("Mouse wheel: scroll reference | Esc: resume"),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.9, 0.6)),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            PauseReferenceHint,
        ));
    });
}

#[derive(Component)]
pub(crate) struct PauseReference;

#[derive(Component)]
pub(crate) struct PauseReferenceScroll;

#[derive(Component)]
pub(crate) struct PauseReferenceHint;

type ReferenceTexts<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text, Has<PauseReference>),
    Or<(With<PauseReference>, With<PauseReferenceHint>)>,
>;

/// The complete aircraft-specific reference is available only while simulation
/// is already paused. No flight keys are repurposed for reading or scrolling.
pub(crate) fn update_pause_reference(
    guidance: Option<Res<crate::FlightGuidance>>,
    status: Option<Res<crate::ReplayStatus>>,
    mut references: ReferenceTexts,
) {
    if guidance.as_ref().is_some_and(|value| !value.is_changed())
        && status.as_ref().is_none_or(|value| !value.is_changed())
        && references.iter().all(|(text, _)| !text.is_empty())
    {
        return;
    }
    let replay = status.as_ref().is_some_and(|value| value.active);
    let full = if replay {
        format!(
            "{}\n\n{}\n\nDISPLAY SETTINGS\n{}",
            crate::replay_help_text(),
            crate::format_replay_banner(status.as_ref().expect("active replay status")),
            display_settings_text()
        )
    } else {
        let flight = guidance
            .as_ref()
            .and_then(|value| value.live_help.as_deref())
            .map_or_else(crate::help_text, str::to_owned);
        format!("{flight}\n\n{}", pause_text())
    };
    let hint = if replay {
        "Mouse wheel: scroll reference | Replay transport: F5/F6/F7/F8"
    } else {
        "Mouse wheel: scroll reference | Esc: resume"
    };
    for (mut text, is_reference) in &mut references {
        let desired = if is_reference { full.as_str() } else { hint };
        if text.as_str() != desired {
            **text = desired.to_owned();
        }
    }
}

/// Drain wheel input even while hidden so stale events never scroll a new pause.
pub(crate) fn scroll_pause_reference(
    paused: Res<Paused>,
    mut wheel: MessageReader<MouseWheel>,
    mut scroll: Query<(&mut ScrollPosition, &ComputedNode), With<PauseReferenceScroll>>,
) {
    let delta: f32 = wheel
        .read()
        .map(|event| {
            -event.y
                * match event.unit {
                    MouseScrollUnit::Line => 42.0,
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum();
    for (mut position, node) in &mut scroll {
        if !paused.is_paused() {
            position.y = 0.0;
        } else {
            let max =
                ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            position.y = (position.y + delta).clamp(0.0, max);
        }
    }
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
    format!(
        "PAUSED\n\nEsc ... resume\nR ..... restart this flight\n{}",
        display_settings_text()
    )
}

fn display_settings_text() -> String {
    [
        "F1 .... water quality",
        "Shift+F1 ... restore LIGHT water",
        "F2 .... local detail distance",
        "Shift+F2 ... restore STANDARD",
        "F3 .... cloud quality",
        "Shift+F3 ... restore LIGHT clouds",
        "F4 .... graphics quality",
        "Shift+F4 ... restore LIGHT",
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

    #[test]
    fn complete_reference_follows_replay_transitions_without_guidance_changes() {
        let mut app = App::new();
        app.insert_resource(crate::FlightGuidance {
            live_help: Some("LIVE AIRCRAFT\nPageUp power\nJ/L roll trim".into()),
            compact_live_help: None,
            tutorial_enabled: false,
        })
        .init_resource::<crate::ReplayStatus>()
        .add_systems(Update, update_pause_reference);
        let reference = app
            .world_mut()
            .spawn((Text::default(), PauseReference))
            .id();
        let hint = app
            .world_mut()
            .spawn((Text::default(), PauseReferenceHint))
            .id();
        app.update();
        let live = app.world().get::<Text>(reference).unwrap().clone();
        assert!(live.contains("PageUp power") && live.contains("restart this flight"));
        for notice in ["REPLAY NOTICE ONE", "REPLAY NOTICE TWO"] {
            *app.world_mut().resource_mut::<crate::ReplayStatus>() = crate::ReplayStatus {
                active: true,
                paused: true,
                notice: Some(notice.into()),
                ..default()
            };
            app.update();
            let text = app.world().get::<Text>(reference).unwrap();
            assert!(text.starts_with(crate::replay_help_text()));
            assert!(text.contains(notice) && text.contains("F5"));
            assert!(!text.contains("LIVE AIRCRAFT") && !text.contains("PageUp"));
            assert!(!text.contains("restart this flight") && !text.contains("Esc ... resume"));
            assert!(
                app.world()
                    .get::<Text>(hint)
                    .unwrap()
                    .contains("Replay transport")
            );
        }
        *app.world_mut().resource_mut::<crate::ReplayStatus>() = default();
        app.update();
        assert_eq!(app.world().get::<Text>(reference), Some(&live));
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .contains("Esc: resume")
        );
    }
}
