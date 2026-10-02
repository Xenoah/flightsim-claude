//! Adversarial keyboard tests through the same sampling system as the app.
use super::*;

fn app_with_controls(controls: PilotControls) -> App {
    let mut app = App::new();
    app.insert_resource(controls)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputDevices>()
        .init_resource::<InputSettings>()
        .init_resource::<SampledPilotInput>()
        .add_systems(Update, (sample_pilot_input, apply_two_fixed_steps).chain());
    app
}

fn apply_two_fixed_steps(sample: Res<SampledPilotInput>, mut controls: ResMut<PilotControls>) {
    // Test driver mirrors the app's ownership: input samples, simulation ticks.
    for _ in 0..2 {
        controls.update_from_sample(Seconds(1.0 / 120.0), &sample);
    }
}

#[test]
fn physical_pitch_aliases_and_signs_reach_the_runtime_sampler() {
    for (key, sign) in [
        (KeyCode::KeyS, 1.0),
        (KeyCode::ArrowDown, 1.0),
        (KeyCode::KeyW, -1.0),
        (KeyCode::ArrowUp, -1.0),
    ] {
        let mut app = app_with_controls(PilotControls::default());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let controls = app.world().resource::<PilotControls>();
        assert!(
            controls.elevator.value() * sign > 0.0,
            "wrong sign for {key:?}"
        );
        assert_eq!(controls.trim.value().to_bits(), 0.09_f64.to_bits());
    }
}

