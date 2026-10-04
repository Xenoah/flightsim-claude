use super::{
    MachNumber, PressureRatio, PropellerMap, SampledGovernor, TemperatureRatio, TurbinePowerTable,
    TurbopropConfigError, TurbopropDomainStatus, axis, status,
};
use crate::{
    MassProperties,
    subsonic::{AirframeConfig, MachAeroSchedule},
};
use flightsim_core::RadiansPerSecond;
use serde::{Deserialize, Serialize};

/// Raw SI input boundary. The admissible set is the intersection of this box,
/// component maps, helical tip limit and axial-flow limit, not the entire box.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurbopropEnvelopeDefinition {
    pub pressure_ratio: [f64; 2],
    pub temperature_ratio: [f64; 2],
    pub mach: [f64; 2],
    pub relative_shaft_rad_s: [f64; 2],
    pub absolute_spin_rad_s: [f64; 2],
    pub maximum_helical_tip_mach: f64,
    pub maximum_crossflow_tip_ratio: f64,
}
#[derive(Debug, Clone, Copy)]
pub struct TurbopropEnvelope {
    definition: TurbopropEnvelopeDefinition,
}
impl TurbopropEnvelope {
    /// # Errors
    /// Invalid intervals or unsupported numerical limits.
    pub fn from_definition(
        definition: TurbopropEnvelopeDefinition,
    ) -> Result<Self, TurbopropConfigError> {
        for (name, values, min, max) in [
            ("pressure_ratio", definition.pressure_ratio, 0.1, 2.0),
            ("temperature_ratio", definition.temperature_ratio, 0.25, 2.0),
            (
                "mach",
                definition.mach,
                0.0,
                crate::subsonic::MAX_TABLE_MACH,
            ),
            (
                "relative_shaft_rad_s",
                definition.relative_shaft_rad_s,
                20.0,
                1000.0,
            ),
            (
                "absolute_spin_rad_s",
                definition.absolute_spin_rad_s,
                20.0,
                1000.0,
            ),
        ] {
            axis(name, &values, min, max, 2)?;
        }
        for (name, value, maximum) in [
            (
                "maximum_helical_tip_mach",
                definition.maximum_helical_tip_mach,
                0.8,
            ),
            (
                "maximum_crossflow_tip_ratio",
                definition.maximum_crossflow_tip_ratio,
                0.1,
            ),
        ] {
            if !value.is_finite() || value <= 0.0 || value > maximum {
                return Err(TurbopropConfigError(format!(
                    "{name} must be finite and in (0,{maximum}]"
                )));
            }
        }
        Ok(Self { definition })
    }
    #[must_use]
    pub const fn definition(&self) -> &TurbopropEnvelopeDefinition {
        &self.definition
    }
    pub(super) fn base_status(
        self,
        pressure: PressureRatio,
        temperature: TemperatureRatio,
        mach: MachNumber,
        relative: RadiansPerSecond,
        absolute: RadiansPerSecond,
    ) -> TurbopropDomainStatus {
        let d = self.definition;
        TurbopropDomainStatus {
            pressure: status(&d.pressure_ratio, pressure.0),
            temperature: status(&d.temperature_ratio, temperature.0),
            mach: status(&d.mach, mach.0),
            relative_shaft: status(&d.relative_shaft_rad_s, relative.get()),
            absolute_spin: status(&d.absolute_spin_rad_s, absolute.get()),
            tip_mach: super::AxisStatus::Within,
            crossflow: super::AxisStatus::Within,
        }
    }
}
/// Immutable complete physical model, with locked-rotor airframe inertia and a
/// separately cached rotor-free axial body inertia. No initial state/default
/// or metadata-dependent family selection is stored here.
#[derive(Debug, Clone)]
pub struct TurbopropAircraftConfig {
    airframe: AirframeConfig,
    turbine: TurbinePowerTable,
    propeller: PropellerMap,
    governor: SampledGovernor,
    aero: MachAeroSchedule,
    envelope: TurbopropEnvelope,
    effective_body: MassProperties,
}
impl TurbopropAircraftConfig {
    /// # Errors
    /// Envelope/map disagreement, invalid governor reference/stops or a rotor
    /// subtraction producing a non-positive/ill-conditioned body tensor.
    pub fn new(
        airframe: AirframeConfig,
        turbine: TurbinePowerTable,
        propeller: PropellerMap,
        governor: SampledGovernor,
        aero: MachAeroSchedule,
        envelope: TurbopropEnvelope,
    ) -> Result<Self, TurbopropConfigError> {
        let e = envelope.definition();
        let power = turbine.definition();
        for (name, bounds, values) in [
            (
                "pressure",
                e.pressure_ratio,
                power.pressure_ratios.as_slice(),
            ),
            (
                "temperature",
                e.temperature_ratio,
                power.temperature_ratios.as_slice(),
            ),
        ] {
            if bounds[0] < values[0] || bounds[1] > values[values.len() - 1] {
                return Err(TurbopropConfigError(format!(
                    "{name} envelope exceeds turbine table"
                )));
            }
        }
        let knots = &aero.definition().knots;
        if e.mach[0] < knots[0].mach || e.mach[1] > knots[knots.len() - 1].mach {
            return Err(TurbopropConfigError(
                "Mach envelope exceeds aero schedule".into(),
            ));
        }
        let g = governor.definition();
        let pitches = &propeller.definition().blade_pitch_rad;
        if g.minimum_pitch_rad < pitches[0] || g.maximum_pitch_rad > pitches[pitches.len() - 1] {
            return Err(TurbopropConfigError(
                "governor stops exceed propeller pitch axis".into(),
            ));
        }
        if !(e.relative_shaft_rad_s[0]..=e.relative_shaft_rad_s[1]).contains(&g.reference_rad_s) {
            return Err(TurbopropConfigError(
                "governor reference outside relative shaft interval".into(),
            ));
        }
        let locked = airframe.mass_properties().inertia();
        let xx = locked.x_axis.x - propeller.rotor_inertia().get();
        let yy = locked.y_axis.y;
        let zz = locked.z_axis.z;
        let xz = -locked.x_axis.z;
        let block = xx * zz - xz * xz;
        // Mirror MassProperties::new's exact matrix and operation order.
        // yy*(xx*zz-xz*xz) is algebraically equal but can round to the other
        // side of its absolute determinant assertion at the admission boundary.
        let reduced = glam::DMat3::from_cols_array(&[xx, 0.0, -xz, 0.0, yy, 0.0, -xz, 0.0, zz]);
        let determinant = reduced.determinant();
        if xx <= 0.0
            || block <= 1e-9 * xx * zz
            || !determinant.is_finite()
            || determinant <= f64::EPSILON
        {
            return Err(TurbopropConfigError(
                "rotor-subtracted inertia must be positive definite and conditioned".into(),
            ));
        }
        if !reduced.inverse().is_finite() {
            return Err(TurbopropConfigError(
                "rotor-subtracted inverse inertia is nonfinite".into(),
            ));
        }
        // The existing constructor assertion is now guaranteed. This tensor is
        // ALSO used for contact frequency: locked inertia would underestimate
        // contact response when the rotor is allowed to turn independently.
        let effective_body = MassProperties::new(airframe.mass_properties().mass(), xx, yy, zz, xz);
        Ok(Self {
            airframe,
            turbine,
            propeller,
            governor,
            aero,
            envelope,
            effective_body,
        })
    }
    #[must_use]
    pub const fn airframe(&self) -> &AirframeConfig {
        &self.airframe
    }
    #[must_use]
    pub const fn turbine(&self) -> &TurbinePowerTable {
        &self.turbine
    }
    #[must_use]
    pub const fn propeller(&self) -> &PropellerMap {
        &self.propeller
    }
    #[must_use]
    pub const fn governor(&self) -> &SampledGovernor {
        &self.governor
    }
    #[must_use]
    pub const fn aero(&self) -> &MachAeroSchedule {
        &self.aero
    }
    #[must_use]
    pub const fn envelope(&self) -> &TurbopropEnvelope {
        &self.envelope
    }
    /// Derived I_B, not a second identity input. Use I_L from airframe for total
    /// locked momentum and identity; I_B for body response/contact resolution.
    #[must_use]
    pub const fn effective_body_mass_properties(&self) -> &MassProperties {
        &self.effective_body
    }
}
