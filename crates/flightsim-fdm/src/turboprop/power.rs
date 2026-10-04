use super::{
    MAX_POWER_AXIS_KNOTS, PowerDomainStatus, PressureRatio, TemperatureRatio, TurbineFraction,
    TurbopropConfigError, TurbopropEvaluationError, axis, bracket, finite, lerp, positive, range,
    status,
};
use flightsim_core::{NewtonMeters, RadiansPerSecond, Seconds, Watts};
use serde::{Deserialize, Serialize};

/// Raw SI configuration input; callers must bound bytes before deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurbineDefinition {
    pub schema: u16,
    pub pressure_ratios: Vec<f64>,
    pub temperature_ratios: Vec<f64>,
    /// `[pressure][temperature]`, temperature fastest. Output shaft power after
    /// gearbox/accessory losses; there is no additional efficiency multiplier.
    pub cells: Vec<PowerCellDefinition>,
    pub rise_seconds: f64,
    pub fall_seconds: f64,
    pub output_torque_limit_nm: f64,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerCellDefinition {
    pub idle_w: f64,
    pub maximum_w: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerConditions {
    pub pressure_ratio: PressureRatio,
    pub temperature_ratio: TemperatureRatio,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerSample {
    pub available: Watts,
    pub delivered: Watts,
    pub drive_torque: NewtonMeters,
}
#[derive(Debug, Clone)]
pub struct TurbinePowerTable {
    definition: TurbineDefinition,
}
impl TurbinePowerTable {
    /// # Errors
    /// Unknown schema, unbounded dimensions, invalid axes/powers/response/torque.
    pub fn from_definition(definition: TurbineDefinition) -> Result<Self, TurbopropConfigError> {
        if definition.schema != 1 {
            return Err(TurbopropConfigError("unsupported turbine schema".into()));
        }
        axis(
            "pressure_ratios",
            &definition.pressure_ratios,
            0.1,
            2.0,
            MAX_POWER_AXIS_KNOTS,
        )?;
        axis(
            "temperature_ratios",
            &definition.temperature_ratios,
            0.25,
            2.0,
            MAX_POWER_AXIS_KNOTS,
        )?;
        if definition.cells.len()
            != definition.pressure_ratios.len() * definition.temperature_ratios.len()
        {
            return Err(TurbopropConfigError(
                "turbine cell count does not match bounded axes".into(),
            ));
        }
        for c in &definition.cells {
            range("idle_w", c.idle_w, 0.0, 5e6)?;
            range("maximum_w", c.maximum_w, 0.0, 5e6)?;
            if c.maximum_w < c.idle_w {
                return Err(TurbopropConfigError("maximum power is below idle".into()));
            }
        }
        range("rise_seconds", definition.rise_seconds, 0.1, 30.0)?;
        range("fall_seconds", definition.fall_seconds, 0.1, 30.0)?;
        range(
            "output_torque_limit_nm",
            definition.output_torque_limit_nm,
            1.0,
            100_000.0,
        )?;
        Ok(Self { definition })
    }
    #[must_use]
    pub const fn definition(&self) -> &TurbineDefinition {
        &self.definition
    }
    /// Bilinear temperature-then-pressure interpolation; no extrapolation.
    /// # Errors
    /// Nonfinite/unsupported conditions or nonpositive/nonfinite relative speed.
    pub fn sample(
        &self,
        conditions: PowerConditions,
        fraction: TurbineFraction,
        shaft: RadiansPerSecond,
    ) -> Result<PowerSample, TurbopropEvaluationError> {
        finite("pressure_ratio", conditions.pressure_ratio.0)?;
        finite("temperature_ratio", conditions.temperature_ratio.0)?;
        positive("relative_shaft", shaft.get())?;
        let d = &self.definition;
        let domain = PowerDomainStatus {
            pressure: status(&d.pressure_ratios, conditions.pressure_ratio.0),
            temperature: status(&d.temperature_ratios, conditions.temperature_ratio.0),
        };
        if !domain.is_supported() {
            return Err(TurbopropEvaluationError::OutsidePowerDomain(domain));
        }
        let (i, a) = bracket(&d.pressure_ratios, conditions.pressure_ratio.0);
        let (j, b) = bracket(&d.temperature_ratios, conditions.temperature_ratio.0);
        let interpolate = |field: fn(PowerCellDefinition) -> f64| {
            let row = |p| {
                lerp(
                    field(d.cells[p * d.temperature_ratios.len() + j]),
                    field(d.cells[p * d.temperature_ratios.len() + j + 1]),
                    b,
                )
            };
            lerp(row(i), row(i + 1), a)
        };
        let available = lerp(
            interpolate(|c| c.idle_w),
            interpolate(|c| c.maximum_w),
            fraction.get(),
        );
        let torque = (available / shaft.get()).min(d.output_torque_limit_nm);
        let delivered = torque * shaft.get();
        finite("delivered_power", delivered)?;
        Ok(PowerSample {
            available: Watts(available),
            delivered: Watts(delivered),
            drive_torque: NewtonMeters(torque),
        })
    }
    /// Exact held-command response. Zero duration preserves all accepted bits.
    /// # Errors
    /// Negative/nonfinite duration or a nonfinite derived state.
    pub fn fraction_after(
        &self,
        start: TurbineFraction,
        command: TurbineFraction,
        elapsed: Seconds,
    ) -> Result<TurbineFraction, TurbopropEvaluationError> {
        if !elapsed.is_finite() || elapsed.get() < 0.0 {
            return Err(TurbopropEvaluationError::InvalidInput("elapsed"));
        }
        if elapsed.get() == 0.0 || start.get().to_bits() == command.get().to_bits() {
            return Ok(start);
        }
        let tau = if command.get() > start.get() {
            self.definition.rise_seconds
        } else {
            self.definition.fall_seconds
        };
        TurbineFraction::new(
            command.get() + (start.get() - command.get()) * (-elapsed.get() / tau).exp(),
        )
    }
}
