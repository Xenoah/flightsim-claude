//! Explicit trim must never change the legacy zero-trim control stream.
use super::*;

const FIXED_DT: Seconds = Seconds(1.0 / 120.0);

fn input_bits(inputs: ControlInputs) -> [u64; 6] {
    [
        inputs.aileron(),
        inputs.elevator(),
        inputs.rudder(),
        inputs.throttle(),
        inputs.flaps(),
        inputs.brakes(),
    ]
    .map(f64::to_bits)
}

fn assert_legacy_output(controls: PilotControls) {
    // Keep the complete pre-lateral-trim output expression as the oracle.
    let legacy = ControlInputs::new(
        controls.aileron.value(),
        sanitise(controls.elevator.value() + controls.trim.value()),
        controls.rudder.value(),
        controls.throttle.value(),
        controls.flaps.value(),
    )
    .with_brakes(controls.brakes);
    assert_eq!(input_bits(controls.to_control_inputs()), input_bits(legacy));
}

#[test]
fn zero_lateral_trim_preserves_every_legacy_output_bit_including_signed_zero() {
    let mut controls = PilotControls::default();
    assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
    assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
    for trim_zero in [-0.0_f64, 0.0] {
        controls.aileron_trim.set(trim_zero);
        controls.rudder_trim.set(trim_zero);
        for surface in [
            -1.0,
            -0.0142,
            -f64::from_bits(1),
            -0.0,
            0.0,
            f64::from_bits(1),
            0.00109,
            0.5,
            1.0,
        ] {
            controls.aileron.set_absolute(surface);
            controls.rudder.set_absolute(-surface);
            assert_eq!(controls.effective_aileron().to_bits(), surface.to_bits());
            assert_eq!(controls.effective_rudder().to_bits(), (-surface).to_bits());
            assert_legacy_output(controls);
        }
    }
}

#[test]
fn old_keyboard_sequences_keep_exact_control_arithmetic_in_all_entry_points() {
    let mut direct = PilotControls::default();
    let mut sampled = direct;
    let mut gamepad = direct;
    let configuration = InputConfiguration::default();
    let devices = InputDevices::default();
    let mappings = GamepadAxisMappings::default();
    for tick in 0..1200 {
        let keys = PilotKeys {
            roll_right: tick < 70,
            roll_left: (70..100).contains(&tick),
            pitch_up: (15..25).contains(&tick),
            pitch_down: (25..35).contains(&tick),
            yaw_right: (150..180).contains(&tick),
            yaw_left: (170..200).contains(&tick),
            throttle_up: tick < 400,
            throttle_down: (600..800).contains(&tick),
            flaps_extend: (600..900).contains(&tick),
            flaps_retract: (1000..1100).contains(&tick),
            trim_up: (300..400).contains(&tick),
            trim_down: (600..800).contains(&tick),
            brakes: tick > 1000,
            ..default()
        };
        direct.update(FIXED_DT, keys);
        sampled.update_with_bindings(FIXED_DT, keys, &configuration, &devices);
        gamepad.update_with_gamepad(FIXED_DT, keys, None, &mappings);
        for controls in [direct, sampled, gamepad] {
            assert_legacy_output(controls);
            assert_eq!(
                input_bits(controls.to_control_inputs()),
                input_bits(direct.to_control_inputs())
            );
            assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
            assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
        }
    }
}

#[test]
fn physical_trim_keys_are_independent_and_use_both_shift_keys_for_fine_control() {
    for (key, expected) in [
        (
            KeyCode::KeyJ,
            PilotKeys {
                aileron_trim_left: true,
                ..default()
            },
        ),
        (
            KeyCode::KeyL,
            PilotKeys {
                aileron_trim_right: true,
                ..default()
            },
        ),
        (
            KeyCode::KeyU,
            PilotKeys {
                rudder_trim_left: true,
                ..default()
            },
        ),
        (
            KeyCode::KeyO,
            PilotKeys {
                rudder_trim_right: true,
                ..default()
            },
        ),
        (
            KeyCode::KeyK,
            PilotKeys {
                lateral_trim_reset: true,
                ..default()
            },
        ),
    ] {
        let mut keyboard = ButtonInput::default();
        keyboard.press(key);
        assert_eq!(PilotKeys::from_keyboard(&keyboard), expected);
        // Continue holding into a later frame before checking key release.
        keyboard.clear();
        for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
            keyboard.press(shift);
            let fine = PilotKeys::from_keyboard(&keyboard);
            assert_eq!(
                fine,
                PilotKeys {
                    lateral_trim_fine: true,
                    ..expected
                }
            );
            keyboard.release(shift);
        }
        keyboard.release(key);
        assert_eq!(PilotKeys::from_keyboard(&keyboard), PilotKeys::default());
    }
}

