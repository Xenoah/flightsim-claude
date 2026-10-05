// Ordered keyboard, transactional drafts and modal ownership regressions.
use super::*;

fn weather_state() -> WorldMapState {
    WorldMapState {
        weather_settings: WeatherSettingsView {
            background_visibility: "10000.000".into(),
            cloud_base: Some("1200.123".into()),
            cloud_base_max: Some(Meters(30_000.0)),
            retained: "Retained layer thickness: 1000 m\nRetained precipitation: Rain 5 mm/h\nWind / turbulence unchanged".into(),
            enabled: true,
            error: String::new(),
        },
        ..aircraft_picker_state()
    }
}

fn weather_app() -> App {
    let mut app = input_app();
    let mut state = weather_state();
    assert!(state.show_weather_settings());
    app.insert_resource(state);
    app
}

fn click(app: &mut App, button: WorldMapButton) {
    let entity = app.world_mut().spawn((Interaction::Pressed, button)).id();
    region_keys(app, &[]);
    app.world_mut().despawn(entity);
}

fn replace_number(app: &mut App, text: &str) {
    send_keys(app, [
        key_event(KeyCode::ControlLeft, Key::Control, None, ButtonState::Pressed),
        typed(KeyCode::KeyA, "a"),
        key_event(KeyCode::ControlLeft, Key::Control, None, ButtonState::Released),
        typed(KeyCode::Digit1, text),
    ]);
    region_keys(app, &[]);
}

#[test]
fn opening_and_untouched_apply_do_not_round_trip_display_values_or_start() {
    let mut app = input_app();
    app.insert_resource(weather_state());
    region_keys(&mut app, &[KeyCode::Enter]);
    let old = app.world_mut().resource_mut::<WorldMapActions>().start_at.take().unwrap();
    let opening = app.world_mut().spawn((Interaction::Pressed, WorldMapButton::OpenWeatherSettings)).id();
    send_keys(&mut app, [key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed)]);
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12, KeyCode::PageDown]);
    app.world_mut().despawn(opening);
    let state = app.world().resource::<WorldMapState>();
    assert!(state.weather_editor.is_some());
    assert!(!state.new_flight_modal_ready());
    assert_eq!(state.aircraft_choice, 0);
    let actions = app.world().resource::<WorldMapActions>();
    assert!(actions.generation > old.generation);
    assert!(actions.start_at.is_none() && actions.weather.is_none());
    assert!(!actions.new_flight_shortcuts_available);
    assert_eq!(app.world().resource::<ButtonInput<KeyCode>>().get_just_pressed().count(), 0);
    let view = state.weather_settings.clone();
    click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
    assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit::default()));
    let state = app.world().resource::<WorldMapState>();
    assert!(state.visible && state.weather_editor.is_none());
    assert_eq!(state.weather_settings, view);
}

#[test]
fn each_bound_and_individual_field_preserves_untouched_values_by_omission() {
    for (field, text, expected) in [
        (WeatherSettingsButton::BackgroundVisibility, "10", WorldMapWeatherEdit { background_visibility: Some(Meters(10.0)), ..default() }),
        (WeatherSettingsButton::BackgroundVisibility, "200000", WorldMapWeatherEdit { background_visibility: Some(Meters(200_000.0)), ..default() }),
        (WeatherSettingsButton::CloudBase, "0", WorldMapWeatherEdit { cloud_base: Some(Meters(0.0)), ..default() }),
        (WeatherSettingsButton::CloudBase, "30000", WorldMapWeatherEdit { cloud_base: Some(Meters(30_000.0)), ..default() }),
    ] {
        let mut app = weather_app();
        click(&mut app, WorldMapButton::Weather(field));
        replace_number(&mut app, text);
        send_keys(&mut app, [
            key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
            key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
            typed(KeyCode::Digit9, "9"),
        ]);
        region_keys(&mut app, &[KeyCode::Enter, KeyCode::PageDown, KeyCode::F12]);
        let actions = app.world().resource::<WorldMapActions>();
        assert_eq!(actions.weather, Some(expected));
        assert!(actions.start_at.is_none() && actions.conditions.is_none());
        assert!(!actions.new_flight_shortcuts_available);
        assert_eq!(app.world().resource::<WorldMapState>().weather_settings, weather_state().weather_settings);
    }
}

