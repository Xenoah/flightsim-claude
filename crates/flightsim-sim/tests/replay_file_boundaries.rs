//! Format-v1 numeric trust boundaries, tested using independently encoded bytes.
//!
//! Malformed scalar values must be rejected before controls can be clamped or
//! state restored. The fixture deliberately does not use the production writer.

use flightsim_core::{Attitude, Geodetic, Ned, Radians, Seconds};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use flightsim_sim::replay::{
    Conditions, MAX_VISUAL_EPOCH, Player, Recorder, Recording, ReplayError,
};

const CONDITIONS: usize = 22; // magic, version, empty name length, fingerprint
const FRAMES: usize = CONDITIONS + 80 + 8;
const FRAME_BYTES: usize = 56;
const KEYFRAME_BYTES: usize = 108;

fn fixture(frame_times: &[f64], keyframes: bool) -> Vec<u8> {
    let mut bytes = b"FSREPLAY".to_vec();
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    // lat, lon, alt, heading, wind direction/speed, turbulence intensity.
    for value in [0.0_f64, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.extend_from_slice(&2_461_212.5_f64.to_le_bytes());
    bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(frame_times.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u32::from(keyframes).to_le_bytes());
    for &dt in frame_times {
        bytes.extend_from_slice(&dt.to_le_bytes());
        for value in [0.1_f64, -0.2, 0.3, 0.4, 0.5, 0.6] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    if keyframes {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        // ECEF at equator, velocity, unit attitude, angular velocity.
        for value in [
            6_378_237.0_f64,
            0.0,
            0.0,
            10.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn replace(bytes: &mut [u8], offset: usize, value: f64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read(bytes: &[u8]) -> Result<Recording, ReplayError> {
    Recording::read_from(&mut &bytes[..])
}

#[test]
fn rejects_nonfinite_and_negative_stored_durations() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
        assert!(
            read(&fixture(&[value], false)).is_err(),
            "accepted dt {value}"
        );
    }
}

#[test]
fn rejects_each_nonfinite_keyframe_scalar() {
    let original = fixture(&[0.0], true);
    let state = original.len() - KEYFRAME_BYTES + 4;
    for index in 0..13 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = original.clone();
            replace(&mut bytes, state + index * 8, value);
            assert!(read(&bytes).is_err(), "accepted state[{index}]={value}");
        }
    }
}

#[test]
fn rejects_nonfinite_conditions() {
    for index in [0, 1, 2, 3, 4, 5, 6, 8, 9] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = fixture(&[0.0], true);
            replace(&mut bytes, CONDITIONS + index * 8, value);
            assert!(read(&bytes).is_err(), "accepted condition[{index}]={value}");
        }
    }
}

#[test]
fn rejects_corrupt_control_scalars_instead_of_sanitizing_the_flight() {
    for index in 1..7 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -2.0, 2.0] {
            let mut bytes = fixture(&[0.01], false);
            replace(&mut bytes, FRAMES + index * 8, value);
            assert!(read(&bytes).is_err(), "accepted control[{index}]={value}");
        }
    }
}

#[test]
fn rejects_duration_sum_overflow() {
    let mut bytes = fixture(&[f64::MAX, f64::MAX], false);
    // A paused visual clock isolates the overflow in the duration sum itself.
    replace(&mut bytes, CONDITIONS + 9 * 8, 0.0);
    assert!(read(&bytes).is_err());
}

#[test]
fn independent_valid_v1_bytes_round_trip_without_changes() {
    let bytes = fixture(&[0.0, 0.01, 0.02], true);
    assert_eq!(bytes.len(), FRAMES + 3 * FRAME_BYTES + KEYFRAME_BYTES);
    let recording = read(&bytes).unwrap();
    let mut written = Vec::new();
    recording.write_to(&mut written).unwrap();
    assert_eq!(written, bytes);
}

