//! Open-loop longitudinal physics: no autopilot or feedback to hide instability.
//! These generic-aircraft tests are invariants/trends, not certified POH numbers.
mod longitudinal_support;
use flightsim_core::{
    Attitude, FixedStep, Geodetic, Knots, Meters, MetersPerSecond, Ned, Radians, Seconds,
};
use flightsim_fdm::{
    Atmosphere, ControlInputs, Environment, FlightDynamics, RigidBodyState, aero, aero_angles_of,
};
use glam::DVec3;
use longitudinal_support::{climb, configs, level};

fn ias(state: &RigidBodyState, env: &Environment) -> f64 {
    aero_angles_of(state, env).true_airspeed.get()
        * env
            .atmosphere
            .sample(state.altitude())
            .density_ratio()
            .sqrt()
}
fn speed(knots: f64) -> f64 {
    Knots(knots).to_meters_per_second().get()
}
fn environment() -> Environment {
    Environment::still_air().with_ground(Meters(-10_000.0))
}

#[test]
fn static_thrust_scales_with_power_and_is_continuous_at_idle() {
    for c in configs() {
        let e = c.engine;
        assert_eq!(e.thrust(0.0, 0.0, 1.0).get().to_bits(), 0.0_f64.to_bits());
        // Actuator disk: one eighth of the power produces one quarter of thrust.
        assert!((e.thrust(0.125, 0.0, 1.0).get() / e.static_thrust.get() - 0.25).abs() < 1e-12);
        assert!(e.thrust(1e-9, 0.0, 1.0).get() < 0.01);
        for sigma in [0.0, 0.4, 0.8, 1.0, 1.2] {
            for v in [0.0, 1e-9, 1.0, 10.0, 25.0, 40.0, 70.0, 150.0] {
                let mut previous = 0.0;
                for t in [0.0, 1e-9, 0.0001, 0.01, 0.1, 0.25, 0.5, 0.75, 1.0] {
                    let thrust = e.thrust(t, v, sigma).get();
                    assert!(thrust.is_finite() && thrust >= previous - 1e-10);
                    assert!(thrust <= e.static_thrust.get() * sigma + 1e-10);
                    assert!(
                        thrust * v <= t * e.max_shaft_power * e.propeller_efficiency * sigma + 1e-8,
                        "power created at t={t},v={v},sigma={sigma}"
                    );
                    previous = thrust;
                }
                // Full power preserves the original cap / constant-power curve.
                let expected = e
                    .static_thrust
                    .get()
                    .min(e.max_shaft_power * e.propeller_efficiency / v.max(f64::EPSILON))
                    * sigma;
                assert!((e.thrust(1.0, v, sigma).get() - expected).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn actual_level_trim_remains_level_for_ten_minutes_without_feedback() {
    for c in configs() {
        for altitude in [1000.0, 3000.0] {
            let trim = level(&c, speed(85.0), altitude, Atmosphere::standard()).unwrap();
            let mut fdm = FlightDynamics::new(c.clone(), trim.state(altitude));
            let env = environment();
            for _ in 0..72_000 {
                fdm.step(Seconds(1.0 / 120.0), trim.controls, &env);
                let s = fdm.state();
                // Small nonzero drift is expected: initial local-flat equilibrium
                // does not include the ECEF path-curvature angular rate.
                assert!(s.is_finite());
                assert!(
                    (s.altitude().get() - altitude).abs() < 1.0,
                    "{}: {}",
                    fdm.config().name,
                    s.altitude()
                );
                assert!(
                    (ias(s, &env)
                        - trim.tas
                            * env
                                .atmosphere
                                .sample(Meters(altitude))
                                .density_ratio()
                                .sqrt())
                    .abs()
                        < 0.02
                );
                assert!((s.orientation.length() - 1.0).abs() < 1e-12);
            }
        }
    }
}

#[test]
fn pitch_and_speed_disturbances_decay_with_fixed_trim_and_throttle() {
    for c in configs() {
        for altitude in [1000.0, 3000.0] {
            for (pitch_delta, speed_factor) in [(2.0, 1.0), (-2.0, 1.0), (0.0, 1.1), (0.0, 0.9)] {
                let trim = level(&c, speed(85.0), altitude, Atmosphere::standard()).unwrap();
                let initial = trim.state(altitude);
                let state = RigidBodyState::from_geodetic(
                    initial.geodetic(),
                    Attitude::from_degrees(0.0, trim.alpha.to_degrees() + pitch_delta, 0.0),
                    Ned::new(trim.tas * speed_factor, 0.0, 0.0),
                );
                let mut fdm = FlightDynamics::new(c.clone(), state);
                let env = environment();
                let mut early: f64 = 0.0;
                let mut late: f64 = 0.0;
                for step in 0..72_000 {
                    fdm.step(Seconds(1.0 / 120.0), trim.controls, &env);
                    let s = fdm.state();
                    assert!(s.is_finite() && s.altitude().get() > altitude - 100.0);
                    assert!(
                        aero_angles_of(s, &env)
                            .angle_of_attack
                            .to_degrees()
                            .get()
                            .abs()
                            < 10.0
                    );
                    let amplitude =
                        (ias(s, &env) - speed(85.0)).abs() + s.vertical_speed().get().abs();
                    if step < 7200 {
                        early = early.max(amplitude);
                    }
                    if step >= 64_800 {
                        late = late.max(amplitude);
                    }
                }
                assert!(
                    late < early * 0.02,
                    "{} pitch={pitch_delta},speed_factor={speed_factor}: early={early},late={late}",
                    c.name
                );
            }
        }
    }
}

#[test]
fn trim_speed_and_climb_envelope_follow_force_and_energy_balance() {
    for c in configs() {
        let at = Atmosphere::standard();
        let mut last_elevator = 1.0;
        for kt in [65.0, 75.0, 85.0, 100.0] {
            let trim = level(&c, speed(kt), 1000.0, at).unwrap();
            assert!(
                trim.controls.elevator() < last_elevator,
                "slower trim needs more nose-up control"
            );
            last_elevator = trim.controls.elevator();
        }
        assert!(
            climb(&c, speed(40.0), 1.0, 1000.0, at).is_none(),
            "below-stall speed cannot be a normal steady climb"
        );
        let base = climb(&c, speed(80.0), 1.0, 1000.0, at).unwrap();
        let base_roc = base.tas * base.gamma.sin();
        assert!(base_roc > 1.0);
        for altered in [
            climb(&c, speed(80.0), 0.6, 1000.0, at).unwrap(),
            climb(&c, speed(80.0), 1.0, 4000.0, at).unwrap(),
            climb(
                &c,
                speed(80.0),
                1.0,
                1000.0,
                Atmosphere::with_temperature_offset(25.0),
            )
            .unwrap(),
        ] {
            assert!(altered.tas * altered.gamma.sin() < base_roc);
        }
        let mut heavy = flightsim_fdm::definition::AircraftDefinition::from_config(&c);
        heavy.mass_kg *= 1.15;
        let heavy = climb(&heavy.to_config().unwrap(), speed(80.0), 1.0, 1000.0, at).unwrap();
        assert!(heavy.tas * heavy.gamma.sin() < base_roc);
        let air = at.sample(Meters(1000.0));
        let thrust = c
            .engine
            .thrust(base.controls.throttle(), base.tas, air.density_ratio())
            .get();
        let (_, drag) = longitudinal_support::loads(&c, base.alpha, base.tas, at, 1000.0);
        let weight = c.mass_properties.mass().get()
            * flightsim_fdm::gravity::magnitude(Geodetic::from_degrees(35.0, 139.0, 1000.0));
        assert!(((thrust * base.alpha.cos() - drag) * base.tas - weight * base_roc).abs() < 1e-7);
        let mut fdm = FlightDynamics::new(c, base.state(1000.0));
        let env = environment();
        for _ in 0..7200 {
            fdm.step(Seconds(1.0 / 120.0), base.controls, &env);
        }
        assert!(fdm.state().altitude().get() > 1000.0 + base_roc * 60.0 * 0.7);
        assert!((ias(fdm.state(), &env) - speed(80.0)).abs() < 1.0);
    }
}

#[test]
fn stall_loses_lift_and_releasing_overpull_can_still_cost_height() {
    for (c, default_trim) in configs()
        .into_iter()
        .zip(longitudinal_support::default_trims())
    {
        let coeff = |deg: f64| {
            aero::coefficients(
                &c.aero,
                &c.geometry,
                aero::AeroAngles {
                    angle_of_attack: Radians(deg.to_radians()),
                    sideslip: Radians::ZERO,
                    true_airspeed: MetersPerSecond(35.0),
                },
                DVec3::ZERO,
                ControlInputs::neutral(),
            )
        };
        assert!(coeff(30.0).lift < coeff(12.0).lift * 0.5);
        assert!(coeff(30.0).drag > coeff(3.0).drag * 5.0);
        let env = environment();
        let tas = speed(45.0) / env.atmosphere.sample(Meters(1000.0)).density_ratio().sqrt();
        let gamma = 20.0_f64.to_radians();
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.0, 139.0, 1000.0),
            Attitude::from_degrees(0.0, 40.0, 0.0),
            Ned::new(tas * gamma.cos(), 0.0, -tas * gamma.sin()),
        );
        let mut fdm = FlightDynamics::new(c, state);
        let mut minimum = 1000.0_f64;
        let mut alpha_max = 0.0_f64;
        for _ in 0..10_800 {
            fdm.step(
                Seconds(1.0 / 120.0),
                ControlInputs::neutral()
                    .with_throttle(1.0)
                    .with_elevator(default_trim),
                &env,
            );
            assert!(fdm.state().is_finite());
            minimum = minimum.min(fdm.state().altitude().get());
            alpha_max = alpha_max.max(aero_angles_of(fdm.state(), &env).angle_of_attack.get());
        }
        assert!(alpha_max > fdm.config().aero.stall_angle.get());
        assert!(
            (20.0..200.0).contains(&(1000.0 - minimum)),
            "recovery must lose real altitude: {}",
            1000.0 - minimum
        );
        assert!(
            fdm.state().altitude().get() > 1000.0,
            "sufficiently high release should eventually recover"
        );
    }
}

#[test]
fn fixed_step_physics_is_identical_across_render_cadences() {
    for c in configs() {
        let trim = level(&c, speed(85.0), 1000.0, Atmosphere::standard()).unwrap();
        let run = |cadence: &[f64]| {
            let mut fdm = FlightDynamics::new(c.clone(), trim.state(1000.0));
            let mut clock = FixedStep::new(Seconds(1.0 / 120.0));
            let mut ticks = 0;
            let mut frame = 0;
            while ticks < 7200 {
                for _ in 0..clock.advance(Seconds(cadence[frame % cadence.len()])) {
                    if ticks >= 7200 {
                        break;
                    }
                    // Identical timestamped physical input, independent of FPS.
                    let pulse = if (120..160).contains(&ticks) {
                        0.12
                    } else {
                        0.0
                    };
                    fdm.step(
                        clock.fixed_dt(),
                        trim.controls
                            .with_elevator(trim.controls.elevator() + pulse),
                        &environment(),
                    );
                    ticks += 1;
                }
                frame += 1;
            }
            *fdm.state()
        };
        let reference = run(&[1.0 / 60.0]);
        for cadence in [
            &[1.0 / 6.0][..],
            &[1.0 / 15.0],
            &[1.0 / 30.0],
            &[1.0 / 144.0],
            &[0.004, 0.12, 0.009, 0.032],
        ] {
            assert_eq!(run(cadence), reference);
        }
    }
}

#[test]
fn full_back_elevator_stalls_both_aircraft_and_release_allows_recovery() {
    for c in configs() {
        let trim = level(&c, speed(85.0), 2000.0, Atmosphere::standard()).unwrap();
        let mut fdm = FlightDynamics::new(c, trim.state(2000.0));
        let env = environment();
        let mut peak_alpha = 0.0_f64;
        let mut minimum = 2000.0_f64;
        for tick in 0..12_000 {
            let controls = if tick < 1200 {
                trim.controls.with_throttle(1.0).with_elevator(1.0)
            } else {
                trim.controls.with_throttle(1.0)
            };
            fdm.step(Seconds(1.0 / 120.0), controls, &env);
            assert!(fdm.state().is_finite());
            peak_alpha = peak_alpha.max(aero_angles_of(fdm.state(), &env).angle_of_attack.get());
            minimum = minimum.min(fdm.state().altitude().get());
        }
        assert!(peak_alpha > fdm.config().aero.stall_angle.get());
        assert!(minimum > 1000.0, "test must not rely on ground contact");
        assert!(
            aero_angles_of(fdm.state(), &env)
                .angle_of_attack
                .get()
                .abs()
                < 0.1
        );
        assert!(ias(fdm.state(), &env) > speed(65.0));
    }
}

#[test]
fn both_aircraft_converge_when_the_fixed_timestep_is_halved() {
    for c in configs() {
        let trim = level(&c, speed(85.0), 1000.0, Atmosphere::standard()).unwrap();
        let mut state = trim.state(1000.0);
        state.velocity *= 1.1;
        let run = |hz: u32| {
            let mut fdm = FlightDynamics::new(c.clone(), state);
            for _ in 0..120 * hz {
                fdm.step(Seconds(1.0 / f64::from(hz)), trim.controls, &environment());
            }
            *fdm.state()
        };
        let coarse = run(120);
        let fine = run(240);
        assert!(coarse.position.distance_to(fine.position).get() < 0.01);
        assert!(coarse.velocity.distance(fine.velocity) < 0.001);
        assert!(coarse.orientation.angle_between(fine.orientation) < 1e-5);
    }
}

#[test]
fn longitudinal_lift_is_perpendicular_and_drag_dissipates_energy() {
    for c in configs() {
        for alpha_degrees in -180..=180 {
            let alpha = f64::from(alpha_degrees).to_radians();
            let velocity = DVec3::new(40.0 * alpha.cos(), 0.0, 40.0 * alpha.sin());
            let angles = aero::AeroAngles {
                angle_of_attack: Radians(alpha),
                sideslip: Radians::ZERO,
                true_airspeed: MetersPerSecond(40.0),
            };
            let density = Atmosphere::standard().sample(Meters(1000.0)).density;
            let controls = ControlInputs::neutral();
            let (force, _) = aero::body_force_and_moment(
                &c.aero,
                &c.geometry,
                angles,
                DVec3::ZERO,
                controls,
                density,
            );
            let cd = aero::coefficients(&c.aero, &c.geometry, angles, DVec3::ZERO, controls).drag;
            let drag_power =
                0.5 * density.get() * 40.0_f64.powi(3) * c.geometry.wing_area.get() * cd;
            assert!((force.dot(velocity) + drag_power).abs() < 1e-8);
            assert!(force.dot(velocity) <= 1e-8);
        }
    }
}

#[test]
fn gentle_release_near_rotation_speed_keeps_trim_but_can_need_height() {
    for (c, default_trim) in configs()
        .into_iter()
        .zip(longitudinal_support::default_trims())
    {
        for kt in [60.0, 65.0, 70.0] {
            let env = environment();
            let tas = speed(kt) / env.atmosphere.sample(Meters(100.0)).density_ratio().sqrt();
            let gamma = 5.0_f64.to_radians();
            let state = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(35.0, 139.0, 100.0),
                Attitude::from_degrees(0.0, 10.0, 0.0),
                Ned::new(tas * gamma.cos(), 0.0, -tas * gamma.sin()),
            );
            let controls = ControlInputs::neutral()
                .with_throttle(1.0)
                .with_elevator(default_trim);
            let mut fdm = FlightDynamics::new(c.clone(), state);
            let mut minimum = 100.0_f64;
            for _ in 0..10_800 {
                fdm.step(Seconds(1.0 / 120.0), controls, &env);
                assert!(fdm.state().is_finite());
                assert!(
                    aero_angles_of(fdm.state(), &env)
                        .angle_of_attack
                        .to_degrees()
                        .get()
                        .abs()
                        < 10.0
                );
                minimum = minimum.min(fdm.state().altitude().get());
            }
            assert!(minimum > 80.0, "normal release exceeded 20 m capture loss");
            assert!(fdm.state().altitude().get() > 300.0);
            if kt >= 65.0 {
                assert!(
                    minimum > 99.0,
                    "65+ kt release should not consume initial height here"
                );
            }
        }
    }
}
