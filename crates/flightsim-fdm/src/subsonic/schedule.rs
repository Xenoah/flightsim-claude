use super::{
    AxisStatus, EvaluationError, MAX_TABLE_MACH, MachNumber, TableError, axis_bytes, axis_status,
    bracket, lerp, nonnegative_input, range, scalar_bytes, validate_axis,
};
use crate::{AeroCoefficients, definition::AerodynamicDefinition};
use flightsim_core::Radians;
use serde::{Deserialize, Serialize};

/// External-data boundary for a complete coefficient schedule. Owners must
/// bound serialized bytes before deserialization, then call `from_definition`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachAeroDefinition {
    pub schema: u16,
    pub knots: Vec<MachAeroKnotDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachAeroKnotDefinition {
    pub mach: f64,
    pub aero: AerodynamicDefinition,
}

/// Continuous piecewise-linear, complete aerodynamic coefficient schedule.
/// Values are continuous, derivatives need not be continuous at knots. This
/// supplies no singular compressibility formula and no claimed transonic,
/// Reynolds-number, sweep, buffet or supersonic fidelity.
#[derive(Debug, Clone)]
pub struct MachAeroSchedule {
    definition: MachAeroDefinition,
    mach: Vec<f64>,
    values: Vec<[f64; 25]>,
}

#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MachAeroSample {
    pub coefficients: AeroCoefficients,
    pub domain: AxisStatus,
}

impl MachAeroSchedule {
    /// Validate complete coefficients at 2..=32 ordered Mach knots, starting at
    /// zero and ending no higher than 0.95. Every coefficient has absolute value
    /// <=100. Additional denominator/stall/drag bounds match the v1 numerical
    /// ranges. Stability/control signs are not inferred or repaired: this
    /// primitive does not certify static/dynamic stability or trimability.
    ///
    /// # Errors
    /// Unsupported schema, malformed axis, or invalid/unsupported coefficients.
    pub fn from_definition(definition: MachAeroDefinition) -> Result<Self, TableError> {
        if definition.schema != 1 {
            return Err(TableError("unsupported Mach-aero schedule schema".into()));
        }
        if !(2..=super::MAX_AXIS_KNOTS).contains(&definition.knots.len()) {
            return Err(TableError("Mach-aero schedule needs 2..=32 knots".into()));
        }
        let mach: Vec<_> = definition.knots.iter().map(|knot| knot.mach).collect();
        validate_axis("mach", &mach, 0.0, MAX_TABLE_MACH)?;
        if mach[0].abs() > 0.0 {
            return Err(TableError("Mach schedule must start at zero".into()));
        }
        let mut values = Vec::with_capacity(mach.len());
        for knot in &definition.knots {
            let scalars = coefficient_scalars(knot.aero);
            for &value in &scalars {
                range("aerodynamic coefficient", value, -100.0, 100.0)?;
            }
            let aero = knot.aero;
            range("stall_angle_rad", aero.stall_angle_rad, 0.05, 0.7)?;
            range("stall_blend_rate", aero.stall_blend_rate, 0.1, 100.0)?;
            range("drag_min", aero.drag_min, 0.0001, 1.0)?;
            range("oswald_efficiency", aero.oswald_efficiency, 0.05, 1.0)?;
            range("lift_flaps", aero.lift_flaps, 0.0, 100.0)?;
            range("drag_flaps", aero.drag_flaps, 0.0, 100.0)?;
            values.push(scalars);
        }
        Ok(Self {
            definition,
            mach,
            values,
        })
    }

    #[must_use]
    pub const fn definition(&self) -> &MachAeroDefinition {
        &self.definition
    }

