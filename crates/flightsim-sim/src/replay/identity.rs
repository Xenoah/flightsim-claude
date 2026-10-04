//! Versioned aircraft identity, separate from the frozen v1/v2 fingerprint.
//!
//! Identity compares configuration and model-law revision, not trajectory or
//! security. A complete match does not attest terrain, initial state, input,
//! platform floating-point behavior, or validity of an unvalidated configuration.
//! This module does not change any recording format or upgrade legacy evidence.

use flightsim_fdm::{
    AeroCoefficients, AircraftConfig, EngineConfig, FDM_MODEL_REVISION, Geometry, LandingGearConfig,
};

use super::aircraft_fingerprint;

/// FNV-1a 64-bit over the explicitly versioned canonical byte sequence.
/// This is a non-cryptographic change detector, never authentication.
pub const AIRCRAFT_IDENTITY_ALGORITHM: u16 = 1;

/// Complete parameter ordering for the current fixed-gear propeller model.
/// New model variants or parameter fields require a new schema.
pub const AIRCRAFT_IDENTITY_SCHEMA: u16 = 1;

const DOMAIN: &[u8] = b"flightsim/aircraft-identity\0";
const LEGACY_PROPELLER_KIND: u16 = 1;

/// Complete identity of a validated aircraft configuration and its model law.
///
/// Fields are explicit for diagnostics and classified comparisons. The v3 wire
/// boundary rejects unsupported algorithms/schemas/model revisions; the pure
/// classifier reports them as mismatches, never reinterpreting their hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AircraftIdentity {
    pub algorithm: u16,
    pub schema: u16,
    pub fdm_model_revision: u32,
    pub fingerprint: u64,
}

impl AircraftIdentity {
    /// Identify the current model using every configurable force-affecting field.
    ///
    /// Callers must validate external profiles before constructing their config.
    /// SI f64 bit patterns are retained exactly, including signed zero. The name,
    /// visual model, sound and input mappings are excluded: replay already stores
    /// the effective controls delivered to physics.
    #[must_use]
    pub fn for_config(config: &AircraftConfig) -> Self {
        Self::for_model_revision(config, FDM_MODEL_REVISION)
    }

    fn for_model_revision(config: &AircraftConfig, fdm_model_revision: u32) -> Self {
        let mut hash = IdentityHash::new();
        hash.bytes(DOMAIN);
        hash.bytes(&AIRCRAFT_IDENTITY_ALGORITHM.to_le_bytes());
        hash.bytes(&AIRCRAFT_IDENTITY_SCHEMA.to_le_bytes());
        hash.bytes(&fdm_model_revision.to_le_bytes());

        // Intentionally exhaustive. New public model fields must make identity
        // coverage a compile-time decision rather than silently disappear.
        let AircraftConfig {
            name: _,
            mass_properties,
            geometry,
            aero,
            engine,
            landing_gear,
        } = config;
        hash.scalar(mass_properties.mass().get());
        for value in mass_properties.inertia().to_cols_array() {
            hash.scalar(value);
        }
        // inverse_inertia is deterministically derived from this matrix by the
        // private MassProperties constructor, not a separate profile parameter.
        let Geometry {
            wing_area,
            wing_span,
            mean_chord,
        } = *geometry;
        for value in [wing_area.get(), wing_span.get(), mean_chord.get()] {
            hash.scalar(value);
        }
        hash.aerodynamics(*aero);
        hash.propeller(*engine);
        hash.landing_gear(landing_gear);

        Self {
            algorithm: AIRCRAFT_IDENTITY_ALGORITHM,
            schema: AIRCRAFT_IDENTITY_SCHEMA,
            fdm_model_revision,
            fingerprint: hash.0,
        }
    }
}

/// Evidence actually present in a recording. A v1/v2 u64 is always partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedAircraftIdentity {
    /// Historical algorithm omits `aero.yaw_rate_p`; no profile was recorded.
    LegacyPartial { fingerprint: u64 },
    /// Explicit algorithm/schema/revision from the format-3 envelope.
    /// Existing v1/v2 files never produce this variant.
    Complete(AircraftIdentity),
}

