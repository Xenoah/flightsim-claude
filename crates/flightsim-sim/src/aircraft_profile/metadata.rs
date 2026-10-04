//! Presentation/input metadata only. None selects or modifies the FDM model.
use super::{ExactF64, ProfileError};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

/// Raw external model metadata, accessible immutably from a validated profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub path: String,
    pub forward: String,
    pub up: String,
    pub length_m: ExactF64,
}

/// Input rates and initial approach hints. They are not a trim/stability proof.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDefinition {
    pub surface_rate: ExactF64,
    #[serde(default)]
    pub elevator_rate: Option<ExactF64>,
    pub centering_rate: ExactF64,
    #[serde(default)]
    pub elevator_centering_rate: Option<ExactF64>,
    pub throttle_rate: ExactF64,
    pub flap_rate: ExactF64,
    pub default_trim: ExactF64,
    pub trim_rate: ExactF64,
    pub approach_speed_mps: ExactF64,
    pub approach_pitch_rad: ExactF64,
    pub approach_trim: ExactF64,
    pub approach_throttle: ExactF64,
    pub approach_flaps: ExactF64,
}

/// Presentation sound category, independent of the explicit dynamics kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSound {
    Piston,
    Turbine,
}

impl EngineSound {
    pub(super) fn parse(value: &str) -> Result<Self, ProfileError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "piston" | "prop" | "propeller" => Ok(Self::Piston),
            "turbine" | "jet" | "turbofan" | "fighter" => Ok(Self::Turbine),
            _ => Err(ProfileError(
                "engine_sound must be piston or turbine".into(),
            )),
        }
    }
}

pub(super) fn range(name: &str, value: ExactF64, min: f64, max: f64) -> Result<(), ProfileError> {
    if !(min..=max).contains(&value.get()) {
        return Err(ProfileError(format!("{name} must be {min}..={max}")));
    }
    Ok(())
}

impl ModelDefinition {
    pub(super) fn validate(&self) -> Result<(), ProfileError> {
        let path = Path::new(&self.path);
        if self.path.is_empty()
            || self.path.len() > 256
            || path.is_absolute()
            || self.path.contains(['\\', ':'])
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !self.path.ends_with(".glb")
        {
            return Err(ProfileError(
                "model path must be a relative .glb asset path without parent traversal".into(),
            ));
        }
        let axis = |value: &str| match value.trim().to_ascii_lowercase().as_str() {
            "x" | "+x" | "-x" => Ok(0),
            "y" | "+y" | "-y" => Ok(1),
            "z" | "+z" | "-z" => Ok(2),
            _ => Err(ProfileError("model axis must be +/-x, +/-y or +/-z".into())),
        };
        if axis(&self.forward)? == axis(&self.up)? {
            return Err(ProfileError(
                "model forward and up axes must be perpendicular".into(),
            ));
        }
        range("model length_m", self.length_m, 1.0, 100.0)
    }
}

impl ControlDefinition {
    pub(super) fn validate(&self) -> Result<(), ProfileError> {
        for value in [
            self.surface_rate,
            self.elevator_rate.unwrap_or(self.surface_rate),
            self.throttle_rate,
            self.flap_rate,
            self.trim_rate,
        ] {
            range("control rate", value, 0.01, 10.0)?;
        }
        for value in [
            self.centering_rate,
            self.elevator_centering_rate.unwrap_or(self.centering_rate),
        ] {
            range("centering rate", value, 0.0, 10.0)?;
        }
        for value in [self.default_trim, self.approach_trim] {
            range("trim", value, -1.0, 1.0)?;
        }
        range("approach speed_mps", self.approach_speed_mps, 10.0, 150.0)?;
        range(
            "approach pitch_rad",
            self.approach_pitch_rad,
            -std::f64::consts::FRAC_PI_4,
            std::f64::consts::FRAC_PI_4,
        )?;
        range("approach throttle", self.approach_throttle, 0.0, 1.0)?;
        range("approach flaps", self.approach_flaps, 0.0, 1.0)
    }
}
