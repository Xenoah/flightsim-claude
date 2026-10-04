//! Synthetic numerical configuration with explicitly authored negative rows.
//! It is not a measured propeller map or a qualified aircraft preset.
#![allow(dead_code)]
#[path = "../turboprop_common/mod.rs"]
mod law1;
use flightsim_fdm::turboprop::{self, PropellerCellDefinition, near_static};
pub use law1::initial;
pub fn config() -> near_static::TurbopropAircraftConfig {
    wrap_forward(law1::config())
}
/// Original test coefficients at J=-0.01 and -0.005. Each row satisfies the
/// positive static power floor and the retained pitch feedback convention.
pub fn wrap_forward(
    forward: turboprop::TurbopropAircraftConfig,
) -> near_static::TurbopropAircraftConfig {
    near_static::TurbopropAircraftConfig::from_forward(
        forward,
        [
            vec![
                PropellerCellDefinition { ct: 0.01, cp: 0.03 },
                PropellerCellDefinition { ct: 0.01, cp: 0.3 },
            ],
            vec![
                PropellerCellDefinition { ct: 0.01, cp: 0.03 },
                PropellerCellDefinition { ct: 0.01, cp: 0.3 },
            ],
        ],
    )
    .unwrap()
}

use flightsim_core::{Ecef, Radians, RadiansPerSecond};
use flightsim_fdm::{
    RigidBodyState,
    subsonic::{AirframeDefinition, MachAeroDefinition, MachAeroSchedule},
};
use glam::{DQuat, DVec3};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CedarComponents {
    airframe: AirframeDefinition,
    turbine: turboprop::TurbineDefinition,
    propeller: turboprop::PropellerDefinition,
    governor: turboprop::GovernorDefinition,
    envelope: turboprop::TurbopropEnvelopeDefinition,
    aero: MachAeroDefinition,
}
// Fixture-only exact-token reader, preserving the accepted law-1 source bits.
// This is not a new profile parser and performs no runtime row generation.
fn exact_fixture_value(raw: &RawValue) -> Value {
    let token = raw.get();
    match token.as_bytes()[0] {
        b'{' => {
            let fields: BTreeMap<String, &RawValue> = serde_json::from_str(token).unwrap();
            Value::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| (k, exact_fixture_value(v)))
                    .collect(),
            )
        }
        b'[' => {
            let values: Vec<&RawValue> = serde_json::from_str(token).unwrap();
            Value::Array(values.into_iter().map(exact_fixture_value).collect())
        }
        b'"' | b'n' | b't' | b'f' => serde_json::from_str(token).unwrap(),
        _ if token.contains(['.', 'e', 'E']) || token == "-0" => {
            Value::Number(serde_json::Number::from_f64(token.parse::<f64>().unwrap()).unwrap())
        }
        _ => serde_json::from_str(token).unwrap(),
    }
}

pub fn cedar_forward() -> turboprop::TurbopropAircraftConfig {
    let raw: &RawValue = serde_json::from_str(include_str!(
        "../../../flightsim-fdm/tests/fixtures/cedar-law1-components.json"
    ))
    .unwrap();
    let d: CedarComponents = serde_json::from_value(exact_fixture_value(raw)).unwrap();
    turboprop::TurbopropAircraftConfig::new(
        d.airframe.to_config().unwrap(),
        turboprop::TurbinePowerTable::from_definition(d.turbine).unwrap(),
        turboprop::PropellerMap::from_definition(d.propeller).unwrap(),
        turboprop::SampledGovernor::from_definition(d.governor).unwrap(),
        MachAeroSchedule::from_definition(d.aero).unwrap(),
        turboprop::TurbopropEnvelope::from_definition(d.envelope).unwrap(),
    )
    .unwrap()
}

/// Original Cedar candidate continuation, stored explicitly for this fixture.
/// CT=0.5*(beta-atan(J/(0.75*pi))); CP=1.22*CT/2*(J+sqrt(J²+8*CT/pi))+0.005+0.01*beta.
/// At negative J this is authored approximation data, not an empirical map.
pub fn cedar() -> near_static::TurbopropAircraftConfig {
    let raw: &RawValue = serde_json::from_str(include_str!(
        "../fixtures/near-static-cedar-negative-rows.json"
    ))
    .unwrap();
    let rows = serde_json::from_value(exact_fixture_value(raw)).unwrap();
    near_static::TurbopropAircraftConfig::from_forward(cedar_forward(), rows).unwrap()
}

pub fn boundary_state() -> turboprop::TurbopropState {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../flightsim-fdm/tests/fixtures/cedar-law1-calm-braking-boundary.json"
    ))
    .unwrap();
    let values: Vec<_> = fixture["last_committed_state"]["canonical_state_bits_hex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap()))
        .collect();
    turboprop::TurbopropState {
        rigid_body: RigidBodyState {
            position: Ecef(DVec3::new(values[0], values[1], values[2])),
            velocity: DVec3::new(values[3], values[4], values[5]),
            orientation: DQuat::from_xyzw(values[6], values[7], values[8], values[9]),
            angular_velocity: DVec3::new(values[10], values[11], values[12]),
        },
        turbine_fraction: turboprop::TurbineFraction::new(values[13]).unwrap(),
        shaft_rad_s: RadiansPerSecond(values[14]),
        blade_pitch_rad: Radians(values[15]),
    }
}
