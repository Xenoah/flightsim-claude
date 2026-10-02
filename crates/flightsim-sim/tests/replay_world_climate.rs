//! Version-2 world/climate extension and version-1 compatibility boundaries.

use flightsim_core::Seconds;
use flightsim_fdm::{AircraftConfig, ControlInputs};
use flightsim_sim::replay::{Conditions, Recorder, Recording, ReplayError};
use flightsim_world::global::GLOBAL_TERRAIN_FINGERPRINT;
use flightsim_world::{ClimateDate, GLOBAL_CLIMATE_FINGERPRINT};

const EXTENSION: usize = 102; // Empty name; immediately after v1 time_rate.

fn bytes(world: bool, climate: Option<ClimateDate>) -> Vec<u8> {
    let mut recorder = Recorder::new(Conditions::default().with_world_climate(world, climate));
    recorder.record(Seconds(0.01), ControlInputs::neutral(), None);
    let mut bytes = Vec::new();
    recorder.finish().write_to(&mut bytes).unwrap();
    bytes
}

fn read(bytes: &[u8]) -> Result<Recording, ReplayError> {
    Recording::read_from(&mut &bytes[..])
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn legacy_no_world_no_climate_stays_byte_identical_v1() {
    let bytes = bytes(false, None);
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 1);
    let recording = read(&bytes).unwrap();
    let conditions = recording.conditions();
    assert!(!conditions.world_terrain);
    assert_eq!(conditions.terrain_fingerprint, 0);
    assert!(conditions.climate_date.is_none());
    assert_eq!(conditions.climate_fingerprint, 0);
    let mut round_trip = Vec::new();
    recording.write_to(&mut round_trip).unwrap();
    assert_eq!(bytes, round_trip);
}

#[test]
fn both_optional_settings_round_trip_independently() {
    for world in [false, true] {
        for climate in [None, ClimateDate::from_month(1), ClimateDate::from_month(7)] {
            let bytes = bytes(world, climate);
            let recording = read(&bytes).unwrap();
            assert_eq!(recording.conditions().world_terrain, world);
            assert_eq!(recording.conditions().climate_date, climate);
            if world {
                assert_eq!(
                    recording.conditions().terrain_fingerprint,
                    GLOBAL_TERRAIN_FINGERPRINT
                );
            }
            if climate.is_some() {
                assert_eq!(
                    recording.conditions().climate_fingerprint,
                    GLOBAL_CLIMATE_FINGERPRINT
                );
            }
            let mut restored = Vec::new();
            recording.write_to(&mut restored).unwrap();
            assert_eq!(bytes, restored);
        }
    }
}

#[test]
fn extension_is_exactly_32_bytes_and_rejects_truncation() {
    let legacy = bytes(false, None);
    let extended = bytes(true, ClimateDate::from_month(7));
    assert_eq!(extended.len(), legacy.len() + 32);
    for length in 0..extended.len() {
        assert!(
            read(&extended[..length]).is_err(),
            "accepted truncation at {length}"
        );
    }
}

#[test]
fn unknown_flags_invalid_phases_and_inconsistent_presence_are_rejected() {
    for flags in [4, 8, u64::MAX] {
        let mut b = bytes(true, ClimateDate::from_month(7));
        put_u64(&mut b, EXTENSION, flags);
        assert!(read(&b).is_err());
    }
    for phase in [-0.001, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut b = bytes(true, ClimateDate::from_month(7));
        put_u64(&mut b, EXTENSION + 16, phase.to_bits());
        assert!(read(&b).is_err(), "accepted climate phase {phase}");
    }
    for field in [EXTENSION + 8, EXTENSION + 24] {
        let mut b = bytes(true, ClimateDate::from_month(7));
        put_u64(&mut b, field, 0);
        assert!(read(&b).is_err());
    }
    for phase in [0.5_f64, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut disabled = bytes(true, None);
        put_u64(&mut disabled, EXTENSION + 16, phase.to_bits());
        assert!(
            read(&disabled).is_err(),
            "disabled climate accepted phase {phase}"
        );
    }
    let mut disabled = bytes(true, None);
    put_u64(&mut disabled, EXTENSION + 24, GLOBAL_CLIMATE_FINGERPRINT);
    assert!(
        read(&disabled).is_err(),
        "disabled climate accepted a dataset identity"
    );
    let mut disabled = bytes(false, ClimateDate::from_month(7));
    put_u64(&mut disabled, EXTENSION + 8, GLOBAL_TERRAIN_FINGERPRINT);
    assert!(read(&disabled).is_err());
}

#[test]
fn nonmatching_dataset_identity_is_named_as_reproduction_error() {
    let config = AircraftConfig::light_single();
    for terrain in [true, false] {
        let mut conditions = Conditions::default()
            .with_aircraft(&config)
            .with_world_climate(true, ClimateDate::from_month(7));
        if terrain {
            conditions.terrain_fingerprint ^= 1;
        } else {
            conditions.climate_fingerprint ^= 1;
        }
        let recording = Recorder::new(conditions).finish();
        match recording.check_reproducible_with(&config) {
            Err(ReplayError::ConditionsMismatch { detail }) => {
                assert!(detail.contains(if terrain { "terrain" } else { "climate" }));
            }
            other => panic!("expected dataset mismatch, got {other:?}"),
        }
    }
}

#[test]
fn invalid_writer_conditions_fail_before_writing_any_bytes() {
    for conditions in [
        Conditions {
            world_terrain: true,
            ..Conditions::default()
        },
        Conditions {
            terrain_fingerprint: 1,
            ..Conditions::default()
        },
        Conditions {
            climate_date: ClimateDate::from_month(7),
            ..Conditions::default()
        },
        Conditions {
            climate_fingerprint: 1,
            ..Conditions::default()
        },
    ] {
        let mut output = Vec::new();
        assert!(
            Recorder::new(conditions)
                .finish()
                .write_to(&mut output)
                .is_err()
        );
        assert!(output.is_empty());
    }
}
