//! Schema-3 complete physical identity for the running-only turboprop.
//!
//! This separately named gate never broadens the schema-2 jet gate used by v4
//! replay. Initial conditions and presentation do not enter physical identity.
use crate::model_identity::{MODEL_IDENTITY_ALGORITHM, ModelIdentity};
use flightsim_fdm::turboprop::{
    GovernorDefinition, PowerCellDefinition, PropellerCellDefinition, PropellerConvention,
    PropellerDefinition, RUNNING_TURBOPROP_MODEL_KIND_ID, TURBOPROP_FDM_MODEL_REVISION,
    TurbineDefinition, TurbopropAircraftConfig, TurbopropEnvelopeDefinition,
};

pub const TURBOPROP_MODEL_IDENTITY_SCHEMA: u16 = 3;

impl ModelIdentity {
    #[must_use]
    pub fn for_turboprop(config: &TurbopropAircraftConfig) -> Self {
        let fingerprint = canonical_turboprop_bytes(config)
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Self {
            algorithm: MODEL_IDENTITY_ALGORITHM,
            schema: TURBOPROP_MODEL_IDENTITY_SCHEMA,
            kind: RUNNING_TURBOPROP_MODEL_KIND_ID,
            law_revision: TURBOPROP_FDM_MODEL_REVISION,
            fingerprint,
        }
    }

    /// Checks the new family tuple only. This is neither fingerprint matching
    /// nor replay admission; ModelIdentity::supported remains jet-only.
    #[must_use]
    pub const fn supported_turboprop(self) -> bool {
        self.algorithm == MODEL_IDENTITY_ALGORITHM
            && self.schema == TURBOPROP_MODEL_IDENTITY_SCHEMA
            && self.kind == RUNNING_TURBOPROP_MODEL_KIND_ID
            && self.law_revision == TURBOPROP_FDM_MODEL_REVISION
    }
}

/// Validated physical bits in specified schema-3 order, all little endian. The
/// locked inertia tensor is included once; cached rotor-subtracted inertia is
/// derived. Component lengths/counts are u32, revisions u16, sense a signed i8.
/// Signed zero is preserved. FNV-1a64 is a change detector, not authentication.
#[must_use]
pub fn canonical_turboprop_bytes(config: &TurbopropAircraftConfig) -> Vec<u8> {
    let mut bytes = b"flightsim/model-identity\0".to_vec();
    bytes.extend(MODEL_IDENTITY_ALGORITHM.to_le_bytes());
    bytes.extend(TURBOPROP_MODEL_IDENTITY_SCHEMA.to_le_bytes());
    bytes.extend(RUNNING_TURBOPROP_MODEL_KIND_ID.to_le_bytes());
    bytes.extend(TURBOPROP_FDM_MODEL_REVISION.to_le_bytes());
    let airframe = config.airframe();
    scalar(&mut bytes, airframe.mass_properties().mass().get());
    scalars(
        &mut bytes,
        airframe.mass_properties().inertia().to_cols_array(),
    );
    let flightsim_fdm::Geometry {
        wing_area,
        wing_span,
        mean_chord,
    } = *airframe.geometry();
    scalars(
        &mut bytes,
        [wing_area.get(), wing_span.get(), mean_chord.get()],
    );
    let gear = airframe.landing_gear();
    scalars(
        &mut bytes,
        [
            gear.rolling_friction_coefficient(),
            gear.braking_friction_coefficient(),
            gear.lateral_friction_coefficient(),
            gear.friction_transition_speed().get(),
        ],
    );
    count(&mut bytes, 3);
    for leg in gear.legs() {
        let point = leg.contact_point().as_vec();
        scalars(
            &mut bytes,
            [
                point.x,
                point.y,
                point.z,
                leg.spring_rate().get(),
                leg.damping_coefficient().get(),
                leg.max_stroke().get(),
                leg.bottom_stop_travel().get(),
                leg.max_recoil_speed().get(),
            ],
        );
    }
    // Exhaustive destructuring makes newly added public component fields a
    // compile-time identity review point instead of silently omitting them.
    let TurbopropEnvelopeDefinition {
        pressure_ratio,
        temperature_ratio,
        mach,
        relative_shaft_rad_s,
        absolute_spin_rad_s,
        maximum_helical_tip_mach,
        maximum_crossflow_tip_ratio,
    } = *config.envelope().definition();
    for values in [
        pressure_ratio,
        temperature_ratio,
        mach,
        relative_shaft_rad_s,
        absolute_spin_rad_s,
    ] {
        scalars(&mut bytes, values);
    }
    scalars(
        &mut bytes,
        [maximum_helical_tip_mach, maximum_crossflow_tip_ratio],
    );

    let TurbineDefinition {
        schema,
        pressure_ratios,
        temperature_ratios,
        cells,
        rise_seconds,
        fall_seconds,
        output_torque_limit_nm,
    } = config.turbine().definition();
    let mut turbine = schema.to_le_bytes().to_vec();
    axis(&mut turbine, pressure_ratios);
    axis(&mut turbine, temperature_ratios);
    count(&mut turbine, cells.len());
    for &PowerCellDefinition { idle_w, maximum_w } in cells {
        scalars(&mut turbine, [idle_w, maximum_w]);
    }
    scalars(
        &mut turbine,
        [*rise_seconds, *fall_seconds, *output_torque_limit_nm],
    );
    component(&mut bytes, &turbine);

    let PropellerDefinition {
        schema,
        convention,
        diameter_m,
        rotor_axial_inertia_kg_m2,
        rotation_sense,
        advance_ratio,
        blade_pitch_rad,
        cells,
    } = config.propeller().definition();
    let mut propeller = schema.to_le_bytes().to_vec();
    let convention: u16 = match convention {
        PropellerConvention::IsolatedAxialPropeller => 1,
    };
    propeller.extend(convention.to_le_bytes());
    scalars(&mut propeller, [*diameter_m, *rotor_axial_inertia_kg_m2]);
    propeller.extend(rotation_sense.to_le_bytes());
    axis(&mut propeller, advance_ratio);
    axis(&mut propeller, blade_pitch_rad);
    count(&mut propeller, cells.len());
    for &PropellerCellDefinition { ct, cp } in cells {
        scalars(&mut propeller, [ct, cp]);
    }
    component(&mut bytes, &propeller);

    let GovernorDefinition {
        schema,
        reference_rad_s,
        gain,
        fine_rate_rad_s,
        coarse_rate_rad_s,
        minimum_pitch_rad,
        maximum_pitch_rad,
    } = *config.governor().definition();
    let mut governor = schema.to_le_bytes().to_vec();
    scalars(
        &mut governor,
        [
            reference_rad_s,
            gain,
            fine_rate_rad_s,
            coarse_rate_rad_s,
            minimum_pitch_rad,
            maximum_pitch_rad,
        ],
    );
    component(&mut bytes, &governor);
    component(&mut bytes, &config.aero().canonical_bytes());
    bytes
}

fn scalar(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_bits().to_le_bytes());
}
fn scalars(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f64>) {
    for value in values {
        scalar(bytes, value);
    }
}
fn count(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend(
        u32::try_from(value)
            .expect("immutable component count is bounded")
            .to_le_bytes(),
    );
}
fn axis(bytes: &mut Vec<u8>, values: &[f64]) {
    count(bytes, values.len());
    scalars(bytes, values.iter().copied());
}
fn component(bytes: &mut Vec<u8>, component: &[u8]) {
    count(bytes, component.len());
    bytes.extend(component);
}
