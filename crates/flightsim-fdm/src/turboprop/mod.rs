//! Experimental, running-only single-rotor turboprop law.
//!
//! Three engine states, paired isolated-propeller CT/CP maps and whole-call
//! rollback. No app preset, profile/replay dispatcher or real-aircraft fidelity
//! is implied. The existing legacy and dry-jet laws are not changed.

mod config;
mod governor;
mod power;
mod propeller;
mod runtime;

pub use crate::subsonic::{AxisStatus, MachNumber, PressureRatio, TemperatureRatio};
pub use config::{TurbopropAircraftConfig, TurbopropEnvelope, TurbopropEnvelopeDefinition};
pub use governor::{GovernorDefinition, GovernorSample, SampledGovernor};
pub use power::{
    PowerCellDefinition, PowerConditions, PowerSample, TurbineDefinition, TurbinePowerTable,
};
pub use propeller::{
    PropellerCellDefinition, PropellerConvention, PropellerDefinition, PropellerMap,
    PropellerQuery, PropellerSample, RotationSense,
};
pub use runtime::{
    MAX_TURBOPROP_STEP_DT, MAX_TURBOPROP_SUBSTEP_DT, MAX_TURBOPROP_SUBSTEPS,
    RUNNING_TURBOPROP_MODEL_KIND_ID, TURBOPROP_FDM_MODEL_REVISION, TurbopropDerivative,
    TurbopropDiagnosticValues, TurbopropDiagnostics, TurbopropFailureReason,
    TurbopropFlightDynamics, TurbopropInvalidInput, TurbopropStage, TurbopropState,
    TurbopropStepError, TurbopropStepReport,
};

/// Minimum spacing of every ordered axis/interval. Numerical, not physical.
pub const MIN_AXIS_SPACING: f64 = 1e-9;
pub const MAX_POWER_AXIS_KNOTS: usize = 16;
pub const MAX_PROPELLER_AXIS_KNOTS: usize = 32;

/// Modeled idle-to-maximum turbine response, never a measured spool speed.
/// Construction preserves accepted bits, including negative zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbineFraction(f64);
impl TurbineFraction {
    pub const IDLE: Self = Self(0.0);
    pub const MAXIMUM: Self = Self(1.0);
    /// # Errors
    /// Nonfinite values or a fraction outside `[0,1]`. No clamping.
    pub fn new(value: f64) -> Result<Self, TurbopropEvaluationError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(TurbopropEvaluationError::InvalidInput("turbine_fraction"))
        }
    }
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Axial advance ratio Va/(n D), with n in revolutions per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdvanceRatio(pub f64);
/// Paired dimensionless airplane-convention coefficients. Never independently
/// clipped or repaired; one common interpolation supplies both channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropellerCoefficients {
    pub ct: f64,
    pub cp: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerDomainStatus {
    pub pressure: AxisStatus,
    pub temperature: AxisStatus,
}
impl PowerDomainStatus {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self.pressure, AxisStatus::Within)
            && matches!(self.temperature, AxisStatus::Within)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropellerDomainStatus {
    pub advance_ratio: AxisStatus,
    pub blade_pitch: AxisStatus,
}
impl PropellerDomainStatus {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self.advance_ratio, AxisStatus::Within)
            && matches!(self.blade_pitch, AxisStatus::Within)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurbopropDomainStatus {
    pub pressure: AxisStatus,
    pub temperature: AxisStatus,
    pub mach: AxisStatus,
    pub relative_shaft: AxisStatus,
    pub absolute_spin: AxisStatus,
    pub tip_mach: AxisStatus,
    pub crossflow: AxisStatus,
}
impl TurbopropDomainStatus {
    #[must_use]
    pub fn is_supported(self) -> bool {
        [
            self.pressure,
            self.temperature,
            self.mach,
            self.relative_shaft,
            self.absolute_spin,
            self.tip_mach,
            self.crossflow,
        ]
        .iter()
        .all(|s| *s == AxisStatus::Within)
    }
}

/// Closed reason codes for positive-thrust finite-disk admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PropellerPowerBound {
    NonpositivePower = 1,
    BelowIdealDisk = 2,
    InvalidDerivedBound = 3,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurbopropEvaluationError {
    InvalidInput(&'static str),
    OutsidePowerDomain(PowerDomainStatus),
    OutsidePropellerDomain(PropellerDomainStatus),
    PropellerPowerBound(PropellerPowerBound),
}
impl std::fmt::Display for TurbopropEvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported turboprop evaluation: {self:?}")
    }
}
impl std::error::Error for TurbopropEvaluationError {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurbopropConfigError(pub String);
impl std::fmt::Display for TurbopropConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TurbopropConfigError {}

fn range(name: &str, value: f64, min: f64, max: f64) -> Result<(), TurbopropConfigError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(TurbopropConfigError(format!(
            "{name} must be finite and in {min}..={max}"
        )))
    }
}
fn axis(
    name: &str,
    values: &[f64],
    min: f64,
    max: f64,
    count: usize,
) -> Result<(), TurbopropConfigError> {
    if !(2..=count).contains(&values.len()) {
        return Err(TurbopropConfigError(format!(
            "{name} needs 2..={count} knots"
        )));
    }
    for &v in values {
        range(name, v, min, max)?;
    }
    if values.windows(2).any(|p| p[1] - p[0] < MIN_AXIS_SPACING) {
        return Err(TurbopropConfigError(format!(
            "{name} needs strictly increasing knots at least 1e-9 apart"
        )));
    }
    Ok(())
}
fn status(axis: &[f64], value: f64) -> AxisStatus {
    if value < axis[0] {
        AxisStatus::Below
    } else if value > axis[axis.len() - 1] {
        AxisStatus::Above
    } else {
        AxisStatus::Within
    }
}
fn bracket(axis: &[f64], value: f64) -> (usize, f64) {
    let i = axis
        .partition_point(|&v| v <= value)
        .saturating_sub(1)
        .min(axis.len() - 2);
    (i, (value - axis[i]) / (axis[i + 1] - axis[i]))
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}
fn finite(name: &'static str, value: f64) -> Result<(), TurbopropEvaluationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(TurbopropEvaluationError::InvalidInput(name))
    }
}
fn positive(name: &'static str, value: f64) -> Result<(), TurbopropEvaluationError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(TurbopropEvaluationError::InvalidInput(name))
    }
}

#[cfg(test)]
mod tests;
