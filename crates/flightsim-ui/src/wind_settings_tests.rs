// Regressions exercise the real ordered input system and GPU-free layout.
use super::*;
use flightsim_core::Knots;

fn wind_state() -> WorldMapState {
    WorldMapState {
        wind_settings: WindSettingsView {
            wind_from: "123.5".into(),
            wind_speed: "12.3".into(),
            turbulence: "Horizontal bound: 2.3456789 m/s\nDeterministic seed: 987654321".into(),
            turbulence_seed: 987654321,
            note: "Authored physical wind; pending until Start".into(),
            error: String::new(),
            enabled: true,
        },
        ..aircraft_picker_state()
    }
}

fn wind_app() -> App {
    let mut app = input_app();
    let mut state = wind_state();
    assert!(state.show_wind_settings());
    app.insert_resource(state);
    app
}

fn click(app: &mut App, button: WorldMapButton) {
    let entity = app.world_mut().spawn((Interaction::Pressed, button)).id();
    region_keys(app, &[]);
    app.world_mut().despawn(entity);
}

fn replace_number(app: &mut App, text: &str) {
    send_keys(
        app,
        [
            key_event(
                KeyCode::ControlLeft,
                Key::Control,
                None,
                ButtonState::Pressed,
            ),
            typed(KeyCode::KeyA, "a"),
            key_event(
                KeyCode::ControlLeft,
                Key::Control,
                None,
                ButtonState::Released,
            ),
            typed(KeyCode::Digit1, text),
        ],
    );
    region_keys(app, &[]);
}

#[test]
fn opening_and_untouched_apply_preserve_exact_app_values_by_omission() {
    let mut app = input_app();
    app.insert_resource(wind_state());
    region_keys(&mut app, &[KeyCode::Enter]);
    let old = app
        .world_mut()
        .resource_mut::<WorldMapActions>()
        .start_at
        .take()
        .unwrap();
    let opening = app
        .world_mut()
        .spawn((Interaction::Pressed, WorldMapButton::OpenWindSettings))
        .id();
    send_keys(
        &mut app,
        [key_event(
            KeyCode::Enter,
            Key::Enter,
            None,
            ButtonState::Pressed,
        )],
    );
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12, KeyCode::PageDown]);
    app.world_mut().despawn(opening);
    let state = app.world().resource::<WorldMapState>();
    assert!(state.wind_editor.is_some());
    assert!(!state.new_flight_modal_ready());
    assert_eq!(state.aircraft_choice, 0);
    let actions = app.world().resource::<WorldMapActions>();
    assert!(actions.generation > old.generation);
    assert!(actions.start_at.is_none() && actions.conditions.is_none());
    assert!(!actions.new_flight_shortcuts_available);
    assert_eq!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .get_just_pressed()
            .count(),
        0
    );
    let view = app
        .world()
        .resource::<WorldMapState>()
        .wind_settings
        .clone();
    click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
    assert_eq!(
        app.world().resource::<WorldMapActions>().conditions,
        Some(WorldMapConditionsEdit::default())
    );
    let state = app.world().resource::<WorldMapState>();
    assert!(state.visible && state.wind_editor.is_none());
    assert_eq!(state.wind_settings, view);
}

#[test]
fn individual_numeric_fields_preserve_untouched_components_and_custom_turbulence() {
    for (field, text, expected) in [
        (
            WindSettingsButton::WindFrom,
            "360",
            WorldMapConditionsEdit {
                wind_from: Some(Degrees(360.0)),
                ..default()
            },
        ),
        (
            WindSettingsButton::WindSpeed,
            "300",
            WorldMapConditionsEdit {
                wind_speed: Some(Knots(300.0)),
                ..default()
            },
        ),
        (
            WindSettingsButton::WindSpeed,
            "0",
            WorldMapConditionsEdit {
                wind_speed: Some(Knots(0.0)),
                ..default()
            },
        ),
    ] {
        let mut app = wind_app();
        click(&mut app, WorldMapButton::Wind(field));
        replace_number(&mut app, text);
        send_keys(
            &mut app,
            [
                key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                typed(KeyCode::Digit9, "9"),
            ],
        );
        region_keys(&mut app, &[KeyCode::Enter, KeyCode::PageDown, KeyCode::F12]);
        let actions = app.world().resource::<WorldMapActions>();
        assert_eq!(actions.conditions, Some(expected));
        assert!(actions.start_at.is_none());
        assert!(!actions.new_flight_shortcuts_available);
        assert_eq!(app.world().resource::<WorldMapState>().aircraft_choice, 0);
    }
}

