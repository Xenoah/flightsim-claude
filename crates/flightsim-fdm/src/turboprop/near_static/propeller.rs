use super::{
    AxisStatus, PropellerCellDefinition, PropellerCoefficients, PropellerDomainStatus,
    PropellerQuery, PropellerSample, RotationSense, TurbopropConfigError, TurbopropEvaluationError,
    bracket, finite, lerp, positive, range, status,
};
use flightsim_core::{
    KilogramSquareMeters, KilogramsPerCubicMeter, Meters, MetersPerSecond, NewtonMeters, Newtons,
    RadiansPerSecond, Watts,
};
use std::f64::consts::{PI, TAU};

/// Authored law limits, fixed before qualification. Never widened at runtime.
pub const NEGATIVE_ADVANCE_RATIO_KNOTS: [f64; 2] = [-0.01, -0.005];
pub const MAX_ADVERSE_INFLOW_RATIO: f64 = 0.10;
pub const MAX_TRANSVERSE_INFLOW_RATIO: f64 = 0.10;

/// New law's conservative static-power condition, not an adverse-flow theorem.
/// This namespace has no old replay wire representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticPowerBound {
    NonpositiveThrust,
    NonpositivePower,
    BelowStaticFloor,
    InvalidDerivedBound,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationError {
    Forward(TurbopropEvaluationError),
    StaticPowerBound(StaticPowerBound),
}
impl From<TurbopropEvaluationError> for EvaluationError {
    fn from(error: TurbopropEvaluationError) -> Self {
        Self::Forward(error)
    }
}
impl std::fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported near-static propeller evaluation: {self:?}")
    }
}
impl std::error::Error for EvaluationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NearStaticDomainStatus {
    pub adverse_inflow: AxisStatus,
    pub transverse_inflow: AxisStatus,
}
impl NearStaticDomainStatus {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self.adverse_inflow, AxisStatus::Within)
            && matches!(self.transverse_inflow, AxisStatus::Within)
    }
}

#[derive(Debug, Clone)]
pub struct PropellerMap {
    forward: super::super::PropellerMap,
    negative_rows: [Vec<PropellerCellDefinition>; 2],
}
impl PropellerMap {
    /// Add explicit paired rows at the two fixed negative J knots. The last
    /// cell shares the exact existing J=0 row; forward queries delegate to law1.
    /// The combined table stays within 32 J knots and 1024 pairs.
    /// # Errors
    /// Wrong row size, nonpositive CT/CP, invalid coefficients, static-power
    /// floor, nonmonotone pitch feedback or combined table resource bounds.
    pub fn from_forward(
        forward: super::super::PropellerMap,
        negative_rows: [Vec<PropellerCellDefinition>; 2],
    ) -> Result<Self, TurbopropConfigError> {
        let d = forward.definition();
        let pitches = d.blade_pitch_rad.len();
        if d.advance_ratio.len() + 2 > super::super::MAX_PROPELLER_AXIS_KNOTS
            || negative_rows.iter().any(|row| row.len() != pitches)
        {
            return Err(TurbopropConfigError(
                "invalid bounded negative row shape".into(),
            ));
        }
        // Positive CT/CP imply CP-J*CT>0 everywhere on J<0. Convexity of
        // CT^(3/2) makes the nodal floor sufficient in exact arithmetic for
        // these two bilinear cells; queries still check actual roundoff.
        for row in [
            negative_rows[0].as_slice(),
            negative_rows[1].as_slice(),
            &d.cells[..pitches],
        ] {
            for (p, c) in row.iter().enumerate() {
                range("near-static ct", c.ct, -2.0, 2.0)?;
                range("near-static cp", c.cp, -2.0, 2.0)?;
                static_power_bound(PropellerCoefficients { ct: c.ct, cp: c.cp }).map_err(
                    |reason| TurbopropConfigError(format!("near-static node violates {reason:?}")),
                )?;
                if p > 0 && c.cp < row[p - 1].cp {
                    return Err(TurbopropConfigError(
                        "CP must not decrease with pitch at J knots".into(),
                    ));
                }
            }
        }
        Ok(Self {
            forward,
            negative_rows,
        })
    }
    #[must_use]
    pub const fn forward_map(&self) -> &super::super::PropellerMap {
        &self.forward
    }
    #[must_use]
    pub const fn negative_rows(&self) -> &[Vec<PropellerCellDefinition>; 2] {
        &self.negative_rows
    }
    #[must_use]
    pub fn diameter(&self) -> Meters {
        self.forward.diameter()
    }
    #[must_use]
    pub fn rotor_inertia(&self) -> KilogramSquareMeters {
        self.forward.rotor_inertia()
    }
    #[must_use]
    pub fn rotation_sense(&self) -> RotationSense {
        self.forward.rotation_sense()
    }