#[test]
fn rejects_invalid_condition_domains_and_finite_clock_overflow() {
    for (index, value) in [
        (0, 2.0),
        (0, -2.0),
        (1, 4.0),
        (1, -4.0),
        (5, -1.0),
        (6, -1.0),
        (8, -1.0),
        (8, f64::MAX),
        (9, -1.0),
    ] {
        let mut bytes = fixture(&[0.01], true);
        replace(&mut bytes, CONDITIONS + index * 8, value);
        assert!(read(&bytes).is_err(), "accepted condition[{index}]={value}");
    }
    let mut bytes = fixture(&[2.0], false);
    replace(&mut bytes, CONDITIONS + 9 * 8, f64::MAX);
    assert!(read(&bytes).is_err(), "visual elapsed product overflows");
}

#[test]
fn rejects_zero_nonunit_and_overflowing_quaternions_without_repair() {
    for quaternion in [
        [0.0_f64; 4],
        [10.0, 0.0, 0.0, 0.0],
        [f64::MAX, 0.0, 0.0, 1.0],
    ] {
        let mut bytes = fixture(&[0.0], true);
        let orientation = bytes.len() - KEYFRAME_BYTES + 4 + 6 * 8;
        for (index, value) in quaternion.into_iter().enumerate() {
            replace(&mut bytes, orientation + index * 8, value);
        }
        assert!(read(&bytes).is_err(), "accepted quaternion {quaternion:?}");
    }
}

#[test]
fn zero_negative_zero_and_subnormal_durations_keep_their_bits() {
    let bytes = fixture(&[0.0, -0.0, f64::from_bits(1), 0.5], true);
    let recording = read(&bytes).unwrap();
    let mut rewritten = Vec::new();
    recording.write_to(&mut rewritten).unwrap();
    assert_eq!(rewritten, bytes);
    let mut player = Player::new(recording);
    assert_eq!(
        player.next_due().unwrap().frame_time.get().to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(
        player.next_due().unwrap().frame_time.get().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(player.next_due().is_none());
}

#[test]
fn rejects_visual_end_overflow_even_when_each_scalar_is_finite() {
    let mut bytes = fixture(&[86_461.0], false);
    // Maximum date is final representable i32 calendar year's last midnight.
    replace(&mut bytes, CONDITIONS + 8 * 8, MAX_VISUAL_EPOCH - 1.0);
    assert!(read(&bytes).is_err());
}

#[test]
fn preserves_safe_legacy_rates_and_long_frames_instead_of_applying_cli_caps() {
    let mut bytes = fixture(&[0.0, 10.0], true);
    replace(&mut bytes, CONDITIONS + 9 * 8, 21_600.0);
    let recording = read(&bytes).unwrap();
    let mut rewritten = Vec::new();
    recording.write_to(&mut rewritten).unwrap();
    assert_eq!(rewritten, bytes);
}

#[test]
fn all_truncations_including_keyframes_remain_errors() {
    let bytes = fixture(&[0.0, 0.01], true);
    for length in 0..bytes.len() {
        assert!(
            read(&bytes[..length]).is_err(),
            "accepted prefix of {length} bytes"
        );
    }
}

#[test]
fn duplicate_descending_and_out_of_range_keyframe_indices_remain_errors() {
    let mut bytes = fixture(&[0.01; 121], true);
    let first = bytes[bytes.len() - KEYFRAME_BYTES..].to_vec();
    bytes.extend_from_slice(&first);
    bytes[FRAMES - 4..FRAMES].copy_from_slice(&2_u32.to_le_bytes());
    let first_at = bytes.len() - 2 * KEYFRAME_BYTES;
    let second_at = bytes.len() - KEYFRAME_BYTES;
    for (first, second) in [(0_u32, 0_u32), (120, 0), (0, 121)] {
        bytes[first_at..first_at + 4].copy_from_slice(&first.to_le_bytes());
        bytes[second_at..second_at + 4].copy_from_slice(&second.to_le_bytes());
        assert!(matches!(
            read(&bytes),
            Err(ReplayError::InvalidKeyframe { .. })
        ));
    }
}

fn assert_writer_rejects(recording: &Recording) {
    let mut bytes = vec![0x55_u8];
    assert!(recording.write_to(&mut bytes).is_err());
    assert_eq!(
        bytes,
        [0x55],
        "invalid data must be rejected before any output"
    );
}

#[test]
fn writer_rejects_invalid_in_memory_durations_before_writing_anything() {
    for dt in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
        let mut recorder = Recorder::new(Conditions::default());
        recorder.record(Seconds(dt), ControlInputs::neutral(), None);
        assert_writer_rejects(&recorder.finish());
    }
    let mut recorder = Recorder::new(Conditions {
        time_rate: 0.0,
        ..Conditions::default()
    });
    for _ in 0..2 {
        recorder.record(Seconds(f64::MAX), ControlInputs::neutral(), None);
    }
    assert_writer_rejects(&recorder.finish());
}

#[test]
fn writer_rejects_invalid_in_memory_conditions_and_oversized_utf8_names() {
    for conditions in [
        Conditions {
            time_rate: f64::NAN,
            ..Conditions::default()
        },
        Conditions {
            start_epoch: f64::MAX,
            ..Conditions::default()
        },
        Conditions {
            aircraft_name: "a".repeat(257),
            ..Conditions::default()
        },
        Conditions {
            aircraft_name: format!("{}é", "a".repeat(255)),
            ..Conditions::default()
        },
    ] {
        assert_writer_rejects(&Recorder::new(conditions).finish());
    }
}

#[test]
fn writer_rejects_invalid_keyframe_state_without_changing_recorder_api() {
    let base = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0.0, 0.0, 100.0),
        Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut states = [base; 4];
    states[0].position.0.x = f64::NAN;
    states[1].velocity.y = f64::INFINITY;
    states[2].angular_velocity.z = f64::NEG_INFINITY;
    states[3].orientation = glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    for state in states {
        let mut recorder = Recorder::new(Conditions::default());
        recorder.record(Seconds(0.0), ControlInputs::neutral(), Some(&state));
        assert_writer_rejects(&recorder.finish());
    }
}

