//! Independent wire fixtures and hostile input for explicit replay format dispatch.
//! Fixtures follow the documented layout directly, never the production writer.

use std::io::Cursor;

use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, Turbulence};
use flightsim_sim::replay::{
    Conditions, CurrentConditions, CurrentRecorder, CurrentRecording, EnvironmentConditions,
    MAX_CONDITIONS_BYTES, MAX_FRAMES, MAX_NAME_BYTES, MAX_VISUAL_EPOCH, MAX_WEATHER_BYTES,
    Recorder, Recording, ReplayError, ReplayFile,
    identity::{AircraftCompatibility, AircraftIdentity, RecordedAircraftIdentity},
};
use flightsim_sim::weather::{ModeledFogLayer, WeatherPreset, WeatherScenario, WeatherSelection};
use flightsim_world::{
    ClimateDate, GLOBAL_CLIMATE_FINGERPRINT, global::GLOBAL_TERRAIN_FINGERPRINT,
};

const COMPLETE: u64 = 0xb7fa_864d_c478_24f7;
const LEGACY: u64 = 0x0505_e664_4bb2_9a53;
const IDENTITY: usize = 19; // 14-byte header, u32 name length, one-byte name.
const ENV: usize = IDENTITY + 16;
const WORLD: usize = ENV + 80;
const WEATHER: usize = 151; // 14 + 136 + 1.

