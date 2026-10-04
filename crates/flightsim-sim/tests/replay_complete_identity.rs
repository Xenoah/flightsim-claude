//! Independent field mutations and golden identities for the frozen and new
//! algorithms. Legacy recordings cannot prove their omitted yaw coefficient.

use flightsim_core::Seconds;
use flightsim_fdm::{
    AircraftConfig, ControlInputs, FDM_MODEL_REVISION, definition::AircraftDefinition,
};
use flightsim_sim::replay::{
    Conditions, CurrentConditions, CurrentRecorder, EnvironmentConditions, Recorder, Recording,
    ReplayError, ReplayFile, aircraft_fingerprint,
    identity::{AircraftCompatibility, AircraftIdentity, RecordedAircraftIdentity},
};
use flightsim_world::global::GLOBAL_TERRAIN_FINGERPRINT;

const LIGHT_LEGACY: u64 = 0x0505_e664_4bb2_9a53;
const SWIFT_LEGACY: u64 = 0x0606_8a31_d11a_75e0;

// Explicit differences from the Light Single fixture, independently transcribed
// from assets/aircraft/swift_sport.json. No new JSON dependency is needed by sim.
fn swift_config() -> AircraftConfig {
    let mut definition = AircraftDefinition::from_config(&AircraftConfig::light_single());
    definition.name = "Swift Sport (generic)".to_owned();
    definition.mass_kg = 750.0;
    definition.inertia_kg_m2 = [800.0, 1200.0, 1700.0, 0.0];
    definition.wing_area_m2 = 12.3;
    definition.wing_span_m = 9.4;
    definition.mean_chord_m = 1.31;
    definition.aero.lift_zero = 0.28;
    definition.aero.lift_alpha = 5.2;
    definition.aero.drag_min = 0.024;
    definition.aero.roll_rate_p = -0.44;
    definition.aero.roll_aileron = 0.075;
    definition.max_shaft_power_w = 134_226.0;
    definition.static_thrust_n = 2600.0;
    for (leg, contact) in definition.landing_gear.iter_mut().zip([
        [1.35, 0.0, 0.85],
        [-0.7, -1.25, 0.85],
        [-0.7, 1.25, 0.85],
    ]) {
        leg.contact_m = contact;
        leg.spring_n_per_m = 100_000.0;
        leg.damping_ns_per_m = 11_000.0;
    }
    definition.to_config().unwrap()
}

fn assert_parameter_changed(before: &AircraftConfig, after: &AircraftConfig, field: &str) {
    let original = AircraftIdentity::for_config(before);
    let changed = AircraftIdentity::for_config(after);
    assert_ne!(original.fingerprint, changed.fingerprint, "omitted {field}");
    let recording = CurrentRecorder::new(CurrentConditions::for_aircraft(
        before,
        EnvironmentConditions::default(),
    ))
    .finish();
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    let restored = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
    assert_eq!(
        restored.check_compatibility_with(before).unwrap(),
        AircraftCompatibility::CompleteMatch
    );
    assert_eq!(
        restored.check_compatibility_with(after).unwrap(),
        AircraftCompatibility::Mismatch,
        "serialized identity accepted changed {field}"
    );

    assert_eq!(
        RecordedAircraftIdentity::Complete(original).verify(after),
        AircraftCompatibility::Mismatch,
        "accepted changed {field}"
    );
}

macro_rules! scalar_mutations {
    ($($name:ident: $($field:ident).+),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let original = AircraftConfig::light_single();
                let mut definition = AircraftDefinition::from_config(&original);
                definition.$($field).+ += 0.0001;
                let changed = definition.to_config().unwrap();
                assert_parameter_changed(&original, &changed, stringify!($($field).+));
            }
        )+
    };
}