#[test]
fn ordered_click_ctrl_a_and_repeated_digits_target_the_clicked_field() {
    let mut app = wind_app();
    let field = app
        .world_mut()
        .spawn((
            Interaction::Pressed,
            WorldMapButton::Wind(WindSettingsButton::WindSpeed),
        ))
        .id();
    send_keys(
        &mut app,
        [
            key_event(
                KeyCode::ControlLeft,
                Key::Control,
                None,
                ButtonState::Pressed,
            ),
            typed(KeyCode::KeyA, "a"),
            key_event(
                KeyCode::ControlLeft,
                Key::Control,
                None,
                ButtonState::Released,
            ),
            typed(KeyCode::Digit1, "1"),
            typed(KeyCode::Digit1, "1"),
            typed(KeyCode::Period, ",5"),
        ],
    );
    region_keys(&mut app, &[]);
    app.world_mut().despawn(field);
    click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
    assert_eq!(
        app.world().resource::<WorldMapActions>().conditions,
        Some(WorldMapConditionsEdit {
            wind_speed: Some(Knots(11.5)),
            ..default()
        })
    );
}

#[test]
fn invalid_and_overlong_numbers_block_the_entire_draft_until_corrected() {
    for (field, bad) in [
        (WindSettingsButton::WindFrom, "360.1"),
        (WindSettingsButton::WindFrom, "-1"),
        (WindSettingsButton::WindSpeed, "300.1"),
        (WindSettingsButton::WindSpeed, "NaN"),
        (WindSettingsButton::WindSpeed, "inf"),
        (WindSettingsButton::WindSpeed, "1e2"),
        (WindSettingsButton::WindSpeed, "1\n2"),
        (WindSettingsButton::WindSpeed, ""),
        (WindSettingsButton::WindSpeed, "0.0000000000000000000001"),
    ] {
        let mut app = wind_app();
        click(&mut app, WorldMapButton::Wind(field));
        replace_number(&mut app, bad);
        click(
            &mut app,
            WorldMapButton::Wind(WindSettingsButton::Turbulence(WorldMapTurbulence::Severe)),
        );
        click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
        let state = app.world().resource::<WorldMapState>();
        assert!(state.wind_editor.is_some(), "accepted {bad:?}");
        assert!(!wind_settings::format_text(WindSettingsText::Error, state).is_empty());
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .conditions
                .is_none()
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        replace_number(&mut app, "0");
        click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .wind_editor
                .is_none()
        );
    }
}

#[test]
fn each_edit_and_restore_invalidates_even_a_consumed_start_generation() {
    let mut app = wind_app();
    region_keys(&mut app, &[]);
    let mut previous = app.world().resource::<WorldMapActions>().generation;
    for text in ["90", "123.5", "45", "123.5"] {
        replace_number(&mut app, text);
        let actions = app.world().resource::<WorldMapActions>();
        assert!(actions.generation > previous);
        previous = actions.generation;
        assert!(actions.start_at.is_none() && actions.conditions.is_none());
    }
    click(&mut app, WorldMapButton::Wind(WindSettingsButton::Cancel));
    assert!(app.world().resource::<WorldMapActions>().generation > previous);
    assert_eq!(
        app.world().resource::<WorldMapState>().wind_settings,
        wind_state().wind_settings
    );
}