    /// Signed, pitch-then-J interpolation. Both zero signs use the unchanged
    /// forward evaluator. No clamping, extrapolation or runtime row generation.
    /// # Errors
    /// Nonfinite input, unsupported map position or power constraint failure.
    pub fn coefficients(
        &self,
        query: PropellerQuery,
    ) -> Result<PropellerCoefficients, EvaluationError> {
        finite("advance_ratio", query.advance_ratio.0)?;
        finite("blade_pitch", query.blade_pitch.get())?;
        if query.advance_ratio.0 >= 0.0 {
            return self.forward.coefficients(query).map_err(Into::into);
        }
        let pitches = &self.forward.definition().blade_pitch_rad;
        let axis = [
            NEGATIVE_ADVANCE_RATIO_KNOTS[0],
            NEGATIVE_ADVANCE_RATIO_KNOTS[1],
            0.0,
        ];
        let domain = PropellerDomainStatus {
            advance_ratio: status(&axis, query.advance_ratio.0),
            blade_pitch: status(pitches, query.blade_pitch.get()),
        };
        if !domain.is_supported() {
            return Err(TurbopropEvaluationError::OutsidePropellerDomain(domain).into());
        }
        let (i, a) = bracket(&axis, query.advance_ratio.0);
        let (j, b) = bracket(pitches, query.blade_pitch.get());
        let row = |r: usize| {
            if r < 2 {
                self.negative_rows[r].as_slice()
            } else {
                &self.forward.definition().cells[..pitches.len()]
            }
        };
        let interpolate = |field: fn(PropellerCellDefinition) -> f64| {
            let at = |r| lerp(field(row(r)[j]), field(row(r)[j + 1]), b);
            lerp(at(i), at(i + 1), a)
        };
        let c = PropellerCoefficients {
            ct: interpolate(|c| c.ct),
            cp: interpolate(|c| c.cp),
        };
        static_power_bound(c).map_err(EvaluationError::StaticPowerBound)?;
        Ok(c)
    }
    /// Dimensional component loads. Runtime additionally checks actual axial
    /// and transverse flow against the current hover scale at every stage.
    /// # Errors
    /// Invalid density/spin, map/power failure or nonfinite/underflowed loads.
    pub fn sample(
        &self,
        query: PropellerQuery,
        density: KilogramsPerCubicMeter,
        absolute_spin: RadiansPerSecond,
    ) -> Result<PropellerSample, EvaluationError> {
        if query.advance_ratio.0 >= 0.0 {
            return self
                .forward
                .sample(query, density, absolute_spin)
                .map_err(Into::into);
        }
        positive("density", density.get())?;
        positive("absolute_spin", absolute_spin.get())?;
        let coefficients = self.coefficients(query)?;
        let n = absolute_spin.get() / TAU;
        let d = self.diameter().get();
        let thrust = coefficients.ct * density.get() * n * n * d.powi(4);
        let power = coefficients.cp * density.get() * n.powi(3) * d.powi(5);
        let torque = power / absolute_spin.get();
        for v in [thrust, power, torque] {
            positive("near-static propeller load", v)?;
        }
        Ok(PropellerSample {
            coefficients,
            thrust: Newtons(thrust),
            absorbed_power: Watts(power),
            load_torque: NewtonMeters(torque),
        })
    }
}

fn static_power_bound(c: PropellerCoefficients) -> Result<(), StaticPowerBound> {
    if !c.ct.is_finite() || !c.cp.is_finite() {
        return Err(StaticPowerBound::InvalidDerivedBound);
    }
    if c.ct <= 0.0 {
        return Err(StaticPowerBound::NonpositiveThrust);
    }
    if c.cp <= 0.0 {
        return Err(StaticPowerBound::NonpositivePower);
    }
    let floor = (2.0 / PI).sqrt() * c.ct * c.ct.sqrt();
    if !floor.is_finite() || floor <= 0.0 {
        return Err(StaticPowerBound::InvalidDerivedBound);
    }
    let tolerance = 64.0 * f64::EPSILON * c.cp.abs().max(floor.abs());
    if c.cp < floor - tolerance {
        Err(StaticPowerBound::BelowStaticFloor)
    } else {
        Ok(())
    }
}

pub(super) struct FlowScale {
    pub hover_velocity: MetersPerSecond,
    pub adverse_ratio: f64,
    pub transverse_ratio: f64,
}
impl FlowScale {
    pub fn domain(&self) -> NearStaticDomainStatus {
        NearStaticDomainStatus {
            adverse_inflow: status(&[0.0, MAX_ADVERSE_INFLOW_RATIO], self.adverse_ratio),
            transverse_inflow: status(&[0.0, MAX_TRANSVERSE_INFLOW_RATIO], self.transverse_ratio),
        }
    }
}

pub(super) fn flow_scale(
    thrust: Newtons,
    density: KilogramsPerCubicMeter,
    diameter: Meters,
    axial: MetersPerSecond,
    transverse: MetersPerSecond,
) -> Option<FlowScale> {
    let area = PI * diameter.get() * diameter.get() / 4.0;
    let denominator = 2.0 * density.get() * area;
    let square = thrust.get() / denominator;
    let hover = square.sqrt();
    if [
        thrust.get(),
        density.get(),
        diameter.get(),
        area,
        denominator,
        square,
        hover,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || !axial.is_finite()
        || axial.get() >= 0.0
        || !transverse.is_finite()
        || transverse.get() < 0.0
    {
        return None;
    }
    let adverse_ratio = -axial.get() / hover;
    let transverse_ratio = transverse.get() / hover;
    if !adverse_ratio.is_finite()
        || !transverse_ratio.is_finite()
        || adverse_ratio <= 0.0
        || (transverse.get() > 0.0 && transverse_ratio <= 0.0)
    {
        return None;
    }
    Some(FlowScale {
        hover_velocity: MetersPerSecond(hover),
        adverse_ratio,
        transverse_ratio,
    })
}
