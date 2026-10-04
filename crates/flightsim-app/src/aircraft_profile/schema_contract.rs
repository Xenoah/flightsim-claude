//! Shared public-schema examples and boundary corpus against the real loader.
use super::aircraft_profile::AircraftProfile;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
struct Corpus {
    base: String,
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
    raw_replace: Option<[String; 2]>,
    pad_to_bytes: Option<usize>,
}

fn materialize(base: &Value, case: &Case) -> String {
    let mut value = base.clone();
    for (pointer, replacement) in &case.set {
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let target = value.pointer_mut(parent).unwrap();
        if let Some(array) = target.as_array_mut() {
            array[key.parse::<usize>().unwrap()] = replacement.clone();
        } else {
            target
                .as_object_mut()
                .unwrap()
                .insert(key.to_owned(), replacement.clone());
        }
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
fn shared_schema_corpus_matches_runtime_loading_and_rejection() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../../../schemas/tests/aircraft-profile-v1-cases.json"
    ))
    .unwrap();
    let base: Value =
        serde_json::from_slice(&std::fs::read(root.join(corpus.base)).unwrap()).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "flightsim-profile-schema-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("case.json");
    let mut mismatches = Vec::new();
    for case in &corpus.cases {
        std::fs::write(&file, materialize(&base, case)).unwrap();
        let result = AircraftProfile::load(file.to_str().unwrap());
        if result.is_ok() != case.runtime_valid {
            mismatches.push(format!("{}: {result:?}", case.name));
        }
    }
    // Clean up before reporting fixture failures as well as on success.
    std::fs::remove_dir_all(directory).unwrap();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn documented_examples_load_and_optional_pitch_values_preserve_defaults() {
    let examples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/aircraft-profiles");
    let load_example =
        |name: &str| AircraftProfile::load(examples.join(name).to_str().unwrap()).unwrap();
    let required = load_example("required-only.json");
    let explicit = load_example("explicit-pitch-controls.json");
    assert_eq!(required.controls.elevator_rate, None);
    assert_eq!(required.controls.elevator_centering_rate, None);
    for approach in [false, true] {
        let mut inherited = required.pilot_controls(approach);
        let mut specified = explicit.pilot_controls(approach);
        for pitch_up in [true, true, false, false] {
            let keys = flightsim_input::PilotKeys {
                pitch_up,
                ..Default::default()
            };
            inherited.update(flightsim_core::Seconds(0.1), keys);
            specified.update(flightsim_core::Seconds(0.1), keys);
            assert_eq!(
                inherited.to_control_inputs().elevator().to_bits(),
                specified.to_control_inputs().elevator().to_bits()
            );
        }
    }
}
