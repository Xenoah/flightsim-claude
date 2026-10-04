//! Validated propulsion-neutral structure and explicit authored jet envelope.

use super::{DryJetTable, JetConditions, JetDomainStatus, MachAeroSchedule, axis_status};
use crate::{
    BodyPoint, DampingCoefficient, Geometry, LandingGearConfig, LandingGearLeg, MassProperties,
    SpringRate, definition::GearDefinition,
};
use flightsim_core::{Kilograms, Meters, MetersPerSecond, SquareMeters};
use serde::{Deserialize, Serialize};

/// Raw SI input boundary, independent of a propulsion type. Owners must bound
/// serialized bytes before decoding. No legacy propeller definition is involved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AirframeDefinition {
    pub name: String,
    pub mass_kg: f64,
    /// Body-frame [Ixx, Iyy, Izz, Ixz], kg m².
    pub inertia_kg_m2: [f64; 4],
    pub wing_area_m2: f64,
    pub wing_span_m: f64,
    pub mean_chord_m: f64,
    pub landing_gear: [GearDefinition; 3],
    pub rolling_friction: f64,
    pub braking_friction: f64,
    pub lateral_friction: f64,
    pub friction_transition_mps: f64,
}

/// Immutable, validated mass, geometry and fixed three-point gear. Numerical
/// bounds do not establish stability, structural integrity or aircraft fidelity.
#[derive(Debug, Clone)]
pub struct AirframeConfig {
    name: String,
    mass_properties: MassProperties,
    geometry: Geometry,
    landing_gear: LandingGearConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetConfigError(pub String);

impl std::fmt::Display for JetConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for JetConfigError {}

fn range(name: &str, value: f64, min: f64, max: f64) -> Result<(), JetConfigError> {
    super::range(name, value, min, max).map_err(|error| JetConfigError(error.0))
}

impl AirframeDefinition {
    /// Validate before calling the legacy assertion-based physical constructors.
    /// Structural bounds match the legacy external definition's numerical policy.
    ///
    /// # Errors
    /// Invalid name, nonfinite/out-of-policy fields, or ill-conditioned inertia.
    pub fn to_config(&self) -> Result<AirframeConfig, JetConfigError> {
        if self.name.trim().is_empty()
            || self.name.len() > 96
            || !self.name.is_ascii()
            || self.name.chars().any(char::is_control)
        {
            return Err(JetConfigError(
                "name must be 1..=96 printable ASCII bytes".into(),
            ));
        }
        range("mass_kg", self.mass_kg, 100.0, 100_000.0)?;
        let [xx, yy, zz, xz] = self.inertia_kg_m2;
        for (name, value) in [("Ixx", xx), ("Iyy", yy), ("Izz", zz)] {
            range(name, value, 1.0, 1e10)?;
        }
        range("Ixz", xz, -1e10, 1e10)?;
        if xx * zz - xz * xz <= 1e-9 * xx * zz {
            return Err(JetConfigError(
                "inertia must be positive definite with a finite inverse".into(),
            ));
        }
        range("wing_area_m2", self.wing_area_m2, 1.0, 1000.0)?;
        range("wing_span_m", self.wing_span_m, 1.0, 100.0)?;
        range("mean_chord_m", self.mean_chord_m, 0.1, 20.0)?;
        range("rolling_friction", self.rolling_friction, 0.0, 2.0)?;
        range("braking_friction", self.braking_friction, 0.0, 2.0)?;
        range("lateral_friction", self.lateral_friction, 0.0, 2.0)?;
        range(
            "friction_transition_mps",
            self.friction_transition_mps,
            0.01,
            10.0,
        )?;
        for gear in &self.landing_gear {
            for value in gear.contact_m {
                range("gear contact_m", value, -100.0, 100.0)?;
            }
            range("gear spring_n_per_m", gear.spring_n_per_m, 1.0, 1e8)?;
            range("gear damping_ns_per_m", gear.damping_ns_per_m, 0.0, 1e8)?;
            range("gear max_stroke_m", gear.max_stroke_m, 0.001, 10.0)?;
            range(
                "gear bottom_stop_travel_m",
                gear.bottom_stop_travel_m,
                0.001,
                10.0,
            )?;
            range("gear max_recoil_mps", gear.max_recoil_mps, 0.01, 20.0)?;
        }
        let legs = self.landing_gear.map(|gear| {
            LandingGearLeg::new(
                BodyPoint::new(
                    Meters(gear.contact_m[0]),
                    Meters(gear.contact_m[1]),
                    Meters(gear.contact_m[2]),
                ),
                SpringRate(gear.spring_n_per_m),
                DampingCoefficient(gear.damping_ns_per_m),
                Meters(gear.max_stroke_m),
                Meters(gear.bottom_stop_travel_m),
                MetersPerSecond(gear.max_recoil_mps),
            )
        });
        Ok(AirframeConfig {
            name: self.name.clone(),
            mass_properties: MassProperties::new(Kilograms(self.mass_kg), xx, yy, zz, xz),
            geometry: Geometry {
                wing_area: SquareMeters(self.wing_area_m2),
                wing_span: Meters(self.wing_span_m),
                mean_chord: Meters(self.mean_chord_m),
            },
            landing_gear: LandingGearConfig::new(
                legs,
                self.rolling_friction,
                self.braking_friction,
                self.lateral_friction,
                MetersPerSecond(self.friction_transition_mps),
            ),
        })
    }