fn f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
fn u16_at(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
fn f64_at(bytes: &mut [u8], offset: usize, value: f64) {
    u64_at(bytes, offset, value.to_bits());
}
fn read(mut bytes: &[u8]) -> Result<ReplayFile, ReplayError> {
    ReplayFile::read_from(&mut bytes)
}
fn write(recording: &ReplayFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    bytes
}

fn weather(preset: u16, fog_with_cloud: bool) -> Vec<u8> {
    let (ambient, precip, rate, cloud, fog) = match preset {
        1 => (100_000.0, 0_u16, 0.0, None, None),
        2 => (
            40_000.0,
            0,
            0.0,
            Some((2_u16, 1000.0, 2200.0, 0.65, 500.0)),
            None,
        ),
        3 => (50_000.0, 0, 0.0, None, Some((-500.0, -200.0, 250.0))),
        4 => (
            10_000.0,
            1,
            0.005 / 3600.0,
            Some((1, 100.0, 2100.0, 1.0, 250.0)),
            None,
        ),
        5 => (
            3000.0,
            2,
            0.001 / 3600.0,
            Some((1, -200.0, 1300.0, 1.0, 200.0)),
            None,
        ),
        6 => (
            5000.0,
            1,
            0.025 / 3600.0,
            Some((3, 0.0, 7500.0, 1.0, 150.0)),
            None,
        ),
        0 => (
            10_000.0,
            1,
            0.005 / 3600.0,
            Some((1, 100.0, 2100.0, 1.0, 250.0)),
            if fog_with_cloud {
                Some((-500.0, -200.0, 250.0))
            } else {
                None
            },
        ),
        _ => unreachable!(),
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&preset.to_le_bytes());
    bytes.extend_from_slice(&precip.to_le_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    f64s(&mut bytes, &[0.25, -0.5, -500.0, ambient, rate]);
    let flags = u16::from(cloud.is_some()) | (u16::from(fog.is_some()) << 1);
    bytes.extend_from_slice(&flags.to_le_bytes());
    if let Some((kind, base, top, coverage, visibility)) = cloud {
        bytes.extend_from_slice(&kind.to_le_bytes());
        f64s(&mut bytes, &[base, top, coverage, visibility]);
    }
    if let Some((bottom, top, visibility)) = fog {
        f64s(&mut bytes, &[bottom, top, visibility]);
    }
    bytes
}

fn fixture(weather: &[u8], world: bool, climate: bool) -> Vec<u8> {
    let mut bytes = b"FSREPLAY".to_vec();
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    bytes.extend_from_slice(&(137_u32 + u32::try_from(weather.len()).unwrap()).to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(b'L');
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&COMPLETE.to_le_bytes());
    f64s(&mut bytes, &[0.25, -0.5, 1000.0, 0.125, -0.0, 4.0, 0.5]);
    bytes.extend_from_slice(&0x1234_5678_9abc_def0_u64.to_le_bytes());
    f64s(&mut bytes, &[2_461_317.5, 60.0]);
    let flags = u64::from(world) | (u64::from(climate) << 1);
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&(if world { GLOBAL_TERRAIN_FINGERPRINT } else { 0 }).to_le_bytes());
    f64s(&mut bytes, &[if climate { 0.25 } else { 0.0 }]);
    bytes.extend_from_slice(
        &(if climate {
            GLOBAL_CLIMATE_FINGERPRINT
        } else {
            0
        })
        .to_le_bytes(),
    );
    bytes.extend_from_slice(&u32::try_from(weather.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(weather);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for dt in [-0.0, 1.0 / 120.0, 0.025] {
        f64s(&mut bytes, &[dt, -0.0, -1.0, 1.0, 0.75, 0.25, 0.5]);
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    f64s(
        &mut bytes,
        &[
            6_378_137.0,
            0.0,
            -0.0,
            50.0,
            -0.0,
            0.0,
            -0.0,
            0.0,
            0.0,
            1.0,
            0.01,
            0.02,
            0.03,
        ],
    );
    bytes
}

fn legacy_fixture(version: u16, world: bool) -> Vec<u8> {
    let mut b = b"FSREPLAY".to_vec();
    b.extend_from_slice(&version.to_le_bytes());
    b.extend_from_slice(&1_u32.to_le_bytes());
    b.push(b'L');
    b.extend_from_slice(&LEGACY.to_le_bytes());
    f64s(&mut b, &[0.25, -0.5, 1000.0, 0.125, -0.0, 4.0, 0.5]);
    b.extend_from_slice(&0x1234_5678_9abc_def0_u64.to_le_bytes());
    f64s(&mut b, &[2_461_317.5, 60.0]);
    if version == 2 {
        b.extend_from_slice(&u64::from(world).to_le_bytes());
        b.extend_from_slice(&(if world { GLOBAL_TERRAIN_FINGERPRINT } else { 0 }).to_le_bytes());
        b.extend_from_slice(&[0; 16]);
    }
    b.extend_from_slice(&1_u32.to_le_bytes());
    b.extend_from_slice(&0_u32.to_le_bytes());
    f64s(&mut b, &[-0.0, -0.0, -1.0, 1.0, 0.75, 0.25, 0.5]);
    b
}

#[test]
fn independent_fixtures_resolve_every_field_and_round_trip_exactly() {
    for preset in 0..=6 {
        let block = weather(preset, true);
        assert_eq!(
            block.len(),
            match preset {
                0 => 120,
                1 => 62,
                3 => 86,
                _ => 96,
            }
        );
        for world in [false, true] {
            for climate in [false, true] {
                let bytes = fixture(&block, world, climate);
                let recording = read(&bytes).unwrap();
                assert_eq!(recording.format_version(), 3);
                assert_eq!(recording.aircraft_name(), "L");
                assert_eq!(
                    recording.aircraft_identity(),
                    RecordedAircraftIdentity::Complete(AircraftIdentity {
                        algorithm: 1,
                        schema: 1,
                        fdm_model_revision: 2,
                        fingerprint: COMPLETE,
                    })
                );
                let env = recording.environment();
                assert_eq!(
                    env.start,
                    Geodetic {
                        latitude: Radians(0.25),
                        longitude: Radians(-0.5),
                        altitude: Meters(1000.0)
                    }
                );
                assert_eq!(env.heading, Radians(0.125));
                assert_eq!(env.wind.from.get().to_bits(), (-0.0_f64).to_bits());
                assert_eq!(env.wind.speed, MetersPerSecond(4.0));
                assert_eq!(
                    env.turbulence,
                    Turbulence {
                        intensity: MetersPerSecond(0.5),
                        seed: 0x1234_5678_9abc_def0
                    }
                );
                assert_eq!(env.start_epoch.to_bits(), 2_461_317.5_f64.to_bits());
                assert_eq!(env.time_rate.to_bits(), 60.0_f64.to_bits());
                assert_eq!(env.world_terrain, world);
                assert_eq!(
                    env.climate_date,
                    if climate {
                        ClimateDate::from_annual_phase(0.25)
                    } else {
                        None
                    }
                );
                let WeatherSelection::Modeled(scenario) = recording.weather() else {
                    panic!("missing scenario")
                };
                let p = scenario.parameters();
                assert_eq!(p.preset as u16, preset);
                assert_eq!(p.seed, u64::MAX);
                assert_eq!(p.departure_reference.altitude, Meters(-500.0));
                if preset != 0 {
                    let authored = WeatherScenario::from_preset(
                        WeatherPreset::try_from(preset).unwrap(),
                        p.departure_reference,
                        u64::MAX,
                    )
                    .unwrap();
                    assert_eq!(scenario, authored);
                } else {
                    assert_eq!(p.fog.unwrap().bottom, Meters(-500.0));
                    assert_eq!(p.cloud.unwrap().base, Meters(100.0));
                }
                assert_eq!(
                    recording
                        .check_compatibility_with(&AircraftConfig::light_single())
                        .unwrap(),
                    AircraftCompatibility::CompleteMatch
                );
                assert_eq!(write(&recording), bytes);
            }
        }
    }
}

#[test]
fn absent_weather_and_explicit_clear_are_distinct_and_remain_so() {
    for block in [vec![], weather(1, false)] {
        let bytes = fixture(&block, false, false);
        let recording = read(&bytes).unwrap();
        assert_eq!(
            recording.weather() == WeatherSelection::Legacy,
            block.is_empty()
        );
        assert_eq!(write(&recording), bytes);
    }
}

#[test]
fn legacy_export_preserves_source_version_and_partial_evidence() {
    for (version, world) in [(1, false), (2, false), (2, true)] {
        let bytes = legacy_fixture(version, world);
        let file = read(&bytes).unwrap();
        assert_eq!(file.format_version(), version);
        assert_eq!(write(&file), bytes);
        assert_eq!(
            file.aircraft_identity(),
            RecordedAircraftIdentity::LegacyPartial {
                fingerprint: LEGACY
            }
        );
        assert_eq!(file.weather(), WeatherSelection::Legacy);
        assert_eq!(
            file.check_compatibility_with(&AircraftConfig::light_single())
                .unwrap(),
            AircraftCompatibility::LegacyPartialMatch
        );
        let recording = Recording::read_from(&mut bytes.as_slice()).unwrap();
        let mut historical = Vec::new();
        recording.write_to(&mut historical).unwrap();
        if version == 2 && !world {
            assert_eq!(historical, legacy_fixture(1, false));
        } else {
            assert_eq!(historical, bytes);
        }
        let mut explicit = Vec::new();
        if version == 1 {
            recording.write_v1_to(&mut explicit).unwrap();
        } else {
            recording.write_v2_to(&mut explicit).unwrap();
        }
        assert_eq!(explicit, bytes);
    }
}

#[test]
fn v1_export_cannot_discard_world_data_and_all_validation_precedes_output() {
    let record = Recorder::new(Conditions::default().with_world_climate(true, None)).finish();
    let mut output = vec![0x55];
    assert!(record.write_v1_to(&mut output).is_err());
    let invalid_file = ReplayFile::V1(record);
    assert!(
        invalid_file
            .check_compatibility_with(&AircraftConfig::light_single())
            .is_err()
    );
    assert!(invalid_file.write_to(&mut output).is_err());
    assert_eq!(output, [0x55]);
}

#[test]
fn formats_and_identity_versions_are_never_guessed() {
    for version in [0, 4, u16::MAX] {
        let mut bytes = fixture(&[], false, false);
        u16_at(&mut bytes, 8, version);
        assert!(
            matches!(read(&bytes), Err(ReplayError::UnsupportedVersion { found, expected: 3 }) if found == version)
        );
    }
    for (offset, wide) in [
        (IDENTITY, false),
        (IDENTITY + 2, false),
        (IDENTITY + 4, true),
    ] {
        for value in [0, 3, u16::MAX.into(), u32::MAX] {
            let mut b = fixture(&[], false, false);
            if wide {
                u32_at(&mut b, offset, value);
            } else if let Ok(value) = u16::try_from(value) {
                u16_at(&mut b, offset, value);
            } else {
                continue;
            }
            assert!(read(&b).is_err(), "identity {offset} accepted {value}");
        }
    }
    let mut b = fixture(&[], false, false);
    u64_at(&mut b, IDENTITY + 8, LEGACY);
    assert_eq!(
        read(&b)
            .unwrap()
            .check_compatibility_with(&AircraftConfig::light_single())
            .unwrap(),
        AircraftCompatibility::Mismatch
    );
    let b = fixture(&[], false, false);
    assert!(Recording::read_from(&mut b.as_slice()).is_err());
    assert!(CurrentRecording::read_from(&mut legacy_fixture(2, false).as_slice()).is_err());
}

#[test]
fn all_fixture_prefixes_are_rejected_without_panics() {
    for block in [
        vec![],
        weather(1, false),
        weather(2, false),
        weather(3, false),
        weather(0, true),
    ] {
        let b = fixture(&block, true, true);
        for end in 0..b.len() {
            assert!(
                read(&b[..end]).is_err(),
                "accepted prefix {end}/{}",
                b.len()
            );
        }
    }
}

#[test]
fn envelope_length_limits_and_exact_consumption_precede_frame_counts() {
    for (offset, values) in [
        (
            10,
            vec![
                0,
                1,
                135,
                136,
                138,
                MAX_CONDITIONS_BYTES,
                MAX_CONDITIONS_BYTES + 1,
                u32::MAX,
            ],
        ),
        (14, vec![2, MAX_NAME_BYTES, MAX_NAME_BYTES + 1, u32::MAX]),
        (
            WEATHER - 4,
            vec![
                1,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                61,
                62,
                MAX_WEATHER_BYTES,
                MAX_WEATHER_BYTES + 1,
                u32::MAX,
            ],
        ),
    ] {
        for value in values {
            let mut b = fixture(&[], false, false);
            u32_at(&mut b, offset, value);
            let mut cursor = Cursor::new(&b);
            assert!(
                ReplayFile::read_from(&mut cursor).is_err(),
                "offset {offset} accepted {value}"
            );
            assert!(
                cursor.position() <= WEATHER as u64,
                "read frame counts for invalid envelope"
            );
        }
    }
    // Declared C and W agree, but unsupported lengths/layer flags may not read
    // even one byte of the frame counts appended after the bounded block.
    for w in [1_u32, 7, 8, 61, 63, 85, 87, 95, 97, 119, 121, 512] {
        let mut b = fixture(&vec![0; w as usize], false, false);
        u16_at(&mut b, WEATHER, 1);
        let mut cursor = Cursor::new(&b);
        assert!(ReplayFile::read_from(&mut cursor).is_err());
        assert!(cursor.position() <= u64::from(151 + w));
    }
    let mut b = fixture(&weather(1, false), false, false);
    // Extend C/W with a byte between the typed payload and the frame counts.
    b.insert(WEATHER + 62, 0);
    u32_at(&mut b, 10, 200);
    u32_at(&mut b, WEATHER - 4, 63);
    assert!(read(&b).is_err());
}

#[test]
fn names_have_exact_utf8_and_byte_bounds() {
    for name in ["x".repeat(256), "飛".repeat(85), String::new()] {
        let mut b = fixture(&[], false, false);
        b.splice(18..19, name.as_bytes().iter().copied());
        u32_at(&mut b, 14, u32::try_from(name.len()).unwrap());
        u32_at(&mut b, 10, 136 + u32::try_from(name.len()).unwrap());
        let file = read(&b).unwrap();
        assert_eq!(file.aircraft_name(), name);
        assert_eq!(write(&file), b);
    }
    let mut b = fixture(&[], false, false);
    b[18] = 0xff;
    assert!(matches!(read(&b), Err(ReplayError::InvalidName)));
}

#[test]
fn weather_tags_revisions_reserved_bits_and_exact_layer_sizes_are_checked() {
    for (offset, values) in [
        (0, vec![0, 2, u16::MAX]),
        (2, vec![0, 2, u16::MAX]),
        (8, vec![7, u16::MAX]),
        (10, vec![3, u16::MAX]),
        (60, vec![0, 1, 2, 4, u16::MAX]),
        (62, vec![0, 4, u16::MAX]),
    ] {
        for value in values {
            let mut b = fixture(&weather(0, true), false, false);
            u16_at(&mut b, WEATHER + offset, value);
            assert!(read(&b).is_err(), "weather tag {offset} accepted {value}");
        }
    }
    for revision in [0, 2, u32::MAX] {
        let mut b = fixture(&weather(0, true), false, false);
        u32_at(&mut b, WEATHER + 4, revision);
        assert!(read(&b).is_err());
    }
    for (preset, flags) in [(1, 1), (1, 2), (1, 3), (2, 0), (2, 2), (3, 1), (3, 3)] {
        let mut b = fixture(&weather(preset, false), false, false);
        u16_at(&mut b, WEATHER + 60, flags);
        let mut cursor = Cursor::new(&b);
        assert!(ReplayFile::read_from(&mut cursor).is_err());
        assert_eq!(
            cursor.position(),
            (WEATHER + 62) as u64,
            "read variable layers before validating their exact length"
        );
    }
}

#[test]
fn every_weather_scalar_rejects_nonfinite_and_out_of_bounds_values() {
    let fields = [
        (20, -2.0, 2.0),
        (28, -4.0, 4.0),
        (36, -1001.0, 10_001.0),
        (44, 9.0, 200_001.0),
        (52, -0.001, 0.301 / 3600.0),
        (64, -1001.0, 30_001.0),
        (72, -1001.0, 30_001.0),
        (80, 0.0, 1.001),
        (88, 9.0, 200_001.0),
        (96, -1001.0, 15_001.0),
        (104, -1001.0, 15_001.0),
        (112, 9.0, 200_001.0),
    ];
    for (offset, low, high) in fields {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, low, high] {
            let mut b = fixture(&weather(0, true), false, false);
            f64_at(&mut b, WEATHER + offset, value);
            assert!(
                read(&b).is_err(),
                "weather scalar {offset} accepted {value}"
            );
        }
    }
    for (offset, value) in [
        (72, 100.0),
        (72, 20_101.0),
        (104, -500.0),
        (104, 4501.0),
        (52, 0.0),
    ] {
        let mut b = fixture(&weather(0, true), false, false);
        f64_at(&mut b, WEATHER + offset, value);
        assert!(
            read(&b).is_err(),
            "weather cross-field {offset} accepted {value}"
        );
    }
    let mut b = fixture(&weather(1, false), false, false);
    f64_at(&mut b, WEATHER + 52, 0.001 / 3600.0);
    assert!(read(&b).is_err());
    for preset in 1..=6 {
        let mut b = fixture(&weather(preset, false), false, false);
        f64_at(&mut b, WEATHER + 44, 1001.0);
        assert!(read(&b).is_err(), "accepted mislabeled preset {preset}");
        u16_at(&mut b, WEATHER + 8, 0);
        assert!(read(&b).is_ok(), "explicit Custom override rejected");
    }
}

#[test]
fn environmental_world_and_clock_rules_are_unchanged() {
    for offset in [0, 8, 16, 24, 32, 40, 48, 64, 72] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut b = fixture(&[], false, false);
            f64_at(&mut b, ENV + offset, value);
            assert!(read(&b).is_err(), "environment {offset} accepted {value}");
        }
    }
    for (offset, value) in [
        (0, 2.0),
        (8, 4.0),
        (16, f64::MAX),
        (40, -1.0),
        (48, -1.0),
        (40, f64::MAX),
        (48, f64::MAX),
        (64, -1.0),
        (64, MAX_VISUAL_EPOCH + 1.0),
        (72, -1.0),
        (72, f64::MAX),
    ] {
        let mut b = fixture(&[], false, false);
        f64_at(&mut b, ENV + offset, value);
        assert!(read(&b).is_err(), "environment {offset} accepted {value}");
    }
    for flags in [4, 8, u64::MAX] {
        let mut b = fixture(&[], true, true);
        u64_at(&mut b, WORLD, flags);
        assert!(read(&b).is_err());
    }
    for offset in [8, 24] {
        let mut b = fixture(&[], true, true);
        u64_at(&mut b, WORLD + offset, 0);
        assert!(read(&b).is_err());
        let mut b = fixture(&[], false, false);
        u64_at(&mut b, WORLD + offset, 1);
        assert!(read(&b).is_err());
        let mut b = fixture(&[], true, true);
        u64_at(&mut b, WORLD + offset, 1);
        assert!(
            read(&b)
                .unwrap()
                .check_compatibility_with(&AircraftConfig::light_single())
                .is_err()
        );
    }
    for enabled in [false, true] {
        for phase in [-1.0, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut b = fixture(&[], false, enabled);
            f64_at(&mut b, WORLD + 16, phase);
            assert!(read(&b).is_err());
        }
    }
    let mut b = fixture(&[], false, false);
    f64_at(&mut b, WORLD + 16, -0.0);
    assert!(read(&b).is_err());
    let mut b = fixture(&[], false, false);
    f64_at(&mut b, ENV + 64, MAX_VISUAL_EPOCH);
    f64_at(&mut b, WEATHER + 8, 86_400.0);
    assert!(read(&b).is_err());
}

#[test]
fn frame_keyframe_control_and_duration_boundaries_are_unchanged() {
    for (offset, value) in [
        (WEATHER, MAX_FRAMES + 1),
        (WEATHER, u32::MAX),
        (WEATHER + 4, 2),
        (WEATHER + 4, u32::MAX),
    ] {
        let mut b = fixture(&[], false, false);
        u32_at(&mut b, offset, value);
        assert!(matches!(read(&b), Err(ReplayError::TooLarge { .. })));
    }
    for index in 0..7 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.001] {
            let mut b = fixture(&[], false, false);
            f64_at(&mut b, WEATHER + 8 + index * 8, value);
            assert!(read(&b).is_err());
        }
        if index > 0 {
            let mut b = fixture(&[], false, false);
            f64_at(&mut b, WEATHER + 8 + index * 8, 1.001);
            assert!(read(&b).is_err());
        }
    }
    for index in 4..7 {
        let mut b = fixture(&[], false, false);
        f64_at(&mut b, WEATHER + 8 + index * 8, -0.001);
        assert!(read(&b).is_err());
    }
    let key = WEATHER + 8 + 3 * 56;
    for index in 0..13 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let mut b = fixture(&[], false, false);
            f64_at(&mut b, key + 4 + index * 8, value);
            assert!(read(&b).is_err());
        }
    }
    for frame in [3, u32::MAX] {
        let mut b = fixture(&[], false, false);
        u32_at(&mut b, key, frame);
        assert!(matches!(read(&b), Err(ReplayError::InvalidKeyframe { .. })));
    }
    let mut b = fixture(&[], false, false);
    f64_at(&mut b, key + 4 + 9 * 8, 2.0);
    assert!(read(&b).is_err());
    let mut b = fixture(&[], false, false);
    f64_at(&mut b, ENV + 72, 0.0);
    f64_at(&mut b, WEATHER + 8, f64::MAX);
    f64_at(&mut b, WEATHER + 8 + 56, f64::MAX);
    assert!(read(&b).is_err());
}