#[test]
fn reset_tap_before_keyboard_poll_resets_both_trims_on_an_executed_step() {
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::KeyK);
    keyboard.release(KeyCode::KeyK);
    assert!(!keyboard.pressed(KeyCode::KeyK));
    assert!(keyboard.just_pressed(KeyCode::KeyK));

    let keys = PilotKeys::from_keyboard(&keyboard);
    assert!(keys.lateral_trim_reset);
    assert_eq!(PilotKeys::from_keyboard(&keyboard), keys);
    let sample = SampledPilotInput::from_bindings(
        keys,
        &InputConfiguration::default(),
        &InputDevices::default(),
    );
    let mut controls = PilotControls::default();
    controls.aileron_trim.set(0.0142);
    controls.rudder_trim.set(-0.00109);
    controls.update_from_sample(FIXED_DT, &sample);
    assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
    assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());

    // Bevy clears edges for the following frame. A released tap is not queued.
    keyboard.clear();
    assert_eq!(PilotKeys::from_keyboard(&keyboard), PilotKeys::default());
}

#[test]
fn held_reset_remains_active_after_the_press_edge_is_cleared() {
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::KeyK);
    for _ in 0..3 {
        keyboard.clear();
        assert!(keyboard.pressed(KeyCode::KeyK));
        assert!(!keyboard.just_pressed(KeyCode::KeyK));
        let keys = PilotKeys::from_keyboard(&keyboard);
        assert!(keys.lateral_trim_reset);
        let mut controls = PilotControls::default();
        controls.aileron_trim.set(-0.0142);
        controls.rudder_trim.set(0.00109);
        controls.update(FIXED_DT, keys);
        assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
        assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
    }
    keyboard.release(KeyCode::KeyK);
    assert_eq!(PilotKeys::from_keyboard(&keyboard), PilotKeys::default());
}

#[test]
fn shortcut_modifiers_cannot_change_or_reset_lateral_trim() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        for key in [
            KeyCode::KeyJ,
            KeyCode::KeyL,
            KeyCode::KeyU,
            KeyCode::KeyO,
            KeyCode::KeyK,
        ] {
            for held in [false, true] {
                let mut keyboard = ButtonInput::default();
                keyboard.press(modifier);
                keyboard.press(key);
                if !held {
                    keyboard.release(key);
                }
                let keys = PilotKeys::from_keyboard(&keyboard);
                assert_eq!(keys, PilotKeys::default());
                let mut controls = PilotControls::default();
                controls.aileron_trim.set(0.0142);
                controls.rudder_trim.set(0.00109);
                let before = controls;
                controls.update(FIXED_DT, keys);
                assert_eq!(controls.aileron_trim, before.aileron_trim);
                assert_eq!(controls.rudder_trim, before.rudder_trim);
            }
        }
    }
}

#[test]
fn released_shortcut_modifier_edges_suppress_reset_taps_for_that_frame_only() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        for began_previous_frame in [false, true] {
            let mut keyboard = ButtonInput::default();
            keyboard.press(modifier);
            if began_previous_frame {
                keyboard.clear();
            }
            keyboard.press(KeyCode::KeyK);
            keyboard.release(KeyCode::KeyK);
            keyboard.release(modifier);
            assert!(!keyboard.pressed(modifier));
            assert_eq!(keyboard.just_pressed(modifier), !began_previous_frame);
            assert!(keyboard.just_released(modifier));
            assert!(!keyboard.pressed(KeyCode::KeyK));
            assert!(keyboard.just_pressed(KeyCode::KeyK));
            let keys = PilotKeys::from_keyboard(&keyboard);
            assert_eq!(keys, PilotKeys::default(), "{modifier:?}");
            let mut controls = PilotControls::default();
            controls.aileron_trim.set(0.0142);
            controls.rudder_trim.set(-0.00109);
            let before = controls;
            controls.update(FIXED_DT, keys);
            assert_eq!(controls.aileron_trim, before.aileron_trim);
            assert_eq!(controls.rudder_trim, before.rudder_trim);

            keyboard.clear();
            assert_eq!(PilotKeys::from_keyboard(&keyboard), PilotKeys::default());
            keyboard.press(KeyCode::KeyK);
            keyboard.release(KeyCode::KeyK);
            controls.update(FIXED_DT, PilotKeys::from_keyboard(&keyboard));
            assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
            assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
        }
    }
}

