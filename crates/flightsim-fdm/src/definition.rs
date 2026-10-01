//! Versioned aircraft dynamics at the JSON input boundary.
//!
//! Raw numbers occur only in this external representation. Field names state
//! their units; validation converts them to the existing unit-typed FDM API.
//! Invalid files return errors before any assertion-based constructor is called.

use crate::{
    AeroCoefficients, AircraftConfig, BodyPoint, DampingCoefficient, EngineConfig, Geometry,
    LandingGearConfig, LandingGearLeg, MassProperties, SpringRate,
};
use flightsim_core::{Kilograms, Meters, MetersPerSecond, Newtons, Radians, SquareMeters};
use serde::{Deserialize, Serialize};

/// Serializable complete dynamics definition. Coefficients use normalized
/// control deflection, just like [`AeroCoefficients`], never degrees or radians.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AircraftDefinition {
    pub name: String,
    pub mass_kg: f64,
    /// `[Ixx, Iyy, Izz, Ixz]` in kg m², body frame.
    pub inertia_kg_m2: [f64; 4],
    pub wing_area_m2: f64,
    pub wing_span_m: f64,
    pub mean_chord_m: f64,
    pub aero: AerodynamicDefinition,
    pub max_shaft_power_w: f64,
    pub propeller_efficiency: f64,
    pub static_thrust_n: f64,
    pub landing_gear: [GearDefinition; 3],
    pub rolling_friction: f64,
    pub braking_friction: f64,
    pub lateral_friction: f64,
    pub friction_transition_mps: f64,
}

/// Individual wheel contact and bounded strut model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearDefinition {
    /// Body +X forward, +Y right, +Z down, metres from center of mass.
    pub contact_m: [f64; 3],
    pub spring_n_per_m: f64,
    pub damping_ns_per_m: f64,
    pub max_stroke_m: f64,
    pub bottom_stop_travel_m: f64,
    pub max_recoil_mps: f64,
}

/// Dimensionless aerodynamic coefficients; angular slopes are per radian.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AerodynamicDefinition {
    pub lift_zero: f64,
    pub lift_alpha: f64,
    pub lift_flaps: f64,
    pub stall_angle_rad: f64,
    pub stall_blend_rate: f64,
    pub drag_min: f64,
    pub oswald_efficiency: f64,
    pub drag_flaps: f64,
    pub side_beta: f64,
    pub side_rudder: f64,
    pub roll_beta: f64,
    pub roll_rate_p: f64,
    pub roll_rate_r: f64,
    pub roll_aileron: f64,
    pub roll_rudder: f64,
    pub pitch_zero: f64,
    pub pitch_alpha: f64,
    pub pitch_rate_q: f64,
    pub pitch_elevator: f64,
    pub pitch_flaps: f64,
    pub yaw_beta: f64,
    pub yaw_rate_p: f64,
    pub yaw_rate_r: f64,
    pub yaw_aileron: f64,
    pub yaw_rudder: f64,
}

/// A named field violates the supported model's physical domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionError(pub String);
impl std::fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DefinitionError {}

fn range(name: &str, value: f64, min: f64, max: f64) -> Result<(), DefinitionError> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(DefinitionError(format!(
            "{name} must be finite and in {min}..={max}"
        )));
    }
    Ok(())
}