#[test]
fn preset_selection_only_changes_turbulence_and_keep_current_undoes_it() {
    for preset in [
        WorldMapTurbulence::Calm,
        WorldMapTurbulence::Light,
        WorldMapTurbulence::Moderate,
        WorldMapTurbulence::Severe,
    ] {
        for keep in [false, true] {
            let mut app = wind_app();
            click(
                &mut app,
                WorldMapButton::Wind(WindSettingsButton::Turbulence(preset)),
            );
            let summary = wind_settings::format_text(
                WindSettingsText::Turbulence,
                app.world().resource::<WorldMapState>(),
            );
            assert!(summary.contains("seed 987654321"));
            if keep {
                click(
                    &mut app,
                    WorldMapButton::Wind(WindSettingsButton::CurrentTurbulence),
                );
            }
            click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
            assert_eq!(
                app.world().resource::<WorldMapActions>().conditions,
                Some(WorldMapConditionsEdit {
                    turbulence: (!keep).then_some(preset),
                    ..default()
                })
            );
        }
    }
}

#[test]
fn editor_captures_underlying_pointer_shortcuts_and_cancel_only_dismisses_child() {
    let mut app = wind_app();
    app.init_resource::<ButtonInput<Key>>();
    for button in [
        WorldMapButton::Start,
        WorldMapButton::NextAircraft,
        WorldMapButton::Destination(1),
        WorldMapButton::NextMonth,
        WorldMapButton::OpenRegions,
        WorldMapButton::OpenCredits,
    ] {
        click(&mut app, button);
    }
    let before = wind_state();
    let state = app.world().resource::<WorldMapState>();
    assert_eq!(state.aircraft_choice, before.aircraft_choice);
    assert_eq!(state.selected, before.selected);
    assert_eq!(state.month, before.month);
    assert!(!state.regions.visible && !state.credits_visible);
    assert!(state.wind_editor.is_some());
    assert!(
        app.world()
            .resource::<WorldMapActions>()
            .regions
            .pending
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<Key>>()
        .press(Key::PageDown);
    app.world_mut()
        .resource_mut::<ButtonInput<Key>>()
        .press(Key::F12);
    replace_number(&mut app, "270");
    send_keys(
        &mut app,
        [key_event(
            KeyCode::Enter,
            Key::Enter,
            None,
            ButtonState::Pressed,
        )],
    );
    region_keys(
        &mut app,
        &[
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::PageDown,
            KeyCode::KeyG,
            KeyCode::F12,
        ],
    );
    let state = app.world().resource::<WorldMapState>();
    assert!(state.visible && state.wind_editor.is_none());
    assert_eq!(state.wind_settings, before.wind_settings);
    assert!(
        app.world()
            .resource::<WorldMapActions>()
            .conditions
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<ButtonInput<Key>>()
            .get_just_pressed()
            .count(),
        0
    );
    assert_eq!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .get_just_pressed()
            .count(),
        0
    );
}

#[test]
fn close_and_reopen_discard_drafts_and_disabled_sessions_cannot_enter() {
    for via_key in [false, true] {
        let mut app = wind_app();
        replace_number(&mut app, "270");
        if via_key {
            region_keys(&mut app, &[KeyCode::KeyM]);
        } else {
            click(&mut app, WorldMapButton::Close);
        }
        let state = app.world().resource::<WorldMapState>();
        assert!(!state.visible && state.wind_editor.is_none());
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .conditions
                .is_none()
        );
        region_keys(&mut app, &[KeyCode::KeyM]);
        click(&mut app, WorldMapButton::OpenWindSettings);
        click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
        assert_eq!(
            app.world().resource::<WorldMapActions>().conditions,
            Some(WorldMapConditionsEdit::default())
        );
    }
    let mut state = wind_state();
    state.wind_settings.enabled = false;
    assert!(!state.show_wind_settings());
    state.wind_settings.enabled = true;
    state.navigation_enabled = false;
    assert!(
        state.show_wind_settings(),
        "physical controls do not depend on navigation/cloud policy"
    );
    assert!(!state.new_flight_modal_ready());
}

