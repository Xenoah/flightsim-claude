//! Exact, bounded profile v3 for the experimental running-only turboprop.
//!
//! This is an explicit loader, not an app or replay dispatcher. Construction
//! validates physical components and the declared initial engine values; actual
//! position, attitude, wind and atmosphere still require runtime evaluation.
mod wire;

pub use crate::aircraft_profile::{
    ControlDefinition, EngineSound, ExactF64, MAX_JSON_DEPTH, MAX_NUMBER_BYTES, MAX_PROFILE_BYTES,
    ModelDefinition, ProfileError,
};
use crate::aircraft_profile::{exact, metadata};
use flightsim_core::{Meters, Radians, RadiansPerSecond};
use flightsim_fdm::{
    subsonic::MachAeroSchedule,
    turboprop::{
        PropellerMap, SampledGovernor, TURBOPROP_FDM_MODEL_REVISION, TurbineFraction,
        TurbinePowerTable, TurbopropAircraftConfig, TurbopropEnvelope,
    },
};
use std::{io::Read, path::Path};
use wire::ProfileWire;

pub const AIRCRAFT_PROFILE_V3_VERSION: u16 = 3;
pub const RUNNING_TURBOPROP_DYNAMICS_KIND: &str = "running_turboprop_table";

/// Explicit default initial condition, excluded from physical model identity.
/// These values are structurally valid for their profile, not an equilibrium or
/// a guarantee that any chosen body/environment state is supported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunningTurbopropStart {
    turbine_fraction: TurbineFraction,
    shaft_rad_s: RadiansPerSecond,
    blade_pitch_rad: Radians,
}
impl RunningTurbopropStart {
    #[must_use]
    pub const fn turbine_fraction(self) -> TurbineFraction {
        self.turbine_fraction
    }
    #[must_use]
    pub const fn shaft_speed(self) -> RadiansPerSecond {
        self.shaft_rad_s
    }
    #[must_use]
    pub const fn blade_pitch(self) -> Radians {
        self.blade_pitch_rad
    }
}

/// Immutable validated profile. No deserializer or public field bypasses the
/// original-token numeric parser and fallible physical constructors.
#[derive(Debug, Clone)]
pub struct AircraftProfileV3 {
    wire: ProfileWire,
    configuration: TurbopropAircraftConfig,
    sound: EngineSound,
    running_start: RunningTurbopropStart,
}