#[test]
fn runtime_release_centers_only_transient_controls_and_keeps_persistent_settings() {
    let mut app = app_with_controls(PilotControls::default());
    for key in [
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyQ,
        KeyCode::PageUp,
        KeyCode::BracketRight,
        KeyCode::KeyF,
        KeyCode::Space,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    for _ in 0..12 {
        app.update();
    }
    let before = *app.world().resource::<PilotControls>();
    assert!(before.elevator.value() > 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    for _ in 0..120 {
        app.update();
    }
    let after = *app.world().resource::<PilotControls>();
    for value in [
        after.elevator.value(),
        after.aileron.value(),
        after.rudder.value(),
        after.brakes,
    ] {
        assert!(value.abs() < 1e-12);
    }
    assert_eq!(after.trim, before.trim);
    assert_eq!(after.throttle, before.throttle);
    assert_eq!(after.flaps, before.flaps);
    assert_eq!(
        after.to_control_inputs().elevator().to_bits(),
        before.trim.value().to_bits()
    );
}

#[test]
fn keyboard_control_values_depend_on_elapsed_time_not_frame_count() {
    for hz in [15_u32, 30, 60, 144] {
        let mut controls = PilotControls::default();
        // A third-second hold stays below saturation, followed by enough
        // release time to center without overshoot. Persistent settings hold.
        for _ in 0..hz / 3 {
            controls.update_with_bindings(
                Seconds(1.0 / f64::from(hz)),
                PilotKeys {
                    pitch_up: true,
                    throttle_up: true,
                    trim_up: true,
                    ..default()
                },
                &InputConfiguration::default(),
                &InputDevices::default(),
            );
        }
        let hold_time = f64::from(hz / 3) / f64::from(hz);
        assert!((controls.elevator.value() - 2.5 * hold_time).abs() < 1e-12);
        assert!((controls.throttle.value() - 0.25 * hold_time).abs() < 1e-12);
        assert!((controls.trim.value() - 0.09 - 0.12 * hold_time).abs() < 1e-12);
        for _ in 0..hz {
            controls.update_with_bindings(
                Seconds(1.0 / f64::from(hz)),
                PilotKeys::default(),
                &InputConfiguration::default(),
                &InputDevices::default(),
            );
        }
        assert!(controls.elevator.value().abs() < 1e-12);
        assert!((controls.throttle.value() - 0.25 * hold_time).abs() < 1e-12);
        assert!((controls.trim.value() - 0.09 - 0.12 * hold_time).abs() < 1e-12);
    }
}

#[test]
fn an_opposite_key_unwinds_at_least_as_fast_as_release_in_both_directions() {
    for (rate, centering_rate) in [(0.2_f64, 1.8_f64), (1.8, 0.2)] {
        for sign in [-1.0, 1.0] {
            for elapsed in [0.0, 0.1, 0.2] {
                let mut reversing = AxisState::new(rate, centering_rate);
                reversing.set_absolute(sign * 0.6);
                let mut released = reversing;
                reversing.update(Seconds(elapsed), sign < 0.0, sign > 0.0);
                released.update(Seconds(elapsed), false, false);
                let expected = sign * (0.6 - rate.max(centering_rate) * elapsed);
                assert!((reversing.value() - expected).abs() < 1e-12);
                assert!(reversing.value().abs() <= released.value().abs() + 1e-12);
            }
        }
    }
}

#[test]
fn slow_axis_reversal_splits_at_neutral_without_amplifying_opposite_authority() {
    // 0.45 / 1.8 = 0.25 s to neutral. Afterward, only 0.2 per second applies.
    for sign in [-1.0, 1.0] {
        for (elapsed, expected) in [(0.125, 0.225), (0.25, 0.0), (0.5, -0.05), (6.0, -1.0)] {
            let mut axis = AxisState::new(0.2, 1.8);
            axis.set_absolute(sign * 0.45);
            axis.update(Seconds(elapsed), sign < 0.0, sign > 0.0);
            assert!((axis.value() - sign * expected).abs() < 1e-12);
        }
    }
}

#[test]
fn axis_reversal_and_release_are_independent_of_time_partitioning() {
    for (rate, centering_rate) in [(0.2, 1.8), (1.8, 0.2), (2.5, 1.8), (0.2, 0.0), (0.2, 0.2)] {
        for initial in [-1.0, -0.91, -0.4, -0.03, 0.0, 0.03, 0.4, 0.91, 1.0] {
            for (positive, negative) in [(true, false), (false, true), (true, true), (false, false)]
            {
                for ticks in [0, 1, 2, 5, 24, 60, 121, 360, 1200] {
                    let mut combined = AxisState::new(rate, centering_rate);
                    combined.set_absolute(initial);
                    let mut partitioned = combined;
                    combined.update(Seconds(f64::from(ticks) / 120.0), positive, negative);
                    for _ in 0..ticks {
                        partitioned.update(Seconds(1.0 / 120.0), positive, negative);
                    }
                    assert!(
                        (combined.value() - partitioned.value()).abs() < 1e-12,
                        "rate={rate}, centering={centering_rate}, initial={initial}, keys=({positive}, {negative}), ticks={ticks}: combined={}, partitioned={}",
                        combined.value(),
                        partitioned.value()
                    );
                    assert!((-1.0..=1.0).contains(&combined.value()));
                    assert!((-1.0..=1.0).contains(&partitioned.value()));
                }
            }
        }
    }
}

#[test]
fn axes_with_a_faster_command_rate_keep_the_existing_arithmetic() {
    for (rate, centering_rate) in [(1.8, 0.2), (2.5, 1.8), (0.2, 0.0), (0.2, 0.2)] {
        for initial in [-1.0_f64, -0.45, 0.0, 0.45, 1.0] {
            for (positive, negative) in [(true, false), (false, true), (true, true), (false, false)]
            {
                for elapsed in [0.0, 1.0 / 120.0, 0.125, 0.5, 6.0] {
                    let direction = f64::from(i8::from(positive) - i8::from(negative));
                    let expected = if direction.abs() > 0.0 {
                        sanitise(initial + direction * (rate * elapsed))
                    } else if initial.abs() <= centering_rate * elapsed {
                        0.0
                    } else {
                        initial - initial.signum() * (centering_rate * elapsed)
                    };
                    let mut axis = AxisState::new(rate, centering_rate);
                    axis.set_absolute(initial);
                    axis.update(Seconds(elapsed), positive, negative);
                    assert_eq!(axis.value().to_bits(), expected.to_bits());
                }
            }
        }
    }
}

#[test]
fn reversal_ignores_zero_and_invalid_elapsed_time() {
    for elapsed in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        // The smallest subnormal divided by 2 underflows to zero; a
        // time-to-neutral comparison alone would erase it on a zero-dt step.
        for initial in [-0.45, -f64::from_bits(1), f64::from_bits(1), 0.45] {
            let mut axis = AxisState::new(0.2, 2.0);
            axis.set_absolute(initial);
            let original = axis;
            axis.update(Seconds(elapsed), initial < 0.0, initial > 0.0);
            assert_eq!(axis, original);
        }
    }
}

#[test]
fn runtime_opposite_pitch_keys_use_fast_return_without_changing_persistent_settings() {
    for (key, sign) in [(KeyCode::KeyW, 1.0), (KeyCode::KeyS, -1.0)] {
        let mut initial = PilotControls {
            elevator: AxisState::new(0.2, 1.8),
            ..default()
        };
        initial.elevator.set_absolute(sign * 0.45);
        initial.throttle.set_absolute(0.7);
        initial.flaps.set_absolute(0.3);
        initial.trim.set(0.12);
        let mut app = app_with_controls(initial);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        // Two 120 Hz simulation steps per render update; 0.5 s elapsed.
        for _ in 0..30 {
            app.update();
        }
        let after = *app.world().resource::<PilotControls>();
        assert!((after.elevator.value() + sign * 0.05).abs() < 1e-12);
        assert_eq!(after.trim, initial.trim);
        assert_eq!(after.throttle, initial.throttle);
        assert_eq!(after.flaps, initial.flaps);
        assert!((after.effective_elevator() - (0.12 - sign * 0.05)).abs() < 1e-12);
    }
}

#[test]
fn absolute_controller_values_still_bypass_keyboard_reversal_rates() {
    for value in [-1.0, -0.45, 0.0, 0.45, 1.0] {
        for elapsed in [0.0, 1.0 / 120.0, 0.5] {
            let mut controls = PilotControls {
                elevator: AxisState::new(0.2, 1.8),
                ..default()
            };
            controls.elevator.set_absolute(0.45);
            controls.update_from_sample(
                Seconds(elapsed),
                &SampledPilotInput {
                    keys: PilotKeys {
                        pitch_down: true,
                        ..default()
                    },
                    pitch: Some(controllers::MappedControl {
                        value,
                        mode: configuration::BindingMode::Absolute,
                        active: true,
                    }),
                    ..default()
                },
            );
            assert_eq!(controls.elevator.value().to_bits(), value.to_bits());
        }
    }
}

#[test]
fn rate_controller_bindings_keep_their_existing_fractional_rate_semantics() {
    for value in [-1.0, -0.4, 0.0, 0.4, 1.0] {
        let mut controls = PilotControls {
            elevator: AxisState::new(0.2, 1.8),
            ..default()
        };
        controls.elevator.set_absolute(0.45);
        controls.update_from_sample(
            Seconds(0.5),
            &SampledPilotInput {
                keys: PilotKeys {
                    pitch_down: true,
                    ..default()
                },
                pitch: Some(controllers::MappedControl {
                    value,
                    mode: configuration::BindingMode::Rate,
                    active: true,
                }),
                ..default()
            },
        );
        // A nonzero rate binding applies its fraction directly; zero centers.
        let expected = if value.abs() > 0.0 {
            0.45 + value * 0.2 * 0.5
        } else {
            0.0
        };
        assert_eq!(controls.elevator.value().to_bits(), expected.to_bits());
    }
}

#[test]
fn invalid_elapsed_time_must_not_move_or_poison_any_axis() {
    for dt in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut surface = AxisState::control_surface();
        surface.set_absolute(0.4);
        let original = surface;
        surface.update(Seconds(dt), false, false);
        assert_eq!(surface, original, "centering dt={dt}");
        surface.update(Seconds(dt), true, false);
        assert_eq!(surface, original, "deflection dt={dt}");
        let mut ramp = RampAxis::new(0.5, 0.25);
        let original = ramp;
        ramp.update(Seconds(dt), true, false);
        assert_eq!(ramp, original, "ramp dt={dt}");
        ramp.update_analog(Seconds(dt), -0.5);
        assert_eq!(ramp, original, "analog ramp dt={dt}");
        let mut trim = ElevatorTrim::default();
        let original = trim;
        trim.update(Seconds(dt), true, false);
        assert_eq!(trim, original, "trim dt={dt}");
    }
}

