//! Complete identity for the additive immutable jet model, independent of legacy hashes.
use flightsim_fdm::subsonic::{DRY_JET_MODEL_KIND_ID, JET_FDM_MODEL_REVISION, JetAircraftConfig};

/// FNV-1a64 is a change detector, not authentication.
pub const MODEL_IDENTITY_ALGORITHM: u16 = 1;
/// Complete jet airframe, ordered gear, envelope and component byte ordering.
pub const MODEL_IDENTITY_SCHEMA: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelIdentity {
    pub algorithm: u16,
    pub schema: u16,
    pub kind: u16,
    pub law_revision: u32,
    pub fingerprint: u64,
}

impl ModelIdentity {
    #[must_use]
    pub fn for_jet(config: &JetAircraftConfig) -> Self {
        let fingerprint = canonical_jet_bytes(config)
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Self {
            algorithm: MODEL_IDENTITY_ALGORITHM,
            schema: MODEL_IDENTITY_SCHEMA,
            kind: DRY_JET_MODEL_KIND_ID,
            law_revision: JET_FDM_MODEL_REVISION,
            fingerprint,
        }
    }

    #[must_use]
    pub const fn supported(self) -> bool {
        self.algorithm == MODEL_IDENTITY_ALGORITHM
            && self.schema == MODEL_IDENTITY_SCHEMA
            && self.kind == DRY_JET_MODEL_KIND_ID
            && self.law_revision == JET_FDM_MODEL_REVISION
    }
}

/// Schema-2 canonical SI bits, including signed zero and ordered gear. Immutable
/// component constructors bound all counts; names and presentation are excluded.
#[must_use]
pub fn canonical_jet_bytes(config: &JetAircraftConfig) -> Vec<u8> {
    let mut bytes = b"flightsim/model-identity\0".to_vec();
    bytes.extend(MODEL_IDENTITY_ALGORITHM.to_le_bytes());
    bytes.extend(MODEL_IDENTITY_SCHEMA.to_le_bytes());
    bytes.extend(DRY_JET_MODEL_KIND_ID.to_le_bytes());
    bytes.extend(JET_FDM_MODEL_REVISION.to_le_bytes());
    let airframe = config.airframe();
    scalar(&mut bytes, airframe.mass_properties().mass().get());
    for value in airframe.mass_properties().inertia().to_cols_array() {
        scalar(&mut bytes, value);
    }
    let flightsim_fdm::Geometry {
        wing_area,
        wing_span,
        mean_chord,
    } = *airframe.geometry();
    for value in [wing_area.get(), wing_span.get(), mean_chord.get()] {
        scalar(&mut bytes, value);
    }
    let gear = airframe.landing_gear();
    for value in [
        gear.rolling_friction_coefficient(),
        gear.braking_friction_coefficient(),
        gear.lateral_friction_coefficient(),
        gear.friction_transition_speed().get(),
    ] {
        scalar(&mut bytes, value);
    }
    bytes.extend(3_u32.to_le_bytes());
    for leg in gear.legs() {
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
            scalar(&mut bytes, value);
        }
    }
    let flightsim_fdm::subsonic::OperatingEnvelopeDefinition {
        pressure_ratio,
        temperature_ratio,
        mach,
    } = *config.envelope().definition();
    for value in pressure_ratio
        .into_iter()
        .chain(temperature_ratio)
        .chain(mach)
    {
        scalar(&mut bytes, value);
    }
    for component in [
        config.thrust().canonical_bytes(),
        config.aero().canonical_bytes(),
    ] {
        // Both immutable component formats are bounded far below u32::MAX.
        bytes.extend(
            u32::try_from(component.len())
                .expect("bounded component")
                .to_le_bytes(),
        );
        bytes.extend(component);
    }
    bytes
}
fn scalar(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_bits().to_le_bytes());
}
