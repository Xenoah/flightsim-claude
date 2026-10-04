//! Original numerical fixtures, not performance data for a real aircraft.
use flightsim_core::{Attitude, Ecef, Geodetic, Meters, Ned, Seconds};
use flightsim_fdm::{
    Atmosphere, ControlInputs, Environment, GroundSlope, RigidBodyState,
    definition::{AerodynamicDefinition, GearDefinition},
    state,
    subsonic::{
        AirframeDefinition, AxisStatus, DryJetDefinition, DryJetTable, JetAircraftConfig,
        JetFailureReason, JetFlightDynamics, JetInvalidInput, JetStage, MAX_JET_STEP_DT,
        MachAeroDefinition, MachAeroKnotDefinition, MachAeroSchedule, NetThrustCellDefinition,
        OperatingEnvelope, OperatingEnvelopeDefinition,
    },
};
use glam::{DQuat, DVec3};

fn airframe() -> AirframeDefinition {
    let gear = |x, y| GearDefinition {
        contact_m: [x, y, 1.0],
        spring_n_per_m: 120_000.0,
        damping_ns_per_m: 13_000.0,
        max_stroke_m: 0.25,
        bottom_stop_travel_m: 0.05,
        max_recoil_mps: 0.5,
    };
    AirframeDefinition {
        name: "Original analytic jet fixture".into(),
        mass_kg: 1000.0,
        inertia_kg_m2: [2000.0, 3000.0, 4000.0, 0.0],
        wing_area_m2: 16.0,
        wing_span_m: 10.0,
        mean_chord_m: 1.6,
        landing_gear: [gear(1.6, 0.0), gear(-0.8, -1.3), gear(-0.8, 1.3)],
        rolling_friction: 0.015,
        braking_friction: 0.7,
        lateral_friction: 0.8,
        friction_transition_mps: 0.25,
    }
}
fn coefficients(mach: f64) -> AerodynamicDefinition {
    AerodynamicDefinition {
        lift_zero: 0.0,
        lift_alpha: 0.0,
        lift_flaps: 0.0,
        stall_angle_rad: 0.3,
        stall_blend_rate: 20.0,
        drag_min: 0.02 + 0.03 * mach,
        oswald_efficiency: 0.8,
        drag_flaps: 0.0,
        side_beta: 0.0,
        side_rudder: 0.0,
        roll_beta: 0.0,
        roll_rate_p: 0.0,
        roll_rate_r: 0.0,
        roll_aileron: 0.2 + mach * 0.1,
        roll_rudder: 0.0,
        pitch_zero: 0.0,
        pitch_alpha: 0.0,
        pitch_rate_q: 0.0,
        pitch_elevator: 0.1 + mach * 0.2,
        pitch_flaps: 0.0,
        yaw_beta: 0.0,
        yaw_rate_p: 0.0,
        yaw_rate_r: 0.0,
        yaw_aileron: 0.0,
        yaw_rudder: 0.3 + mach * 0.15,
    }
}
fn thrust_polynomial(p: f64, t: f64, m: f64, command: f64) -> f64 {
    let idle = p * (-100.0 + 30.0 * t - 20.0 * m + 10.0 * t * m);
    let dry = p * (10000.0 - 500.0 * t + 1200.0 * m - 100.0 * t * m);
    (1.0 - command) * idle + command * dry
}
fn table() -> DryJetTable {
    let pressure_ratios = vec![0.0, 1.0, 2.0];
    let temperature_ratios = vec![0.25, 1.0, 2.0];
    let mach = vec![0.0, 0.4, 0.95];
    let mut cells = Vec::new();
    for &p in &pressure_ratios {
        for &t in &temperature_ratios {
            for &m in &mach {
                cells.push(NetThrustCellDefinition {
                    idle_n: thrust_polynomial(p, t, m, 0.0),
                    maximum_dry_n: thrust_polynomial(p, t, m, 1.0),
                });
            }
        }
    }
    DryJetTable::from_definition(DryJetDefinition {
        schema: 1,
        pressure_ratios,
        temperature_ratios,
        mach,
        cells,
    })
    .unwrap()
}
fn schedule() -> MachAeroSchedule {
    MachAeroSchedule::from_definition(MachAeroDefinition {
        schema: 1,
        knots: [0.0, 0.4, 0.95]
            .into_iter()
            .map(|mach| MachAeroKnotDefinition {
                mach,
                aero: coefficients(mach),
            })
            .collect(),
    })
    .unwrap()
}
fn envelope() -> OperatingEnvelopeDefinition {
    OperatingEnvelopeDefinition {
        pressure_ratio: [0.0, 2.0],
        temperature_ratio: [0.25, 2.0],
        mach: [0.0, 0.95],
    }
}
fn config(bounds: OperatingEnvelopeDefinition) -> JetAircraftConfig {
    JetAircraftConfig::new(
        airframe().to_config().unwrap(),
        table(),
        schedule(),
        OperatingEnvelope::from_definition(bounds).unwrap(),
    )
    .unwrap()
}
fn level(height: f64, speed: f64) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0.0, 0.0, height),
        Attitude::default(),
        Ned::new(speed, 0.0, 0.0),
    )
}
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual:.16e} != {expected:.16e} (difference {})",
        actual - expected
    );
}
fn mach(state: &RigidBodyState) -> f64 {
    let air = Atmosphere::standard().sample(state.altitude());
    state.body_velocity().length() / air.speed_of_sound.get()
}