impl AircraftDefinition {
    /// Validate all external values, then construct the unit-typed configuration.
    ///
    /// # Errors
    /// Invalid, nonfinite or unsupported values, including indefinite inertia.
    pub fn to_config(&self) -> Result<AircraftConfig, DefinitionError> {
        if self.name.trim().is_empty()
            || self.name.len() > 96
            || !self.name.is_ascii()
            || self.name.chars().any(char::is_control)
        {
            return Err(DefinitionError(
                "name must be 1..=96 printable ASCII bytes".into(),
            ));
        }
        range("mass_kg", self.mass_kg, 100.0, 100_000.0)?;
        let [xx, yy, zz, xz] = self.inertia_kg_m2;
        for (name, v) in [("Ixx", xx), ("Iyy", yy), ("Izz", zz)] {
            range(name, v, 1.0, 1e10)?;
        }
        range("Ixz", xz, -1e10, 1e10)?;
        // Positive determinant alone is insufficient. For this symmetric tensor,
        // xx>0, yy>0, xx*zz-xz²>0 are Sylvester's positive-definite conditions.
        if xx * zz - xz * xz <= 1e-9 * xx * zz {
            return Err(DefinitionError(
                "inertia must be positive definite with a finite inverse".into(),
            ));
        }
        range("wing_area_m2", self.wing_area_m2, 1.0, 1000.0)?;
        range("wing_span_m", self.wing_span_m, 1.0, 100.0)?;
        range("mean_chord_m", self.mean_chord_m, 0.1, 20.0)?;
        range("max_shaft_power_w", self.max_shaft_power_w, 1.0, 1e8)?;
        range("propeller_efficiency", self.propeller_efficiency, 0.01, 1.0)?;
        range("static_thrust_n", self.static_thrust_n, 1.0, 1e6)?;
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
        let aero = self.aero.to_coefficients()?;
        let legs = self.landing_gear.map(|g| {
            LandingGearLeg::new(
                BodyPoint::new(
                    Meters(g.contact_m[0]),
                    Meters(g.contact_m[1]),
                    Meters(g.contact_m[2]),
                ),
                SpringRate(g.spring_n_per_m),
                DampingCoefficient(g.damping_ns_per_m),
                Meters(g.max_stroke_m),
                Meters(g.bottom_stop_travel_m),
                MetersPerSecond(g.max_recoil_mps),
            )
        });
        Ok(AircraftConfig {
            name: self.name.clone(),
            mass_properties: MassProperties::new(Kilograms(self.mass_kg), xx, yy, zz, xz),
            geometry: Geometry {
                wing_area: SquareMeters(self.wing_area_m2),
                wing_span: Meters(self.wing_span_m),
                mean_chord: Meters(self.mean_chord_m),
            },
            aero,
            engine: EngineConfig {
                max_shaft_power: self.max_shaft_power_w,
                propeller_efficiency: self.propeller_efficiency,
                static_thrust: Newtons(self.static_thrust_n),
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

    /// Losslessly serialize an existing in-memory configuration for editing.
    #[must_use]
    pub fn from_config(config: &AircraftConfig) -> Self {
        let i = config.mass_properties.inertia();
        Self {
            name: config.name.clone(),
            mass_kg: config.mass_properties.mass().get(),
            inertia_kg_m2: [i.x_axis.x, i.y_axis.y, i.z_axis.z, -i.x_axis.z],
            wing_area_m2: config.geometry.wing_area.get(),
            wing_span_m: config.geometry.wing_span.get(),
            mean_chord_m: config.geometry.mean_chord.get(),
            aero: AerodynamicDefinition::from_coefficients(config.aero),
            max_shaft_power_w: config.engine.max_shaft_power,
            propeller_efficiency: config.engine.propeller_efficiency,
            static_thrust_n: config.engine.static_thrust.get(),
            landing_gear: config.landing_gear.legs().map(|g| GearDefinition {
                contact_m: g.contact_point().as_vec().to_array(),
                spring_n_per_m: g.spring_rate().get(),
                damping_ns_per_m: g.damping_coefficient().get(),
                max_stroke_m: g.max_stroke().get(),
                bottom_stop_travel_m: g.bottom_stop_travel().get(),
                max_recoil_mps: g.max_recoil_speed().get(),
            }),
            rolling_friction: config.landing_gear.rolling_friction_coefficient(),
            braking_friction: config.landing_gear.braking_friction_coefficient(),
            lateral_friction: config.landing_gear.lateral_friction_coefficient(),
            friction_transition_mps: config.landing_gear.friction_transition_speed().get(),
        }
    }
}

impl AerodynamicDefinition {
    fn to_coefficients(self) -> Result<AeroCoefficients, DefinitionError> {
        range("lift_zero", self.lift_zero, -100.0, 100.0)?;
        range("lift_alpha", self.lift_alpha, -100.0, 100.0)?;
        range("lift_flaps", self.lift_flaps, 0.0, 100.0)?;
        range("stall_angle_rad", self.stall_angle_rad, -100.0, 100.0)?;
        range("stall_blend_rate", self.stall_blend_rate, -100.0, 100.0)?;
        range("drag_min", self.drag_min, -100.0, 100.0)?;
        range("oswald_efficiency", self.oswald_efficiency, -100.0, 100.0)?;
        range("drag_flaps", self.drag_flaps, 0.0, 100.0)?;
        range("side_beta", self.side_beta, -100.0, 100.0)?;
        range("side_rudder", self.side_rudder, -100.0, 100.0)?;
        range("roll_beta", self.roll_beta, -100.0, 100.0)?;
        range("roll_rate_p", self.roll_rate_p, -100.0, 100.0)?;
        range("roll_rate_r", self.roll_rate_r, -100.0, 100.0)?;
        range("roll_aileron", self.roll_aileron, -100.0, 100.0)?;
        range("roll_rudder", self.roll_rudder, -100.0, 100.0)?;
        range("pitch_zero", self.pitch_zero, -100.0, 100.0)?;
        range("pitch_alpha", self.pitch_alpha, -100.0, 100.0)?;
        range("pitch_rate_q", self.pitch_rate_q, -100.0, 100.0)?;
        range("pitch_elevator", self.pitch_elevator, -100.0, 100.0)?;
        range("pitch_flaps", self.pitch_flaps, -100.0, 100.0)?;
        range("yaw_beta", self.yaw_beta, -100.0, 100.0)?;
        range("yaw_rate_p", self.yaw_rate_p, -100.0, 100.0)?;
        range("yaw_rate_r", self.yaw_rate_r, -100.0, 100.0)?;
        range("yaw_aileron", self.yaw_aileron, -100.0, 100.0)?;
        range("yaw_rudder", self.yaw_rudder, -100.0, 100.0)?;
        range("stall_angle_rad", self.stall_angle_rad, 0.05, 0.7)?;
        range("stall_blend_rate", self.stall_blend_rate, 0.1, 100.0)?;
        range("drag_min", self.drag_min, 0.0001, 1.0)?;
        range("oswald_efficiency", self.oswald_efficiency, 0.05, 1.0)?;
        if self.pitch_alpha >= 0.0
            || self.yaw_beta <= 0.0
            || self.lift_alpha <= 0.0
            || self.roll_rate_p >= 0.0
            || self.pitch_rate_q >= 0.0
            || self.yaw_rate_r >= 0.0
            || self.roll_aileron <= 0.0
            || self.pitch_elevator <= 0.0
            || self.yaw_rudder <= 0.0
        {
            return Err(DefinitionError(
                "aircraft stability and control coefficients have unsupported signs".into(),
            ));
        }
        Ok(AeroCoefficients {
            lift_zero: self.lift_zero,
            lift_alpha: self.lift_alpha,
            lift_flaps: self.lift_flaps,
            stall_angle: Radians(self.stall_angle_rad),
            stall_blend_rate: self.stall_blend_rate,
            drag_min: self.drag_min,
            oswald_efficiency: self.oswald_efficiency,
            drag_flaps: self.drag_flaps,
            side_beta: self.side_beta,
            side_rudder: self.side_rudder,
            roll_beta: self.roll_beta,
            roll_rate_p: self.roll_rate_p,
            roll_rate_r: self.roll_rate_r,
            roll_aileron: self.roll_aileron,
            roll_rudder: self.roll_rudder,
            pitch_zero: self.pitch_zero,
            pitch_alpha: self.pitch_alpha,
            pitch_rate_q: self.pitch_rate_q,
            pitch_elevator: self.pitch_elevator,
            pitch_flaps: self.pitch_flaps,
            yaw_beta: self.yaw_beta,
            yaw_rate_p: self.yaw_rate_p,
            yaw_rate_r: self.yaw_rate_r,
            yaw_aileron: self.yaw_aileron,
            yaw_rudder: self.yaw_rudder,
        })
    }
    fn from_coefficients(a: AeroCoefficients) -> Self {
        Self {
            lift_zero: a.lift_zero,
            lift_alpha: a.lift_alpha,
            lift_flaps: a.lift_flaps,
            stall_angle_rad: a.stall_angle.get(),
            stall_blend_rate: a.stall_blend_rate,
            drag_min: a.drag_min,
            oswald_efficiency: a.oswald_efficiency,
            drag_flaps: a.drag_flaps,
            side_beta: a.side_beta,
            side_rudder: a.side_rudder,
            roll_beta: a.roll_beta,
            roll_rate_p: a.roll_rate_p,
            roll_rate_r: a.roll_rate_r,
            roll_aileron: a.roll_aileron,
            roll_rudder: a.roll_rudder,
            pitch_zero: a.pitch_zero,
            pitch_alpha: a.pitch_alpha,
            pitch_rate_q: a.pitch_rate_q,
            pitch_elevator: a.pitch_elevator,
            pitch_flaps: a.pitch_flaps,
            yaw_beta: a.yaw_beta,
            yaw_rate_p: a.yaw_rate_p,
            yaw_rate_r: a.yaw_rate_r,
            yaw_aileron: a.yaw_aileron,
            yaw_rudder: a.yaw_rudder,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlInputs, Environment, FlightDynamics, RigidBodyState};
    use flightsim_core::{Attitude, Geodetic, Ned, Seconds};

    #[test]
    fn json_round_trip_preserves_the_existing_aircraft_and_trajectory() {
        let original = AircraftConfig::light_single();
        let definition = AircraftDefinition::from_config(&original);
        let json = serde_json::to_string(&definition).expect("serialize");
        let decoded: AircraftDefinition = serde_json::from_str(&json).expect("parse");
        let restored = decoded.to_config().expect("valid aircraft");
        assert_eq!(
            serde_json::to_value(&definition).unwrap(),
            serde_json::to_value(AircraftDefinition::from_config(&restored)).unwrap()
        );
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 1000.0),
            Attitude::from_degrees(0.0, 3.0, 90.0),
            Ned::new(0.0, 40.0, 0.0),
        );
        let mut a = FlightDynamics::new(original, state);
        let mut b = FlightDynamics::new(restored, state);
        for _ in 0..1200 {
            let controls = ControlInputs::neutral()
                .with_throttle(0.6)
                .with_elevator(0.09);
            a.step(Seconds(1.0 / 120.0), controls, &Environment::still_air());
            b.step(Seconds(1.0 / 120.0), controls, &Environment::still_air());
            assert_eq!(a.state(), b.state());
        }
    }

    #[test]
    fn invalid_mass_inertia_and_gear_return_errors_without_panicking() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let mut d = AircraftDefinition::from_config(&AircraftConfig::light_single());
            d.mass_kg = value;
            assert!(d.to_config().is_err());
            let mut d = AircraftDefinition::from_config(&AircraftConfig::light_single());
            d.landing_gear[0].spring_n_per_m = value;
            assert!(d.to_config().is_err());
        }
        let mut d = AircraftDefinition::from_config(&AircraftConfig::light_single());
        d.inertia_kg_m2 = [10.0, 10.0, 10.0, 20.0];
        assert!(d.to_config().is_err());
    }

    #[test]
    fn flaps_cannot_create_thrust_or_reverse_supported_lift() {
        let original = AircraftDefinition::from_config(&AircraftConfig::light_single());
        for negative in [-0.0001, -1.0, -100.0] {
            let mut definition = original.clone();
            definition.aero.drag_flaps = negative;
            assert!(definition.to_config().is_err());
            definition = original.clone();
            definition.aero.lift_flaps = negative;
            assert!(definition.to_config().is_err());
        }
        let mut no_flaps = original;
        no_flaps.aero.drag_flaps = 0.0;
        no_flaps.aero.lift_flaps = 0.0;
        assert!(no_flaps.to_config().is_ok());
    }

    #[test]
    fn unknown_fields_and_wrong_units_are_not_silently_ignored() {
        let mut value = serde_json::to_value(AircraftDefinition::from_config(
            &AircraftConfig::light_single(),
        ))
        .unwrap();
        value["mass_lb"] = serde_json::json!(2300.0);
        assert!(serde_json::from_value::<AircraftDefinition>(value).is_err());
        let mut d = AircraftDefinition::from_config(&AircraftConfig::light_single());
        d.aero.stall_angle_rad = 16.0;
        assert!(d.to_config().is_err());
    }
}
