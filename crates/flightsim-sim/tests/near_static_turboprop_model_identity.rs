use flightsim_sim::{
    aircraft_profile_v4::AircraftProfileV4,
    model_identity::ModelIdentity,
    near_static_turboprop_identity::{
        NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA, canonical_near_static_turboprop_bytes,
    },
};
use serde_json::{Number, Value};

const SOURCE: &str = include_str!(
    "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
);
fn identity(text: &str) -> ModelIdentity {
    ModelIdentity::for_near_static_turboprop(
        AircraftProfileV4::parse(text).unwrap().configuration(),
    )
}

#[test]
fn independent_python_golden_pins_every_byte_and_digest() {
    let p = AircraftProfileV4::parse(SOURCE).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/near-static-turboprop-identity-v4.json"
    ))
    .unwrap();
    let bytes = canonical_near_static_turboprop_bytes(p.configuration());
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(bytes.len(), 1582);
    assert_eq!(hex, fixture["canonical_hex"].as_str().unwrap());
    assert_eq!(
        format!("{:016x}", identity(SOURCE).fingerprint),
        fixture["fingerprint"].as_str().unwrap()
    );
    assert_eq!(identity(SOURCE).fingerprint, 0xf3bb_440d_4797_f5c4);
    let forward = flightsim_sim::turboprop_identity::canonical_turboprop_bytes(
        p.configuration().forward_config(),
    );
    assert_eq!(forward.len(), 1379);
    let prefix = b"flightsim/model-identity\0".len() + 10;
    assert_eq!(&bytes[prefix + 4..prefix + 4 + forward.len()], forward);
    let old = flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
    ))
    .unwrap();
    assert_eq!(
        forward,
        flightsim_sim::turboprop_identity::canonical_turboprop_bytes(old.configuration())
    );
    assert_eq!(
        ModelIdentity::for_turboprop(old.configuration()).fingerprint,
        0xa302_a97f_8e77_8c26
    );
    // A full forward component of 1379 bytes, not its 8-byte digest.
    assert!(forward.len() > std::mem::size_of::<u64>());
}

#[test]
fn actual_v5_reader_and_writer_keep_the_complete_law1_gate_closed() {
    use flightsim_sim::replay_v5::TurbopropRecording;
    let original = include_bytes!("fixtures/v5_zero.fsreplay");
    let old = TurbopropRecording::read_from(&mut original.as_slice()).unwrap();
    let mut bytes = Vec::new();
    old.write_to(&mut bytes).unwrap();
    assert_eq!(bytes, original);
    let name_len = u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize;
    let offset = 18 + name_len;
    let id = identity(SOURCE);
    for candidate in [
        id,
        ModelIdentity { schema: 3, ..id },
        ModelIdentity {
            law_revision: 1,
            ..id
        },
    ] {
        let mut tampered = bytes.clone();
        tampered[offset..offset + 2].copy_from_slice(&candidate.algorithm.to_le_bytes());
        tampered[offset + 2..offset + 4].copy_from_slice(&candidate.schema.to_le_bytes());
        tampered[offset + 4..offset + 6].copy_from_slice(&candidate.kind.to_le_bytes());
        tampered[offset + 6..offset + 10].copy_from_slice(&candidate.law_revision.to_le_bytes());
        tampered[offset + 10..offset + 18].copy_from_slice(&candidate.fingerprint.to_le_bytes());
        let error = TurbopropRecording::read_from(&mut tampered.as_slice()).unwrap_err();
        assert!(error.to_string().contains("schema3 turboprop identity"));
    }
    assert_eq!(flightsim_sim::replay_v5::TURBOPROP_FORMAT_VERSION, 5);
}

#[test]
fn line_endings_and_roundtrip_have_identical_independent_golden_bits() {
    let normalized = SOURCE.replace("\r\n", "\n");
    let golden = canonical_near_static_turboprop_bytes(
        AircraftProfileV4::parse(&normalized)
            .unwrap()
            .configuration(),
    );
    for source in [&normalized, &normalized.replace('\n', "\r\n")] {
        let profile = AircraftProfileV4::parse(source).unwrap();
        let exported = profile.to_json().unwrap();
        let again = AircraftProfileV4::parse(&exported).unwrap();
        assert_eq!(
            canonical_near_static_turboprop_bytes(again.configuration()),
            golden
        );
    }
}

fn physical_numeric_paths(value: &Value, path: String, paths: &mut Vec<String>) {
    match value {
        Value::Number(_) => paths.push(path),
        Value::Array(values) => {
            for (i, value) in values.iter().enumerate() {
                physical_numeric_paths(value, format!("{path}/{i}"), paths);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if ![
                    "schema",
                    "revision",
                    "rotation_sense",
                    "running_start",
                    "negative_advance_ratio",
                    "near_static_domain",
                ]
                .contains(&key.as_str())
                {
                    physical_numeric_paths(value, format!("{path}/{key}"), paths);
                }
            }
        }
        _ => {}
    }
}

