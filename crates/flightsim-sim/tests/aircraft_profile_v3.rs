//! Shared schema corpus plus physical-bit/export and hostile-input boundaries.
//! Value is used only to synthesize test documents, never in the public loader.
use flightsim_sim::aircraft_profile_v3::{AircraftProfileV3, EngineSound, MAX_PROFILE_BYTES};
use flightsim_sim::{model_identity::ModelIdentity, turboprop_identity::canonical_turboprop_bytes};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const EXAMPLE: &str =
    include_str!("../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json");

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
        "../../../schemas/tests/aircraft-profile-v3-cases.json"
    ))
    .unwrap();
    let mut mismatches = Vec::new();
    for case in &corpus.cases {
        let result = AircraftProfileV3::parse(&materialize(case));
        if result.is_ok() != case.runtime_valid {
            mismatches.push(format!("{}: {result:?}", case.name));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn byte_limit_applies_to_all_entry_points_before_decoding() {
    let mut at_limit = EXAMPLE.as_bytes().to_vec();
    at_limit.resize(MAX_PROFILE_BYTES, b' ');
    assert!(AircraftProfileV3::from_bytes(&at_limit).is_ok());
    let dir = std::env::temp_dir().join(format!(
        "turboprop-profile-bounds-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("profile.json");
    std::fs::write(&file, &at_limit).unwrap();
    assert!(AircraftProfileV3::load(&file).is_ok());
    at_limit.push(b' ');
    std::fs::write(&file, &at_limit).unwrap();
    let load = AircraftProfileV3::load(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    for result in [
        load,
        AircraftProfileV3::from_bytes(&at_limit),
        AircraftProfileV3::parse(std::str::from_utf8(&at_limit).unwrap()),
    ] {
        assert!(result.unwrap_err().0.contains("exceeds 1 MiB"));
    }
    assert!(AircraftProfileV3::from_bytes(&[0xff]).is_err());
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
    let profile = AircraftProfileV3::parse(&value.to_string()).unwrap();
    let again = AircraftProfileV3::parse(&profile.to_json().unwrap()).unwrap();
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
        let profile = AircraftProfileV3::parse(&text).unwrap();
        let exported = profile.to_json().unwrap();
        assert!(exported.len() < MAX_PROFILE_BYTES);
        assert!(AircraftProfileV3::parse(&exported).is_ok());
    }
}

#[test]
fn exact_export_preserves_physical_and_initial_condition_bits() {
    let original = AircraftProfileV3::parse(EXAMPLE).unwrap();
    let exported = original.to_json().unwrap();
    let again = AircraftProfileV3::parse(&exported).unwrap();
    assert_eq!(exported, again.to_json().unwrap());
    assert_eq!(
        canonical_turboprop_bytes(original.configuration()),
        canonical_turboprop_bytes(again.configuration())
    );
    for p in [&original, &again] {
        let cfg = p.configuration();
        assert_eq!(
            cfg.aero().definition().knots[0].aero.pitch_rate_q.to_bits(),
            0xc03e_6666_6666_6667
        );
        assert_eq!(
            cfg.aero().definition().knots[0].aero.yaw_rate_p.to_bits(),
            0x8000_0000_0000_0000
        );
        assert_eq!(
            p.running_start().turbine_fraction().get().to_bits(),
            0x8000_0000_0000_0000
        );
        assert_eq!(
            p.running_start().shaft_speed().get().to_bits(),
            180_f64.to_bits()
        );
        assert_eq!(
            p.running_start().blade_pitch().get().to_bits(),
            0.3_f64.to_bits()
        );
        assert_eq!(p.engine_sound(), EngineSound::Turbine);
        assert_eq!(p.id(), "numerical-turboprop-fixture");
    }
}

#[test]
fn signed_zero_and_subnormal_physical_inputs_are_never_repaired() {
    let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
    for path in [
        "/dynamics/airframe/inertia_kg_m2/3",
        "/dynamics/turbine/cells/0/idle_w",
        "/dynamics/propeller/advance_ratio/0",
        "/dynamics/envelope/mach/0",
    ] {
        assign(&mut v, path, serde_json::json!(-0.0));
    }
    assign(
        &mut v,
        "/dynamics/envelope/maximum_helical_tip_mach",
        serde_json::json!(f64::from_bits(1)),
    );
    let p = AircraftProfileV3::parse(&v.to_string()).unwrap();
    let c = p.configuration();
    for number in [
        c.turbine().definition().cells[0].idle_w,
        c.propeller().definition().advance_ratio[0],
        c.envelope().definition().mach[0],
    ] {
        assert_eq!(number.to_bits(), 0x8000_0000_0000_0000);
    }
    assert_eq!(
        c.envelope().definition().maximum_helical_tip_mach.to_bits(),
        1
    );
    let again = AircraftProfileV3::parse(&p.to_json().unwrap()).unwrap();
    assert_eq!(
        canonical_turboprop_bytes(c),
        canonical_turboprop_bytes(again.configuration())
    );
}

#[test]
fn duplicate_nested_fields_and_trailing_or_deep_values_reject() {
    for (old, new) in [
        ("\"gain\": 0.01", "\"gain\":0.01,\"gain\":0.01"),
        ("\"ct\": 0.05", "\"ct\":0.05,\"ct\":0.05"),
        ("\"idle_w\": 8000", "\"idle_w\":8000,\"idle_w\":8000"),
        (
            "\"turbine_fraction\": -0.0",
            "\"turbine_fraction\":0,\"turbine_fraction\":0",
        ),
    ] {
        assert!(EXAMPLE.contains(old));
        assert!(AircraftProfileV3::parse(&EXAMPLE.replacen(old, new, 1)).is_err());
    }
    assert!(AircraftProfileV3::parse(&format!("{EXAMPLE}{{}}")).is_err());
    let deep = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    assert!(
        AircraftProfileV3::parse(&EXAMPLE.replacen("1043.0", &deep, 1))
            .unwrap_err()
            .0
            .contains("nesting exceeds 128")
    );
    assert!(AircraftProfileV3::load(Path::new("not-a-real-turboprop-file.json")).is_err());
}

#[test]
fn excess_array_rejects_before_parsing_its_hostile_element() {
    for (path, count, value) in [
        (
            "/dynamics/turbine/pressure_ratios",
            16,
            serde_json::json!(1),
        ),
        (
            "/dynamics/turbine/temperature_ratios",
            16,
            serde_json::json!(1),
        ),
        (
            "/dynamics/turbine/cells",
            256,
            serde_json::json!({"idle_w":0,"maximum_w":1}),
        ),
        (
            "/dynamics/propeller/advance_ratio",
            32,
            serde_json::json!(0),
        ),
        (
            "/dynamics/propeller/blade_pitch_rad",
            32,
            serde_json::json!(0),
        ),
        (
            "/dynamics/propeller/cells",
            1024,
            serde_json::json!({"ct":0,"cp":0}),
        ),
    ] {
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        let mut values = vec![value; count];
        values.push(serde_json::json!("HOSTILE_EXCESS"));
        assign(&mut v, path, Value::Array(values));
        let text = v
            .to_string()
            .replace("\"HOSTILE_EXCESS\"", "{this is deliberately invalid JSON}");
        let error = AircraftProfileV3::parse(&text).unwrap_err();
        assert!(
            error.0.contains("profile array exceeds its element limit"),
            "{path}: {error}"
        );
    }
}

#[test]
fn maximum_component_shapes_roundtrip_inside_one_mib() {
    let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
    assign(
        &mut v,
        "/dynamics/turbine/pressure_ratios",
        serde_json::json!(
            (0..16)
                .map(|i| 0.5 + f64::from(i) / 15.0)
                .collect::<Vec<_>>()
        ),
    );
    assign(
        &mut v,
        "/dynamics/turbine/temperature_ratios",
        serde_json::json!(
            (0..16)
                .map(|i| 0.5 + f64::from(i) / 15.0)
                .collect::<Vec<_>>()
        ),
    );
    assign(
        &mut v,
        "/dynamics/turbine/cells",
        serde_json::json!(vec![
            serde_json::json!({"idle_w":-0.0,"maximum_w":300000.0});
            256
        ]),
    );
    assign(
        &mut v,
        "/dynamics/propeller/advance_ratio",
        serde_json::json!(
            (0..32)
                .map(|i| 4.0 * f64::from(i) / 31.0)
                .collect::<Vec<_>>()
        ),
    );
    assign(
        &mut v,
        "/dynamics/propeller/blade_pitch_rad",
        serde_json::json!((0..32).map(|i| f64::from(i) / 31.0).collect::<Vec<_>>()),
    );
    assign(
        &mut v,
        "/dynamics/propeller/cells",
        serde_json::json!(vec![serde_json::json!({"ct":0.01,"cp":0.1}); 1024]),
    );
    let aero = v.pointer("/dynamics/aero/knots/0/aero").unwrap().clone();
    assign(
        &mut v,
        "/dynamics/aero/knots",
        serde_json::json!(
            (0..32)
                .map(|i| serde_json::json!({"mach":0.9*f64::from(i)/31.0,"aero":aero}))
                .collect::<Vec<_>>()
        ),
    );
    let p = AircraftProfileV3::parse(&v.to_string()).unwrap();
    let text = p.to_json().unwrap();
    assert!(text.len() < MAX_PROFILE_BYTES);
    let again = AircraftProfileV3::parse(&text).unwrap();
    assert_eq!(
        canonical_turboprop_bytes(p.configuration()),
        canonical_turboprop_bytes(again.configuration())
    );
}

#[test]
fn profile_admission_does_not_promise_pointwise_operating_support() {
    use flightsim_core::Radians;
    use flightsim_fdm::turboprop::{
        AdvanceRatio, PropellerPowerBound, PropellerQuery, TurbopropEvaluationError,
    };
    let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
    assign(
        &mut v,
        "/dynamics/propeller/advance_ratio",
        serde_json::json!([0, 2]),
    );
    assign(
        &mut v,
        "/dynamics/propeller/blade_pitch_rad",
        serde_json::json!([0.12, 0.7]),
    );
    assign(
        &mut v,
        "/dynamics/propeller/cells",
        serde_json::json!([
            {"ct":0.2,"cp":0.1},{"ct":0.2,"cp":0.1},{"ct":0,"cp":0.1},{"ct":0,"cp":0.1}
        ]),
    );
    let p = AircraftProfileV3::parse(&v.to_string()).unwrap();
    // Independent reviewer fixture: at J=1 CT=.1, CP=.1, but ideal CP is
    // .10600553340847296. Every node and the continuous weaker bound pass.
    assert_eq!(
        p.configuration().propeller().coefficients(PropellerQuery {
            advance_ratio: AdvanceRatio(1.0),
            blade_pitch: Radians(0.3)
        }),
        Err(TurbopropEvaluationError::PropellerPowerBound(
            PropellerPowerBound::BelowIdealDisk
        ))
    );
}

#[test]
fn v2_and_v3_loaders_remain_explicit_and_incompatible() {
    use flightsim_sim::aircraft_profile::AircraftProfileV2;
    let jet = include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json");
    assert!(AircraftProfileV3::parse(jet).is_err());
    assert!(AircraftProfileV2::parse(EXAMPLE).is_err());
    let p = AircraftProfileV3::parse(EXAMPLE).unwrap();
    assert!(!ModelIdentity::for_turboprop(p.configuration()).supported());
}

#[test]
fn near_singular_reduced_tensor_from_original_json_rejects_without_unwinding() {
    // Independent review witness: scalar determinant reassociation previously
    // passed a precheck, then the actual matrix constructor asserted. Keep the
    // original decimals here, outside a test-only Value/f64 conversion too.
    let normalized = EXAMPLE.replace("\r\n", "\n");
    for fixture in [normalized.clone(), normalized.replace('\n', "\r\n")] {
        let fixture = fixture.replace("\r\n", "\n");
        let with_inertia = fixture.replacen(
            "1285.0,\n        1825.0,\n        2667.0,\n        12.5",
            "1,5.763158480729852,6.247241073718394,2.5594038453272185e-08",
            1,
        );
        assert_ne!(with_inertia, fixture, "inertia substitution did not occur");
        let source = with_inertia.replacen(
            "\"rotor_axial_inertia_kg_m2\": 18.0",
            "\"rotor_axial_inertia_kg_m2\": 0.9999999999999999",
            1,
        );
        assert_ne!(source, with_inertia, "rotor substitution did not occur");
        let result = std::panic::catch_unwind(|| AircraftProfileV3::parse(&source));
        assert!(
            result.is_ok(),
            "untrusted profile panicked during physical construction"
        );
        assert!(result.unwrap().is_err());
    }
}