#[test]
fn modifier_edges_keep_existing_held_reset_semantics() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        let mut keyboard = ButtonInput::default();
        keyboard.press(KeyCode::KeyK);
        keyboard.clear();
        keyboard.press(modifier);
        assert!(!PilotKeys::from_keyboard(&keyboard).lateral_trim_reset);
        keyboard.release(modifier);
        assert!(keyboard.just_pressed(modifier) && keyboard.just_released(modifier));
        assert!(PilotKeys::from_keyboard(&keyboard).lateral_trim_reset);
    }
}

#[test]
fn shift_edges_and_held_shift_allow_reset_taps() {
    for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        for held in [false, true] {
            let mut keyboard = ButtonInput::default();
            keyboard.press(shift);
            keyboard.press(KeyCode::KeyK);
            keyboard.release(KeyCode::KeyK);
            if !held {
                keyboard.release(shift);
            }
            let keys = PilotKeys::from_keyboard(&keyboard);
            assert_eq!(
                keys,
                PilotKeys {
                    lateral_trim_reset: true,
                    lateral_trim_fine: held,
                    ..default()
                }
            );
        }
    }
}

#[test]
fn coarse_and_fine_trim_reach_small_pilot_selected_biases() {
    let mut trim = LateralTrim::default();
    trim.update(FIXED_DT, true, false, false);
    assert!((trim.value() - 1.0 / 12000.0).abs() < 1e-15);
    trim.set(0.0);
    trim.update(FIXED_DT, true, false, true);
    assert!((trim.value() - 1.0 / 60000.0).abs() < 1e-15);
    trim.set(0.0);
    for _ in 0..170 {
        trim.update(FIXED_DT, true, false, false);
    }
    for _ in 0..2 {
        trim.update(FIXED_DT, true, false, true);
    }
    assert!((trim.value() - 0.0142).abs() < 1e-14);
    trim.set(0.0);
    for _ in 0..65 {
        trim.update(FIXED_DT, true, false, true);
    }
    assert!((trim.value() - 0.00109).abs() < 1.0 / 120000.0);
    let before = trim.value();
    trim.update(FIXED_DT, false, true, true);
    assert!((before - trim.value() - 1.0 / 60000.0).abs() < 1e-15);
}

#[test]
fn trim_rates_use_simulated_time_and_match_all_input_paths() {
    for fine in [false, true] {
        let rate = if fine { 0.002 } else { 0.01 };
        for hz in [15, 30, 60, 120, 144] {
            let mut direct = PilotControls::default();
            let mut sampled = direct;
            let mut gamepad = direct;
            let keys = PilotKeys {
                aileron_trim_right: true,
                rudder_trim_left: true,
                lateral_trim_fine: fine,
                ..default()
            };
            for _ in 0..hz {
                let dt = Seconds(1.0 / f64::from(hz));
                direct.update(dt, keys);
                sampled.update_from_sample(dt, &SampledPilotInput { keys, ..default() });
                gamepad.update_with_gamepad(dt, keys, None, &GamepadAxisMappings::default());
            }
            for controls in [direct, sampled, gamepad] {
                assert!((controls.aileron_trim.value() - rate).abs() < 1e-14);
                assert!((controls.rudder_trim.value() + rate).abs() < 1e-14);
                assert_eq!(controls.aileron_trim, direct.aileron_trim);
                assert_eq!(controls.rudder_trim, direct.rudder_trim);
            }
        }
    }
}

#[test]
fn release_opposing_keys_and_invalid_elapsed_time_preserve_exact_trim_bits() {
    for initial in [-0.2_f64, -0.0142, -0.0, 0.0, 0.00109, 0.2] {
        let mut trim = LateralTrim::default();
        trim.set(initial);
        for fine in [false, true] {
            for (right, left) in [(false, false), (true, true)] {
                trim.update(Seconds(100.0), right, left, fine);
                assert_eq!(trim.value().to_bits(), initial.to_bits());
            }
            for dt in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                trim.update(Seconds(dt), true, false, fine);
                trim.update(Seconds(dt), false, true, fine);
                assert_eq!(trim.value().to_bits(), initial.to_bits());
            }
        }
    }
}