scalar_mutations! {
    mass: mass_kg,
    wing_area: wing_area_m2,
    wing_span: wing_span_m,
    mean_chord: mean_chord_m,
    lift_zero: aero.lift_zero,
    lift_alpha: aero.lift_alpha,
    lift_flaps: aero.lift_flaps,
    stall_angle: aero.stall_angle_rad,
    stall_blend_rate: aero.stall_blend_rate,
    drag_min: aero.drag_min,
    oswald_efficiency: aero.oswald_efficiency,
    drag_flaps: aero.drag_flaps,
    side_beta: aero.side_beta,
    side_rudder: aero.side_rudder,
    roll_beta: aero.roll_beta,
    roll_rate_p: aero.roll_rate_p,
    roll_rate_r: aero.roll_rate_r,
    roll_aileron: aero.roll_aileron,
    roll_rudder: aero.roll_rudder,
    pitch_zero: aero.pitch_zero,
    pitch_alpha: aero.pitch_alpha,
    pitch_rate_q: aero.pitch_rate_q,
    pitch_elevator: aero.pitch_elevator,
    pitch_flaps: aero.pitch_flaps,
    yaw_beta: aero.yaw_beta,
    yaw_rate_p: aero.yaw_rate_p,
    yaw_rate_r: aero.yaw_rate_r,
    yaw_aileron: aero.yaw_aileron,
    yaw_rudder: aero.yaw_rudder,
    shaft_power: max_shaft_power_w,
    propeller_efficiency: propeller_efficiency,
    static_thrust: static_thrust_n,
    rolling_friction: rolling_friction,
    braking_friction: braking_friction,
    lateral_friction: lateral_friction,
    friction_transition: friction_transition_mps,
}

#[test]
fn every_independent_inertia_parameter_changes_identity() {
    let original = AircraftConfig::light_single();
    for index in 0..4 {
        let mut definition = AircraftDefinition::from_config(&original);
        definition.inertia_kg_m2[index] += 0.0001;
        assert_parameter_changed(
            &original,
            &definition.to_config().unwrap(),
            &format!("inertia[{index}]"),
        );
    }
}

#[test]
fn every_scalar_of_each_gear_leg_changes_identity() {
    let original = AircraftConfig::light_single();
    for leg in 0..3 {
        for scalar in 0..8 {
            let mut definition = AircraftDefinition::from_config(&original);
            let gear = &mut definition.landing_gear[leg];
            let field = match scalar {
                0..=2 => &mut gear.contact_m[scalar],
                3 => &mut gear.spring_n_per_m,
                4 => &mut gear.damping_ns_per_m,
                5 => &mut gear.max_stroke_m,
                6 => &mut gear.bottom_stop_travel_m,
                7 => &mut gear.max_recoil_mps,
                _ => unreachable!(),
            };
            *field += 0.0001;
            assert_parameter_changed(
                &original,
                &definition.to_config().unwrap(),
                &format!("gear[{leg}][{scalar}]"),
            );
        }
    }
}

#[test]
fn gear_order_is_retained_because_it_can_change_force_summation() {
    let original = AircraftConfig::light_single();
    let mut definition = AircraftDefinition::from_config(&original);
    definition.landing_gear.swap(1, 2);
    assert_parameter_changed(&original, &definition.to_config().unwrap(), "gear order");
}

#[test]
fn golden_current_model_identities_match_independent_python_encoding() {
    // docs/replay-identity.md specifies the canonical bytes. Python struct.pack
    // over the two JSON dynamics fixtures independently produced these values.
    assert_eq!(
        FDM_MODEL_REVISION, 2,
        "update model-law golden evidence deliberately"
    );
    for (config, legacy, complete) in [
        (
            AircraftConfig::light_single(),
            LIGHT_LEGACY,
            0xb7fa_864d_c478_24f7,
        ),
        (swift_config(), SWIFT_LEGACY, 0x1721_6717_4cf9_0012),
    ] {
        assert_eq!(aircraft_fingerprint(&config), legacy);
        let identity = AircraftIdentity::for_config(&config);
        assert_eq!(identity.fingerprint, complete);
        assert_eq!(
            RecordedAircraftIdentity::Complete(identity).verify(&config),
            AircraftCompatibility::CompleteMatch
        );
        let mut changed = config.clone();
        changed.aero.yaw_rate_p += 0.01;
        assert_eq!(aircraft_fingerprint(&changed), legacy);
        assert_parameter_changed(&config, &changed, "legacy omission: yaw_rate_p");
    }
}

#[test]
fn names_neither_change_complete_identity_nor_upgrade_legacy_evidence() {
    let original = AircraftConfig::light_single();
    let identity = AircraftIdentity::for_config(&original);
    let mut renamed = original.clone();
    renamed.name = "custom profile with an arbitrary name".to_owned();
    assert_eq!(AircraftIdentity::for_config(&renamed), identity);
    assert_eq!(
        RecordedAircraftIdentity::LegacyPartial {
            fingerprint: LIGHT_LEGACY
        }
        .verify(&renamed),
        AircraftCompatibility::LegacyPartialMatch
    );
    renamed.aero.yaw_rate_p += 0.01;
    assert_eq!(
        RecordedAircraftIdentity::LegacyPartial {
            fingerprint: LIGHT_LEGACY
        }
        .verify(&renamed),
        AircraftCompatibility::LegacyPartialMatch
    );
}