#[test]
fn current_writer_validates_in_memory_metadata_environment_and_frames_before_header() {
    let base = CurrentConditions::for_aircraft(
        &AircraftConfig::light_single(),
        EnvironmentConditions::default(),
    );
    let mut cases = Vec::new();
    for identity in [
        AircraftIdentity {
            algorithm: 0,
            ..base.aircraft_identity
        },
        AircraftIdentity {
            schema: 2,
            ..base.aircraft_identity
        },
        AircraftIdentity {
            fdm_model_revision: 1,
            ..base.aircraft_identity
        },
    ] {
        let mut c = base.clone();
        c.aircraft_identity = identity;
        cases.push(c);
    }
    let mut c = base.clone();
    c.aircraft_name = "x".repeat(257);
    cases.push(c);
    let mut c = base.clone();
    c.environment.wind.speed = MetersPerSecond(-1.0);
    cases.push(c);
    let mut c = base.clone();
    c.environment.start_epoch = f64::NAN;
    cases.push(c);
    let mut c = base.clone();
    c.environment.terrain_fingerprint = 1;
    cases.push(c);
    let mut c = base.clone();
    c.environment.climate_date = ClimateDate::from_month(1);
    cases.push(c);
    for conditions in cases {
        let recording = CurrentRecorder::new(conditions).finish();
        let mut bytes = vec![0x55];
        assert!(recording.write_to(&mut bytes).is_err());
        assert_eq!(bytes, [0x55]);
    }
    for dt in [f64::NAN, -1.0, f64::INFINITY] {
        let mut recorder = CurrentRecorder::new(base.clone());
        recorder.record(Seconds(dt), ControlInputs::neutral(), None);
        let mut bytes = vec![0x55];
        assert!(recorder.finish().write_to(&mut bytes).is_err());
        assert_eq!(bytes, [0x55]);
    }
    // No caller can put an unvalidated WeatherParameters inside CurrentConditions:
    // WeatherScenario has private fields and is constructible only through validation.
    let mut invalid = WeatherScenario::from_preset(
        WeatherPreset::Clear,
        Geodetic::from_degrees(0.0, 0.0, 0.0),
        0,
    )
    .unwrap()
    .parameters();
    invalid.ambient_visibility = Meters(f64::NAN);
    assert!(WeatherScenario::try_from(invalid).is_err());
}