#[test]
fn invalid_second_field_cannot_partially_apply_a_valid_first_field() {
    for bad in ["-1", "30000.1", "NaN", "inf", "1e2", "1\n2", "", "0.00000000000000000001"] {
        let mut app = weather_app();
        replace_number(&mut app, "2000");
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::CloudBase));
        replace_number(&mut app, bad);
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
        let state = app.world().resource::<WorldMapState>();
        assert!(state.weather_editor.is_some(), "accepted {bad:?}");
        assert!(!weather_settings::format_text(WeatherSettingsText::Error, state).is_empty());
        assert!(app.world().resource::<WorldMapActions>().weather.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        replace_number(&mut app, "200");
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
        assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit {
            background_visibility: Some(Meters(2000.0)), cloud_base: Some(Meters(200.0)),
        }));
    }
    for bad in ["9.999", "200000.1", "-1", "NaN", "inf", ""] {
        let mut app = weather_app();
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::CloudBase));
        replace_number(&mut app, "200");
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::BackgroundVisibility));
        replace_number(&mut app, bad);
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
        assert!(app.world().resource::<WorldMapState>().weather_editor.is_some(), "accepted {bad:?}");
        assert!(app.world().resource::<WorldMapActions>().weather.is_none());
    }
}

#[test]
fn ordered_click_ctrl_a_then_tab_preserves_focus_and_applies_atomically() {
    let mut app = weather_app();
    let field = app.world_mut().spawn((Interaction::Pressed, WorldMapButton::Weather(WeatherSettingsButton::CloudBase))).id();
    send_keys(&mut app, [
        key_event(KeyCode::ControlLeft, Key::Control, None, ButtonState::Pressed),
        typed(KeyCode::KeyA, "a"),
        key_event(KeyCode::ControlLeft, Key::Control, None, ButtonState::Released),
        typed(KeyCode::Digit2, "2"),
        typed(KeyCode::Digit2, "2"),
        typed(KeyCode::Period, ",5"),
        key_event(KeyCode::Tab, Key::Tab, None, ButtonState::Pressed),
        key_event(KeyCode::ControlRight, Key::Control, None, ButtonState::Pressed),
        typed(KeyCode::KeyA, "a"),
        key_event(KeyCode::ControlRight, Key::Control, None, ButtonState::Released),
        typed(KeyCode::Digit2, "2000"),
        key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
    ]);
    region_keys(&mut app, &[KeyCode::Enter]);
    app.world_mut().despawn(field);
    let actions = app.world().resource::<WorldMapActions>();
    assert_eq!(actions.weather, Some(WorldMapWeatherEdit {
        background_visibility: Some(Meters(2000.0)), cloud_base: Some(Meters(22.5)),
    }));
    assert!(actions.start_at.is_none());
    assert!(!actions.new_flight_shortcuts_available);
}

#[test]
fn cloudless_scenarios_offer_no_cloud_base_edit_and_tab_stays_on_visibility() {
    let mut app = input_app();
    let mut state = weather_state();
    state.weather_settings.cloud_base = None;
    state.weather_settings.cloud_base_max = None;
    assert!(state.show_weather_settings());
    assert!(weather_settings::disabled(WeatherSettingsButton::CloudBase, &state));
    assert!(weather_settings::format_text(WeatherSettingsText::CloudBase, &state).contains("no cloud layer"));
    app.insert_resource(state);
    click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::CloudBase));
    send_keys(&mut app, [key_event(KeyCode::Tab, Key::Tab, None, ButtonState::Pressed)]);
    region_keys(&mut app, &[]);
    replace_number(&mut app, "1500");
    click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
    assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit {
        background_visibility: Some(Meters(1500.0)), cloud_base: None,
    }));
}

