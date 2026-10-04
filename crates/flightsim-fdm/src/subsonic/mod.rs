//! Opt-in, stateless dry-jet components and a separate bounded jet flight model.
//!
//! These APIs are deliberately disconnected from the legacy propeller model.
//! Tables describe authored approximations; validation establishes numerical
//! bounds, not fidelity to a real aircraft. Unsupported conditions return an
//! error with domain diagnostics, never silently held/extrapolated forces.

mod config;
mod jet;
mod runtime;
mod schedule;

pub use config::{
    AirframeConfig, AirframeDefinition, JetAircraftConfig, JetConfigError, OperatingEnvelope,
    OperatingEnvelopeDefinition,
};
pub use jet::{DryJetDefinition, DryJetTable, NetThrustCellDefinition, ThrustSample};
pub use runtime::{
    DRY_JET_MODEL_KIND_ID, JET_FDM_MODEL_REVISION, JetFailureReason, JetFlightDynamics,
    JetInvalidInput, JetStage, JetStepError, JetStepReport, MAX_JET_STEP_DT, MAX_JET_SUBSTEP_DT,
    MAX_JET_SUBSTEPS,
};
pub use schedule::{MachAeroDefinition, MachAeroKnotDefinition, MachAeroSample, MachAeroSchedule};

use crate::atmosphere::{AtmosphereSample, SEA_LEVEL_PRESSURE, SEA_LEVEL_TEMPERATURE};
use flightsim_core::MetersPerSecond;

/// Numerical resource bound, not a statement of physical accuracy.
pub const MAX_AXIS_KNOTS: usize = 32;
/// Numerical resource bound for pressure × temperature × Mach cells.
pub const MAX_THRUST_CELLS: usize = 4096;
/// This foundation does not admit sonic or supersonic table knots.
pub const MAX_TABLE_MACH: f64 = 0.95;
/// Minimum ordered-axis spacing prevents a poorly conditioned interpolation.
pub const MIN_AXIS_SPACING: f64 = 1e-9;

/// Ambient static pressure / ISA sea-level static pressure, dimensionless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureRatio(pub f64);
/// Ambient static absolute temperature / ISA sea-level temperature, dimensionless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemperatureRatio(pub f64);
/// True airspeed / local sound speed, dimensionless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MachNumber(pub f64);
/// Commanded idle-to-maximum-dry thrust fraction, in [0, 1]. Zero means idle,
/// not engine shutdown. It does not represent shaft speed or fuel flow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DryThrottle(pub f64);

/// Ambient conditions for a table query; validated at every evaluation boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JetConditions {
    pub pressure_ratio: PressureRatio,
    pub temperature_ratio: TemperatureRatio,
    pub mach: MachNumber,
}

impl JetConditions {
    /// Reuse the FDM atmosphere's static pressure, absolute temperature and
    /// local sound speed. Density is not a jet thrust scaling input.
    ///
    /// # Errors
    /// Negative/nonfinite pressure or airspeed, nonpositive/nonfinite temperature
    /// or sound speed, or nonfinite derived ratios. The unused density field is
    /// not inspected; this does not validate an entire atmosphere sample.
    pub fn from_atmosphere(
        air: AtmosphereSample,
        true_airspeed: MetersPerSecond,
    ) -> Result<Self, EvaluationError> {
        nonnegative_input("pressure", air.pressure.get())?;
        positive_input("temperature", air.temperature.get())?;
        positive_input("speed_of_sound", air.speed_of_sound.get())?;
        nonnegative_input("true_airspeed", true_airspeed.get())?;
        let conditions = Self {
            pressure_ratio: PressureRatio(air.pressure.get() / SEA_LEVEL_PRESSURE),
            temperature_ratio: TemperatureRatio(air.temperature.get() / SEA_LEVEL_TEMPERATURE),
            mach: MachNumber(air.mach(true_airspeed)),
        };
        conditions.validate()?;
        Ok(conditions)
    }

    fn validate(self) -> Result<(), EvaluationError> {
        nonnegative_input("pressure_ratio", self.pressure_ratio.0)?;
        positive_input("temperature_ratio", self.temperature_ratio.0)?;
        nonnegative_input("mach", self.mach.0)
    }
}

/// Inclusive table endpoints are supported. No runtime axis is clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStatus {
    Below,
    Within,
    Above,
}

/// Diagnostics include every table axis, even when several are unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JetDomainStatus {
    pub pressure: AxisStatus,
    pub temperature: AxisStatus,
    pub mach: AxisStatus,
}

impl JetDomainStatus {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self.pressure, AxisStatus::Within)
            && matches!(self.temperature, AxisStatus::Within)
            && matches!(self.mach, AxisStatus::Within)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationError {
    InvalidInput(&'static str),
    OutsideJetDomain(JetDomainStatus),
    OutsideMachDomain(AxisStatus),
}

impl std::fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported subsonic evaluation: {self:?}")
    }
}
impl std::error::Error for EvaluationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableError(pub String);
impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TableError {}

fn nonnegative_input(name: &'static str, value: f64) -> Result<(), EvaluationError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(EvaluationError::InvalidInput(name))
    }
}

fn positive_input(name: &'static str, value: f64) -> Result<(), EvaluationError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(EvaluationError::InvalidInput(name))
    }
}

fn range(name: &str, value: f64, min: f64, max: f64) -> Result<(), TableError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(TableError(format!(
            "{name} must be finite and in {min}..={max}"
        )))
    }
}

fn validate_axis(name: &str, axis: &[f64], min: f64, max: f64) -> Result<(), TableError> {
    if !(2..=MAX_AXIS_KNOTS).contains(&axis.len()) {
        return Err(TableError(format!(
            "{name} needs 2..={MAX_AXIS_KNOTS} knots"
        )));
    }
    for &value in axis {
        range(name, value, min, max)?;
    }
    if axis
        .windows(2)
        .any(|pair| pair[1] - pair[0] < MIN_AXIS_SPACING)
    {
        return Err(TableError(format!(
            "{name} knots must increase by at least {MIN_AXIS_SPACING}"
        )));
    }
    Ok(())
}

fn axis_status(axis: &[f64], value: f64) -> AxisStatus {
    if value < axis[0] {
        AxisStatus::Below
    } else if value > axis[axis.len() - 1] {
        AxisStatus::Above
    } else {
        AxisStatus::Within
    }
}

// Only called with an immutable validated axis and a finite supported query.
fn bracket(axis: &[f64], value: f64) -> (usize, f64) {
    let lower = axis
        .partition_point(|&knot| knot <= value)
        .saturating_sub(1)
        .min(axis.len() - 2);
    (
        lower,
        (value - axis[lower]) / (axis[lower + 1] - axis[lower]),
    )
}

// Exact endpoint branches preserve the authored IEEE-754 bits (including -0).
// Bounded values make b-a finite; no fused multiply-add is requested.
fn lerp(a: f64, b: f64, fraction: f64) -> f64 {
    if fraction <= 0.0 {
        a
    } else if fraction >= 1.0 {
        b
    } else {
        a + (b - a) * fraction
    }
}

fn scalar_bytes(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn axis_bytes(bytes: &mut Vec<u8>, axis: &[f64]) {
    bytes.extend_from_slice(
        &u32::try_from(axis.len())
            .expect("validated bounded axis")
            .to_le_bytes(),
    );
    for &value in axis {
        scalar_bytes(bytes, value);
    }
}
