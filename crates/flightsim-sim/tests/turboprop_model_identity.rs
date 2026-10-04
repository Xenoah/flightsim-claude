use flightsim_sim::{
    aircraft_profile_v3::AircraftProfileV3,
    model_identity::ModelIdentity,
    turboprop_identity::{TURBOPROP_MODEL_IDENTITY_SCHEMA, canonical_turboprop_bytes},
};
use serde_json::{Number, Value};

const SOURCE: &str =
    include_str!("../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json");
fn identity(text: &str) -> ModelIdentity {
    ModelIdentity::for_turboprop(AircraftProfileV3::parse(text).unwrap().configuration())
}

#[test]
fn independent_python_golden_pins_every_byte_and_digest() {
    let p = AircraftProfileV3::parse(SOURCE).unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/turboprop-identity-v3.json")).unwrap();
    let bytes = canonical_turboprop_bytes(p.configuration());
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(bytes.len(), 1379);
    assert_eq!(hex, fixture["canonical_hex"].as_str().unwrap());
    assert_eq!(
        format!("{:016x}", identity(SOURCE).fingerprint),
        fixture["fingerprint"].as_str().unwrap()
    );
    assert_eq!(identity(SOURCE).fingerprint, 0xa302_a97f_8e77_8c26);
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
                if !["schema", "revision", "rotation_sense", "running_start"]
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
        152,
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
            if let Ok(p) = AircraftProfileV3::parse(&v.to_string()) {
                assert_ne!(
                    ModelIdentity::for_turboprop(p.configuration()),
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
    v["dynamics"]["propeller"]["rotation_sense"] = serde_json::json!(-1);
    assert_ne!(identity(&v.to_string()), expected);
    v["dynamics"]["propeller"]["rotation_sense"] = serde_json::json!(1);
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
    assert_eq!(TURBOPROP_MODEL_IDENTITY_SCHEMA, 3);
    let id = identity(SOURCE);
    assert_eq!(
        (id.algorithm, id.schema, id.kind, id.law_revision),
        (1, 3, 3, 1)
    );
    assert!(id.supported_turboprop());
    assert!(!id.supported());
    for invalid in [
        ModelIdentity { algorithm: 2, ..id },
        ModelIdentity { schema: 2, ..id },
        ModelIdentity { kind: 2, ..id },
        ModelIdentity {
            law_revision: 2,
            ..id
        },
    ] {
        assert!(!invalid.supported_turboprop());
        assert!(!invalid.supported());
    }
    let jet = flightsim_sim::aircraft_profile::AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap();
    let jet_id = ModelIdentity::for_jet(jet.configuration());
    assert!(jet_id.supported());
    assert!(!jet_id.supported_turboprop());
}

#[test]
fn component_revisions_and_convention_are_explicit_not_silently_reused() {
    let baseline: Value = serde_json::from_str(SOURCE).unwrap();
    for path in [
        "/dynamics/revision",
        "/dynamics/turbine/schema",
        "/dynamics/propeller/schema",
        "/dynamics/governor/schema",
        "/dynamics/aero/schema",
    ] {
        let mut v = baseline.clone();
        *v.pointer_mut(path).unwrap() = serde_json::json!(2);
        assert!(AircraftProfileV3::parse(&v.to_string()).is_err(), "{path}");
    }
}

#[test]
fn actual_v4_reader_rejects_complete_schema3_turboprop_identity() {
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
