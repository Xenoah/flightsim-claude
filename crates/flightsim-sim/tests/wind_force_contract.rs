//! Physical wind is independent of authored visual weather. These short, open-loop
//! cases check direction/unit truth and actual forces and motion, not just metadata.

use flightsim_core::{Attitude, Degrees, Geodetic, Knots, MetersPerSecond, Ned};
use flightsim_fdm::{
    AeroAngles, AircraftConfig, ControlInputs, RigidBodyState, Turbulence,
    aero::body_force_and_moment,
};
use flightsim_sim::{
    GroundSampler, Simulation, Wind,
    aircraft_profile::AircraftProfileV2,
    model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation},
    replay_v4::state_bits_equal,
};
use flightsim_world::{MemoryTileSource, Terrain};
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
enum Model {
    Legacy,
    Jet,
}

enum Flight {
    Legacy(Box<Simulation<MemoryTileSource>>),
    Jet(Box<JetSimulation>),
}

fn initial() -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 0.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    )
}

fn wind(from_degrees: f64, speed: f64) -> Wind {
    Wind {
        from: Degrees(from_degrees).to_radians(),
        speed: MetersPerSecond(speed),
    }
}

fn controls() -> ControlInputs {
    // Identical constant inputs in every branch; no trim or feedback controller.
    ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65)
}

impl Flight {
    fn new(model: Model, wind: Wind, turbulence: Turbulence) -> Self {
        match model {
            Model::Legacy => {
                let mut sim = Simulation::from_state(
                    AircraftConfig::light_single(),
                    initial(),
                    Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
                    GroundSampler::default(),
                );
                sim.set_wind(wind);
                sim.set_turbulence(turbulence);
                Self::Legacy(Box::new(sim))
            }
            Model::Jet => {
                let config = AircraftProfileV2::parse(include_str!(
                    "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
                ))
                .unwrap()
                .configuration()
                .clone();
                let mut environment = JetEnvironment::default();
                environment.conditions.wind = wind;
                environment.conditions.turbulence = turbulence;
                Self::Jet(Box::new(
                    JetSimulation::from_state(config, initial(), environment).unwrap(),
                ))
            }
        }
    }

    fn state(&self) -> &RigidBodyState {
        match self {
            Self::Legacy(sim) => sim.state(),
            Self::Jet(sim) => sim.state(),
        }
    }

    fn angles(&self) -> AeroAngles {
        match self {
            Self::Legacy(sim) => sim.aero_angles(),
            Self::Jet(sim) => sim.aero_angles(),
        }
    }

    fn airspeed(&self) -> MetersPerSecond {
        match self {
            Self::Legacy(sim) => sim.airspeed(),
            Self::Jet(sim) => sim.airspeed(),
        }
    }

    fn aerodynamic_loads(&self) -> (DVec3, DVec3) {
        match self {
            Self::Legacy(sim) => {
                let config = AircraftConfig::light_single();
                body_force_and_moment(
                    &config.aero,
                    &config.geometry,
                    sim.aero_angles(),
                    sim.state().angular_velocity,
                    controls(),
                    sim.atmosphere_sample().density,
                )
            }
            Self::Jet(sim) => {
                let presentation = sim.presentation();
                body_force_and_moment(
                    &presentation.aero_coefficients.unwrap(),
                    sim.config().airframe().geometry(),
                    presentation.aero_angles,
                    sim.state().angular_velocity,
                    controls(),
                    presentation.atmosphere.density,
                )
            }
        }
    }

    fn step(&mut self) {
        match self {
            Self::Legacy(sim) => {
                let report = sim.advance(JET_FIXED_DT, controls());
                assert_eq!(report.steps, 1);
                assert!(!report.diverged && !sim.crashed());
            }
            Self::Jet(sim) => {
                let report = sim.advance(JET_FIXED_DT, controls());
                assert!(report.terminal().is_none(), "{:?}", report.terminal());
                assert_eq!(report.committed_steps(), 1);
            }
        }
        assert!(self.state().is_finite());
    }
}