#[test]
fn each_child_frame_edit_restore_and_cancel_invalidates_start_generation() {
    let mut app = weather_app();
    region_keys(&mut app, &[]);
    let mut previous = app.world().resource::<WorldMapActions>().generation;
    // Even an unchanged modal frame invalidates a previously prepared request.
    region_keys(&mut app, &[]);
    assert!(app.world().resource::<WorldMapActions>().generation > previous);
    previous = app.world().resource::<WorldMapActions>().generation;
    for text in ["2000", "10000.000", "10", "10000.000"] {
        replace_number(&mut app, text);
        let actions = app.world().resource::<WorldMapActions>();
        assert!(actions.generation > previous);
        previous = actions.generation;
        assert!(actions.start_at.is_none() && actions.weather.is_none());
    }
    click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Cancel));
    assert!(app.world().resource::<WorldMapActions>().generation > previous);
    assert_eq!(app.world().resource::<WorldMapState>().weather_settings, weather_state().weather_settings);
}

#[test]
fn editor_consumes_underlying_actions_and_escape_wins_over_apply_and_f12() {
    let mut app = weather_app();
    app.init_resource::<ButtonInput<Key>>();
    app.world_mut().resource_mut::<WorldMapState>().wind_settings.enabled = true;
    for button in [
        WorldMapButton::Start, WorldMapButton::NextAircraft, WorldMapButton::Destination(1),
        WorldMapButton::NextMonth, WorldMapButton::Latitude, WorldMapButton::OpenWindSettings,
        WorldMapButton::OpenRegions, WorldMapButton::OpenCredits,
    ] { click(&mut app, button); }
    let before = weather_state();
    let state = app.world().resource::<WorldMapState>();
    assert_eq!(state.aircraft_choice, before.aircraft_choice);
    assert_eq!(state.selected, before.selected);
    assert_eq!(state.month, before.month);
    assert!(state.weather_editor.is_some() && state.wind_editor.is_none());
    assert!(state.coordinate_field.is_none());
    assert!(!state.regions.visible && !state.credits_visible);
    replace_number(&mut app, "2000");
    app.world_mut().resource_mut::<ButtonInput<Key>>().press(Key::F12);
    let apply = app.world_mut().spawn((Interaction::Pressed, WorldMapButton::Weather(WeatherSettingsButton::Apply))).id();
    send_keys(&mut app, [key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed)]);
    region_keys(&mut app, &[KeyCode::Escape, KeyCode::Enter, KeyCode::F12, KeyCode::PageDown, KeyCode::KeyG]);
    app.world_mut().despawn(apply);
    let state = app.world().resource::<WorldMapState>();
    assert!(state.visible && state.weather_editor.is_none());
    assert_eq!(state.weather_settings, before.weather_settings);
    let actions = app.world().resource::<WorldMapActions>();
    assert!(actions.weather.is_none() && actions.conditions.is_none() && actions.start_at.is_none());
    assert!(actions.regions.pending.is_none());
    assert!(!actions.new_flight_shortcuts_available);
    assert_eq!(app.world().resource::<ButtonInput<Key>>().get_just_pressed().count(), 0);
    assert_eq!(app.world().resource::<ButtonInput<KeyCode>>().get_just_pressed().count(), 0);
}

#[test]
fn close_reopen_and_disabled_policy_never_commit_drafts() {
    for via_key in [false, true] {
        let mut app = weather_app();
        replace_number(&mut app, "2000");
        if via_key { region_keys(&mut app, &[KeyCode::KeyM]); }
        else { click(&mut app, WorldMapButton::Close); }
        let state = app.world().resource::<WorldMapState>();
        assert!(!state.visible && state.weather_editor.is_none());
        assert!(app.world().resource::<WorldMapActions>().weather.is_none());
        region_keys(&mut app, &[KeyCode::KeyM]);
        click(&mut app, WorldMapButton::OpenWeatherSettings);
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
        assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit::default()));
    }
    let mut app = input_app();
    let mut state = weather_state();
    state.weather_settings.enabled = false;
    assert!(!state.show_weather_settings());
    app.insert_resource(state);
    click(&mut app, WorldMapButton::OpenWeatherSettings);
    assert!(app.world().resource::<WorldMapState>().weather_editor.is_none());
}