#[test]
fn lateral_trim_stays_bounded_and_effective_controls_keep_full_surface_limits() {
    let mut trim = LateralTrim::default();
    for value in [
        f64::NEG_INFINITY,
        -0.201,
        -0.2,
        0.2,
        0.201,
        f64::INFINITY,
        f64::NAN,
    ] {
        trim.set(value);
        assert!(trim.value().is_finite());
        assert!((-0.2..=0.2).contains(&trim.value()));
    }
    assert_eq!(trim.value().to_bits(), 0.0_f64.to_bits());
    trim.update(Seconds(f64::MAX), true, false, false);
    assert_eq!(trim.value().to_bits(), 0.2_f64.to_bits());
    trim.update(Seconds(f64::MAX), false, true, true);
    assert_eq!(trim.value().to_bits(), (-0.2_f64).to_bits());
    for sign in [-1.0, 1.0] {
        let mut controls = PilotControls::default();
        controls.aileron_trim.set(sign * 0.2);
        controls.rudder_trim.set(sign * 0.2);
        controls.aileron.set_absolute(sign * 0.9);
        controls.rudder.set_absolute(sign * 0.9);
        assert_eq!(controls.effective_aileron().to_bits(), sign.to_bits());
        assert_eq!(controls.effective_rudder().to_bits(), sign.to_bits());
        controls.aileron.set_absolute(-sign * 0.7);
        controls.rudder.set_absolute(-sign * 0.7);
        assert!((controls.effective_aileron() + sign * 0.5).abs() < 1e-14);
        assert!((controls.effective_rudder() + sign * 0.5).abs() < 1e-14);
    }
}

#[test]
fn reset_wins_over_held_adjustments_and_changes_only_lateral_trims() {
    let mut controls = PilotControls::default();
    controls.aileron_trim.set(0.0142);
    controls.rudder_trim.set(-0.00109);
    controls.aileron.set_absolute(0.4);
    controls.rudder.set_absolute(-0.3);
    controls.throttle.set_absolute(0.6);
    controls.flaps.set_absolute(0.2);
    controls.trim.set(0.11);
    let mut otherwise_identical = controls;
    controls.update(
        FIXED_DT,
        PilotKeys {
            lateral_trim_reset: true,
            aileron_trim_left: true,
            rudder_trim_right: true,
            lateral_trim_fine: true,
            brakes: true,
            ..default()
        },
    );
    otherwise_identical.update(
        FIXED_DT,
        PilotKeys {
            brakes: true,
            ..default()
        },
    );
    assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
    assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
    assert_eq!(controls.aileron, otherwise_identical.aileron);
    assert_eq!(controls.elevator, otherwise_identical.elevator);
    assert_eq!(controls.rudder, otherwise_identical.rudder);
    assert_eq!(controls.trim, otherwise_identical.trim);
    assert_eq!(controls.throttle, otherwise_identical.throttle);
    assert_eq!(controls.flaps, otherwise_identical.flaps);
    assert_eq!(
        controls.brakes.to_bits(),
        otherwise_identical.brakes.to_bits()
    );
    assert_legacy_output(controls);
}

#[test]
fn active_analog_controls_and_keyboard_trim_combine_without_cross_axis_changes() {
    let keys = PilotKeys {
        aileron_trim_right: true,
        rudder_trim_left: true,
        lateral_trim_fine: true,
        ..default()
    };
    let mut sampled = PilotControls::default();
    let absolute = |value| {
        Some(controllers::MappedControl {
            value,
            mode: configuration::BindingMode::Absolute,
            active: true,
        })
    };
    sampled.update_from_sample(
        Seconds(0.5),
        &SampledPilotInput {
            keys,
            roll: absolute(0.4),
            yaw: absolute(-0.25),
            throttle: absolute(0.8),
            ..default()
        },
    );
    assert!((sampled.effective_aileron() - 0.401).abs() < 1e-14);
    assert!((sampled.effective_rudder() + 0.251).abs() < 1e-14);
    assert_eq!(sampled.throttle.value().to_bits(), 0.8_f64.to_bits());
    let mut gamepad = PilotControls::default();
    gamepad.update_with_gamepad(
        Seconds(0.5),
        keys,
        Some(PilotGamepad {
            left_stick_x: 0.4,
            right_stick_x: -0.25,
            ..default()
        }),
        &GamepadAxisMappings::default(),
    );
    assert_eq!(gamepad.aileron_trim, sampled.aileron_trim);
    assert_eq!(gamepad.rudder_trim, sampled.rudder_trim);
    assert!((gamepad.effective_aileron() - gamepad.aileron.value() - 0.001).abs() < 1e-14);
    assert!((gamepad.effective_rudder() - gamepad.rudder.value() + 0.001).abs() < 1e-14);
}

