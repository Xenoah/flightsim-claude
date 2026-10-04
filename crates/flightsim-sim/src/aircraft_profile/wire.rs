//! Private wire DTOs are decoded once from bounded original JSON. They are
//! deliberately concrete structs, not internally tagged/untagged serde enums.
use super::{ControlDefinition, ExactF64, ModelDefinition, ProfileError, exact::BoundedVec};
use flightsim_fdm::{
    definition::{AerodynamicDefinition, GearDefinition},
    subsonic::{
        AirframeDefinition, DryJetDefinition, MachAeroDefinition, MachAeroKnotDefinition,
        NetThrustCellDefinition, OperatingEnvelopeDefinition,
    },
};
use serde::{Deserialize, Serialize};

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
    pub thrust: ThrustWire,
    pub aero: ScheduleWire,
    pub envelope: EnvelopeWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AirframeWire {
    pub name: String,
    pub mass_kg: ExactF64,
    pub inertia_kg_m2: [ExactF64; 4],
    pub wing_area_m2: ExactF64,
    pub wing_span_m: ExactF64,
    pub mean_chord_m: ExactF64,
    pub landing_gear: BoundedVec<GearWire, 3>,
    pub rolling_friction: ExactF64,
    pub braking_friction: ExactF64,
    pub lateral_friction: ExactF64,
    pub friction_transition_mps: ExactF64,
}