    /// Linearly interpolate every coefficient with the same Mach weight.
    /// Exact knots return their authored scalar bits, including signed zero.
    ///
    /// # Errors
    /// Invalid Mach or an out-of-table query. No clamp/extrapolation is supplied.
    pub fn sample(&self, mach: MachNumber) -> Result<MachAeroSample, EvaluationError> {
        nonnegative_input("mach", mach.0)?;
        let domain = axis_status(&self.mach, mach.0);
        if domain != AxisStatus::Within {
            return Err(EvaluationError::OutsideMachDomain(domain));
        }
        let (index, fraction) = bracket(&self.mach, mach.0);
        let values = std::array::from_fn(|field| {
            lerp(
                self.values[index][field],
                self.values[index + 1][field],
                fraction,
            )
        });
        Ok(MachAeroSample {
            coefficients: coefficients(values),
            domain,
        })
    }

    /// Canonical component bytes, not a replay identity or compatibility gate.
    /// Domain + u16-LE local schema + u32-LE knot count + all Mach f64-LE bits,
    /// followed by each knot's 25 coefficient f64-LE bits in the documented
    /// `AerodynamicDefinition` field order. The stall angle is radians.
    /// Signed zero survives; metadata is absent. The host must separately bind
    /// aircraft/model kind, numerical revision and remaining physical fields.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = b"flightsim/subsonic-mach-aero\0".to_vec();
        bytes.extend_from_slice(&self.definition.schema.to_le_bytes());
        axis_bytes(&mut bytes, &self.mach);
        for values in &self.values {
            for &value in values {
                scalar_bytes(&mut bytes, value);
            }
        }
        bytes
    }
}

// Exhaustive destructuring makes future coefficient additions an explicit
// schedule/identity decision. The order is fixed by component schema 1.
fn coefficient_scalars(aero: AerodynamicDefinition) -> [f64; 25] {
    let AerodynamicDefinition {
        lift_zero,
        lift_alpha,
        lift_flaps,
        stall_angle_rad,
        stall_blend_rate,
        drag_min,
        oswald_efficiency,
        drag_flaps,
        side_beta,
        side_rudder,
        roll_beta,
        roll_rate_p,
        roll_rate_r,
        roll_aileron,
        roll_rudder,
        pitch_zero,
        pitch_alpha,
        pitch_rate_q,
        pitch_elevator,
        pitch_flaps,
        yaw_beta,
        yaw_rate_p,
        yaw_rate_r,
        yaw_aileron,
        yaw_rudder,
    } = aero;
    [
        lift_zero,
        lift_alpha,
        lift_flaps,
        stall_angle_rad,
        stall_blend_rate,
        drag_min,
        oswald_efficiency,
        drag_flaps,
        side_beta,
        side_rudder,
        roll_beta,
        roll_rate_p,
        roll_rate_r,
        roll_aileron,
        roll_rudder,
        pitch_zero,
        pitch_alpha,
        pitch_rate_q,
        pitch_elevator,
        pitch_flaps,
        yaw_beta,
        yaw_rate_p,
        yaw_rate_r,
        yaw_aileron,
        yaw_rudder,
    ]
}

fn coefficients(values: [f64; 25]) -> AeroCoefficients {
    let [
        lift_zero,
        lift_alpha,
        lift_flaps,
        stall_angle_rad,
        stall_blend_rate,
        drag_min,
        oswald_efficiency,
        drag_flaps,
        side_beta,
        side_rudder,
        roll_beta,
        roll_rate_p,
        roll_rate_r,
        roll_aileron,
        roll_rudder,
        pitch_zero,
        pitch_alpha,
        pitch_rate_q,
        pitch_elevator,
        pitch_flaps,
        yaw_beta,
        yaw_rate_p,
        yaw_rate_r,
        yaw_aileron,
        yaw_rudder,
    ] = values;
    AeroCoefficients {
        lift_zero,
        lift_alpha,
        lift_flaps,
        stall_angle: Radians(stall_angle_rad),
        stall_blend_rate,
        drag_min,
        oswald_efficiency,
        drag_flaps,
        side_beta,
        side_rudder,
        roll_beta,
        roll_rate_p,
        roll_rate_r,
        roll_aileron,
        roll_rudder,
        pitch_zero,
        pitch_alpha,
        pitch_rate_q,
        pitch_elevator,
        pitch_flaps,
        yaw_beta,
        yaw_rate_p,
        yaw_rate_r,
        yaw_aileron,
        yaw_rudder,
    }
}
