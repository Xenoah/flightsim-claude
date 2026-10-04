//! Original numerical profile/state reproducing the headless approach failure.
//! This fixture is a numerical integration regression, not real aircraft data.
use flightsim_core::{Ecef, Geodetic, Meters, Seconds};
use flightsim_fdm::subsonic::{
    AirframeDefinition, DryJetDefinition, DryJetTable, JetAircraftConfig, JetFlightDynamics,
    MachAeroDefinition, MachAeroSchedule, OperatingEnvelope, OperatingEnvelopeDefinition,
};
use flightsim_fdm::{ControlInputs, Environment, GroundSlope, RigidBodyState};
use glam::{DQuat, DVec3};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    airframe: AirframeDefinition,
    thrust: DryJetDefinition,
    aero: MachAeroDefinition,
    envelope: OperatingEnvelopeDefinition,
    initial_state_bits: [u64; 13],
    precontact_state_bits: [u64; 13],
}
fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/jet-contact-dynamics.json")).unwrap()
}
fn decoded_state(bits: [u64; 13]) -> RigidBodyState {
    let values = bits.map(f64::from_bits);
    RigidBodyState {
        position: Ecef(DVec3::from_array(values[0..3].try_into().unwrap())),
        velocity: DVec3::from_array(values[3..6].try_into().unwrap()),
        orientation: DQuat::from_array(values[6..10].try_into().unwrap()),
        angular_velocity: DVec3::from_array(values[10..13].try_into().unwrap()),
    }
}
fn config() -> JetAircraftConfig {
    let fixture = fixture();
    JetAircraftConfig::new(
        fixture.airframe.to_config().unwrap(),
        DryJetTable::from_definition(fixture.thrust).unwrap(),
        MachAeroSchedule::from_definition(fixture.aero).unwrap(),
        OperatingEnvelope::from_definition(fixture.envelope).unwrap(),
    )
    .unwrap()
}
fn controls() -> ControlInputs {
    ControlInputs::neutral()
        .with_elevator(0.14485858714049138)
        .with_throttle(0.28584586205596874)
        .with_flaps(0.5)
}
fn plane(state: &RigidBodyState) -> Environment {
    let position = state.geodetic();
    Environment::still_air().with_ground_plane(
        Geodetic {
            altitude: Meters::ZERO,
            ..position
        },
        Meters::ZERO,
        GroundSlope::LEVEL,
    )
}

#[test]
fn exact_headless_precontact_state_advances_before_contact_in_next_interval() {
    // Last committed state at cursor 3574 of the original 35 m/s, 2 m/s
    // descending trimmed approach. Before correction K4 looked beyond dt and
    // returned SubstepBudgetExceeded although this interval was still airborne.
    let start = decoded_state(fixture().precontact_state_bits);
    let mut model = JetFlightDynamics::new(config(), start).unwrap();
    let report = model
        .step(Seconds(1.0 / 120.0), controls(), &plane(&start))
        .unwrap();
    assert_eq!(report.substeps, 1);
    assert!(model.state().altitude().get() < start.altitude().get());
    let mut contact = false;
    for _ in 0..120 {
        let environment = plane(model.state());
        model
            .step(Seconds(1.0 / 120.0), controls(), &environment)
            .unwrap();
        contact |= model
            .config()
            .airframe()
            .landing_gear()
            .legs()
            .iter()
            .any(|leg| {
                Ecef(
                    model.state().position.as_vec()
                        + model.state().orientation * leg.contact_point().as_vec(),
                )
                .to_geodetic()
                .altitude
                .get()
                    < 0.0
            });
    }
    assert!(
        contact,
        "the real contact interval must be resolved, not skipped"
    );
}

#[test]
fn original_trimmed_approach_runs_from_sixty_meters_through_touchdown() {
    // The exact starting state and held controls from solve_jet_trim in the
    // headless numerical scenario. No gentler descent or acceptance change.
    let start = decoded_state(fixture().initial_state_bits);
    let mut model = JetFlightDynamics::new(config(), start).unwrap();
    let mut contact = false;
    for _ in 0..4800 {
        let environment = plane(model.state());
        model
            .step(Seconds(1.0 / 120.0), controls(), &environment)
            .unwrap();
        if model
            .config()
            .airframe()
            .landing_gear()
            .legs()
            .iter()
            .any(|leg| {
                Ecef(
                    model.state().position.as_vec()
                        + model.state().orientation * leg.contact_point().as_vec(),
                )
                .to_geodetic()
                .altitude
                .get()
                    < 0.0
            })
        {
            let sink = -model.state().vertical_speed().get();
            assert!((0.5..4.0).contains(&sink), "sink rate {sink}");
            contact = true;
            break;
        }
    }
    assert!(contact);
}
