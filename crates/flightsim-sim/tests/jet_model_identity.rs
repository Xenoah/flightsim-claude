use flightsim_sim::{
    aircraft_profile::AircraftProfileV2,
    model_identity::{ModelIdentity, canonical_jet_bytes},
};
use serde_json::{Number, Value};
fn profile(text: &str) -> AircraftProfileV2 {
    AircraftProfileV2::parse(text).unwrap()
}
fn digest(text: &str) -> u64 {
    ModelIdentity::for_jet(profile(text).configuration()).fingerprint
}
fn source() -> &'static str {
    include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json")
}
#[test]
fn independent_complete_identity_golden_matches_every_canonical_byte() {
    let p = profile(source());
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/jet-identity-v2.json")).unwrap();
    let bytes = canonical_jet_bytes(p.configuration());
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hex, fixture["canonical_hex"].as_str().unwrap());
    assert_eq!(
        format!(
            "{:016x}",
            ModelIdentity::for_jet(p.configuration()).fingerprint
        ),
        fixture["fingerprint"].as_str().unwrap()
    );
}
fn numeric_paths(value: &Value, path: String, paths: &mut Vec<String>) {
    match value {
        Value::Number(_) => paths.push(path),
        Value::Array(values) => {
            for (i, v) in values.iter().enumerate() {
                numeric_paths(v, format!("{path}/{i}"), paths);
            }
        }
        Value::Object(values) => {
            for (k, v) in values {
                if k != "schema" && k != "revision" {
                    numeric_paths(v, format!("{path}/{k}"), paths);
                }
            }
        }
        _ => {}
    }
}
#[test]
fn every_independently_configurable_physical_scalar_changes_identity() {
    let baseline: Value = serde_json::from_str(source()).unwrap();
    let encoded = serde_json::to_string(&baseline).unwrap();
    let expected = digest(&encoded);
    let mut paths = Vec::new();
    numeric_paths(&baseline["dynamics"], "/dynamics".into(), &mut paths);
    assert!(paths.len() >= 116);
    for path in paths {
        let old = baseline.pointer(&path).unwrap().as_f64().unwrap();
        let candidates = if old.abs().to_bits() == 0 {
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
            let mut value = baseline.clone();
            *value.pointer_mut(&path).unwrap() = Value::Number(Number::from_f64(next).unwrap());
            let text = serde_json::to_string(&value).unwrap();
            if let Ok(profile) = AircraftProfileV2::parse(&text) {
                assert_ne!(
                    ModelIdentity::for_jet(profile.configuration()).fingerprint,
                    expected,
                    "omitted scalar{path}"
                );
                changed = true;
                break;
            }
        }
        assert!(changed, "no valid scalar perturbation found for{path}");
    }
}
#[test]
fn gear_order_changes_but_presentation_names_do_not() {
    let mut value: Value = serde_json::from_str(source()).unwrap();
    let baseline = digest(&serde_json::to_string(&value).unwrap());
    value["dynamics"]["airframe"]["landing_gear"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    assert_ne!(digest(&serde_json::to_string(&value).unwrap()), baseline);
    value["dynamics"]["airframe"]["landing_gear"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    value["dynamics"]["airframe"]["name"] = Value::String("Another numerical fixture".into());
    value["id"] = Value::String("alternate-fixture".into());
    value["controls"]["default_trim"] = serde_json::json!(0.12);
    value["camera_eye_m"][0] = serde_json::json!(0.7);
    assert_eq!(digest(&serde_json::to_string(&value).unwrap()), baseline);
}
