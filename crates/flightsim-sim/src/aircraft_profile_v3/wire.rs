//! Concrete original-token DTOs. No Value/from_value or buffered tagged enum.
use super::{ControlDefinition, ExactF64, ModelDefinition, ProfileError};
use crate::aircraft_profile::{
    exact::BoundedVec,
    wire::{AirframeWire, ScheduleWire},
};
use flightsim_fdm::turboprop::{
    GovernorDefinition, PowerCellDefinition, PropellerCellDefinition, PropellerConvention,
    PropellerDefinition, TurbineDefinition, TurbopropEnvelopeDefinition,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileWire {
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
pub(crate) struct DynamicsWire {
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
pub(crate) struct TurbineWire {
    pub schema: u16,
    pub pressure_ratios: BoundedVec<ExactF64, 16>,
    pub temperature_ratios: BoundedVec<ExactF64, 16>,
    pub cells: BoundedVec<PowerCellWire, 256>,
    pub rise_seconds: ExactF64,
    pub fall_seconds: ExactF64,
    pub output_torque_limit_nm: ExactF64,
}
impl TurbineWire {
    pub fn definition(&self) -> TurbineDefinition {
        TurbineDefinition {
            schema: self.schema,
            pressure_ratios: self.pressure_ratios.0.iter().map(|v| v.get()).collect(),
            temperature_ratios: self.temperature_ratios.0.iter().map(|v| v.get()).collect(),
            cells: self
                .cells
                .0
                .iter()
                .map(|c| PowerCellDefinition {
                    idle_w: c.idle_w.get(),
                    maximum_w: c.maximum_w.get(),
                })
                .collect(),
            rise_seconds: self.rise_seconds.get(),
            fall_seconds: self.fall_seconds.get(),
            output_torque_limit_nm: self.output_torque_limit_nm.get(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PowerCellWire {
    pub idle_w: ExactF64,
    pub maximum_w: ExactF64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropellerWire {
    pub schema: u16,
    pub convention: String,
    pub diameter_m: ExactF64,
    pub rotor_axial_inertia_kg_m2: ExactF64,
    pub rotation_sense: i8,
    pub advance_ratio: BoundedVec<ExactF64, 32>,
    pub blade_pitch_rad: BoundedVec<ExactF64, 32>,
    pub cells: BoundedVec<PropellerCellWire, 1024>,
}
impl PropellerWire {
    pub fn definition(&self) -> Result<PropellerDefinition, ProfileError> {
        if self.convention != "isolated_axial_propeller" {
            return Err(ProfileError("unsupported propeller convention".into()));
        }
        Ok(PropellerDefinition {
            schema: self.schema,
            convention: PropellerConvention::IsolatedAxialPropeller,
            diameter_m: self.diameter_m.get(),
            rotor_axial_inertia_kg_m2: self.rotor_axial_inertia_kg_m2.get(),
            rotation_sense: self.rotation_sense,
            advance_ratio: self.advance_ratio.0.iter().map(|v| v.get()).collect(),
            blade_pitch_rad: self.blade_pitch_rad.0.iter().map(|v| v.get()).collect(),
            cells: self
                .cells
                .0
                .iter()
                .map(|c| PropellerCellDefinition {
                    ct: c.ct.get(),
                    cp: c.cp.get(),
                })
                .collect(),
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropellerCellWire {
    pub ct: ExactF64,
    pub cp: ExactF64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GovernorWire {
    pub schema: u16,
    pub reference_rad_s: ExactF64,
    pub gain: ExactF64,
    pub fine_rate_rad_s: ExactF64,
    pub coarse_rate_rad_s: ExactF64,
    pub minimum_pitch_rad: ExactF64,
    pub maximum_pitch_rad: ExactF64,
}
impl GovernorWire {
    pub fn definition(&self) -> GovernorDefinition {
        GovernorDefinition {
            schema: self.schema,
            reference_rad_s: self.reference_rad_s.get(),
            gain: self.gain.get(),
            fine_rate_rad_s: self.fine_rate_rad_s.get(),
            coarse_rate_rad_s: self.coarse_rate_rad_s.get(),
            minimum_pitch_rad: self.minimum_pitch_rad.get(),
            maximum_pitch_rad: self.maximum_pitch_rad.get(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvelopeWire {
    pub pressure_ratio: [ExactF64; 2],
    pub temperature_ratio: [ExactF64; 2],
    pub mach: [ExactF64; 2],
    pub relative_shaft_rad_s: [ExactF64; 2],
    pub absolute_spin_rad_s: [ExactF64; 2],
    pub maximum_helical_tip_mach: ExactF64,
    pub maximum_crossflow_tip_ratio: ExactF64,
}
impl EnvelopeWire {
    pub fn definition(&self) -> TurbopropEnvelopeDefinition {
        TurbopropEnvelopeDefinition {
            pressure_ratio: self.pressure_ratio.map(ExactF64::get),
            temperature_ratio: self.temperature_ratio.map(ExactF64::get),
            mach: self.mach.map(ExactF64::get),
            relative_shaft_rad_s: self.relative_shaft_rad_s.map(ExactF64::get),
            absolute_spin_rad_s: self.absolute_spin_rad_s.map(ExactF64::get),
            maximum_helical_tip_mach: self.maximum_helical_tip_mach.get(),
            maximum_crossflow_tip_ratio: self.maximum_crossflow_tip_ratio.get(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunningStartWire {
    pub turbine_fraction: ExactF64,
    pub shaft_rad_s: ExactF64,
    pub blade_pitch_rad: ExactF64,
}
