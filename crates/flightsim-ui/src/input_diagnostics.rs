//! Controller diagnostics presentation. The app copies input-layer data into
//! this resource; UI never depends on the sibling input crate.

use bevy::prelude::*;

/// App-provided input diagnostics. Hidden until requested with F10 or the CLI.
#[derive(Resource, Debug, Clone, Default)]
pub struct InputDiagnosticsPanel {
    pub visible: bool,
    pub text: String,
}

#[derive(Component, Debug)]
pub struct InputDiagnosticsDisplay;

#[derive(Component, Debug)]
pub struct InputDiagnosticsRoot;

pub fn spawn_input_diagnostics(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(135.0),
                bottom: px(12.0),
                right: px(12.0),
                width: px(440.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(20),
            Visibility::Hidden,
            InputDiagnosticsRoot,
            Name::new("controller diagnostics"),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100.0),
                    max_height: percent(100.0),
                    padding: UiRect::all(px(12.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.025, 0.035, 0.05, 0.96)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.96, 1.0)),
                    Node {
                        width: percent(100.0),
                        ..default()
                    },
                    InputDiagnosticsDisplay,
                ));
            });
        });
}

pub fn update_input_diagnostics_panel(
    state: Res<InputDiagnosticsPanel>,
    mut roots: Query<&mut Visibility, With<InputDiagnosticsRoot>>,
    mut displays: Query<&mut Text, With<InputDiagnosticsDisplay>>,
) {
    if !state.is_changed() {
        return;
    }
    for mut visibility in &mut roots {
        let wanted = if state.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    for mut text in &mut displays {
        if state.visible && **text != state.text {
            **text = state.text.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_hidden_until_requested() {
        let state = InputDiagnosticsPanel::default();
        assert!(!state.visible);
        assert!(state.text.is_empty());
    }

    #[test]
    fn app_published_text_and_visibility_reach_the_display() {
        let mut app = App::new();
        app.init_resource::<InputDiagnosticsPanel>()
            .add_systems(Update, update_input_diagnostics_panel);
        let root = app
            .world_mut()
            .spawn((Visibility::Hidden, InputDiagnosticsRoot))
            .id();
        let entity = app
            .world_mut()
            .spawn((Text::new(""), InputDiagnosticsDisplay))
            .id();
        app.world_mut()
            .resource_mut::<InputDiagnosticsPanel>()
            .visible = true;
        app.world_mut().resource_mut::<InputDiagnosticsPanel>().text =
            "Flight Stick\naxis left_stick_x: +0.2500".into();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited)
        );
        assert!(
            app.world()
                .get::<Text>(entity)
                .expect("text")
                .contains("+0.2500")
        );
        app.world_mut()
            .resource_mut::<InputDiagnosticsPanel>()
            .visible = false;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        );
    }
}