#[test]
fn every_independently_configurable_physical_number_changes_identity() {
    let baseline: Value = serde_json::from_str(SOURCE).unwrap();
    let expected = identity(&baseline.to_string());
    let mut paths = Vec::new();
    physical_numeric_paths(&baseline["dynamics"], "/dynamics".into(), &mut paths);
    assert_eq!(
        paths.len(),
        164,
        "update the independent physical coverage witness when fields change"
    );
    for path in paths {
        let old = baseline.pointer(&path).unwrap().as_f64().unwrap();
        let candidates = if old == 0.0 {
            vec![
                if old.is_sign_negative() { 0.0 } else { -0.0 },
                0.0001,
                -0.0001,
            ]
        } else {
            vec![old + old.abs() * 1e-7, old - old.abs() * 1e-7]
        };
        let mut changed = false;
        for next in candidates {
            let mut v = baseline.clone();
            *v.pointer_mut(&path).unwrap() = Value::Number(Number::from_f64(next).unwrap());
            if let Ok(p) = AircraftProfileV4::parse(&v.to_string()) {
                assert_ne!(
                    ModelIdentity::for_near_static_turboprop(p.configuration()),
                    expected,
                    "omitted {path}"
                );
                changed = true;
                break;
            }
        }
        assert!(changed, "no admissible independent perturbation for {path}");
    }
}

#[test]
fn sense_and_order_are_physical_but_names_metadata_and_initial_state_are_not() {
    let mut v: Value = serde_json::from_str(SOURCE).unwrap();
    let expected = identity(&v.to_string());
    v["dynamics"]["propeller"]["forward"]["rotation_sense"] = serde_json::json!(-1);
    assert_ne!(identity(&v.to_string()), expected);
    v["dynamics"]["propeller"]["forward"]["rotation_sense"] = serde_json::json!(1);
    v["dynamics"]["airframe"]["landing_gear"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    assert_ne!(identity(&v.to_string()), expected);
    v["dynamics"]["airframe"]["landing_gear"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    v["dynamics"]["turbine"]["cells"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert_ne!(identity(&v.to_string()), expected);
    v["dynamics"]["turbine"]["cells"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    v["id"] = serde_json::json!("other-numerical-fixture");
    v["dynamics"]["airframe"]["name"] = serde_json::json!("Another authored name");
    v["model"]["path"] = serde_json::json!("unprovided/other.glb");
    v["engine_sound"] = serde_json::json!("piston");
    v["controls"]["default_trim"] = serde_json::json!(0.12);
    v["camera_eye_m"][0] = serde_json::json!(0.7);
    v["dynamics"]["running_start"] =
        serde_json::json!({"turbine_fraction":0.8,"shaft_rad_s":125,"blade_pitch_rad":0.6});
    assert_eq!(identity(&v.to_string()), expected);
}

#[test]
fn additive_support_gate_never_admits_turboprop_through_jet_gate() {
    assert_eq!(NEAR_STATIC_TURBOPROP_MODEL_IDENTITY_SCHEMA, 4);
    let id = identity(SOURCE);
    assert_eq!(
        (id.algorithm, id.schema, id.kind, id.law_revision),
        (1, 4, 3, 2)
    );
    assert!(id.supported_near_static_turboprop());
    assert!(!id.supported_turboprop());
    assert!(!id.supported());
    for invalid in [
        ModelIdentity { algorithm: 2, ..id },
        ModelIdentity { schema: 2, ..id },
        ModelIdentity { kind: 2, ..id },
        ModelIdentity {
            law_revision: 1,
            ..id
        },
    ] {
        assert!(!invalid.supported_near_static_turboprop());
        assert!(!invalid.supported());
    }
    let jet = flightsim_sim::aircraft_profile::AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap();
    let jet_id = ModelIdentity::for_jet(jet.configuration());
    assert!(jet_id.supported());
    assert!(!jet_id.supported_near_static_turboprop());
}

#[test]
fn component_revisions_and_convention_are_explicit_not_silently_reused() {
    let baseline: Value = serde_json::from_str(SOURCE).unwrap();
    for path in [
        "/dynamics/turbine/schema",
        "/dynamics/propeller/forward/schema",
        "/dynamics/governor/schema",
        "/dynamics/aero/schema",
    ] {
        let mut v = baseline.clone();
        *v.pointer_mut(path).unwrap() = serde_json::json!(2);
        assert!(AircraftProfileV4::parse(&v.to_string()).is_err(), "{path}");
    }
}

#[test]
fn actual_v4_reader_rejects_complete_schema4_turboprop_identity() {
    use flightsim_sim::replay_v4::{JetRecording, ModelReplayFile};
    let original = include_bytes!("fixtures/v4_zero.fsreplay");
    let mut bytes = original.to_vec();
    let name_len = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
    let offset = 18 + usize::try_from(name_len).unwrap();
    let id = identity(SOURCE);
    bytes[offset..offset + 2].copy_from_slice(&id.algorithm.to_le_bytes());
    bytes[offset + 2..offset + 4].copy_from_slice(&id.schema.to_le_bytes());
    bytes[offset + 4..offset + 6].copy_from_slice(&id.kind.to_le_bytes());
    bytes[offset + 6..offset + 10].copy_from_slice(&id.law_revision.to_le_bytes());
    bytes[offset + 10..offset + 18].copy_from_slice(&id.fingerprint.to_le_bytes());
    let error = JetRecording::read_from(&mut bytes.as_slice()).unwrap_err();
    assert!(
        error.to_string().contains("schema2 jet identity"),
        "{error}"
    );
    assert!(ModelReplayFile::read_from(&mut bytes.as_slice()).is_err());
    // The unmodified independent old fixture remains admitted byte-for-byte.
    let old = JetRecording::read_from(&mut original.as_slice()).unwrap();
    let mut encoded = Vec::new();
    old.write_to(&mut encoded).unwrap();
    assert_eq!(encoded, original);
}