#[test]
fn finite_components_with_overflowing_magnitudes_are_rejected() {
    for condition in [2, 5, 6] {
        let mut bytes = fixture(&[0.0], false);
        replace(&mut bytes, CONDITIONS + condition * 8, f64::MAX);
        assert!(
            read(&bytes).is_err(),
            "accepted excessive condition {condition}"
        );
    }
    for component in [0, 3, 10] {
        let mut bytes = fixture(&[0.0], true);
        let offset = bytes.len() - KEYFRAME_BYTES + 4 + component * 8;
        replace(&mut bytes, offset, f64::MAX);
        assert!(
            read(&bytes).is_err(),
            "accepted excessive vector component {component}"
        );
    }
}

#[test]
fn near_unit_quaternion_rounding_is_preserved_and_sign_is_not_canonicalized() {
    for w in [1.0_f64 + 5e-13, -1.0, -1.0 - 5e-13] {
        let mut bytes = fixture(&[0.0], true);
        let offset = bytes.len() - KEYFRAME_BYTES + 4 + 9 * 8;
        replace(&mut bytes, offset, w);
        let recording = read(&bytes).unwrap();
        let mut written = Vec::new();
        recording.write_to(&mut written).unwrap();
        assert_eq!(written, bytes);
    }
    for w in [1.0_f64 + 2e-12, -1.0 - 2e-12] {
        let mut bytes = fixture(&[0.0], true);
        let offset = bytes.len() - KEYFRAME_BYTES + 4 + 9 * 8;
        replace(&mut bytes, offset, w);
        assert!(read(&bytes).is_err());
    }
}

#[test]
fn geodetic_edges_and_control_endpoints_remain_valid() {
    for latitude in [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2] {
        for longitude in [-std::f64::consts::PI, std::f64::consts::PI] {
            let mut bytes = fixture(&[0.0], false);
            replace(&mut bytes, CONDITIONS, latitude);
            replace(&mut bytes, CONDITIONS + 8, longitude);
            replace(
                &mut bytes,
                CONDITIONS + 3 * 8,
                -10.0 * std::f64::consts::TAU,
            );
            replace(&mut bytes, CONDITIONS + 4 * 8, 10.0 * std::f64::consts::TAU);
            for (index, value) in [-1.0, 1.0, -0.0, 0.0, 1.0, 1.0].into_iter().enumerate() {
                replace(&mut bytes, FRAMES + (index + 1) * 8, value);
            }
            let recording = read(&bytes).unwrap();
            let mut written = Vec::new();
            recording.write_to(&mut written).unwrap();
            assert_eq!(written, bytes);
        }
    }
}