#[test]
fn exact_bits_are_retained_without_normalizing_signed_zero() {
    let mut config = AircraftConfig::light_single();
    config.aero.yaw_rate_p = 0.0;
    let positive = AircraftIdentity::for_config(&config);
    config.aero.yaw_rate_p = -0.0;
    assert_ne!(
        AircraftIdentity::for_config(&config).fingerprint,
        positive.fingerprint
    );
}

#[test]
fn unsupported_identity_metadata_and_changed_digest_are_mismatches() {
    let config = AircraftConfig::light_single();
    let identity = AircraftIdentity::for_config(&config);
    for changed in [
        AircraftIdentity {
            algorithm: 0,
            ..identity
        },
        AircraftIdentity {
            algorithm: 2,
            ..identity
        },
        AircraftIdentity {
            schema: 0,
            ..identity
        },
        AircraftIdentity {
            schema: 2,
            ..identity
        },
        AircraftIdentity {
            fdm_model_revision: 1,
            ..identity
        },
        AircraftIdentity {
            fdm_model_revision: 3,
            ..identity
        },
        AircraftIdentity {
            fingerprint: identity.fingerprint ^ 1,
            ..identity
        },
    ] {
        assert_eq!(
            RecordedAircraftIdentity::Complete(changed).verify(&config),
            AircraftCompatibility::Mismatch
        );
    }
    // Merely writing a complete digest into a legacy u64 does not change its
    // provenance. There is no heuristic that guesses which algorithm was used.
    assert_eq!(
        RecordedAircraftIdentity::LegacyPartial {
            fingerprint: identity.fingerprint
        }
        .verify(&config),
        AircraftCompatibility::Mismatch
    );
}

