//! Issue #5: numerical reference scenarios, not subjective pilot handling validation.
#[path = "../examples/support/turbulence_scenarios.rs"]
mod scenarios;

use flightsim_core::{Geodetic, MetersPerSecond, Seconds};
use flightsim_fdm::Turbulence;
use scenarios::{Envelope, RUN_SECONDS, SAMPLE_HZ, SEEDS, Scenario, Severity};

#[test]
fn standard_cruise_and_approach_scenarios_stay_inside_numerical_envelopes() {
    for scenario in Scenario::ALL {
        for severity in Severity::ALL {
            for seed in SEEDS {
                let metrics =
                    scenarios::run(scenario, severity.turbulence(seed), RUN_SECONDS, |_| {});
                assert_eq!(metrics.samples, RUN_SECONDS * SAMPLE_HZ);
                let failures = Envelope::for_severity(severity).failures(&metrics);
                assert!(
                    failures.is_empty(),
                    "{} {} seed {seed}: {failures:?}; {metrics:?}",
                    scenario.name(),
                    severity.name()
                );
            }
        }
    }
}

#[test]
fn every_state_and_control_bit_repeats_for_all_three_severities() {
    for scenario in Scenario::ALL {
        for severity in [Severity::Light, Severity::Moderate, Severity::Severe] {
            let turbulence = severity.turbulence(1);
            let mut first = Vec::new();
            scenarios::run(scenario, turbulence, RUN_SECONDS, |sample| {
                first.push(sample.bits())
            });
            let mut index = 0;
            scenarios::run(scenario, turbulence, RUN_SECONDS, |sample| {
                assert_eq!(
                    sample.bits(),
                    first[index],
                    "{} {} sample {index}",
                    scenario.name(),
                    severity.name()
                );
                index += 1;
            });
            assert_eq!(index, first.len());
        }
    }
}

#[test]
fn invalid_strengths_reproduce_the_calm_trajectory() {
    let calm = scenarios::run(Scenario::Cruise, Turbulence::CALM, 5, |_| {});
    for intensity in [-1.0, 0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let result = scenarios::run(
            Scenario::Cruise,
            Turbulence {
                intensity: MetersPerSecond(intensity),
                seed: 1,
            },
            5,
            |_| {},
        );
        assert_eq!(
            result, calm,
            "invalid intensity {intensity} escaped the calm fallback"
        );
    }
}

#[test]
fn independent_seed_and_strength_changes_are_observable() {
    let light = scenarios::run(Scenario::Cruise, Turbulence::light(1), 30, |_| {});
    let moderate = scenarios::run(Scenario::Cruise, Turbulence::moderate(1), 30, |_| {});
    let severe = scenarios::run(Scenario::Cruise, Turbulence::severe(1), 30, |_| {});
    let another_seed = scenarios::run(Scenario::Cruise, Turbulence::moderate(7), 30, |_| {});
    assert_ne!(moderate.final_state, another_seed.final_state);
    // Only compare this named short scenario; different closed-loop paths need not be monotonic.
    assert!(
        light.load_deviation_g.rms(light.samples) < moderate.load_deviation_g.rms(moderate.samples)
    );
    assert!(
        moderate.load_deviation_g.rms(moderate.samples)
            < severe.load_deviation_g.rms(severe.samples)
    );
}

#[test]
fn standard_strengths_scale_the_same_field_before_aircraft_feedback() {
    for step in 0..1200 {
        let t = Seconds(f64::from(step) / 120.0);
        let position = Geodetic::from_degrees(35.55, 139.78, 1200.0);
        let a = Turbulence::light(1).gust_at(t, position);
        let b = Turbulence::moderate(1).gust_at(t, position);
        let c = Turbulence::severe(1).gust_at(t, position);
        for (light, moderate, severe) in [
            (a.north(), b.north(), c.north()),
            (a.east(), b.east(), c.east()),
            (a.down(), b.down(), c.down()),
        ] {
            assert!((2.0 * light - moderate).abs() < 1e-12);
            assert!((2.0 * moderate - severe).abs() < 1e-12);
        }
    }
}

#[test]
fn envelope_rejects_bad_attitude_acceleration_controls_and_nan() {
    let baseline = scenarios::run(Scenario::Cruise, Turbulence::CALM, 1, |_| {});
    let envelope = Envelope::for_severity(Severity::Calm);
    assert!(envelope.failures(&baseline).is_empty());
    for (kind, mutate) in [
        (
            "roll",
            (|m: &mut scenarios::Metrics| m.roll_deg.max = 90.0) as fn(&mut scenarios::Metrics),
        ),
        ("pitch", |m: &mut scenarios::Metrics| {
            m.pitch_deg.min = -90.0
        }),
        ("acceleration", |m: &mut scenarios::Metrics| {
            m.acceleration_mps2.max = 100.0
        }),
        ("normal load", |m: &mut scenarios::Metrics| {
            m.normal_load_g.max = 10.0
        }),
        ("surface margin", |m: &mut scenarios::Metrics| {
            m.controls[0].max = 1.0
        }),
        ("surface saturation", |m: &mut scenarios::Metrics| {
            m.saturation_samples[1] = 1
        }),
        ("terrain clearance", |m: &mut scenarios::Metrics| {
            m.altitude_m.min = 0.0
        }),
        ("stall", |m: &mut scenarios::Metrics| {
            m.stall_fraction.max = 1.1
        }),
        ("quaternion norm", |m: &mut scenarios::Metrics| {
            m.quaternion_error = 0.1
        }),
        ("invalid measurement", |m: &mut scenarios::Metrics| {
            m.roll_deg.max = f64::NAN
        }),
        ("invalid measurement", |m: &mut scenarios::Metrics| {
            m.samples = 0
        }),
    ] {
        let mut broken = baseline.clone();
        mutate(&mut broken);
        assert!(
            envelope.failures(&broken).contains(&kind),
            "checker missed {kind}"
        );
    }
}

#[test]
fn free_fall_has_gravity_acceleration_but_nearly_zero_specific_load() {
    use flightsim_core::{Attitude, Ned};
    use flightsim_fdm::{
        AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
        RigidBodyState,
    };
    let before = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0.0, 0.0, 1000.0),
        Attitude::default(),
        Ned::default(),
    );
    let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), before);
    fdm.step(
        RECOMMENDED_FIXED_DT,
        ControlInputs::neutral(),
        &Environment::still_air(),
    );
    let (acceleration, normal_load) = scenarios::acceleration_sample(before, *fdm.state());
    assert!(
        (9.7..9.9).contains(&acceleration),
        "free-fall acceleration {acceleration} m/s²"
    );
    assert!(
        normal_load.abs() < 0.001,
        "free-fall load should be nearly zero, got {normal_load} g"
    );
}

#[test]
fn severe_cruise_remains_inside_the_envelope_for_ten_minutes() {
    for seed in SEEDS {
        let metrics = scenarios::run(Scenario::Cruise, Turbulence::severe(seed), 600, |_| {});
        let failures = Envelope::for_severity(Severity::Severe).failures(&metrics);
        assert!(
            failures.is_empty(),
            "ten-minute seed {seed}: {failures:?}; {metrics:?}"
        );
    }
}