#[test]
fn epoch_sentinel_and_calendar_limit_are_explicit_and_do_not_cap_duration() {
    assert_eq!(MAX_VISUAL_EPOCH.to_bits(), 784_354_017_363.5_f64.to_bits());
    for epoch in [0.0, MAX_VISUAL_EPOCH] {
        let mut bytes = fixture(&[f64::MAX], false);
        replace(&mut bytes, CONDITIONS + 8 * 8, epoch);
        replace(&mut bytes, CONDITIONS + 9 * 8, 0.0);
        let recording = read(&bytes).unwrap();
        assert_eq!(recording.duration().get().to_bits(), f64::MAX.to_bits());
        let mut written = Vec::new();
        recording.write_to(&mut written).unwrap();
        assert_eq!(written, bytes);
    }
    let mut bytes = fixture(&[], false);
    replace(
        &mut bytes,
        CONDITIONS + 8 * 8,
        f64::from_bits(MAX_VISUAL_EPOCH.to_bits() + 1),
    );
    assert!(read(&bytes).is_err());
}

#[test]
fn numeric_errors_name_the_field_and_frame_and_preserve_io_errors() {
    let mut bytes = fixture(&[0.0, -1.0], false);
    let error = read(&bytes).unwrap_err();
    assert!(matches!(
        error,
        ReplayError::InvalidValue {
            field: "frame duration",
            frame: Some(1),
            ..
        }
    ));
    let message = error.to_string();
    assert!(message.contains("frame duration") && message.contains("frame 1"));
    replace(&mut bytes, CONDITIONS + 9 * 8, f64::NAN);
    assert!(matches!(
        read(&bytes),
        Err(ReplayError::InvalidValue {
            field: "time rate",
            frame: None,
            ..
        })
    ));
    let valid = read(&fixture(&[0.0], false)).unwrap();
    assert!(matches!(
        valid.write_to(&mut &mut [][..]),
        Err(ReplayError::Io(_))
    ));
}

#[test]
fn matching_aircraft_fingerprint_does_not_bypass_in_memory_validation() {
    let config = flightsim_fdm::AircraftConfig::light_single();
    let mut recorder = Recorder::new(Conditions::default().with_aircraft(&config));
    recorder.record(Seconds(f64::NAN), ControlInputs::neutral(), None);
    assert!(matches!(
        recorder.finish().check_reproducible_with(&config),
        Err(ReplayError::InvalidValue { .. })
    ));
}

#[test]
fn player_ignores_finite_speed_product_overflow_and_recovers() {
    let mut player = Player::new(read(&fixture(&[0.01], false)).unwrap());
    player.set_speed(8.0);
    player.accumulate(Seconds(f64::MAX));
    assert!(
        player.next_due().is_none(),
        "overflow must not give an infinite budget"
    );
    player.accumulate(Seconds(0.01));
    assert!(
        player.next_due().is_some(),
        "ordinary accumulation must still work"
    );
}

#[test]
fn player_ignores_budget_sum_overflow_without_discarding_valid_budget() {
    let mut bytes = fixture(&[f64::MAX * 0.5, 0.01], false);
    replace(&mut bytes, CONDITIONS + 9 * 8, 0.0);
    let mut player = Player::new(read(&bytes).unwrap());
    player.accumulate(Seconds(f64::MAX * 0.5));
    player.accumulate(Seconds(f64::MAX));
    assert!(
        player.next_due().is_some(),
        "the earlier finite budget must survive"
    );
    assert!(
        player.next_due().is_none(),
        "overflow must not leave an infinite budget"
    );
    player.accumulate(Seconds(0.01));
    assert!(
        player.next_due().is_some(),
        "ordinary accumulation must still work"
    );
}
