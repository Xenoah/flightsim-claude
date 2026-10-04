//! Schema-4 identity for explicitly selected near-static turboprop law 2.
//!
//! The full nested schema-3 bytes identify the retained forward configuration,
//! not the selected runtime. The outer schema-4/law-2 tuple and extension select
//! the new contract. No old identity helper, support gate or replay is widened.
use crate::{
    aircraft_profile_v4::{
        NEAR_STATIC_PROPELLER_COMPONENT_SCHEMA, STATIC_POWER_EPSILON_MULTIPLIER,
    },
    model_identity::{MODEL_IDENTITY_ALGORITHM, ModelIdentity},
    turboprop_identity::canonical_turboprop_bytes,
};
use flightsim_fdm::turboprop::near_static::{
    MAX_ADVERSE_INFLOW_RATIO, MAX_TRANSVERSE_INFLOW_RATIO, NEGATIVE_ADVANCE_RATIO_KNOTS,
    RUNNING_TURBOPROP_MODEL_KIND_ID, TURBOPROP_FDM_MODEL_REVISION, TurbopropAircraftConfig,
};

pub const NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA: u16 = 4;

impl ModelIdentity {
    #[must_use]
    pub fn for_near_static_turboprop(config: &TurbopropAircraftConfig) -> Self {
        Self {
            algorithm: MODEL_IDENTITY_ALGORITHM,
            schema: NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA,
            kind: RUNNING_TURBOPROP_MODEL_KIND_ID,
            law_revision: TURBOPROP_FDM_MODEL_REVISION,
            fingerprint: digest(&canonical_near_static_turboprop_bytes(config)),
        }
    }
    /// Tuple support only: not fingerprint equality, runtime or replay admission.
    #[must_use]
    pub const fn supported_near_static_turboprop(self) -> bool {
        self.algorithm == MODEL_IDENTITY_ALGORITHM
            && self.schema == NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA
            && self.kind == RUNNING_TURBOPROP_MODEL_KIND_ID
            && self.law_revision == TURBOPROP_FDM_MODEL_REVISION
    }
}

/// Domain header plus two u32-length-prefixed components: full retained forward
/// identity bytes (never merely their digest), and schema-2 near-static extension.
/// Integers and exact binary64 bits are little endian. The extension includes
/// every paired negative cell, fixed domain scalar and closed semantic tag.
/// Presentation and explicit running-start values remain excluded.
#[must_use]
pub fn canonical_near_static_turboprop_bytes(config: &TurbopropAircraftConfig) -> Vec<u8> {
    canonical_with_commitments(config, fixed_commitments(), [1, 1, 1])
}

fn fixed_commitments() -> [f64; 5] {
    [
        NEGATIVE_ADVANCE_RATIO_KNOTS[0],
        NEGATIVE_ADVANCE_RATIO_KNOTS[1],
        MAX_ADVERSE_INFLOW_RATIO,
        MAX_TRANSVERSE_INFLOW_RATIO,
        STATIC_POWER_EPSILON_MULTIPLIER,
    ]
}
fn canonical_with_commitments(
    config: &TurbopropAircraftConfig,
    fixed: [f64; 5],
    tags: [u16; 3],
) -> Vec<u8> {
    let mut bytes = b"flightsim/model-identity\0".to_vec();
    bytes.extend(MODEL_IDENTITY_ALGORITHM.to_le_bytes());
    bytes.extend(NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA.to_le_bytes());
    bytes.extend(RUNNING_TURBOPROP_MODEL_KIND_ID.to_le_bytes());
    bytes.extend(TURBOPROP_FDM_MODEL_REVISION.to_le_bytes());
    component(
        &mut bytes,
        &canonical_turboprop_bytes(config.forward_config()),
    );
    let mut extension = NEAR_STATIC_PROPELLER_COMPONENT_SCHEMA
        .to_le_bytes()
        .to_vec();
    // interpolation, static-power bound, inflow scale: each closed tag 1.
    for tag in tags {
        extension.extend(tag.to_le_bytes());
    }
    count(&mut extension, 2);
    for value in &fixed[..2] {
        scalar(&mut extension, *value);
    }
    count(&mut extension, 2);
    for row in config.propeller().negative_rows() {
        count(&mut extension, row.len());
        for &flightsim_fdm::turboprop::PropellerCellDefinition { ct, cp } in row {
            scalar(&mut extension, ct);
            scalar(&mut extension, cp);
        }
    }
    for value in &fixed[2..] {
        scalar(&mut extension, *value);
    }
    component(&mut bytes, &extension);
    bytes
}
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
fn scalar(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_bits().to_le_bytes());
}
fn count(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend(
        u32::try_from(count)
            .expect("immutable component is bounded")
            .to_le_bytes(),
    );
}
fn component(bytes: &mut Vec<u8>, value: &[u8]) {
    count(bytes, value.len());
    bytes.extend(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_fixed_scalar_and_semantic_tag_participates_in_identity() {
        let profile = crate::aircraft_profile_v4::AircraftProfileV4::parse(include_str!(
            "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
        ))
        .unwrap();
        let config = profile.configuration();
        let baseline = canonical_near_static_turboprop_bytes(config);
        // This private encoder witness does not admit altered laws through the
        // public parser/API. All five scalar edits are rejected by that parser.
        for index in 0..5 {
            let mut fixed = fixed_commitments();
            fixed[index] = f64::from_bits(fixed[index].to_bits() + 1);
            let changed = canonical_with_commitments(config, fixed, [1, 1, 1]);
            assert_ne!(changed, baseline);
            assert_ne!(digest(&changed), digest(&baseline));
        }
        for index in 0..3 {
            let mut tags = [1, 1, 1];
            tags[index] = 2;
            let changed = canonical_with_commitments(config, fixed_commitments(), tags);
            assert_ne!(changed, baseline);
            assert_ne!(digest(&changed), digest(&baseline));
        }
    }
}