impl AircraftProfileV3 {
    /// Read at most 1 MiB plus one size-check byte from a local profile file.
    /// Referenced model assets are metadata only and are never opened here.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|error| ProfileError(error.to_string()))?
            .take((MAX_PROFILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| ProfileError(error.to_string()))?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProfileError> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError("aircraft profile v3 exceeds 1 MiB".into()));
        }
        Self::parse(std::str::from_utf8(bytes).map_err(|error| ProfileError(error.to_string()))?)
    }

    /// Parse concrete structs directly from original bounded UTF-8. Duplicate
    /// and unknown fields, unsupported tags, trailing input, oversized/deep
    /// values and nonzero-decimal underflow to zero are errors, never repairs.
    pub fn parse(json: &str) -> Result<Self, ProfileError> {
        if json.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError("aircraft profile v3 exceeds 1 MiB".into()));
        }
        exact::check_depth(json)?;
        let mut wire: ProfileWire =
            serde_json::from_str(json).map_err(|error| ProfileError(error.to_string()))?;
        if wire.version != AIRCRAFT_PROFILE_V3_VERSION {
            return Err(ProfileError(
                "unsupported aircraft profile version (expected 3)".into(),
            ));
        }
        if wire.dynamics.kind != RUNNING_TURBOPROP_DYNAMICS_KIND
            || wire.dynamics.revision != TURBOPROP_FDM_MODEL_REVISION
        {
            return Err(ProfileError(
                "unsupported aircraft dynamics kind or revision".into(),
            ));
        }
        if wire.id.is_empty()
            || wire.id.len() > 48
            || !wire
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(ProfileError(
                "aircraft id must be a short lowercase ASCII identifier".into(),
            ));
        }
        wire.model.validate()?;
        wire.controls.validate()?;
        for value in wire.camera_eye_m {
            metadata::range("camera eye component_m", value, -10.0, 10.0)?;
        }
        let sound = EngineSound::parse(&wire.engine_sound)?;
        // Match v2 normalization; all physical and initial-condition bits remain
        // untouched. No presentation field is allowed to select the dynamics.
        for axis in [&mut wire.model.forward, &mut wire.model.up] {
            let normalized = axis.trim().to_ascii_lowercase();
            *axis = if normalized.len() == 1 {
                format!("+{normalized}")
            } else {
                normalized
            };
        }
        wire.engine_sound = match sound {
            EngineSound::Piston => "piston",
            EngineSound::Turbine => "turbine",
        }
        .into();
        let d = &wire.dynamics;
        let airframe = d.airframe.definition()?.to_config().map_err(error)?;
        let turbine = TurbinePowerTable::from_definition(d.turbine.definition()).map_err(error)?;
        let propeller = PropellerMap::from_definition(d.propeller.definition()?).map_err(error)?;
        let governor = SampledGovernor::from_definition(d.governor.definition()).map_err(error)?;
        let aero = MachAeroSchedule::from_definition(d.aero.definition()).map_err(error)?;
        let envelope =
            TurbopropEnvelope::from_definition(d.envelope.definition()).map_err(error)?;
        let configuration =
            TurbopropAircraftConfig::new(airframe, turbine, propeller, governor, aero, envelope)
                .map_err(error)?;
        let initial = &d.running_start;
        let bounds = configuration.envelope().definition().relative_shaft_rad_s;
        metadata::range(
            "running_start shaft_rad_s",
            initial.shaft_rad_s,
            bounds[0],
            bounds[1],
        )?;
        let governor = configuration.governor().definition();
        metadata::range(
            "running_start blade_pitch_rad",
            initial.blade_pitch_rad,
            governor.minimum_pitch_rad,
            governor.maximum_pitch_rad,
        )?;
        let running_start = RunningTurbopropStart {
            turbine_fraction: TurbineFraction::new(initial.turbine_fraction.get())
                .map_err(error)?,
            shaft_rad_s: RadiansPerSecond(initial.shaft_rad_s.get()),
            blade_pitch_rad: Radians(initial.blade_pitch_rad.get()),
        };
        Ok(Self {
            wire,
            configuration,
            sound,
            running_start,
        })
    }

    /// Shortest-decimal export reparses with identical physical and initial
    /// condition bits through this loader. Source spelling is not identity.
    pub fn to_json(&self) -> Result<String, ProfileError> {
        let json = serde_json::to_string_pretty(&self.wire).map_err(error)?;
        if json.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError(
                "exported aircraft profile v3 exceeds 1 MiB".into(),
            ));
        }
        Ok(json)
    }
    #[must_use]
    pub fn id(&self) -> &str {
        &self.wire.id
    }
    #[must_use]
    pub const fn configuration(&self) -> &TurbopropAircraftConfig {
        &self.configuration
    }
    #[must_use]
    pub const fn running_start(&self) -> RunningTurbopropStart {
        self.running_start
    }
    #[must_use]
    pub const fn model(&self) -> &ModelDefinition {
        &self.wire.model
    }
    #[must_use]
    pub const fn controls(&self) -> &ControlDefinition {
        &self.wire.controls
    }
    #[must_use]
    pub fn camera_eye(&self) -> [Meters; 3] {
        self.wire.camera_eye_m.map(|value| Meters(value.get()))
    }
    #[must_use]
    pub const fn engine_sound(&self) -> EngineSound {
        self.sound
    }
}

fn error(value: impl std::fmt::Display) -> ProfileError {
    ProfileError(value.to_string())
}