#[test]
fn analytic_net_thrust_drag_and_control_moments_are_independent_of_legacy_propulsion() {
    let controls = ControlInputs::new(0.1, 0.2, 0.3, 0.75, 0.0);
    for altitude in [0.0, 3500.0, 10000.0] {
        for speed in [0.0, 70.0, 220.0] {
            let start = level(altitude, speed);
            let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
            let env = Environment::still_air().with_ground(Meters(-1000.0));
            let actual = model.derivative(&start, controls, &env).unwrap();
            let air = Atmosphere::standard().sample(start.altitude());
            let m = speed / air.speed_of_sound.get();
            let thrust = thrust_polynomial(
                air.pressure.get() / 101325.0,
                air.temperature.get() / 288.15,
                m,
                0.75,
            );
            // At alpha=0, the linear drag's attached fraction is logistic(6)^2.
            let attached = 1.0 / (1.0 + (-6.0_f64).exp()).powi(2);
            let qs = 0.5 * air.density.get() * speed * speed * 16.0;
            let forward = start.orientation * DVec3::X;
            close(
                actual.acceleration.dot(forward),
                (thrust - qs * (0.02 + 0.03 * m) * attached) / 1000.0,
                1e-10,
            );
            close(
                actual.angular_acceleration.x,
                qs * 10.0 * (0.2 + 0.1 * m) * 0.1 / 2000.0,
                1e-10,
            );
            close(
                actual.angular_acceleration.y,
                qs * 1.6 * (0.1 + 0.2 * m) * 0.2 / 3000.0,
                1e-10,
            );
            close(
                actual.angular_acceleration.z,
                qs * 10.0 * (0.3 + 0.15 * m) * 0.3 / 4000.0,
                1e-10,
            );
        }
    }
}

#[test]
fn each_rk4_stage_requeries_the_table_and_schedule() {
    let start = level(2500.0, 120.0);
    let controls = ControlInputs::new(0.005, 0.01, -0.005, 0.8, 0.0);
    let env = Environment::still_air();
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let h = 1.0 / 120.0;
    // Independently arrange classical RK4 using the public instantaneous law.
    // A frozen force/coefficient implementation disagrees with this trajectory.
    let k1 = model.derivative(&start, controls, &env).unwrap();
    let k2 = model
        .derivative(&state::offset(&start, &k1, h / 2.0), controls, &env)
        .unwrap();
    let k3 = model
        .derivative(&state::offset(&start, &k2, h / 2.0), controls, &env)
        .unwrap();
    let k4 = model
        .derivative(&state::offset(&start, &k3, h), controls, &env)
        .unwrap();
    let expected = state::offset(&start, &((k1 + k2 * 2.0 + k3 * 2.0 + k4) * (1.0 / 6.0)), h);
    let frozen = state::offset(&start, &k1, h);
    assert_eq!(model.step(Seconds(h), controls, &env).unwrap().substeps, 1);
    assert_eq!(*model.state(), expected);
    assert!(model.state().velocity.distance(frozen.velocity) > 1e-6);
}

#[test]
fn unsupported_k2_query_rolls_back_with_finite_query_diagnostics() {
    let start = level(1000.0, 100.0);
    let mut bounds = envelope();
    bounds.mach[1] = mach(&start) + 1e-6;
    let mut model = JetFlightDynamics::new(config(bounds), start).unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral().with_throttle(1.0),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert!(
        matches!(error.reason,JetFailureReason::OutsideOperatingEnvelope(status) if status.mach==AxisStatus::Above)
    );
    assert_eq!(error.stage, JetStage::K2);
    assert_eq!(error.substep, 0);
    assert!(error.query.unwrap().mach.0.is_finite());
    assert_eq!(*model.state(), start);
}

