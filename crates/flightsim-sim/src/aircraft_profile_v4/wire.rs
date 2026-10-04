//! New concrete DTOs borrow original numeric tokens. No conversion through v3
//! JSON, Value, tagged enums or floating-point buffers occurs.
use super::{
    ControlDefinition, ExactF64, ModelDefinition, NEAR_STATIC_PROPELLER_COMPONENT_SCHEMA,
    ProfileError, STATIC_POWER_EPSILON_MULTIPLIER,
};
use crate::aircraft_profile::{
    exact::BoundedVec,
    wire::{AirframeWire, ScheduleWire},
};
use crate::aircraft_profile_v3::wire::{
    EnvelopeWire, GovernorWire, PropellerCellWire, PropellerWire as ForwardPropellerWire,
    RunningStartWire, TurbineWire,
};
use flightsim_fdm::turboprop::{
    PropellerCellDefinition,
    near_static::{
        MAX_ADVERSE_INFLOW_RATIO, MAX_TRANSVERSE_INFLOW_RATIO, NEGATIVE_ADVANCE_RATIO_KNOTS,
    },
};
use serde::{Deserialize, Serialize};

/// The values are commitments to the existing law, not configurable tolerances.
pub(super) const INTERPOLATION: &str = "signed_pitch_then_advance_ratio";
pub(super) const STATIC_POWER_BOUND: &str = "positive_static_actuator_disk_floor";
pub(super) const INFLOW_SCALE: &str = "current_positive_thrust_hover_velocity";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProfileWire {
    pub version: u16,
    pub id: String,
    pub dynamics: DynamicsWire,
    pub model: ModelDefinition,
    pub controls: ControlDefinition,
    pub camera_eye_m: [ExactF64; 3],
    pub engine_sound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DynamicsWire {
    pub kind: String,
    pub revision: u32,
    pub airframe: AirframeWire,
    pub turbine: TurbineWire,
    pub propeller: PropellerWire,
    pub governor: GovernorWire,
    pub aero: ScheduleWire,
    pub envelope: EnvelopeWire,
    pub running_start: RunningStartWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PropellerWire {
    pub schema: u16,
    pub forward: ForwardPropellerWire,
    pub negative_advance_ratio: [ExactF64; 2],
    pub negative_rows: [BoundedVec<PropellerCellWire, 32>; 2],
    pub near_static_domain: DomainWire,
}
impl PropellerWire {
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.schema != NEAR_STATIC_PROPELLER_COMPONENT_SCHEMA {
            return Err(ProfileError(
                "unsupported near-static propeller schema (expected 2)".into(),
            ));
        }
        for (actual, expected) in self
            .negative_advance_ratio
            .iter()
            .zip(NEGATIVE_ADVANCE_RATIO_KNOTS)
        {
            exact_commitment("negative_advance_ratio", *actual, expected)?;
        }
        self.near_static_domain.validate()
    }
    pub fn negative_rows(&self) -> [Vec<PropellerCellDefinition>; 2] {
        self.negative_rows.each_ref().map(|row| {
            row.0
                .iter()
                .map(|cell| PropellerCellDefinition {
                    ct: cell.ct.get(),
                    cp: cell.cp.get(),
                })
                .collect()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DomainWire {
    pub maximum_adverse_inflow_ratio: ExactF64,
    pub maximum_transverse_inflow_ratio: ExactF64,
    pub static_power_epsilon_multiplier: ExactF64,
    pub interpolation: String,
    pub static_power_bound: String,
    pub inflow_scale: String,
}
impl DomainWire {
    fn validate(&self) -> Result<(), ProfileError> {
        exact_commitment(
            "maximum_adverse_inflow_ratio",
            self.maximum_adverse_inflow_ratio,
            MAX_ADVERSE_INFLOW_RATIO,
        )?;
        exact_commitment(
            "maximum_transverse_inflow_ratio",
            self.maximum_transverse_inflow_ratio,
            MAX_TRANSVERSE_INFLOW_RATIO,
        )?;
        exact_commitment(
            "static_power_epsilon_multiplier",
            self.static_power_epsilon_multiplier,
            STATIC_POWER_EPSILON_MULTIPLIER,
        )?;
        if self.interpolation != INTERPOLATION
            || self.static_power_bound != STATIC_POWER_BOUND
            || self.inflow_scale != INFLOW_SCALE
        {
            return Err(ProfileError(
                "unsupported near-static domain semantics".into(),
            ));
        }
        Ok(())
    }
}
fn exact_commitment(name: &str, actual: ExactF64, expected: f64) -> Result<(), ProfileError> {
    if actual.get().to_bits() != expected.to_bits() {
        return Err(ProfileError(format!(
            "unsupported fixed near-static {name}"
        )));
    }
    Ok(())
}
