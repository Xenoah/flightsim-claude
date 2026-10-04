#![allow(dead_code)]
use flightsim_core::{Attitude, Geodetic, Ned, Radians, RadiansPerSecond};
use flightsim_fdm::{
    RigidBodyState,
    subsonic::{AirframeDefinition, MachAeroDefinition, MachAeroSchedule},
    turboprop::*,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    airframe: AirframeDefinition,
    turbine: TurbineDefinition,
    propeller: PropellerDefinition,
    governor: GovernorDefinition,
    envelope: TurbopropEnvelopeDefinition,
    aero: MachAeroDefinition,
}
pub fn config() -> TurbopropAircraftConfig {
    let f: Fixture = serde_json::from_str(include_str!(
        "../../../flightsim-fdm/tests/fixtures/turboprop-numerical.json"
    ))
    .unwrap();
    TurbopropAircraftConfig::new(
        f.airframe.to_config().unwrap(),
        TurbinePowerTable::from_definition(f.turbine).unwrap(),
        PropellerMap::from_definition(f.propeller).unwrap(),
        SampledGovernor::from_definition(f.governor).unwrap(),
        MachAeroSchedule::from_definition(f.aero).unwrap(),
        TurbopropEnvelope::from_definition(f.envelope).unwrap(),
    )
    .unwrap()
}
/// Ballistic axial descent keeps this zero-lift numerical fixture within its
/// forward-flow domain for replay tests. It is not a qualified aircraft trim.
pub fn initial() -> TurbopropState {
    TurbopropState {
        rigid_body: RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0., 0., 1000.),
            Attitude::new(
                Radians::ZERO,
                Radians(-std::f64::consts::FRAC_PI_2),
                Radians::ZERO,
            ),
            Ned::new(0., 0., 40.),
        ),
        turbine_fraction: TurbineFraction::new(0.4).unwrap(),
        shaft_rad_s: RadiansPerSecond(180.),
        blade_pitch_rad: Radians(0.3),
    }
}