/// Aircraft identity result, deliberately distinct from playback permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AircraftCompatibility {
    /// Every field in the supported identity schema and model revision matches.
    /// This is not proof of full replay reproducibility or file authenticity.
    CompleteMatch,
    /// The frozen legacy algorithm matches, but omitted coefficients are unknown.
    /// This cannot establish that the recording used a bundled or custom profile.
    /// Playback policy must explicitly handle this uncertainty.
    LegacyPartialMatch,
    /// Values differ, or the algorithm, schema or model revision is unsupported.
    Mismatch,
}

impl RecordedAircraftIdentity {
    /// Compare only aircraft identity; do not authorize playback or infer a
    /// missing coefficient from a name, keyframe, or positional drift tolerance.
    #[must_use]
    pub fn verify(self, config: &AircraftConfig) -> AircraftCompatibility {
        match self {
            Self::Complete(recorded) if recorded == AircraftIdentity::for_config(config) => {
                AircraftCompatibility::CompleteMatch
            }
            Self::LegacyPartial { fingerprint } if fingerprint == aircraft_fingerprint(config) => {
                AircraftCompatibility::LegacyPartialMatch
            }
            Self::Complete(_) | Self::LegacyPartial { .. } => AircraftCompatibility::Mismatch,
        }
    }
}

struct IdentityHash(u64);

impl IdentityHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn scalar(&mut self, value: f64) {
        self.bytes(&value.to_bits().to_le_bytes());
    }

    fn aerodynamics(&mut self, aero: AeroCoefficients) {
        let AeroCoefficients {
            lift_zero,
            lift_alpha,
            lift_flaps,
            stall_angle,
            stall_blend_rate,
            drag_min,
            oswald_efficiency,
            drag_flaps,
            side_beta,
            side_rudder,
            roll_beta,
            roll_rate_p,
            roll_rate_r,
            roll_aileron,
            roll_rudder,
            pitch_zero,
            pitch_alpha,
            pitch_rate_q,
            pitch_elevator,
            pitch_flaps,
            yaw_beta,
            yaw_rate_p,
            yaw_rate_r,
            yaw_aileron,
            yaw_rudder,
        } = aero;
        for value in [
            lift_zero,
            lift_alpha,
            lift_flaps,
            stall_angle.get(),
            stall_blend_rate,
            drag_min,
            oswald_efficiency,
            drag_flaps,
            side_beta,
            side_rudder,
            roll_beta,
            roll_rate_p,
            roll_rate_r,
            roll_aileron,
            roll_rudder,
            pitch_zero,
            pitch_alpha,
            pitch_rate_q,
            pitch_elevator,
            pitch_flaps,
            yaw_beta,
            yaw_rate_p,
            yaw_rate_r,
            yaw_aileron,
            yaw_rudder,
        ] {
            self.scalar(value);
        }
    }

    fn propeller(&mut self, engine: EngineConfig) {
        let EngineConfig {
            max_shaft_power,
            propeller_efficiency,
            static_thrust,
        } = engine;
        self.bytes(&LEGACY_PROPELLER_KIND.to_le_bytes());
        for value in [max_shaft_power, propeller_efficiency, static_thrust.get()] {
            self.scalar(value);
        }
    }

    fn landing_gear(&mut self, gear: &LandingGearConfig) {
        for value in [
            gear.rolling_friction_coefficient(),
            gear.braking_friction_coefficient(),
            gear.lateral_friction_coefficient(),
            gear.friction_transition_speed().get(),
        ] {
            self.scalar(value);
        }
        // Schema 1 has exactly three fixed legs, in stored order. Do not sort:
        // even mathematically equivalent reordering can change force summation.
        let [first, second, third] = gear.legs();
        self.bytes(&3_u32.to_le_bytes());
        for leg in [first, second, third] {
            let point = leg.contact_point().as_vec();
            for value in [
                point.x,
                point.y,
                point.z,
                leg.spring_rate().get(),
                leg.damping_coefficient().get(),
                leg.max_stroke().get(),
                leg.bottom_stop_travel().get(),
                leg.max_recoil_speed().get(),
            ] {
                self.scalar(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_law_revision_changes_the_digest_itself() {
        let config = AircraftConfig::light_single();
        let before = AircraftIdentity::for_model_revision(&config, FDM_MODEL_REVISION - 1);
        let current = AircraftIdentity::for_config(&config);
        assert_ne!(before.fingerprint, current.fingerprint);
        assert_eq!(
            RecordedAircraftIdentity::Complete(before).verify(&config),
            AircraftCompatibility::Mismatch
        );
    }
}