#[test]
fn throttle_changes_and_transient_release_never_adjust_or_center_trim() {
    let mut controls = PilotControls::default();
    controls.aileron_trim.set(0.0142);
    controls.rudder_trim.set(0.00109);
    let original = controls;
    for tick in 0..2400 {
        controls.update(
            FIXED_DT,
            PilotKeys {
                throttle_up: tick < 1200,
                throttle_down: tick >= 1200,
                roll_left: tick % 2 == 0,
                yaw_right: tick % 3 == 0,
                ..default()
            },
        );
        controls.release_transient_controls();
        assert_eq!(controls.aileron_trim, original.aileron_trim);
        assert_eq!(controls.rudder_trim, original.rudder_trim);
        assert_eq!(
            controls.effective_aileron().to_bits(),
            original.aileron_trim.value().to_bits()
        );
        assert_eq!(
            controls.effective_rudder().to_bits(),
            original.rudder_trim.value().to_bits()
        );
    }
}

#[test]
fn repeated_render_samples_do_not_advance_or_reset_persistent_trim() {
    let mut controls = PilotControls::default();
    controls.aileron_trim.set(0.0142);
    controls.rudder_trim.set(0.00109);
    let mut app = App::new();
    app.insert_resource(controls)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputDevices>()
        .init_resource::<InputSettings>()
        .init_resource::<SampledPilotInput>()
        .add_systems(Update, sample_pilot_input);
    for key in [
        KeyCode::KeyL,
        KeyCode::KeyU,
        KeyCode::ShiftRight,
        KeyCode::KeyK,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    for _ in 0..60 {
        app.update();
        let actual = app.world().resource::<PilotControls>();
        assert_eq!(actual.aileron_trim, controls.aileron_trim);
        assert_eq!(actual.rudder_trim, controls.rudder_trim);
    }
    let sample = *app.world().resource::<SampledPilotInput>();
    assert!(sample.keys.lateral_trim_reset && sample.keys.lateral_trim_fine);
    let mut actual = app.world_mut().resource_mut::<PilotControls>();
    actual.update_from_sample(FIXED_DT, &sample);
    assert_eq!(actual.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
    assert_eq!(actual.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
}

#[test]
fn reset_tap_without_control_step_expires_with_next_render_sample() {
    let mut controls = PilotControls::default();
    controls.aileron_trim.set(0.0142);
    controls.rudder_trim.set(-0.00109);
    let mut app = App::new();
    app.insert_resource(controls)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputDevices>()
        .init_resource::<InputSettings>()
        .init_resource::<SampledPilotInput>()
        .add_systems(Update, sample_pilot_input);
    {
        let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keyboard.press(KeyCode::KeyK);
        keyboard.release(KeyCode::KeyK);
    }
    // A no-step or suspended caller samples input without advancing controls.
    app.update();
    assert!(
        app.world()
            .resource::<SampledPilotInput>()
            .keys
            .lateral_trim_reset
    );
    let actual = app.world().resource::<PilotControls>();
    assert_eq!(actual.aileron_trim, controls.aileron_trim);
    assert_eq!(actual.rudder_trim, controls.rudder_trim);

    // Simulate Bevy's next-frame edge clearing before resuming fixed steps.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    let sample = *app.world().resource::<SampledPilotInput>();
    assert_eq!(sample.keys, PilotKeys::default());
    let mut actual = app.world_mut().resource_mut::<PilotControls>();
    actual.update_from_sample(FIXED_DT, &sample);
    assert_eq!(actual.aileron_trim, controls.aileron_trim);
    assert_eq!(actual.rudder_trim, controls.rudder_trim);
}

#[test]
fn legacy_json_configuration_keeps_the_same_schema_and_cannot_seed_trim() {
    let fixture = include_str!("../tests/fixtures/legacy-input-v1.json");
    let configuration = InputConfiguration::from_json(fixture).expect("old v1 fixture");
    assert!(configuration.view_cycle.is_none());
    assert!(
        configuration
            .bindings()
            .iter()
            .all(|(_, binding)| binding.is_none())
    );
    let saved = configuration.to_json().expect("unchanged schema output");
    assert!(!saved.contains("trim"));
    assert_eq!(
        InputConfiguration::from_json(&saved).unwrap(),
        configuration
    );
    let mut controls = PilotControls::default();
    let keys = PilotKeys {
        aileron_trim_right: true,
        rudder_trim_left: true,
        ..default()
    };
    controls.update_with_bindings(Seconds(1.0), keys, &configuration, &InputDevices::default());
    assert!((controls.aileron_trim.value() - 0.01).abs() < 1e-14);
    assert!((controls.rudder_trim.value() + 0.01).abs() < 1e-14);
    for field in ["aileron_trim", "rudder_trim", "lateral_trim_rate"] {
        let extended = fixture.replacen("{", &format!("{{\"{field}\":0.1,"), 1);
        assert!(InputConfiguration::from_json(&extended).is_err());
    }
}