#[test]
fn focus_loss_and_session_lock_discard_input_without_applying() {
    let mut app = wind_app();
    app.world_mut().write_message(KeyboardFocusLost).unwrap();
    send_keys(
        &mut app,
        [
            typed(KeyCode::Digit1, "1"),
            key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
        ],
    );
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12, KeyCode::PageDown]);
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .wind_editor
            .is_some()
    );
    assert!(
        app.world()
            .resource::<WorldMapActions>()
            .conditions
            .is_none()
    );
    app.world_mut()
        .resource_mut::<WorldMapState>()
        .wind_settings
        .enabled = false;
    click(&mut app, WorldMapButton::Wind(WindSettingsButton::Apply));
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .wind_editor
            .is_none()
    );
    assert!(
        app.world()
            .resource::<WorldMapActions>()
            .conditions
            .is_none()
    );
}

#[test]
fn wind_modal_and_entry_control_fit_real_layout_at_supported_viewports() {
    for width in [1024_u16, 1280] {
        let mut state = wind_state();
        state.wind_settings.note = "Pending new-flight wind; active flight is unchanged".into();
        state.wind_settings.error = "Wind speed: enter 0 to 300 knots".into();
        assert!(state.show_wind_settings());
        let mut app = real_layout_app_at_size(state, UVec2::new(u32::from(width), 720));
        let world = app.world_mut();
        let overlay = world
            .query_filtered::<Entity, With<WorldMapWindRoot>>()
            .single(world)
            .unwrap();
        let panel = world.get::<Children>(overlay).unwrap()[0];
        let panel_rect = computed_rect(world, panel);
        assert!(
            panel_rect.min.x >= 0.0
                && panel_rect.min.y >= 0.0
                && panel_rect.max.x <= f32::from(width)
                && panel_rect.max.y <= 720.0
        );
        for (entity, kind) in world.query::<(Entity, &WorldMapText)>().iter(world) {
            if matches!(kind, WorldMapText::Wind(_)) {
                let rect = computed_rect(world, entity);
                assert!(
                    panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                    "{kind:?}: {rect:?}"
                );
                assert_text_fits(world, entity);
            }
        }
        for (entity, button) in world.query::<(Entity, &WorldMapButton)>().iter(world) {
            if matches!(button, WorldMapButton::Wind(_)) {
                let rect = computed_rect(world, entity);
                assert!(
                    panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                    "{button:?}: {rect:?}"
                );
                assert!(rect.height() >= MAP_BUTTON_HEIGHT - 1.0);
                for child in world.get::<Children>(entity).unwrap() {
                    assert_text_fits(world, *child);
                }
            }
        }
    }
}

#[test]
fn ordered_tab_changes_only_focus_and_applies_both_authored_fields_atomically() {
    let mut app = wind_app();
    replace_number(&mut app, "270");
    send_keys(
        &mut app,
        [
            key_event(KeyCode::Tab, Key::Tab, None, ButtonState::Pressed),
            key_event(
                KeyCode::ControlRight,
                Key::Control,
                None,
                ButtonState::Pressed,
            ),
            typed(KeyCode::KeyA, "a"),
            key_event(
                KeyCode::ControlRight,
                Key::Control,
                None,
                ButtonState::Released,
            ),
            typed(KeyCode::Digit1, "15.5"),
            key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
        ],
    );
    region_keys(&mut app, &[KeyCode::Enter]);
    let actions = app.world().resource::<WorldMapActions>();
    assert_eq!(
        actions.conditions,
        Some(WorldMapConditionsEdit {
            wind_from: Some(Degrees(270.0)),
            wind_speed: Some(Knots(15.5)),
            turbulence: None,
        })
    );
    assert!(actions.start_at.is_none());
    assert!(!actions.new_flight_shortcuts_available);
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .new_flight_modal_ready()
    );
}