#[test]
fn failing_later_substep_rolls_back_the_entire_requested_duration() {
    let start = level(1000.0, 100.0);
    let env = Environment::still_air();
    let controls = ControlInputs::neutral().with_throttle(1.0);
    let mut wide = JetFlightDynamics::new(config(envelope()), start).unwrap();
    wide.step(Seconds(1.0 / 120.0), controls, &env).unwrap();
    let after_first = *wide.state();
    let mut bounds = envelope();
    bounds.mach[1] = mach(&after_first) + 1e-5;
    let mut model = JetFlightDynamics::new(config(bounds), start).unwrap();
    let error = model
        .step(Seconds(3.0 / 120.0), controls, &env)
        .unwrap_err();
    assert!(error.substep >= 1, "{error:?}");
    assert!(matches!(
        error.reason,
        JetFailureReason::OutsideOperatingEnvelope(_)
    ));
    assert_eq!(*model.state(), start);
    // Confirm the earlier substep was actually supportable under the same box.
    model.step(Seconds(1.0 / 120.0), controls, &env).unwrap();
    assert_eq!(*model.state(), after_first);
}

#[test]
fn ordinary_airborne_controls_are_deterministic_and_finite() {
    let start = level(8000.0, 180.0);
    let mut a = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let mut b = a.clone();
    for i in 0..1200 {
        let controls = ControlInputs::new(
            0.002 * (f64::from(i) / 100.0).sin(),
            0.002,
            -0.001,
            0.5,
            0.0,
        );
        a.step(Seconds(1.0 / 120.0), controls, &Environment::still_air())
            .unwrap();
        b.step(Seconds(1.0 / 120.0), controls, &Environment::still_air())
            .unwrap();
        assert_eq!(a.state(), b.state());
        assert!(a.state().is_finite());
        close(a.state().orientation.length(), 1.0, 1e-12);
    }
}

#[test]
fn stationary_supported_gear_uses_weight_over_spring_compression() {
    let height = 1.0 - 1000.0 * 9.7803253359 / (3.0 * 120000.0);
    let start = level(height, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    // At rest aerodynamic forces are zero; idle is small signed net drag.
    let derivative = model
        .derivative(&start, ControlInputs::neutral(), &Environment::still_air())
        .unwrap();
    let upward = start
        .local_frame()
        .ned_to_ecef_vector(Ned::new(0.0, 0.0, -1.0));
    close(derivative.acceleration.dot(upward), 0.0, 0.001);
    for _ in 0..240 {
        let report = model
            .step(
                Seconds(1.0 / 120.0),
                ControlInputs::neutral(),
                &Environment::still_air(),
            )
            .unwrap();
        assert!(report.substeps > 1 && report.substeps <= 8);
    }
    assert!(model.state().velocity.length() < 0.2);
    assert!((model.state().altitude().get() - height).abs() < 0.002);
}

#[test]
fn initial_duration_and_rotation_budgets_reject_without_clamping() {
    let start = level(3000.0, 0.0);
    let controls = ControlInputs::neutral();
    let env = Environment::still_air();
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    assert_eq!(
        model
            .step(MAX_JET_STEP_DT, controls, &env)
            .unwrap()
            .substeps,
        8
    );
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let too_long = Seconds(f64::from_bits(MAX_JET_STEP_DT.get().to_bits() + 1));
    assert_eq!(
        model.step(too_long, controls, &env).unwrap_err().reason,
        JetFailureReason::SubstepBudgetExceeded
    );
    assert_eq!(*model.state(), start);
    let mut rotating = start;
    rotating.angular_velocity = DVec3::new(49.0, 0.0, 0.0);
    model.set_state(rotating).unwrap();
    let error = model
        .step(Seconds(1.0 / 120.0), controls, &env)
        .unwrap_err();
    assert_eq!(error.reason, JetFailureReason::SubstepBudgetExceeded);
    assert_eq!(error.stage, JetStage::Initial);
    assert_eq!(*model.state(), rotating);
}

#[test]
fn later_stage_rotation_budget_rejects_whole_step() {
    let mut start = level(10000.0, 250.0);
    start.angular_velocity = DVec3::new(5.9, 0.0, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral().with_aileron(1.0),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(error.reason, JetFailureReason::SubstepBudgetExceeded);
    assert_ne!(error.stage, JetStage::Initial);
    assert_eq!(*model.state(), start);
}

#[test]
fn invalid_state_environment_and_raw_relative_speed_never_become_zero_force() {
    let start = level(1000.0, 100.0);
    let base = config(envelope());
    for quaternion in [
        DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0),
        DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0),
    ] {
        let mut bad = start;
        bad.orientation = quaternion;
        assert_eq!(
            JetFlightDynamics::new(base.clone(), bad)
                .unwrap_err()
                .reason,
            JetFailureReason::InvalidInput(JetInvalidInput::Quaternion)
        );
    }
    let mut model = JetFlightDynamics::new(base, start).unwrap();
    let mut bad = start;
    bad.velocity.x = f64::NAN;
    assert!(model.set_state(bad).is_err());
    assert_eq!(*model.state(), start);
    let mut environments = Vec::new();
    for offset in [f64::NAN, f64::INFINITY, -1000.0] {
        let mut env = Environment::still_air();
        env.atmosphere = Atmosphere::with_temperature_offset(offset);
        environments.push(env);
    }
    for wind in [DVec3::splat(f64::INFINITY), DVec3::splat(1e308)] {
        let mut env = Environment::still_air();
        env.wind_ecef = wind;
        environments.push(env);
    }
    environments.push(Environment::still_air().with_ground(Meters(f64::NAN)));
    environments.push(Environment::still_air().with_ground_plane(
        Geodetic::from_degrees(0.0, 0.0, 0.0),
        Meters(0.0),
        GroundSlope::new(1e308, 0.0),
    ));
    for env in environments {
        let error = model
            .step(Seconds(1.0 / 120.0), ControlInputs::neutral(), &env)
            .unwrap_err();
        assert!(
            matches!(error.reason, JetFailureReason::InvalidInput(_)),
            "{error:?}"
        );
        assert_eq!(*model.state(), start);
    }
    for dt in [f64::NAN, f64::INFINITY, -0.01] {
        assert_eq!(
            model
                .step(
                    Seconds(dt),
                    ControlInputs::neutral(),
                    &Environment::still_air()
                )
                .unwrap_err()
                .reason,
            JetFailureReason::InvalidInput(JetInvalidInput::TimeStep)
        );
    }
    let mut huge = start;
    huge.velocity = DVec3::splat(1e308);
    assert!(model.set_state(huge).is_err());
    huge.velocity = DVec3::new(1e154, 0.0, 0.0);
    model.set_state(huge).unwrap();
    let mut opposite = Environment::still_air();
    opposite.wind_ecef = -huge.velocity;
    assert_eq!(
        model
            .step(Seconds(0.01), ControlInputs::neutral(), &opposite)
            .unwrap_err()
            .reason,
        JetFailureReason::InvalidInput(JetInvalidInput::RelativeVelocity)
    );
    assert_eq!(*model.state(), huge);
}