impl AirframeWire {
    pub fn definition(&self) -> Result<AirframeDefinition, ProfileError> {
        let gear: [GearDefinition; 3] = self
            .landing_gear
            .0
            .iter()
            .map(GearWire::definition)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| ProfileError("exactly three landing gear contacts are required".into()))?;
        Ok(AirframeDefinition {
            name: self.name.clone(),
            mass_kg: self.mass_kg.get(),
            inertia_kg_m2: self.inertia_kg_m2.map(ExactF64::get),
            wing_area_m2: self.wing_area_m2.get(),
            wing_span_m: self.wing_span_m.get(),
            mean_chord_m: self.mean_chord_m.get(),
            landing_gear: gear,
            rolling_friction: self.rolling_friction.get(),
            braking_friction: self.braking_friction.get(),
            lateral_friction: self.lateral_friction.get(),
            friction_transition_mps: self.friction_transition_mps.get(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GearWire {
    pub contact_m: [ExactF64; 3],
    pub spring_n_per_m: ExactF64,
    pub damping_ns_per_m: ExactF64,
    pub max_stroke_m: ExactF64,
    pub bottom_stop_travel_m: ExactF64,
    pub max_recoil_mps: ExactF64,
}

impl GearWire {
    fn definition(&self) -> GearDefinition {
        GearDefinition {
            contact_m: self.contact_m.map(ExactF64::get),
            spring_n_per_m: self.spring_n_per_m.get(),
            damping_ns_per_m: self.damping_ns_per_m.get(),
            max_stroke_m: self.max_stroke_m.get(),
            bottom_stop_travel_m: self.bottom_stop_travel_m.get(),
            max_recoil_mps: self.max_recoil_mps.get(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ThrustWire {
    pub schema: u16,
    pub pressure_ratios: BoundedVec<ExactF64, 32>,
    pub temperature_ratios: BoundedVec<ExactF64, 32>,
    pub mach: BoundedVec<ExactF64, 32>,
    pub cells: BoundedVec<ThrustCellWire, 4096>,
}
impl ThrustWire {
    pub fn definition(&self) -> DryJetDefinition {
        DryJetDefinition {
            schema: self.schema,
            pressure_ratios: self
                .pressure_ratios
                .0
                .iter()
                .map(|value| value.get())
                .collect(),
            temperature_ratios: self
                .temperature_ratios
                .0
                .iter()
                .map(|value| value.get())
                .collect(),
            mach: self.mach.0.iter().map(|value| value.get()).collect(),
            cells: self
                .cells
                .0
                .iter()
                .map(|cell| NetThrustCellDefinition {
                    idle_n: cell.idle_n.get(),
                    maximum_dry_n: cell.maximum_dry_n.get(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ThrustCellWire {
    pub idle_n: ExactF64,
    pub maximum_dry_n: ExactF64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScheduleWire {
    pub schema: u16,
    pub knots: BoundedVec<AeroKnotWire, 32>,
}
impl ScheduleWire {
    pub fn definition(&self) -> MachAeroDefinition {
        MachAeroDefinition {
            schema: self.schema,
            knots: self
                .knots
                .0
                .iter()
                .map(|knot| MachAeroKnotDefinition {
                    mach: knot.mach.get(),
                    aero: knot.aero.definition(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AeroKnotWire {
    pub mach: ExactF64,
    pub aero: AeroWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AeroWire {
    pub lift_zero: ExactF64,
    pub lift_alpha: ExactF64,
    pub lift_flaps: ExactF64,
    pub stall_angle_rad: ExactF64,
    pub stall_blend_rate: ExactF64,
    pub drag_min: ExactF64,
    pub oswald_efficiency: ExactF64,
    pub drag_flaps: ExactF64,
    pub side_beta: ExactF64,
    pub side_rudder: ExactF64,
    pub roll_beta: ExactF64,
    pub roll_rate_p: ExactF64,
    pub roll_rate_r: ExactF64,
    pub roll_aileron: ExactF64,
    pub roll_rudder: ExactF64,
    pub pitch_zero: ExactF64,
    pub pitch_alpha: ExactF64,
    pub pitch_rate_q: ExactF64,
    pub pitch_elevator: ExactF64,
    pub pitch_flaps: ExactF64,
    pub yaw_beta: ExactF64,
    pub yaw_rate_p: ExactF64,
    pub yaw_rate_r: ExactF64,
    pub yaw_aileron: ExactF64,
    pub yaw_rudder: ExactF64,
}
impl AeroWire {
    fn definition(&self) -> AerodynamicDefinition {
        AerodynamicDefinition {
            lift_zero: self.lift_zero.get(),
            lift_alpha: self.lift_alpha.get(),
            lift_flaps: self.lift_flaps.get(),
            stall_angle_rad: self.stall_angle_rad.get(),
            stall_blend_rate: self.stall_blend_rate.get(),
            drag_min: self.drag_min.get(),
            oswald_efficiency: self.oswald_efficiency.get(),
            drag_flaps: self.drag_flaps.get(),
            side_beta: self.side_beta.get(),
            side_rudder: self.side_rudder.get(),
            roll_beta: self.roll_beta.get(),
            roll_rate_p: self.roll_rate_p.get(),
            roll_rate_r: self.roll_rate_r.get(),
            roll_aileron: self.roll_aileron.get(),
            roll_rudder: self.roll_rudder.get(),
            pitch_zero: self.pitch_zero.get(),
            pitch_alpha: self.pitch_alpha.get(),
            pitch_rate_q: self.pitch_rate_q.get(),
            pitch_elevator: self.pitch_elevator.get(),
            pitch_flaps: self.pitch_flaps.get(),
            yaw_beta: self.yaw_beta.get(),
            yaw_rate_p: self.yaw_rate_p.get(),
            yaw_rate_r: self.yaw_rate_r.get(),
            yaw_aileron: self.yaw_aileron.get(),
            yaw_rudder: self.yaw_rudder.get(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnvelopeWire {
    pub pressure_ratio: [ExactF64; 2],
    pub temperature_ratio: [ExactF64; 2],
    pub mach: [ExactF64; 2],
}
impl EnvelopeWire {
    pub fn definition(&self) -> OperatingEnvelopeDefinition {
        OperatingEnvelopeDefinition {
            pressure_ratio: self.pressure_ratio.map(ExactF64::get),
            temperature_ratio: self.temperature_ratio.map(ExactF64::get),
            mach: self.mach.map(ExactF64::get),
        }
    }
}
