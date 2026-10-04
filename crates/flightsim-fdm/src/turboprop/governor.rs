use super::{MIN_AXIS_SPACING, TurbopropConfigError, TurbopropEvaluationError, positive, range};
use flightsim_core::{Radians, RadiansPerSecond, Seconds};
use serde::{Deserialize, Serialize};

/// Raw SI configuration. Gain relates pitch rate to relative shaft error and is
/// dimensionless; beta is the only integral/actuator state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernorDefinition {
    pub schema: u16,
    pub reference_rad_s: f64,
    pub gain: f64,
    pub fine_rate_rad_s: f64,
    pub coarse_rate_rad_s: f64,
    pub minimum_pitch_rad: f64,
    pub maximum_pitch_rad: f64,
}
#[derive(Debug, Clone, Copy)]
pub struct SampledGovernor {
    definition: GovernorDefinition,
}
/// One frozen substep command. Its private stops come from the validated
/// governor. Sampling creates no hidden state and assigns no shaft speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GovernorSample {
    start: Radians,
    rate: RadiansPerSecond,
    minimum: Radians,
    maximum: Radians,
}
impl SampledGovernor {
    /// # Errors
    /// Unknown schema or nonfinite/out-of-policy gain, reference, rates or stops.
    pub fn from_definition(definition: GovernorDefinition) -> Result<Self, TurbopropConfigError> {
        if definition.schema != 1 {
            return Err(TurbopropConfigError("unsupported governor schema".into()));
        }
        range("reference_rad_s", definition.reference_rad_s, 20.0, 1000.0)?;
        range("gain", definition.gain, 0.0001, 10.0)?;
        range("fine_rate_rad_s", definition.fine_rate_rad_s, 0.001, 1.0)?;
        range(
            "coarse_rate_rad_s",
            definition.coarse_rate_rad_s,
            0.001,
            1.0,
        )?;
        range(
            "minimum_pitch_rad",
            definition.minimum_pitch_rad,
            0.0,
            std::f64::consts::FRAC_PI_2,
        )?;
        range(
            "maximum_pitch_rad",
            definition.maximum_pitch_rad,
            0.0,
            std::f64::consts::FRAC_PI_2,
        )?;
        if definition.maximum_pitch_rad - definition.minimum_pitch_rad < MIN_AXIS_SPACING {
            return Err(TurbopropConfigError(
                "governor pitch stops must increase by at least 1e-9".into(),
            ));
        }
        Ok(Self { definition })
    }
    #[must_use]
    pub const fn definition(&self) -> &GovernorDefinition {
        &self.definition
    }
    /// # Errors
    /// Nonpositive/nonfinite shaft rate or a pitch outside mechanical stops.
    pub fn sample(
        &self,
        shaft: RadiansPerSecond,
        pitch: Radians,
    ) -> Result<GovernorSample, TurbopropEvaluationError> {
        positive("relative_shaft", shaft.get())?;
        let d = self.definition;
        if !pitch.is_finite() || !(d.minimum_pitch_rad..=d.maximum_pitch_rad).contains(&pitch.get())
        {
            return Err(TurbopropEvaluationError::InvalidInput("blade_pitch"));
        }
        let rate = (d.gain * (shaft.get() - d.reference_rad_s))
            .clamp(-d.fine_rate_rad_s, d.coarse_rate_rad_s);
        Ok(GovernorSample {
            start: pitch,
            rate: RadiansPerSecond(rate),
            minimum: Radians(d.minimum_pitch_rad),
            maximum: Radians(d.maximum_pitch_rad),
        })
    }
}
impl GovernorSample {
    #[must_use]
    pub const fn rate(self) -> RadiansPerSecond {
        self.rate
    }
    /// Exact saturated pitch ramp at a nonnegative substep abscissa.
    /// # Errors
    /// Negative/nonfinite duration or overflow of the unconstrained ramp.
    pub fn pitch_after(self, elapsed: Seconds) -> Result<Radians, TurbopropEvaluationError> {
        if !elapsed.is_finite() || elapsed.get() < 0.0 {
            return Err(TurbopropEvaluationError::InvalidInput("elapsed"));
        }
        if elapsed.get() == 0.0 {
            return Ok(self.start);
        }
        let raw = self.start.get() + self.rate.get() * elapsed.get();
        if !raw.is_finite() {
            return Err(TurbopropEvaluationError::InvalidInput("blade_pitch"));
        }
        Ok(Radians(raw.clamp(self.minimum.get(), self.maximum.get())))
    }
}