#[test]
fn altitude_and_temperature_are_rejected_before_legacy_sample_clamps() {
    for (height, side) in [(-5001.0, AxisStatus::Below), (86001.0, AxisStatus::Above)] {
        assert_eq!(
            JetFlightDynamics::new(config(envelope()), level(height, 0.0))
                .unwrap_err()
                .reason,
            JetFailureReason::OutsideAtmosphereAltitude(side)
        );
    }
    for height in [-5000.0, 86000.0] {
        JetFlightDynamics::new(config(envelope()), level(height, 0.0)).unwrap();
    }
    let start = level(0.0, 0.0);
    let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let mut env = Environment::still_air();
    env.atmosphere = Atmosphere::with_temperature_offset(-288.15);
    assert_eq!(
        model
            .derivative(&start, ControlInputs::neutral(), &env)
            .unwrap_err()
            .reason,
        JetFailureReason::InvalidInput(JetInvalidInput::AtmosphereTemperature)
    );
    let mut center = start;
    center.position = Ecef(DVec3::ZERO);
    assert!(JetFlightDynamics::new(config(envelope()), center).is_err());
}

#[test]
fn zero_duration_is_validated_and_preserves_bits() {
    let start = level(1500.0, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    assert_eq!(
        model
            .step(
                Seconds(0.0),
                ControlInputs::neutral(),
                &Environment::still_air()
            )
            .unwrap()
            .substeps,
        0
    );
    assert_eq!(*model.state(), start);
    let mut env = Environment::still_air();
    env.wind_ecef = DVec3::splat(f64::NAN);
    assert!(
        model
            .step(Seconds(0.0), ControlInputs::neutral(), &env)
            .is_err()
    );
    assert_eq!(*model.state(), start);
}

#[test]
fn neutral_config_validates_physics_and_envelope_containment_without_propeller() {
    let original = airframe();
    let restored = AirframeDefinition::from_config(&original.to_config().unwrap());
    assert_eq!(
        serde_json::to_string(&original).unwrap(),
        serde_json::to_string(&restored).unwrap()
    );
    let mut invalid = airframe();
    invalid.mass_kg = f64::NAN;
    assert!(invalid.to_config().is_err());
    let mut invalid = airframe();
    invalid.inertia_kg_m2 = [1.0, 1.0, 1.0, 1.0];
    assert!(invalid.to_config().is_err());
    let mut invalid = airframe();
    invalid.landing_gear[0].spring_n_per_m = f64::INFINITY;
    assert!(invalid.to_config().is_err());
    let mut invalid = airframe();
    invalid.wing_area_m2 = 0.0;
    assert!(invalid.to_config().is_err());
    let mut bounds = envelope();
    bounds.mach = [0.3, 0.3];
    assert!(OperatingEnvelope::from_definition(bounds).is_err());
    let mut bounds = envelope();
    bounds.temperature_ratio[0] = f64::NAN;
    assert!(OperatingEnvelope::from_definition(bounds).is_err());
    let mut definition = schedule().definition().clone();
    definition.knots.pop();
    let narrow = MachAeroSchedule::from_definition(definition).unwrap();
    assert!(
        JetAircraftConfig::new(
            airframe().to_config().unwrap(),
            table(),
            narrow,
            OperatingEnvelope::from_definition(envelope()).unwrap()
        )
        .is_err()
    );
    let mut definition = table().definition().clone();
    definition.temperature_ratios = [0.5, 1.0, 2.0].to_vec();
    let narrow = DryJetTable::from_definition(definition).unwrap();
    assert!(
        JetAircraftConfig::new(
            airframe().to_config().unwrap(),
            narrow,
            schedule(),
            OperatingEnvelope::from_definition(envelope()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn endpoint_is_validated_even_when_every_rk4_stage_is_supported() {
    let h = 1.0 / 120.0;
    let mut start = level(3000.0, 10.0);
    start.angular_velocity = DVec3::new(0.0, 5.0, 0.0);
    let controls = ControlInputs::neutral()
        .with_throttle(0.7)
        .with_elevator(-0.4);
    let env = Environment::still_air();
    let mut wide = JetFlightDynamics::new(config(envelope()), start).unwrap();
    wide.step(Seconds(h), controls, &env).unwrap();
    let k1 = wide.derivative(&start, controls, &env).unwrap();
    let s2 = state::offset(&start, &k1, h / 2.0);
    let k2 = wide.derivative(&s2, controls, &env).unwrap();
    let s3 = state::offset(&start, &k2, h / 2.0);
    let k3 = wide.derivative(&s3, controls, &env).unwrap();
    let s4 = state::offset(&start, &k3, h);
    let pressure = |state: &RigidBodyState| {
        Atmosphere::standard()
            .sample(state.altitude())
            .pressure
            .get()
            / 101325.0
    };
    let max_stage = [start, s2, s3, s4]
        .iter()
        .map(pressure)
        .fold(f64::NEG_INFINITY, f64::max);
    let endpoint_pressure = pressure(wide.state());
    assert!(endpoint_pressure > max_stage + 1e-12);
    let mut bounds = envelope();
    bounds.pressure_ratio[1] = (max_stage + endpoint_pressure) / 2.0;
    let mut narrow = JetFlightDynamics::new(config(bounds), start).unwrap();
    for stage in [start, s2, s3, s4] {
        narrow.derivative(&stage, controls, &env).unwrap();
    }
    let error = narrow.step(Seconds(h), controls, &env).unwrap_err();
    assert_eq!(error.stage, JetStage::Endpoint);
    assert!(
        matches!(error.reason,JetFailureReason::OutsideOperatingEnvelope(status) if status.pressure==AxisStatus::Above)
    );
    assert_eq!(*narrow.state(), start);
}

#[test]
fn k3_and_k4_failures_also_preserve_original_state() {
    let start = level(1000.0, 100.0);
    let controls = ControlInputs::neutral().with_throttle(1.0);
    let env = Environment::still_air();
    let h = 1.0 / 120.0;
    let wide = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let k1 = wide.derivative(&start, controls, &env).unwrap();
    let s2 = state::offset(&start, &k1, h / 2.0);
    let k2 = wide.derivative(&s2, controls, &env).unwrap();
    let s3 = state::offset(&start, &k2, h / 2.0);
    let k3 = wide.derivative(&s3, controls, &env).unwrap();
    let s4 = state::offset(&start, &k3, h);
    let pressure = |state: &RigidBodyState| {
        Atmosphere::standard()
            .sample(state.altitude())
            .pressure
            .get()
            / 101325.0
    };
    let mut k3_bounds = envelope();
    k3_bounds.pressure_ratio[1] = (pressure(&s2) + pressure(&s3)) / 2.0;
    let mut k4_bounds = envelope();
    k4_bounds.mach[1] = (mach(&s2).max(mach(&s3)) + mach(&s4)) / 2.0;
    for (bounds, stage) in [(k3_bounds, JetStage::K3), (k4_bounds, JetStage::K4)] {
        let mut model = JetFlightDynamics::new(config(bounds), start).unwrap();
        let error = model.step(Seconds(h), controls, &env).unwrap_err();
        assert_eq!(error.stage, stage);
        assert_eq!(*model.state(), start);
    }
}

#[test]
fn exact_envelope_edges_are_supported_and_next_representable_values_reject() {
    let start = level(1000.0, 100.0);
    let env = Environment::still_air();
    let air = env.atmosphere.sample(start.altitude());
    let mut exact = envelope();
    exact.pressure_ratio[1] = air.pressure.get() / 101325.0;
    exact.temperature_ratio[1] = air.temperature.get() / 288.15;
    exact.mach[1] = mach(&start);
    let mut model = JetFlightDynamics::new(config(exact), start).unwrap();
    model
        .step(Seconds(0.0), ControlInputs::neutral(), &env)
        .unwrap();
    for axis in 0..3 {
        let mut outside = exact;
        let bound = match axis {
            0 => &mut outside.pressure_ratio[1],
            1 => &mut outside.temperature_ratio[1],
            _ => &mut outside.mach[1],
        };
        *bound = f64::from_bits(bound.to_bits() - 1);
        let mut model = JetFlightDynamics::new(config(outside), start).unwrap();
        let error = model
            .step(Seconds(0.0), ControlInputs::neutral(), &env)
            .unwrap_err();
        assert!(matches!(
            error.reason,
            JetFailureReason::OutsideOperatingEnvelope(_)
        ));
        assert_eq!(*model.state(), start);
    }
}

#[test]
fn rotation_phase_boundary_and_stiff_gear_budget_are_explicit() {
    let mut start = level(3000.0, 0.0);
    start.angular_velocity = DVec3::new(48.0, 0.0, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    assert_eq!(
        model
            .step(
                Seconds(1.0 / 120.0),
                ControlInputs::neutral(),
                &Environment::still_air()
            )
            .unwrap()
            .substeps,
        8
    );
    start.angular_velocity.x = 48.000001;
    model.set_state(start).unwrap();
    assert_eq!(
        model
            .step(
                Seconds(1.0 / 120.0),
                ControlInputs::neutral(),
                &Environment::still_air()
            )
            .unwrap_err()
            .reason,
        JetFailureReason::SubstepBudgetExceeded
    );
    assert_eq!(*model.state(), start);
    let mut structure = airframe();
    for gear in &mut structure.landing_gear {
        gear.spring_n_per_m = 1e8;
    }
    let stiff = JetAircraftConfig::new(
        structure.to_config().unwrap(),
        table(),
        schedule(),
        OperatingEnvelope::from_definition(envelope()).unwrap(),
    )
    .unwrap();
    let start = level(0.99, 0.0);
    let mut model = JetFlightDynamics::new(stiff, start).unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(error.reason, JetFailureReason::SubstepBudgetExceeded);
    assert_eq!(error.stage, JetStage::Initial);
    assert_eq!(*model.state(), start);
}

#[test]
fn huge_ground_speed_canceled_by_wind_cannot_hide_gear_overflow() {
    let start = level(0.99, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let mut huge = start;
    huge.velocity = huge
        .local_frame()
        .ned_to_ecef_vector(Ned::new(0.0, 0.0, 1e308));
    let mut env = Environment::still_air();
    env.wind_ecef = huge.velocity;
    assert_eq!(huge.velocity - env.wind_ecef, DVec3::ZERO);
    assert_eq!(
        model
            .derivative(&huge, ControlInputs::neutral(), &env)
            .unwrap_err()
            .reason,
        JetFailureReason::InvalidInput(JetInvalidInput::State)
    );
    assert!(model.set_state(huge).is_err());
    assert_eq!(*model.state(), start);
    assert!(
        model
            .step(Seconds(0.0), ControlInputs::neutral(), &env)
            .is_err()
    );
    assert_eq!(*model.state(), start);
    let mut huge_rotation = start;
    huge_rotation.angular_velocity = DVec3::splat(1e308);
    assert_eq!(
        model.set_state(huge_rotation).unwrap_err().reason,
        JetFailureReason::InvalidInput(JetInvalidInput::AngularRate)
    );
}

#[test]
fn model_discriminators_and_legacy_revision_are_separate() {
    let kind: u16 = flightsim_fdm::subsonic::DRY_JET_MODEL_KIND_ID;
    let revision: u32 = flightsim_fdm::subsonic::JET_FDM_MODEL_REVISION;
    assert_eq!(kind, 2);
    assert_eq!(revision, 1);
    assert_eq!(flightsim_fdm::FDM_MODEL_REVISION, 2);
}

#[test]
fn airborne_contact_window_does_not_predict_beyond_the_requested_step() {
    let mut start = level(1.03, 0.0);
    start.velocity = start
        .local_frame()
        .ned_to_ecef_vector(Ned::new(0.0, 0.0, 2.0));
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let controls = ControlInputs::neutral();
    let environment = Environment::still_air();
    let first = model
        .step(Seconds(1.0 / 120.0), controls, &environment)
        .unwrap();
    assert_eq!(first.substeps, 1);
    assert!(model.state().altitude().get() > 1.001);
    // The following call predicts contact within its own interval and resolves
    // the real landing-gear phase before committing that interval.
    let second = model
        .step(Seconds(1.0 / 120.0), controls, &environment)
        .unwrap();
    assert!(second.substeps > 1);
    assert!(model.state().altitude().get() < 1.0);
}

#[test]
fn unexpected_entry_into_current_contact_margin_still_rejects() {
    // No initial closing velocity: the linear dt predictor sees clearance
    // beyond its 1 mm margin. Gravity subsequently enters that current margin.
    let start = level(1.0012, 0.0);
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(error.reason, JetFailureReason::SubstepBudgetExceeded);
    assert_eq!(error.stage, JetStage::K4);
    assert_eq!(*model.state(), start);
}

#[test]
fn ordinary_descent_crosses_contact_then_settles_without_budget_failure() {
    let mut start = level(1.5, 0.0);
    start.velocity = start
        .local_frame()
        .ned_to_ecef_vector(Ned::new(0.0, 0.0, 2.0));
    let mut model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let mut touched = false;
    for _ in 0..600 {
        model
            .step(
                Seconds(1.0 / 120.0),
                ControlInputs::neutral(),
                &Environment::still_air(),
            )
            .unwrap();
        touched |= model.state().altitude().get() < 1.0;
        assert!(model.state().is_finite());
    }
    assert!(touched);
    assert!(model.state().velocity.length() < 0.2);
    assert!((model.state().altitude().get() - 0.973).abs() < 0.005);
}

#[test]
fn gear_clearances_follow_rotated_contacts_on_flat_and_sloped_planes() {
    for (roll, pitch, yaw) in [
        (0.0, 0.0, 0.0),
        (20.0, 30.0, 40.0),
        (-35.0, -15.0, 120.0),
        (180.0, 0.0, 0.0),
    ] {
        let attitude = Attitude::from_degrees(roll, pitch, yaw);
        let start = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, 3.0),
            attitude,
            Ned::default(),
        );
        let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
        for slope in [GroundSlope::LEVEL, GroundSlope::new(0.2, -0.1)] {
            let env = Environment::still_air().with_ground_plane(
                Geodetic::from_degrees(0.0, 0.0, 0.0),
                Meters(0.4),
                slope,
            );
            let clearances = model.gear_clearances(&start, &env).unwrap();
            for (clearance, leg) in clearances
                .iter()
                .zip(model.config().airframe().landing_gear().legs())
            {
                // Independent local flat-plane projection. The production law
                // samples core geodetic height, so allow sub-micrometer Earth
                // curvature over these meter-scale offsets, not a force margin.
                let offset = attitude.to_quaternion() * leg.contact_point().as_vec();
                let expected =
                    (3.0 - offset.z - 0.4 - slope.north() * offset.x - slope.east() * offset.y)
                        / (1.0 + slope.north().powi(2) + slope.east().powi(2)).sqrt();
                close(clearance.get(), expected, 1e-6);
            }
        }
    }
}

#[test]
fn inverted_aircraft_does_not_report_gear_contact_from_fixed_body_height() {
    let start = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0.0, 0.0, 1.01),
        Attitude::from_degrees(180.0, 0.0, 0.0),
        Ned::default(),
    );
    let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let clearances = model
        .gear_clearances(&start, &Environment::still_air())
        .unwrap();
    assert!(clearances.iter().all(|value| value.get() > 2.0));
    assert_eq!(*model.state(), start);
}

#[test]
fn gear_clearance_uses_the_frozen_reference_plane_after_horizontal_motion() {
    let reference = Geodetic::from_degrees(0.0, 0.0, 0.0);
    let frame = flightsim_core::LocalFrame::new(reference);
    let mut start = level(3.0, 0.0);
    start.position = frame.ned_to_ecef_position(Ned::new(10.0, 20.0, -3.0));
    let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
    let slope = GroundSlope::new(0.1, 0.05);
    let env = Environment::still_air().with_ground_plane(reference, Meters(0.0), slope);
    let clearances = model.gear_clearances(&start, &env).unwrap();
    for (clearance, leg) in clearances
        .iter()
        .zip(model.config().airframe().landing_gear().legs())
    {
        let point = leg.contact_point().as_vec();
        let expected = (3.0 - point.z - 0.1 * (10.0 + point.x) - 0.05 * (20.0 + point.y))
            / (1.0_f64 + 0.1 * 0.1 + 0.05 * 0.05).sqrt();
        close(clearance.get(), expected, 1e-4);
    }
}

#[test]
fn negative_clearances_correspond_to_the_actual_gear_force_and_moment() {
    for (height, pitch) in [(0.98, 0.0), (1.0, 5.0), (1.0, -5.0), (2.0, 0.0)] {
        let start = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, height),
            Attitude::from_degrees(0.0, pitch, 0.0),
            Ned::default(),
        );
        let model = JetFlightDynamics::new(config(envelope()), start).unwrap();
        let env = Environment::still_air();
        let clearances = model.gear_clearances(&start, &env).unwrap();
        let loaded = model
            .derivative(&start, ControlInputs::neutral(), &env)
            .unwrap();
        let unloaded = model
            .derivative(
                &start,
                ControlInputs::neutral(),
                &env.with_ground(Meters(-1000.0)),
            )
            .unwrap();
        let up = start
            .local_frame()
            .ned_to_ecef_vector(Ned::new(0.0, 0.0, -1.0));
        let up_body = start.orientation.inverse() * up;
        let mut expected_force = 0.0;
        let mut expected_moment = DVec3::ZERO;
        for (clearance, leg) in clearances
            .iter()
            .zip(model.config().airframe().landing_gear().legs())
        {
            let penetration = (-clearance.get()).max(0.0);
            assert!(penetration < leg.max_stroke().get());
            let force = penetration * leg.spring_rate().get();
            expected_force += force;
            expected_moment += leg.contact_point().as_vec().cross(up_body * force);
        }
        let mass = model.config().airframe().mass_properties();
        close(
            (loaded.acceleration - unloaded.acceleration).dot(up) * mass.mass().get(),
            expected_force,
            1e-8,
        );
        let moment = mass.inertia() * (loaded.angular_acceleration - unloaded.angular_acceleration);
        assert!(moment.distance(expected_moment) < 1e-8);
    }
}

#[test]
fn successful_reports_carry_clearances_validated_before_commit_including_zero_dt() {
    let mut model = JetFlightDynamics::new(config(envelope()), level(2.0, 10.0)).unwrap();
    let env = Environment::still_air().with_ground_plane(
        Geodetic::from_degrees(0.0, 0.0, 0.0),
        Meters(0.0),
        GroundSlope::new(0.05, 0.02),
    );
    for dt in [Seconds(0.0), Seconds(1.0 / 120.0), Seconds(4.0 / 120.0)] {
        let report = model.step(dt, ControlInputs::neutral(), &env).unwrap();
        let checked = model.gear_clearances(model.state(), &env).unwrap();
        assert_eq!(
            report.gear_clearances.map(|value| value.get().to_bits()),
            checked.map(|value| value.get().to_bits())
        );
    }
    let original = *model.state();
    let mut invalid = original;
    invalid.orientation = DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    assert!(model.gear_clearances(&invalid, &env).is_err());
    assert!(
        model
            .gear_clearances(&original, &env.with_ground(Meters(f64::NAN)))
            .is_err()
    );
    assert_eq!(*model.state(), original);
}

#[test]
fn exact_zero_clearance_is_contact_without_a_height_tolerance() {
    let mut structure = airframe();
    for gear in &mut structure.landing_gear {
        gear.contact_m = [0.0, 0.0, 1.0];
    }
    let config = JetAircraftConfig::new(
        structure.to_config().unwrap(),
        table(),
        schedule(),
        OperatingEnvelope::from_definition(envelope()).unwrap(),
    )
    .unwrap();
    let mut model = JetFlightDynamics::new(config, level(1.0, 0.0)).unwrap();
    let report = model
        .step(
            Seconds::ZERO,
            ControlInputs::neutral(),
            &Environment::still_air(),
        )
        .unwrap();
    for clearance in report.gear_clearances {
        assert_eq!(clearance.get().to_bits(), 0.0_f64.to_bits());
        assert!(clearance.get() <= 0.0);
    }
}