#[test]
fn compass_from_directions_and_knots_match_independent_relative_air_vectors() {
    // International nautical mile = 1852 m, hour = 3600 s: 36 kt = 18.52 m/s.
    let knots = Knots(36.0).to_meters_per_second();
    assert!((knots.get() - 18.52).abs() < 1e-12);
    // North-facing level aircraft moves north at 50 m/s. The listed vectors are
    // ground velocity minus the air's motion; no production wind helper derives them.
    let cases = [
        (Wind::CALM, DVec3::new(50.0, 0.0, 0.0)),
        (wind(0.0, 10.0), DVec3::new(60.0, 0.0, 0.0)),
        (wind(90.0, 10.0), DVec3::new(50.0, 10.0, 0.0)),
        (wind(180.0, 10.0), DVec3::new(40.0, 0.0, 0.0)),
        (wind(270.0, 10.0), DVec3::new(50.0, -10.0, 0.0)),
        (wind(360.0, 10.0), DVec3::new(60.0, 0.0, 0.0)),
        (wind(45.0, 200.0_f64.sqrt()), DVec3::new(60.0, 10.0, 0.0)),
        (wind(270.0, knots.get()), DVec3::new(50.0, -18.52, 0.0)),
        // An aircraft carried exactly with a southerly wind has zero airspeed.
        (wind(180.0, 50.0), DVec3::ZERO),
    ];
    for model in [Model::Legacy, Model::Jet] {
        for (wind, relative) in cases {
            let air_motion = DVec3::new(50.0, 0.0, 0.0) - relative;
            assert!((wind.to_ned().0 - air_motion).length() < 1e-12);
            let sim = Flight::new(model, wind, Turbulence::CALM);
            let angles = sim.angles();
            assert!((sim.airspeed().get() - relative.length()).abs() < 1e-10);
            assert!((angles.true_airspeed.get() - relative.length()).abs() < 1e-10);
            assert!(angles.angle_of_attack.get().abs() < 1e-12);
            let expected_sideslip = relative.y.atan2(relative.x);
            assert!(
                (angles.sideslip.get() - expected_sideslip).abs() < 1e-12,
                "{model:?}, {wind:?}: {angles:?}"
            );
        }
    }
}

#[test]
fn reversing_crosswind_reverses_aerodynamic_side_force_and_early_motion() {
    for model in [Model::Legacy, Model::Jet] {
        let mut calm = Flight::new(model, Wind::CALM, Turbulence::CALM);
        let mut westerly = Flight::new(model, wind(270.0, 10.0), Turbulence::CALM);
        let mut easterly = Flight::new(model, wind(90.0, 10.0), Turbulence::CALM);
        let calm_force = calm.aerodynamic_loads().0;
        assert!(westerly.aerodynamic_loads().0.y - calm_force.y > 1.0);
        assert!(easterly.aerodynamic_loads().0.y - calm_force.y < -1.0);

        let frame = initial().local_frame();
        for step in 0..30 {
            calm.step();
            westerly.step();
            easterly.step();
            if step == 0 {
                for (sim, sign) in [(&westerly, 1.0), (&easterly, -1.0)] {
                    let impulse =
                        frame.ecef_to_ned_vector(sim.state().velocity - calm.state().velocity);
                    assert!(sign * impulse.east() > 1e-5, "{model:?}: {impulse:?}");
                }
            }
        }
        // A quarter-second avoids making false long-term drift claims after the
        // aircraft starts weathercocking. Both actual position and velocity respond.
        for (sim, sign) in [(&westerly, 1.0), (&easterly, -1.0)] {
            let displacement = frame
                .ecef_to_ned_vector(sim.state().position.as_vec() - calm.state().position.as_vec());
            assert!(
                sign * displacement.east() > 0.001,
                "{model:?}: {displacement:?}"
            );
        }
    }
}

#[test]
fn turbulence_intensity_and_seed_change_forces_and_open_loop_trajectory() {
    for model in [Model::Legacy, Model::Jet] {
        let steady = wind(310.0, 7.25);
        let mut calm = Flight::new(model, steady, Turbulence::CALM);
        let mut gusts = Flight::new(model, steady, Turbulence::moderate(24_301));
        let mut other_seed = Flight::new(model, steady, Turbulence::moderate(24_302));
        let mut repeat = Flight::new(model, steady, Turbulence::moderate(24_301));
        for reference in [&calm, &other_seed] {
            let (force, moment) = gusts.aerodynamic_loads();
            let (other_force, other_moment) = reference.aerodynamic_loads();
            assert!((force - other_force).length() > 1e-3, "{model:?}");
            assert!((moment - other_moment).length() > 1e-3, "{model:?}");
        }
        for step in 0..60 {
            calm.step();
            gusts.step();
            other_seed.step();
            repeat.step();
            assert!(state_bits_equal(gusts.state(), repeat.state()), "{model:?}");
            if step == 0 {
                for reference in [&calm, &other_seed] {
                    assert!(
                        (gusts.state().velocity - reference.state().velocity).length() > 1e-6,
                        "{model:?}: gusts must change the integrated acceleration"
                    );
                }
            }
        }
        for reference in [&calm, &other_seed] {
            assert!(
                (gusts.state().position.as_vec() - reference.state().position.as_vec()).length()
                    > 1e-4,
                "{model:?}: gusts must change the trajectory"
            );
        }
    }
}
