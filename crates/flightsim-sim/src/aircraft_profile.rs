//! Bounded, exact-numeric public aircraft profile v2.
//!
//! This additive loader accepts only explicitly tagged dry-jet revision 1 data.
//! It performs no asset reads, app selection, network access or replay migration.
//! Legacy app profile v1 and its 128 KiB/default-f64 decoder remain unchanged.
//! Runtime configuration is immutable; the only construction paths validate the
//! entire profile, component endpoints and envelope containment before returning.
pub(crate) mod exact;
pub(crate) mod metadata;
pub(crate) mod wire;

pub use exact::{ExactF64, MAX_NUMBER_BYTES};
pub use metadata::{ControlDefinition, EngineSound, ModelDefinition};

use flightsim_core::Meters;
use flightsim_fdm::subsonic::{
    DryJetTable, JetAircraftConfig, MachAeroSchedule, OperatingEnvelope,
};
use std::{io::Read, path::Path};
use wire::ProfileWire;

/// Maximum UTF-8 source bytes, including whitespace, before any JSON decoding.
pub const MAX_PROFILE_BYTES: usize = 1024 * 1024;

/// Preflight depth cap, including subtrees mistakenly supplied as numeric values.
pub const MAX_JSON_DEPTH: usize = 128;

/// A syntax, resource, metadata or physical-domain rejection of an exact profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError(pub String);
impl std::fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for ProfileError {}

/// Validated immutable profile. No serde deserializer or public field bypasses
/// the exact numeric boundary and physical constructors.
#[derive(Debug, Clone)]
pub struct AircraftProfileV2 {
    wire: ProfileWire,
    configuration: JetAircraftConfig,
    sound: EngineSound,
}

impl AircraftProfileV2 {
    /// Load a local file, reading at most 1 MiB + one size-check byte.
    ///
    /// A model path is metadata relative to the caller's asset root; it is not
    /// opened or resolved here. This is not a symlink/asset-confinement validator.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|error| ProfileError(error.to_string()))?
            .take((MAX_PROFILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| ProfileError(error.to_string()))?;
        Self::from_bytes(&bytes)
    }

    /// Decode original bytes, preserving every accepted binary64 bit pattern.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProfileError> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError("aircraft profile v2 exceeds 1 MiB".into()));
        }
        let json = std::str::from_utf8(bytes).map_err(|error| ProfileError(error.to_string()))?;
        Self::parse(json)
    }

    /// Decode original UTF-8 directly. No Value/from_value or enum buffer lies
    /// between the bounded input and each borrowed RawValue numeric token.
    ///
    /// Duplicate/unknown fields, unknown tags/revisions, nonfinite numbers,
    /// underflow of nonzero decimals to zero and trailing data are rejected.
    pub fn parse(json: &str) -> Result<Self, ProfileError> {
        if json.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError("aircraft profile v2 exceeds 1 MiB".into()));
        }
        // RawValue scans arbitrary subtrees without charging serde_json's normal
        // recursion counter. Guard container depth without allocating first,
        // then keep the normal recursion limit too. from_str checks trailing input.
        exact::check_depth(json)?;
        let mut wire: ProfileWire =
            serde_json::from_str(json).map_err(|error| ProfileError(error.to_string()))?;
        if wire.version != 2 {
            return Err(ProfileError(
                "unsupported aircraft profile version (expected 2)".into(),
            ));
        }
        if wire.dynamics.kind != "dry_jet_table" || wire.dynamics.revision != 1 {
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
        // Normalize categorical aliases so nearly-limit whitespace spellings
        // cannot expand beyond the input budget when pretty-printed for export.
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
        let dynamics = &wire.dynamics;
        let airframe = dynamics
            .airframe
            .definition()?
            .to_config()
            .map_err(|error| ProfileError(error.to_string()))?;
        let thrust = DryJetTable::from_definition(dynamics.thrust.definition())
            .map_err(|error| ProfileError(error.to_string()))?;
        let aero = MachAeroSchedule::from_definition(dynamics.aero.definition())
            .map_err(|error| ProfileError(error.to_string()))?;
        let envelope = OperatingEnvelope::from_definition(dynamics.envelope.definition())
            .map_err(|error| ProfileError(error.to_string()))?;
        let configuration = JetAircraftConfig::new(airframe, thrust, aero, envelope)
            .map_err(|error| ProfileError(error.to_string()))?;
        Ok(Self {
            wire,
            configuration,
            sound,
        })
    }

    /// Export accepted current numbers using serde_json's shortest formatter.
    /// Reparse with this exact loader to preserve the physical bits; textual
    /// spelling/whitespace is not an identity and the legacy decoder is not exact.
    pub fn to_json(&self) -> Result<String, ProfileError> {
        let json = serde_json::to_string_pretty(&self.wire)
            .map_err(|error| ProfileError(error.to_string()))?;
        if json.len() > MAX_PROFILE_BYTES {
            return Err(ProfileError(
                "exported aircraft profile v2 exceeds 1 MiB".into(),
            ));
        }
        Ok(json)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.wire.id
    }

    #[must_use]
    pub const fn configuration(&self) -> &JetAircraftConfig {
        &self.configuration
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

#[cfg(test)]
mod tests;