    #[must_use]
    pub fn from_config(config: &AirframeConfig) -> Self {
        let inertia = config.mass_properties.inertia();
        Self {
            name: config.name.clone(),
            mass_kg: config.mass_properties.mass().get(),
            inertia_kg_m2: [
                inertia.x_axis.x,
                inertia.y_axis.y,
                inertia.z_axis.z,
                -inertia.x_axis.z,
            ],
            wing_area_m2: config.geometry.wing_area.get(),
            wing_span_m: config.geometry.wing_span.get(),
            mean_chord_m: config.geometry.mean_chord.get(),
            landing_gear: config.landing_gear.legs().map(|gear| GearDefinition {
                contact_m: gear.contact_point().as_vec().to_array(),
                spring_n_per_m: gear.spring_rate().get(),
                damping_ns_per_m: gear.damping_coefficient().get(),
                max_stroke_m: gear.max_stroke().get(),
                bottom_stop_travel_m: gear.bottom_stop_travel().get(),
                max_recoil_mps: gear.max_recoil_speed().get(),
            }),
            rolling_friction: config.landing_gear.rolling_friction_coefficient(),
            braking_friction: config.landing_gear.braking_friction_coefficient(),
            lateral_friction: config.landing_gear.lateral_friction_coefficient(),
            friction_transition_mps: config.landing_gear.friction_transition_speed().get(),
        }
    }
}

impl AirframeConfig {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub const fn mass_properties(&self) -> &MassProperties {
        &self.mass_properties
    }
    #[must_use]
    pub const fn geometry(&self) -> &Geometry {
        &self.geometry
    }
    #[must_use]
    pub const fn landing_gear(&self) -> &LandingGearConfig {
        &self.landing_gear
    }
}

/// Inclusive [minimum, maximum] limits on ambient static ratios and flight Mach.
/// This authored box may be narrower than the component domains.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingEnvelopeDefinition {
    pub pressure_ratio: [f64; 2],
    pub temperature_ratio: [f64; 2],
    pub mach: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OperatingEnvelope {
    definition: OperatingEnvelopeDefinition,
}

impl OperatingEnvelope {
    /// # Errors
    /// Nonfinite, reversed, zero-width, or out-of-policy intervals.
    pub fn from_definition(
        definition: OperatingEnvelopeDefinition,
    ) -> Result<Self, JetConfigError> {
        for (name, axis, min, max) in [
            ("pressure_ratio", definition.pressure_ratio, 0.0, 2.0),
            ("temperature_ratio", definition.temperature_ratio, 0.25, 2.0),
            ("mach", definition.mach, 0.0, super::MAX_TABLE_MACH),
        ] {
            for value in axis {
                range(name, value, min, max)?;
            }
            if axis[1] - axis[0] < super::MIN_AXIS_SPACING {
                return Err(JetConfigError(format!(
                    "{name} envelope needs increasing endpoints separated by at least 1e-9"
                )));
            }
        }
        Ok(Self { definition })
    }
    #[must_use]
    pub const fn definition(&self) -> &OperatingEnvelopeDefinition {
        &self.definition
    }

    pub(super) fn status(self, conditions: JetConditions) -> JetDomainStatus {
        JetDomainStatus {
            pressure: axis_status(&self.definition.pressure_ratio, conditions.pressure_ratio.0),
            temperature: axis_status(
                &self.definition.temperature_ratio,
                conditions.temperature_ratio.0,
            ),
            mach: axis_status(&self.definition.mach, conditions.mach.0),
        }
    }
}

/// Complete immutable jet configuration, with no propeller or hidden engine state.
#[derive(Debug, Clone)]
pub struct JetAircraftConfig {
    airframe: AirframeConfig,
    thrust: DryJetTable,
    aero: MachAeroSchedule,
    envelope: OperatingEnvelope,
}

impl JetAircraftConfig {
    /// # Errors
    /// The authored envelope must be contained in both component domains.
    pub fn new(
        airframe: AirframeConfig,
        thrust: DryJetTable,
        aero: MachAeroSchedule,
        envelope: OperatingEnvelope,
    ) -> Result<Self, JetConfigError> {
        let jet = thrust.definition();
        let bounds = envelope.definition();
        for (name, interval, axis) in [
            (
                "pressure",
                bounds.pressure_ratio,
                jet.pressure_ratios.as_slice(),
            ),
            (
                "temperature",
                bounds.temperature_ratio,
                jet.temperature_ratios.as_slice(),
            ),
            ("Mach", bounds.mach, jet.mach.as_slice()),
        ] {
            if interval[0] < axis[0] || interval[1] > axis[axis.len() - 1] {
                return Err(JetConfigError(format!(
                    "{name} envelope exceeds dry-jet table domain"
                )));
            }
        }
        let knots = &aero.definition().knots;
        if bounds.mach[0] < knots[0].mach || bounds.mach[1] > knots[knots.len() - 1].mach {
            return Err(JetConfigError(
                "Mach envelope exceeds aerodynamic schedule domain".into(),
            ));
        }
        Ok(Self {
            airframe,
            thrust,
            aero,
            envelope,
        })
    }
    #[must_use]
    pub const fn airframe(&self) -> &AirframeConfig {
        &self.airframe
    }
    #[must_use]
    pub const fn thrust(&self) -> &DryJetTable {
        &self.thrust
    }
    #[must_use]
    pub const fn aero(&self) -> &MachAeroSchedule {
        &self.aero
    }
    #[must_use]
    pub const fn envelope(&self) -> &OperatingEnvelope {
        &self.envelope
    }
}
