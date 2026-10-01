//! Validate the dynamics embedded in the bundled application profiles without Bevy.

use flightsim_fdm::{AircraftConfig, definition::AircraftDefinition};

fn definition(json: &str) -> AircraftDefinition {
    let profile: serde_json::Value = serde_json::from_str(json).expect("valid profile JSON");
    assert_eq!(profile["version"], 1);
    serde_json::from_value(profile["dynamics"].clone()).expect("supported dynamics schema")
}

#[test]
fn bundled_light_single_preserves_the_existing_dynamics() {
    let bundled = definition(include_str!("../../../assets/aircraft/light_single.json"));
    let config = bundled.to_config().expect("valid bundled Light Single");
    let legacy = AircraftDefinition::from_config(&AircraftConfig::light_single());
    assert_eq!(
        serde_json::to_value(AircraftDefinition::from_config(&config)).unwrap(),
        serde_json::to_value(legacy).unwrap()
    );
}

#[test]
fn bundled_swift_sport_is_valid_and_has_distinct_dynamics() {
    let light = definition(include_str!("../../../assets/aircraft/light_single.json"));
    let swift = definition(include_str!("../../../assets/aircraft/swift_sport.json"));
    swift.to_config().expect("valid bundled Swift Sport");
    assert_ne!(light.mass_kg.to_bits(), swift.mass_kg.to_bits());
    assert_ne!(
        light.inertia_kg_m2.map(f64::to_bits),
        swift.inertia_kg_m2.map(f64::to_bits)
    );
    assert_ne!(light.wing_area_m2.to_bits(), swift.wing_area_m2.to_bits());
    assert_ne!(
        light.max_shaft_power_w.to_bits(),
        swift.max_shaft_power_w.to_bits()
    );
    assert_ne!(
        light.aero.roll_aileron.to_bits(),
        swift.aero.roll_aileron.to_bits()
    );
}
