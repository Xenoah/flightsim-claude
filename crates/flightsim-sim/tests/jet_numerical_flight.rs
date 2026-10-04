use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_sim::{
    aircraft_profile::AircraftProfileV2,
    jet_scenarios::{JetTakeoffDriver, solve_jet_trim},
    model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation, jet_parked_state},
};
fn config() -> flightsim_fdm::subsonic::JetAircraftConfig {
    AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap()
    .configuration()
    .clone()
}
#[test]
fn authored_points_trim_and_remain_near_with_held_controls() {
    for (altitude, speed) in [
        (1000., 35.),
        (1000., 50.),
        (1000., 65.),
        (3000., 45.),
        (5000., 50.),
    ] {
        let cfg = config();
        let trim = solve_jet_trim(
            &cfg,
            Geodetic::from_degrees(0., 0., altitude),
            MetersPerSecond(speed),
            MetersPerSecond::ZERO,
            0.,
        )
        .unwrap();
        assert!(trim.residual.abs().max_element() < 1e-9);
        let mut sim =
            JetSimulation::from_state(cfg, trim.state, JetEnvironment::default()).unwrap();
        for _ in 0..3600 {
            let report = sim.advance(JET_FIXED_DT, trim.controls);
            assert!(
                report.terminal().is_none(),
                "{altitude}/{speed}:{:?}",
                report.terminal()
            );
        }
        assert!(
            (sim.state().altitude().get() - altitude).abs() < 10.,
            "altitude held-control drift:{} at{altitude}/{speed}",
            sim.state().altitude().get()
        );
        assert!((sim.airspeed().get() - speed).abs() < 1.5);
    }
}
#[test]
fn separately_tuned_driver_takes_off_without_domain_rejection() {
    let cfg = config();
    let state = jet_parked_state(
        &cfg,
        Geodetic::from_degrees(0., 0., 0.),
        Meters::ZERO,
        Radians::ZERO,
    );
    let mut sim = JetSimulation::from_state(cfg, state, JetEnvironment::default()).unwrap();
    let mut driver = JetTakeoffDriver::default();
    for _ in 0..7200 {
        let report =
            sim.advance_with_controller(JET_FIXED_DT, &mut driver, |p, dt, s| p.controls(dt, s));
        assert!(report.terminal().is_none(), "{:?}", report.terminal());
    }
    assert!(
        sim.log().peak_agl.get() > 100.,
        "peak AGL{}",
        sim.log().peak_agl.get()
    );
    assert!(sim.snapshot().airborne);
}
#[test]
fn throttle_free_response_accelerates_or_decelerates_from_same_trim() {
    let cfg = config();
    let trim = solve_jet_trim(
        &cfg,
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap();
    let mut speeds = Vec::new();
    for throttle in [0., 1.] {
        let mut sim =
            JetSimulation::from_state(cfg.clone(), trim.state, JetEnvironment::default()).unwrap();
        for _ in 0..360 {
            let report = sim.advance(JET_FIXED_DT, trim.controls.with_throttle(throttle));
            assert!(report.terminal().is_none());
        }
        speeds.push(sim.airspeed().get());
    }
    assert!(speeds[0] < 49.);
    assert!(speeds[1] > 51.);
}
#[test]
fn descending_trim_approaches_and_observes_contact() {
    let cfg = config();
    let trim = solve_jet_trim(
        &cfg,
        Geodetic::from_degrees(0., 0., 60.),
        MetersPerSecond(35.),
        MetersPerSecond(2.),
        0.5,
    )
    .unwrap();
    let mut sim = JetSimulation::from_state(cfg, trim.state, JetEnvironment::default()).unwrap();
    let mut contact = false;
    for _ in 0..4800 {
        let report = sim.advance(Seconds(1. / 120.), trim.controls);
        assert!(report.terminal().is_none(), "{:?}", report.terminal());
        if sim.snapshot().touchdown_count > 0 {
            contact = true;
            break;
        }
    }
    assert!(
        contact,
        "no contact at height{}",
        sim.state().altitude().get()
    );
    assert!(
        sim.minimum_gear_clearance().get() <= 0.0,
        "touchdown must be physical wheel contact"
    );
    let event = sim.snapshot().last_touchdown.unwrap();
    assert!(
        event.sink_rate.get() > 0.5 && event.sink_rate.get() < 4.0,
        "sink{}",
        event.sink_rate.get()
    );
}

#[test]
fn initial_contact_uses_rotated_gear_and_never_calls_inverted_airborne_pose_a_touchdown() {
    use flightsim_core::{Attitude, Ned};
    use flightsim_fdm::{ControlInputs, RigidBodyState};
    let inverted = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., 1.04),
        Attitude::new(Radians(std::f64::consts::PI), Radians::ZERO, Radians::ZERO),
        Ned::new(0., 0., 0.),
    );
    let mut sim = JetSimulation::from_state(config(), inverted, JetEnvironment::default()).unwrap();
    assert!(sim.snapshot().airborne);
    assert!(sim.minimum_gear_clearance().get() > 2.0);
    assert!(
        sim.advance(JET_FIXED_DT, ControlInputs::neutral())
            .terminal()
            .is_none()
    );
    assert_eq!(sim.snapshot().touchdown_count, 0);
    assert!(sim.minimum_gear_clearance().get() > 2.0);
    let pitched = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., 2.),
        Attitude::new(Radians::ZERO, Radians(0.3), Radians::ZERO),
        Ned::new(0., 0., 0.),
    );
    let sim = JetSimulation::from_state(config(), pitched, JetEnvironment::default()).unwrap();
    let gaps = sim.snapshot().gear_clearances;
    assert!(gaps[0].get() > gaps[1].get() + 0.6);
    assert!((gaps[1].get() - gaps[2].get()).abs() < 1e-8);
}

#[test]
fn bundled_sloped_ground_uses_rotated_contact_geometry_in_held_plane() {
    use flightsim_core::{Attitude, Ecef, LocalFrame, Ned};
    use flightsim_fdm::RigidBodyState;
    use flightsim_sim::model_simulation::JetTerrain;
    let mut environment = JetEnvironment::default();
    environment.terrain = JetTerrain::BundledGlobal;
    environment.conditions = environment.conditions.with_world_climate(true, None);
    let cfg = config();
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(46., 7., 6000.),
        Attitude::new(Radians(0.2), Radians(-0.3), Radians(1.)),
        Ned::new(0., 0., 0.),
    );
    let sim = JetSimulation::from_state(cfg.clone(), state, environment).unwrap();
    let snapshot = sim.snapshot();
    let ground = snapshot.ground;
    assert!(ground.slope.north().abs() + ground.slope.east().abs() > 1e-6);
    let frame = LocalFrame::new(ground.reference);
    let scale = (1. + ground.slope.north().powi(2) + ground.slope.east().powi(2)).sqrt();
    for (leg, gap) in cfg
        .airframe()
        .landing_gear()
        .legs()
        .iter()
        .zip(snapshot.gear_clearances)
    {
        let point = Ecef::from_vec(
            state.position.as_vec() + state.orientation * leg.contact_point().as_vec(),
        );
        let local = frame.ecef_to_ned_position(point);
        let height = ground.elevation.get()
            + ground.slope.north() * local.north()
            + ground.slope.east() * local.east();
        let expected = (point.to_geodetic().altitude.get() - height) / scale;
        assert!((expected - gap.get()).abs() < 1e-9);
    }
}