#[test]
fn current_recorder_captures_only_complete_identity_and_unchanged_frame_bits() {
    let config = AircraftConfig::light_single();
    let mut conditions = CurrentConditions::for_aircraft(&config, EnvironmentConditions::default());
    assert_eq!(conditions.aircraft_identity.fingerprint, COMPLETE);
    assert_eq!(conditions.weather, WeatherSelection::Legacy);
    let mut p = WeatherScenario::from_preset(
        WeatherPreset::Clear,
        Geodetic::from_degrees(0.0, 0.0, -0.0),
        0,
    )
    .unwrap()
    .parameters();
    p.departure_reference.latitude = Radians(-0.0);
    p.precipitation_rate.0 = -0.0;
    conditions.weather = WeatherSelection::Modeled(WeatherScenario::try_from(p).unwrap());
    let mut recorder = CurrentRecorder::new(conditions);
    for dt in [-0.0, f64::from_bits(1), 0.01] {
        recorder.record(Seconds(dt), ControlInputs::neutral(), None);
    }
    assert_eq!(recorder.frame_count(), 3);
    assert!(!recorder.is_full());
    let recording = ReplayFile::V3(recorder.finish());
    let bytes = write(&recording);
    let restored = read(&bytes).unwrap();
    assert_eq!(write(&restored), bytes);
    assert_eq!(restored, recording);
    let mut changed = config;
    changed.aero.yaw_rate_p += 0.01;
    assert_eq!(
        restored.check_compatibility_with(&changed).unwrap(),
        AircraftCompatibility::Mismatch
    );
    let WeatherSelection::Modeled(scenario) = restored.weather() else {
        panic!()
    };
    assert_eq!(
        scenario.parameters().precipitation_rate.0.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        scenario
            .parameters()
            .departure_reference
            .latitude
            .get()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn reader_consumes_exactly_one_recording_from_an_outer_stream() {
    let mut b = fixture(&weather(0, true), false, false);
    let len = b.len();
    b.extend_from_slice(&[0x55; 12]);
    let mut cursor = Cursor::new(&b);
    ReplayFile::read_from(&mut cursor).unwrap();
    assert_eq!(cursor.position(), u64::try_from(len).unwrap());
}

#[test]
fn custom_both_layer_writer_uses_the_independent_120_byte_contract() {
    let reference = Geodetic {
        latitude: Radians(0.25),
        longitude: Radians(-0.5),
        altitude: Meters(-500.0),
    };
    let mut p = WeatherScenario::from_preset(WeatherPreset::Rain, reference, u64::MAX)
        .unwrap()
        .parameters();
    p.preset = WeatherPreset::Custom;
    p.fog = Some(ModeledFogLayer {
        bottom: Meters(-500.0),
        top: Meters(-200.0),
        visibility: Meters(250.0),
    });
    let mut c = CurrentConditions::for_aircraft(
        &AircraftConfig::light_single(),
        EnvironmentConditions::default(),
    );
    c.weather = WeatherSelection::Modeled(WeatherScenario::try_from(p).unwrap());
    c.aircraft_name = "L".to_owned();
    let mut b = Vec::new();
    CurrentRecorder::new(c).finish().write_to(&mut b).unwrap();
    assert_eq!(&b[WEATHER..WEATHER + 120], weather(0, true));
}

#[test]
fn checked_in_python_goldens_match_independent_rust_fixtures() {
    for (golden, expected) in [
        (
            include_bytes!("fixtures/legacy_v1.fsreplay").as_slice(),
            legacy_fixture(1, false),
        ),
        (
            include_bytes!("fixtures/legacy_v2_disabled.fsreplay").as_slice(),
            legacy_fixture(2, false),
        ),
        (
            include_bytes!("fixtures/legacy_v2_world.fsreplay").as_slice(),
            legacy_fixture(2, true),
        ),
        (
            include_bytes!("fixtures/v3_legacy_weather.fsreplay").as_slice(),
            fixture(&[], true, true),
        ),
        (
            include_bytes!("fixtures/v3_custom_both.fsreplay").as_slice(),
            fixture(&weather(0, true), true, true),
        ),
        (
            include_bytes!("fixtures/v3_clear.fsreplay").as_slice(),
            fixture(&weather(1, false), true, true),
        ),
        (
            include_bytes!("fixtures/v3_cloud.fsreplay").as_slice(),
            fixture(&weather(2, false), true, true),
        ),
        (
            include_bytes!("fixtures/v3_fog.fsreplay").as_slice(),
            fixture(&weather(3, false), true, true),
        ),
        (
            include_bytes!("fixtures/v3_rain.fsreplay").as_slice(),
            fixture(&weather(4, false), true, true),
        ),
        (
            include_bytes!("fixtures/v3_snow.fsreplay").as_slice(),
            fixture(&weather(5, false), true, true),
        ),
        (
            include_bytes!("fixtures/v3_storm.fsreplay").as_slice(),
            fixture(&weather(6, false), true, true),
        ),
    ] {
        assert_eq!(golden, expected);
        assert_eq!(write(&read(golden).unwrap()), golden);
    }
}
