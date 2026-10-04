use super::{
    AdvanceRatio, MAX_PROPELLER_AXIS_KNOTS, PropellerCoefficients, PropellerDomainStatus,
    PropellerPowerBound, TurbopropConfigError, TurbopropEvaluationError, axis, bracket, finite,
    lerp, positive, range, status,
};
use flightsim_core::{
    KilogramSquareMeters, KilogramsPerCubicMeter, Meters, NewtonMeters, Newtons, Radians,
    RadiansPerSecond, Watts,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropellerConvention {
    IsolatedAxialPropeller,
}
/// Right-hand sense around body +X. The stored relative speed stays positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationSense {
    Positive,
    Negative,
}
impl RotationSense {
    #[must_use]
    pub const fn sign(self) -> f64 {
        match self {
            Self::Positive => 1.0,
            Self::Negative => -1.0,
        }
    }
}
/// Raw SI configuration boundary; not a directly evaluable runtime object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropellerDefinition {
    pub schema: u16,
    pub convention: PropellerConvention,
    pub diameter_m: f64,
    pub rotor_axial_inertia_kg_m2: f64,
    pub rotation_sense: i8,
    pub advance_ratio: Vec<f64>,
    /// Blade pitch at 0.75 radius, radians.
    pub blade_pitch_rad: Vec<f64>,
    /// `[J][pitch]`, pitch fastest, one paired CT/CP value at every node.
    pub cells: Vec<PropellerCellDefinition>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropellerCellDefinition {
    pub ct: f64,
    pub cp: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropellerQuery {
    pub advance_ratio: AdvanceRatio,
    pub blade_pitch: Radians,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropellerSample {
    pub coefficients: PropellerCoefficients,
    pub thrust: Newtons,
    pub absorbed_power: Watts,
    pub load_torque: NewtonMeters,
}
#[derive(Debug, Clone)]
pub struct PropellerMap {
    definition: PropellerDefinition,
}
impl PropellerMap {
    /// Nodes satisfy the positive-thrust ideal-disk bound. Every bilinear cell
    /// continuously satisfies the weaker CP-J*CT inequality. The stronger
    /// nonlinear bound is checked again at EVERY query, not inferred from nodes.
    /// # Errors
    /// Unknown schema/sense, invalid axes/paired data, energy or feedback sign.
    pub fn from_definition(definition: PropellerDefinition) -> Result<Self, TurbopropConfigError> {
        if definition.schema != 1 {
            return Err(TurbopropConfigError("unsupported propeller schema".into()));
        }
        if ![-1, 1].contains(&definition.rotation_sense) {
            return Err(TurbopropConfigError(
                "rotation_sense must be -1 or +1".into(),
            ));
        }
        range("diameter_m", definition.diameter_m, 0.5, 6.0)?;
        range(
            "rotor_axial_inertia_kg_m2",
            definition.rotor_axial_inertia_kg_m2,
            0.1,
            1000.0,
        )?;
        axis(
            "advance_ratio",
            &definition.advance_ratio,
            0.0,
            4.0,
            MAX_PROPELLER_AXIS_KNOTS,
        )?;
        axis(
            "blade_pitch_rad",
            &definition.blade_pitch_rad,
            0.0,
            FRAC_PI_2,
            MAX_PROPELLER_AXIS_KNOTS,
        )?;
        if definition.advance_ratio[0].abs() > 0.0 {
            return Err(TurbopropConfigError(
                "advance_ratio must start at zero".into(),
            ));
        }
        let pitches = definition.blade_pitch_rad.len();
        if definition.cells.len() != definition.advance_ratio.len() * pitches {
            return Err(TurbopropConfigError(
                "propeller cell count does not match bounded axes".into(),
            ));
        }
        for (i, c) in definition.cells.iter().enumerate() {
            range("ct", c.ct, -2.0, 2.0)?;
            range("cp", c.cp, -2.0, 2.0)?;
            positive_thrust_bound(
                definition.advance_ratio[i / pitches],
                PropellerCoefficients { ct: c.ct, cp: c.cp },
            )
            .map_err(|reason| {
                TurbopropConfigError(format!("propeller node {i} violates {reason:?}"))
            })?;
            if i % pitches > 0 && c.cp < definition.cells[i - 1].cp {
                return Err(TurbopropConfigError(
                    "CP must not decrease with pitch at J knots".into(),
                ));
            }
        }
        // At each pitch endpoint g=CP-J*CT is quadratic in the cell's J
        // fraction. Its endpoints and any interior minimum bound the whole
        // cell because g is linear along pitch. Node-only checking is invalid.
        for i in 0..definition.advance_ratio.len() - 1 {
            let j0 = definition.advance_ratio[i];
            let dj = definition.advance_ratio[i + 1] - j0;
            for p in 0..pitches {
                let a = definition.cells[i * pitches + p];
                let b = definition.cells[(i + 1) * pitches + p];
                let qa = -dj * (b.ct - a.ct);
                let qb = b.cp - a.cp - j0 * (b.ct - a.ct) - dj * a.ct;
                let check = |t: f64| {
                    let j = lerp(j0, j0 + dj, t);
                    let ct = lerp(a.ct, b.ct, t);
                    let cp = lerp(a.cp, b.cp, t);
                    let work = j * ct;
                    cp - work >= -64.0 * f64::EPSILON * 1.0_f64.max(cp.abs()).max(work.abs())
                };
                let minimum = if qa > 0.0 { -qb / (2.0 * qa) } else { -1.0 };
                if !check(0.0) || !check(1.0) || ((0.0..1.0).contains(&minimum) && !check(minimum))
                {
                    return Err(TurbopropConfigError(
                        "interior CP-J*CT work inequality fails".into(),
                    ));
                }
            }
        }
        Ok(Self { definition })
    }
    #[must_use]
    pub const fn definition(&self) -> &PropellerDefinition {
        &self.definition
    }
    #[must_use]
    pub fn diameter(&self) -> Meters {
        Meters(self.definition.diameter_m)
    }
    #[must_use]
    pub fn rotor_inertia(&self) -> KilogramSquareMeters {
        KilogramSquareMeters(self.definition.rotor_axial_inertia_kg_m2)
    }
    #[must_use]
    pub fn rotation_sense(&self) -> RotationSense {
        if self.definition.rotation_sense == 1 {
            RotationSense::Positive
        } else {
            RotationSense::Negative
        }
    }
    /// Pitch-then-J interpolation from the same pair, followed by pointwise
    /// finite-disk admission. Exact knots preserve the authored coefficient bits.
    /// # Errors
    /// Nonfinite query, unsupported J/pitch or a positive-thrust power violation.
    pub fn coefficients(
        &self,
        query: PropellerQuery,
    ) -> Result<PropellerCoefficients, TurbopropEvaluationError> {
        finite("advance_ratio", query.advance_ratio.0)?;
        finite("blade_pitch", query.blade_pitch.get())?;
        let d = &self.definition;
        let domain = PropellerDomainStatus {
            advance_ratio: status(&d.advance_ratio, query.advance_ratio.0),
            blade_pitch: status(&d.blade_pitch_rad, query.blade_pitch.get()),
        };
        if !domain.is_supported() {
            return Err(TurbopropEvaluationError::OutsidePropellerDomain(domain));
        }
        let (i, a) = bracket(&d.advance_ratio, query.advance_ratio.0);
        let (j, b) = bracket(&d.blade_pitch_rad, query.blade_pitch.get());
        let interpolate = |field: fn(PropellerCellDefinition) -> f64| {
            let row = |p| {
                lerp(
                    field(d.cells[p * d.blade_pitch_rad.len() + j]),
                    field(d.cells[p * d.blade_pitch_rad.len() + j + 1]),
                    b,
                )
            };
            lerp(row(i), row(i + 1), a)
        };
        let coefficients = PropellerCoefficients {
            ct: interpolate(|c| c.ct),
            cp: interpolate(|c| c.cp),
        };
        positive_thrust_bound(query.advance_ratio.0, coefficients)
            .map_err(TurbopropEvaluationError::PropellerPowerBound)?;
        Ok(coefficients)
    }
    /// Dimensional loads use absolute axial spin omega+s*p, never RPM or the
    /// governor's relative shaft speed. No division by freestream speed occurs.
    /// # Errors
    /// Invalid density/spin, map/domain/power-bound failure or nonfinite loads.
    pub fn sample(
        &self,
        query: PropellerQuery,
        density: KilogramsPerCubicMeter,
        absolute_spin: RadiansPerSecond,
    ) -> Result<PropellerSample, TurbopropEvaluationError> {
        positive("density", density.get())?;
        positive("absolute_spin", absolute_spin.get())?;
        let coefficients = self.coefficients(query)?;
        let n = absolute_spin.get() / TAU;
        let d = self.definition.diameter_m;
        let thrust = coefficients.ct * density.get() * n * n * d.powi(4);
        let power = coefficients.cp * density.get() * n.powi(3) * d.powi(5);
        let torque = power / absolute_spin.get();
        for v in [thrust, power, torque] {
            finite("propeller_load", v)?;
        }
        Ok(PropellerSample {
            coefficients,
            thrust: Newtons(thrust),
            absorbed_power: Watts(power),
            load_torque: NewtonMeters(torque),
        })
    }
}

fn positive_thrust_bound(j: f64, c: PropellerCoefficients) -> Result<(), PropellerPowerBound> {
    if c.ct <= 0.0 {
        return Ok(());
    }
    if c.cp <= 0.0 {
        return Err(PropellerPowerBound::NonpositivePower);
    }
    let ideal = 0.5 * c.ct * (j + (j * j + 8.0 * c.ct / PI).sqrt());
    if !ideal.is_finite() || ideal <= 0.0 {
        return Err(PropellerPowerBound::InvalidDerivedBound);
    }
    let tolerance = 64.0 * f64::EPSILON * c.cp.abs().max(ideal.abs());
    if c.cp < ideal - tolerance {
        Err(PropellerPowerBound::BelowIdealDisk)
    } else {
        Ok(())
    }
}
