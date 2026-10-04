//! Shared schema corpus plus physical-bit/export and hostile-input boundaries.
//! Value is used only to synthesize test documents, never in the public loader.
use flightsim_fdm::subsonic::AirframeDefinition;
use flightsim_sim::aircraft_profile::{AircraftProfileV2, EngineSound, MAX_PROFILE_BYTES};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const EXAMPLE: &str =
    include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json");

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    runtime_valid: bool,
    #[serde(default)]
    set: BTreeMap<String, Value>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    repeat: Vec<Repeat>,
    raw_replace: Option<[String; 2]>,
    pad_to_bytes: Option<usize>,
}
#[derive(Deserialize)]
struct Repeat {
    pointer: String,
    count: usize,
    value: Value,
}

fn assign(value: &mut Value, pointer: &str, replacement: Value) {
    let (parent, key) = pointer.rsplit_once('/').unwrap();
    let target = value.pointer_mut(parent).unwrap();
    if let Some(array) = target.as_array_mut() {
        array[key.parse::<usize>().unwrap()] = replacement;
    } else {
        target
            .as_object_mut()
            .unwrap()
            .insert(key.to_owned(), replacement);
    }
}

fn materialize(case: &Case) -> String {
    let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
    for (pointer, replacement) in &case.set {
        assign(&mut value, pointer, replacement.clone());
    }
    for recipe in &case.repeat {
        assign(
            &mut value,
            &recipe.pointer,
            Value::Array(vec![recipe.value.clone(); recipe.count]),
        );
    }
    for pointer in &case.remove {
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        assert!(
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key)
                .is_some()
        );
    }
    let mut text = serde_json::to_string(&value).unwrap();
    if let Some([old, new]) = &case.raw_replace {
        assert_eq!(text.matches(old.as_str()).count(), 1, "{}", case.name);
        text = text.replacen(old, new, 1);
    }
    if let Some(size) = case.pad_to_bytes {
        text.push_str(&" ".repeat(size.checked_sub(text.len()).unwrap()));
    }
    text
}