#[test]
fn focus_loss_session_lock_and_coordinate_ownership_prevent_stale_apply() {
    let mut app = weather_app();
    app.world_mut().write_message(KeyboardFocusLost).unwrap();
    send_keys(&mut app, [typed(KeyCode::Digit1, "1"), key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed)]);
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12, KeyCode::PageDown]);
    assert!(app.world().resource::<WorldMapState>().weather_editor.is_some());
    assert!(app.world().resource::<WorldMapActions>().weather.is_none());
    app.world_mut().resource_mut::<WorldMapState>().weather_settings.enabled = false;
    click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
    assert!(app.world().resource::<WorldMapState>().weather_editor.is_none());
    assert!(app.world().resource::<WorldMapActions>().weather.is_none());
    let mut state = weather_state();
    state.begin_coordinate(CoordinateField::Latitude);
    assert!(!state.show_weather_settings());
    state.coordinate_draft = "35.55".into();
    app.insert_resource(state);
    let opening = app.world_mut().spawn((Interaction::Pressed, WorldMapButton::OpenWeatherSettings)).id();
    send_keys(&mut app, [key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed)]);
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12]);
    app.world_mut().despawn(opening);
    let state = app.world().resource::<WorldMapState>();
    assert!(state.weather_editor.is_none());
    assert!(state.coordinate_field.is_none());
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    assert!(!app.world().resource::<WorldMapActions>().new_flight_shortcuts_available);
}

#[test]
fn weather_copy_names_the_departure_reference_and_bounds_without_ceiling_claims() {
    let mut state = weather_state();
    assert!(state.show_weather_settings());
    let visibility = weather_settings::format_text(WeatherSettingsText::BackgroundVisibility, &state);
    assert!(visibility.contains("Background visibility (m, 10-200000)"));
    let base = weather_settings::format_text(WeatherSettingsText::CloudBase, &state);
    assert!(base.contains("above departure ground reference"));
    assert!(!base.to_lowercase().contains("ceiling"));
    assert_eq!(weather_settings::format_text(WeatherSettingsText::Retained, &state), state.weather_settings.retained);
}

#[test]
fn retained_thickness_limit_rejects_invalid_base_without_closing_the_draft() {
    for maximum in [Some(Meters(29_500.0)), Some(Meters(f64::NAN)), Some(Meters(f64::INFINITY)), Some(Meters(-1.0)), None] {
        let mut state = weather_state();
        state.weather_settings.cloud_base_max = maximum;
        assert!(state.show_weather_settings());
        let mut app = input_app();
        app.insert_resource(state);
        replace_number(&mut app, "2000");
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::CloudBase));
        replace_number(&mut app, "30000");
        click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
        let state = app.world().resource::<WorldMapState>();
        assert!(state.weather_editor.is_some());
        assert!(app.world().resource::<WorldMapActions>().weather.is_none());
        assert!(!weather_settings::format_text(WeatherSettingsText::Error, state).is_empty());
        if maximum.is_some_and(|value| value.get().to_bits() == 29_500.0_f64.to_bits()) {
            assert!(weather_settings::format_text(WeatherSettingsText::CloudBase, state).contains("0-29500"));
            replace_number(&mut app, "29500");
            click(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Apply));
            assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit {
                background_visibility: Some(Meters(2000.0)), cloud_base: Some(Meters(29_500.0)),
            }));
        }
    }
}


#[test]
fn externally_hidden_map_still_consumes_the_weather_child_close_frame() {
    let mut app = weather_app();
    app.init_resource::<ButtonInput<Key>>();
    app.world_mut().resource_mut::<ButtonInput<Key>>().press(Key::F12);
    app.world_mut().resource_mut::<WorldMapState>().visible = false;
    region_keys(&mut app, &[KeyCode::Enter, KeyCode::F12, KeyCode::PageDown]);
    assert!(app.world().resource::<WorldMapState>().weather_editor.is_none());
    let actions = app.world().resource::<WorldMapActions>();
    assert!(actions.weather.is_none() && actions.start_at.is_none());
    assert!(!actions.new_flight_shortcuts_available);
    assert_eq!(app.world().resource::<ButtonInput<Key>>().get_just_pressed().count(), 0);
    assert_eq!(app.world().resource::<ButtonInput<KeyCode>>().get_just_pressed().count(), 0);
}