fn legacy_bytes(version: u16, fingerprint: u64) -> Vec<u8> {
    let mut bytes = b"FSREPLAY".to_vec();
    bytes.extend_from_slice(&version.to_le_bytes());
    let name = b"arbitrary recorded name";
    bytes.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&fingerprint.to_le_bytes());
    for value in [0.0_f64, 0.0, 1000.0, 0.0, 0.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    if version == 2 {
        bytes.extend_from_slice(&1_u64.to_le_bytes());
        bytes.extend_from_slice(&GLOBAL_TERRAIN_FINGERPRINT.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    for dt in [-0.0_f64, 1.0 / 60.0] {
        for value in [dt, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn independent_v1_v2_bytes_classify_as_partial_without_relabeling() {
    for (config, fingerprint) in [
        (AircraftConfig::light_single(), LIGHT_LEGACY),
        (swift_config(), SWIFT_LEGACY),
    ] {
        for version in [1, 2] {
            let bytes = legacy_bytes(version, fingerprint);
            let recording = Recording::read_from(&mut &bytes[..]).unwrap();
            assert_eq!(
                recording.check_compatibility_with(&config).unwrap(),
                AircraftCompatibility::LegacyPartialMatch
            );
            let mut yaw_changed = config.clone();
            yaw_changed.aero.yaw_rate_p += 0.01;
            assert_eq!(
                recording.check_compatibility_with(&yaw_changed).unwrap(),
                AircraftCompatibility::LegacyPartialMatch
            );
            // The historical API intentionally retains this limited behavior.
            assert!(recording.check_reproducible_with(&yaw_changed).is_ok());
            let mut rewritten = Vec::new();
            recording.write_to(&mut rewritten).unwrap();
            assert_eq!(rewritten, bytes);
        }
    }
}

#[test]
fn earlier_model_and_changed_included_coefficient_are_mismatches() {
    let config = AircraftConfig::light_single();
    for version in [1, 2] {
        let bytes = legacy_bytes(version, 0xe8ca_8e4c_ac33_cd04);
        let recording = Recording::read_from(&mut &bytes[..]).unwrap();
        assert_eq!(
            recording.check_compatibility_with(&config).unwrap(),
            AircraftCompatibility::Mismatch
        );
        assert!(recording.check_reproducible_with(&config).is_err());
        let bytes = legacy_bytes(version, LIGHT_LEGACY);
        let recording = Recording::read_from(&mut &bytes[..]).unwrap();
        let mut changed = config.clone();
        changed.aero.yaw_beta += 0.01;
        assert_eq!(
            recording.check_compatibility_with(&changed).unwrap(),
            AircraftCompatibility::Mismatch
        );
    }
}

#[test]
fn classifier_keeps_numeric_and_world_validation() {
    let config = AircraftConfig::light_single();
    let conditions = Conditions::default().with_aircraft(&config);
    let mut recorder = Recorder::new(conditions.clone());
    recorder.record(Seconds(f64::NAN), ControlInputs::neutral(), None);
    assert!(recorder.finish().check_compatibility_with(&config).is_err());
    let mut conditions = conditions.with_world_climate(true, None);
    conditions.terrain_fingerprint ^= 1;
    assert!(
        Recorder::new(conditions)
            .finish()
            .check_compatibility_with(&config)
            .is_err()
    );
}

#[test]
fn legacy_reader_remains_explicitly_limited_to_v1_v2() {
    let mut bytes = b"FSREPLAY".to_vec();
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    assert!(matches!(
        Recording::read_from(&mut &bytes[..]),
        Err(ReplayError::UnsupportedVersion {
            found: 3,
            expected: 2
        })
    ));
}

#[test]
fn v3_light_and_swift_fixed_step_round_trips_preserve_every_state_and_clock() {
    use flightsim_core::{Attitude, Geodetic, Ned, Radians};
    use flightsim_fdm::{RigidBodyState, Turbulence};
    use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
    use flightsim_sim::{GroundSampler, Simulation};
    use flightsim_world::{MemoryTileSource, Terrain};

    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    );
    for config in [AircraftConfig::light_single(), swift_config()] {
        for modeled in [false, true] {
            let environment = EnvironmentConditions {
                start: initial.geodetic(),
                heading: Radians::ZERO,
                turbulence: Turbulence::moderate(7),
                ..EnvironmentConditions::default()
            };
            let mut conditions = CurrentConditions::for_aircraft(&config, environment);
            if modeled {
                conditions.weather = WeatherSelection::Modeled(
                    WeatherScenario::from_preset(
                        WeatherPreset::Rain,
                        Geodetic::from_degrees(35.55, 139.78, 0.0),
                        42,
                    )
                    .unwrap(),
                );
            }
            let simulation = |state| {
                let mut sim = Simulation::from_state(
                    config.clone(),
                    state,
                    Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
                    GroundSampler::default(),
                );
                sim.set_turbulence(environment.turbulence);
                sim
            };
            let mut live = simulation(initial);
            let mut recorder = CurrentRecorder::new(conditions);
            for _ in 0..120 {
                for render_dt in [0.0, 1.0 / 144.0, 0.025, 1.0 / 60.0] {
                    live.advance_with_controls(Seconds(render_dt), |dt, state| {
                        let phase = f64::from(recorder.frame_count()) * 0.017;
                        let control = ControlInputs::neutral()
                            .with_throttle(0.65 + phase.sin() * 0.1)
                            .with_elevator(0.03 + phase.cos() * 0.025)
                            .with_aileron((phase * 0.7).sin() * 0.015)
                            .with_rudder((phase * 0.5).cos() * 0.01);
                        recorder.record(dt, control, Some(state));
                        control
                    });
                }
            }
            assert!(!live.diverged());
            let original = recorder.finish();
            let mut bytes = Vec::new();
            original.write_to(&mut bytes).unwrap();
            let decoded = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
            assert_eq!(decoded.frames(), original.frames());
            assert_eq!(decoded.keyframes(), original.keyframes());
            assert_eq!(decoded.duration(), original.duration());
            assert_eq!(
                decoded.check_compatibility_with(&config).unwrap(),
                AircraftCompatibility::CompleteMatch
            );
            let mut replay = simulation(decoded.keyframe_exactly_at(0).unwrap().state);
            for (index, frame) in (0_u32..).zip(decoded.frames()) {
                if let Some(keyframe) = decoded.keyframe_exactly_at(index) {
                    assert_eq!(*replay.state(), keyframe.state);
                    assert_eq!(
                        decoded
                            .drift_at(index, replay.state())
                            .unwrap()
                            .get()
                            .to_bits(),
                        0.0_f64.to_bits()
                    );
                }
                replay.advance(frame.frame_time, frame.controls);
            }
            assert_eq!(live.state(), replay.state());
            assert_eq!(live.elapsed(), replay.elapsed());
            assert_eq!(live.log(), replay.log());
            assert_eq!(live.crash(), replay.crash());
            assert_eq!(live.touchdown_count(), replay.touchdown_count());
            let last_frame = u32::try_from(decoded.frames().len()).unwrap();
            assert_eq!(
                decoded.keyframe_at_or_before(last_frame),
                original.keyframes().last().copied()
            );
            // Rain is carried as data only: this does not claim weather rendering.
            assert_eq!(decoded.weather(), original.conditions().weather);
        }
    }
}