#[test]
fn explicit_release_is_idempotent_and_preserves_all_persistent_settings() {
    let mut controls = PilotControls::default();
    controls.update(
        Seconds(0.25),
        PilotKeys {
            pitch_up: true,
            roll_left: true,
            yaw_right: true,
            brakes: true,
            throttle_up: true,
            flaps_extend: true,
            trim_down: true,
            ..default()
        },
    );
    let original = controls;
    for _ in 0..3 {
        controls.release_transient_controls();
        for value in [
            controls.aileron.value(),
            controls.elevator.value(),
            controls.rudder.value(),
            controls.brakes,
        ] {
            assert!(value.abs() < 1e-12);
        }
        assert_eq!(controls.trim, original.trim);
        assert_eq!(controls.throttle, original.throttle);
        assert_eq!(controls.flaps, original.flaps);
        assert_eq!(
            controls.effective_elevator().to_bits(),
            original.trim.value().to_bits()
        );
    }
    // Explicit reset is not a latch. A fresh authorized sample acts normally.
    controls.update(
        Seconds(0.1),
        PilotKeys {
            pitch_down: true,
            ..default()
        },
    );
    assert!((controls.elevator.value() + 0.25).abs() < 1e-12);
}

#[test]
fn sampling_does_not_advance_controls_before_a_simulation_step() {
    let mut app = App::new();
    app.init_resource::<PilotControls>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputDevices>()
        .init_resource::<InputSettings>()
        .init_resource::<SampledPilotInput>()
        .add_systems(Update, sample_pilot_input);
    for key in [KeyCode::KeyS, KeyCode::PageUp, KeyCode::BracketRight] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    let initial = app.world().resource::<PilotControls>().to_control_inputs();
    // Any number of render samples, including while paused, does not move ramps.
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<PilotControls>().to_control_inputs(),
        initial
    );
    let sample = *app.world().resource::<SampledPilotInput>();
    assert!(sample.keys.pitch_up && sample.keys.throttle_up && sample.keys.trim_up);
    let mut controls = app.world_mut().resource_mut::<PilotControls>();
    for tick in 1..=12 {
        controls.update_from_sample(Seconds(1.0 / 120.0), &sample);
        assert!((controls.elevator.value() - 2.5 * f64::from(tick) / 120.0).abs() < 1e-12);
        assert!((controls.throttle.value() - 0.25 * f64::from(tick) / 120.0).abs() < 1e-12);
        assert!((controls.trim.value() - 0.09 - 0.12 * f64::from(tick) / 120.0).abs() < 1e-12);
    }
}

#[test]
fn a_subframe_press_and_release_is_not_fabricated_as_a_held_command() {
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::PageUp);
    keyboard.release(KeyCode::PageUp);
    keyboard.press(KeyCode::KeyS);
    keyboard.release(KeyCode::KeyS);
    let keys = PilotKeys::from_keyboard(&keyboard);
    assert!(!keys.throttle_up && !keys.pitch_up);
    // There is no event timestamp. Giving a full render frame of authority to a
    // released key would over-command short taps, especially at low frame rates.
}

#[test]
fn initial_ramp_value_is_finite_for_untrusted_values() {
    for initial in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let ramp = RampAxis::new(initial, 0.25);
        assert!((0.0..=1.0).contains(&ramp.value()));
    }
}