#[test]
fn shared_public_schema_corpus_matches_real_loader() {
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../../schemas/tests/aircraft-profile-v2-cases.json"
    ))
    .unwrap();
    let mut mismatches = Vec::new();
    for case in &corpus.cases {
        let result = AircraftProfileV2::parse(&materialize(case));
        if result.is_ok() != case.runtime_valid {
            mismatches.push(format!("{}: {result:?}", case.name));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn exact_export_preserves_constructed_physical_bits_and_metadata() {
    let original = AircraftProfileV2::parse(EXAMPLE).unwrap();
    let exported = original.to_json().unwrap();
    let reparsed = AircraftProfileV2::parse(&exported).unwrap();
    assert_eq!(exported, reparsed.to_json().unwrap());
    let before = original.configuration();
    let after = reparsed.configuration();
    assert_eq!(
        before.thrust().canonical_bytes(),
        after.thrust().canonical_bytes()
    );
    assert_eq!(
        before.aero().canonical_bytes(),
        after.aero().canonical_bytes()
    );
    // Finite shortest-decimal serialization is injective over f64 bits including
    // -0.0. Compare every neutral/gear scalar reconstructed from actual runtime.
    assert_eq!(
        serde_json::to_string(&AirframeDefinition::from_config(before.airframe())).unwrap(),
        serde_json::to_string(&AirframeDefinition::from_config(after.airframe())).unwrap()
    );
    assert_eq!(
        serde_json::to_string(before.envelope().definition()).unwrap(),
        serde_json::to_string(after.envelope().definition()).unwrap()
    );
    assert_eq!(
        before.aero().definition().knots[0]
            .aero
            .pitch_rate_q
            .to_bits(),
        0xc03e_6666_6666_6667
    );
    assert_eq!(
        before.aero().definition().knots[0]
            .aero
            .yaw_rate_p
            .to_bits(),
        0x8000_0000_0000_0000
    );
    assert_eq!(original.engine_sound(), EngineSound::Turbine);
    assert_eq!(original.id(), "numerical-jet-fixture");
}

#[test]
fn physical_signed_zero_and_subnormal_values_survive_configuration() {
    let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
    for path in [
        "/dynamics/airframe/inertia_kg_m2/3",
        "/dynamics/airframe/landing_gear/0/contact_m/1",
        "/dynamics/thrust/cells/0/idle_n",
        "/dynamics/envelope/mach/0",
        "/camera_eye_m/0",
    ] {
        assign(&mut value, path, serde_json::json!(-0.0));
    }
    assign(
        &mut value,
        "/dynamics/aero/knots/0/aero/yaw_rate_p",
        serde_json::json!(f64::from_bits(1)),
    );
    let profile = AircraftProfileV2::parse(&value.to_string()).unwrap();
    let cfg = profile.configuration();
    let airframe = AirframeDefinition::from_config(cfg.airframe());
    for number in [
        airframe.inertia_kg_m2[3],
        airframe.landing_gear[0].contact_m[1],
        cfg.thrust().definition().cells[0].idle_n,
        cfg.envelope().definition().mach[0],
        profile.camera_eye()[0].get(),
    ] {
        assert_eq!(number.to_bits(), 0x8000_0000_0000_0000);
    }
    assert_eq!(
        cfg.aero().definition().knots[0].aero.yaw_rate_p.to_bits(),
        1
    );
    let again = AircraftProfileV2::parse(&profile.to_json().unwrap()).unwrap();
    assert_eq!(
        cfg.aero().canonical_bytes(),
        again.configuration().aero().canonical_bytes()
    );
}

#[test]
fn byte_limit_applies_to_all_entry_points_before_decoding() {
    let mut at_limit = EXAMPLE.as_bytes().to_vec();
    at_limit.resize(MAX_PROFILE_BYTES, b' ');
    assert!(AircraftProfileV2::from_bytes(&at_limit).is_ok());
    let dir = std::env::temp_dir().join(format!(
        "jet-profile-bounds-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("profile.json");
    std::fs::write(&file, &at_limit).unwrap();
    assert!(AircraftProfileV2::load(&file).is_ok());
    at_limit.push(b' ');
    std::fs::write(&file, &at_limit).unwrap();
    let load = AircraftProfileV2::load(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    for result in [
        load,
        AircraftProfileV2::from_bytes(&at_limit),
        AircraftProfileV2::parse(std::str::from_utf8(&at_limit).unwrap()),
    ] {
        assert!(result.unwrap_err().0.contains("exceeds 1 MiB"));
    }
    assert!(AircraftProfileV2::from_bytes(&[0xff]).is_err());
}

#[test]
fn duplicate_nested_fields_and_trailing_or_deep_values_reject() {
    for (old, new) in [
        (
            "\"surface_rate\": 2.5",
            "\"surface_rate\":2.5,\"surface_rate\":2.5",
        ),
        (
            "\"elevator_rate\": 0.25",
            "\"elevator_rate\":null,\"elevator_rate\":0.25",
        ),
        ("\"revision\": 1", "\"revision\":1,\"revision\":1"),
        ("\"idle_n\": 0.0", "\"idle_n\":0,\"idle_n\":0"),
        ("\"yaw_rate_p\": -0.0", "\"yaw_rate_p\":0,\"yaw_rate_p\":0"),
    ] {
        assert!(EXAMPLE.contains(old));
        assert!(
            AircraftProfileV2::parse(&EXAMPLE.replacen(old, new, 1)).is_err(),
            "{old}"
        );
    }
    assert!(AircraftProfileV2::parse(&format!("{EXAMPLE}{{}}")).is_err());
    let deep = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    assert!(
        AircraftProfileV2::parse(&EXAMPLE.replacen("1043.0", &deep, 1))
            .unwrap_err()
            .0
            .contains("nesting exceeds 128")
    );
    assert!(AircraftProfileV2::load(Path::new("not-a-real-profile-file.json")).is_err());
}

#[test]
fn every_metadata_float_uses_exact_rounding_and_exports_the_same_bits() {
    let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
    // Each positive value differs from a simple fraction by one representable ULP.
    // Inject only through serde's shortest formatter, then assert explicit bits.
    let half_next = f64::from_bits(0x3fe0_0000_0000_0001);
    for path in [
        "/controls/surface_rate",
        "/controls/elevator_rate",
        "/controls/centering_rate",
        "/controls/elevator_centering_rate",
        "/controls/throttle_rate",
        "/controls/flap_rate",
        "/controls/default_trim",
        "/controls/trim_rate",
        "/controls/approach_pitch_rad",
        "/controls/approach_trim",
        "/controls/approach_throttle",
        "/controls/approach_flaps",
        "/camera_eye_m/0",
        "/camera_eye_m/1",
        "/camera_eye_m/2",
    ] {
        assign(&mut value, path, serde_json::json!(half_next));
    }
    assign(
        &mut value,
        "/model/length_m",
        serde_json::json!(f64::from_bits(0x4020_0000_0000_0001)),
    );
    assign(
        &mut value,
        "/controls/approach_speed_mps",
        serde_json::json!(f64::from_bits(0x4040_0000_0000_0001)),
    );
    let profile = AircraftProfileV2::parse(&value.to_string()).unwrap();
    let again = AircraftProfileV2::parse(&profile.to_json().unwrap()).unwrap();
    for p in [&profile, &again] {
        let c = p.controls();
        for x in [
            c.surface_rate,
            c.elevator_rate.unwrap(),
            c.centering_rate,
            c.elevator_centering_rate.unwrap(),
            c.throttle_rate,
            c.flap_rate,
            c.default_trim,
            c.trim_rate,
            c.approach_pitch_rad,
            c.approach_trim,
            c.approach_throttle,
            c.approach_flaps,
        ] {
            assert_eq!(x.get().to_bits(), 0x3fe0_0000_0000_0001);
        }
        for x in p.camera_eye() {
            assert_eq!(x.get().to_bits(), 0x3fe0_0000_0000_0001);
        }
        assert_eq!(p.model().length_m.get().to_bits(), 0x4020_0000_0000_0001);
        assert_eq!(c.approach_speed_mps.get().to_bits(), 0x4040_0000_0000_0001);
    }
}

#[test]
fn categorical_alias_whitespace_cannot_make_export_unloadable() {
    for (pointer, alias) in [
        ("/engine_sound", "jet"),
        ("/model/forward", "-x"),
        ("/model/up", "+y"),
    ] {
        let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
        assign(&mut value, pointer, serde_json::json!(alias));
        let size = value.to_string().len();
        assign(
            &mut value,
            pointer,
            serde_json::json!(format!("{}{alias}", " ".repeat(MAX_PROFILE_BYTES - size))),
        );
        let text = value.to_string();
        assert_eq!(text.len(), MAX_PROFILE_BYTES);
        let profile = AircraftProfileV2::parse(&text).unwrap();
        let exported = profile.to_json().unwrap();
        assert!(exported.len() < MAX_PROFILE_BYTES);
        assert!(AircraftProfileV2::parse(&exported).is_ok());
    }
}

#[test]
fn maximum_table_and_schedule_shapes_export_inside_the_byte_budget() {
    let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
    let pressure: Vec<_> = (0..16).map(|i| f64::from(i) / 15.0).collect();
    let temperature: Vec<_> = (0..16).map(|i| 0.5 + f64::from(i) / 15.0).collect();
    let mach: Vec<_> = (0..16).map(|i| 0.9 * f64::from(i) / 15.0).collect();
    assign(
        &mut value,
        "/dynamics/thrust/pressure_ratios",
        serde_json::json!(pressure),
    );
    assign(
        &mut value,
        "/dynamics/thrust/temperature_ratios",
        serde_json::json!(temperature),
    );
    assign(&mut value, "/dynamics/thrust/mach", serde_json::json!(mach));
    let cells: Vec<_> = (0..4096)
        .map(|i| {
            if i < 256 {
                serde_json::json!({"idle_n":-0.0,"maximum_dry_n":0.0})
            } else {
                serde_json::json!({"idle_n":-30.400000000000002,"maximum_dry_n":30.400000000000002})
            }
        })
        .collect();
    assign(&mut value, "/dynamics/thrust/cells", Value::Array(cells));
    let aero = value
        .pointer("/dynamics/aero/knots/0/aero")
        .unwrap()
        .clone();
    let knots: Vec<_> = (0..32)
        .map(|i| serde_json::json!({"mach": 0.9*f64::from(i)/31.0,"aero":aero}))
        .collect();
    assign(&mut value, "/dynamics/aero/knots", Value::Array(knots));
    let profile = AircraftProfileV2::parse(&value.to_string()).unwrap();
    let exported = profile.to_json().unwrap();
    assert!(exported.len() < MAX_PROFILE_BYTES);
    let again = AircraftProfileV2::parse(&exported).unwrap();
    assert_eq!(
        profile.configuration().thrust().canonical_bytes(),
        again.configuration().thrust().canonical_bytes()
    );
    assert_eq!(
        profile.configuration().aero().canonical_bytes(),
        again.configuration().aero().canonical_bytes()
    );
}

// Frozen outside this workspace with serde_json 1.0.151 default+std and no
// raw_value. This regression checksum is not an aircraft/replay identity.
fn legacy_numeric_regression_checksum(value: &Value, hash: &mut u64) {
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for &byte in bytes {
            *hash ^= u64::from(byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    match value {
        Value::Number(number) => feed(hash, &number.as_f64().unwrap().to_bits().to_le_bytes()),
        Value::Array(array) => {
            for item in array {
                legacy_numeric_regression_checksum(item, hash);
            }
        }
        Value::Object(map) => {
            for (name, item) in map {
                feed(hash, name.as_bytes());
                legacy_numeric_regression_checksum(item, hash);
            }
        }
        _ => {}
    }
}

#[test]
fn every_bundled_v1_numeric_bit_is_unchanged_with_feature_unification() {
    for (json, expected) in [
        (
            include_str!("../../../assets/aircraft/light_single.json"),
            0xc3f8_7d8d_4462_6953,
        ),
        (
            include_str!("../../../assets/aircraft/swift_sport.json"),
            0xd1c6_9b20_0bc8_a736,
        ),
        (
            include_str!("../../../assets/aircraft/meadow_trainer.json"),
            0xc3f8_7d8d_4462_6953,
        ),
    ] {
        let value: Value = serde_json::from_str(json).unwrap();
        let mut hash = 0xcbf2_9ce4_8422_2325;
        legacy_numeric_regression_checksum(&value, &mut hash);
        assert_eq!(hash, expected);
    }
}

#[test]
fn all_25_scheduled_coefficients_reach_runtime_with_exact_original_bits() {
    use flightsim_sim::aircraft_profile::ExactF64;
    let mut value: Value = serde_json::from_str(EXAMPLE).unwrap();
    let wanted = f64::from_bits(0x3fe0_0000_0000_0001);
    for knot in value
        .pointer_mut("/dynamics/aero/knots")
        .unwrap()
        .as_array_mut()
        .unwrap()
    {
        for coefficient in knot["aero"].as_object_mut().unwrap().values_mut() {
            *coefficient = serde_json::json!(wanted);
        }
    }
    let profile = AircraftProfileV2::parse(&value.to_string()).unwrap();
    let again = AircraftProfileV2::parse(&profile.to_json().unwrap()).unwrap();
    for p in [&profile, &again] {
        for knot in &p.configuration().aero().definition().knots {
            // Serialize actual constructed coefficients through the injective
            // shortest formatter, then inspect each field with the exact decoder.
            let fields: BTreeMap<String, ExactF64> =
                serde_json::from_str(&serde_json::to_string(&knot.aero).unwrap()).unwrap();
            assert_eq!(fields.len(), 25);
            for (name, value) in fields {
                assert_eq!(value.get().to_bits(), wanted.to_bits(), "{name}");
            }
        }
    }
}
