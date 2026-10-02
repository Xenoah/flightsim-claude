//! App-fed traffic presentation. No network or input sibling dependency.

use bevy::prelude::*;

use crate::InputDiagnosticsPanel;

/// App supplies source, callsign, relative bearing/range/height and stale status.
#[derive(Resource, Debug, Clone, Default)]
pub struct TrafficPanel {
    pub visible: bool,
    pub text: String,
}

/// Includes the source/header lines. Longer lists show an explicit overflow
/// summary rather than silently rendering offscreen; publish nearest traffic first.
pub const TRAFFIC_PANEL_MAX_LINES: usize = 14;

#[derive(Component, Debug)]
pub struct TrafficDisplay;

#[derive(Component, Debug)]
pub struct TrafficRoot;

pub fn spawn_traffic_panel(mut commands: Commands) {
    commands
        .spawn((
            BackgroundColor(Color::srgba(0.025, 0.045, 0.06, 0.87)),
            Node {
                position_type: PositionType::Absolute,
                top: px(190.0),
                right: px(12.0),
                width: px(420.0),
                padding: UiRect::all(px(10.0)),
                ..default()
            },
            GlobalZIndex(5),
            Visibility::Hidden,
            TrafficRoot,
            Name::new("traffic panel"),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgb(0.80, 0.94, 1.0)),
                Node {
                    width: percent(100.0),
                    ..default()
                },
                TrafficDisplay,
            ));
        });
}

/// Keep externally supplied callsigns compatible with the bundled UI font and
/// bound the panel's footprint. The app retains the full underlying information.
#[must_use]
pub fn traffic_display_text(text: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    let visible = if lines.len() > TRAFFIC_PANEL_MAX_LINES {
        TRAFFIC_PANEL_MAX_LINES - 1
    } else {
        lines.len()
    };
    let mut output = Vec::with_capacity(TRAFFIC_PANEL_MAX_LINES);
    for line in lines.iter().take(visible) {
        let mut line: String = line
            .chars()
            .map(|character| {
                if character.is_ascii() && !character.is_control() {
                    character
                } else {
                    '?'
                }
            })
            .collect();
        if line.len() > 56 {
            line.truncate(53);
            line.push_str("...");
        }
        output.push(line);
    }
    if lines.len() > visible {
        output.push(format!("... {} more lines", lines.len() - visible));
    }
    output.join("\n")
}

pub fn update_traffic_panel(
    state: Res<TrafficPanel>,
    diagnostics: Res<InputDiagnosticsPanel>,
    mut roots: Query<&mut Visibility, With<TrafficRoot>>,
    mut displays: Query<&mut Text, With<TrafficDisplay>>,
) {
    if !state.is_changed() && !diagnostics.is_changed() {
        return;
    }
    let visible = state.visible && !diagnostics.visible && !state.text.is_empty();
    for mut visibility in &mut roots {
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut text in &mut displays {
        if visible {
            **text = traffic_display_text(&state.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_is_hidden_by_default() {
        assert!(!TrafficPanel::default().visible);
    }

    #[test]
    fn traffic_stays_ascii_and_reports_overflow() {
        let text = format!("SYNTHETIC 日本\n{}", "A".repeat(90));
        let output = traffic_display_text(&text);
        assert!(output.is_ascii());
        assert!(output.lines().all(|line| line.len() <= 56));
        let output = traffic_display_text(&["aircraft"; 30].join("\n"));
        assert_eq!(output.lines().count(), TRAFFIC_PANEL_MAX_LINES);
        assert!(output.ends_with("17 more lines"));
    }

    #[test]
    fn controller_diagnostics_take_priority_without_erasing_traffic() {
        let mut app = App::new();
        app.init_resource::<TrafficPanel>()
            .init_resource::<InputDiagnosticsPanel>()
            .add_systems(Update, update_traffic_panel);
        let root = app
            .world_mut()
            .spawn((Visibility::Hidden, TrafficRoot))
            .id();
        let entity = app.world_mut().spawn((Text::new(""), TrafficDisplay)).id();
        *app.world_mut().resource_mut::<TrafficPanel>() = TrafficPanel {
            visible: true,
            text: "SYNTHETIC\nTEST01 090deg 2nm +500ft".into(),
        };
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited)
        );
        app.world_mut()
            .resource_mut::<InputDiagnosticsPanel>()
            .visible = true;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        );
        app.world_mut()
            .resource_mut::<InputDiagnosticsPanel>()
            .visible = false;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited)
        );
        assert!(
            app.world()
                .get::<Text>(entity)
                .expect("text")
                .contains("TEST01")
        );
    }
}
