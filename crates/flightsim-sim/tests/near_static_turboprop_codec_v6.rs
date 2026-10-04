mod turboprop_common;

use flightsim_fdm::{
    ControlInputs,
    turboprop::near_static::{StaticPowerBound, TurbopropAircraftConfig, TurbopropFailureReason},
};
use flightsim_sim::{
    aircraft_profile_v4::AircraftProfileV4,
    near_static_turboprop_simulation::{
        NearStaticTurbopropEnvironment, NearStaticTurbopropSimulation, NearStaticTurbopropTerrain,
    },
    replay::{MAX_FRAMES, ReplayFile},
    replay_v4::{JetRecording, ModelReplayFile},
    replay_v5::TurbopropRecording,
    replay_v6::{
        NearStaticTurbopropRecorder, NearStaticTurbopropRecording, NearStaticTurbopropReplayPlayer,
    },
};

const ZERO: &[u8] = include_bytes!("fixtures/v6_zero.fsreplay");
const DIAGNOSTICS: [f64; 12] = [
    1., 1., 0.2, -0.005, 0.3, 180., 180.01, 0.6, 0.01, 20., 0.05, 0.025,
];

fn config() -> TurbopropAircraftConfig {
    AircraftProfileV4::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
    ))
    .unwrap()
    .configuration()
    .clone()
}

fn encoded(record: &NearStaticTurbopropRecording) -> Vec<u8> {
    let mut bytes = Vec::new();
    record.write_to(&mut bytes).unwrap();
    bytes
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read(bytes: &[u8]) -> Result<NearStaticTurbopropRecording, flightsim_sim::replay::ReplayError> {
    NearStaticTurbopropRecording::read_from(&mut &bytes[..])
}

fn real_zero() -> Vec<u8> {
    let sim = NearStaticTurbopropSimulation::from_state(
        config(),
        turboprop_common::initial(),
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    encoded(&NearStaticTurbopropRecorder::new(&sim).unwrap().finish())
}

// Construct hostile wire input independently of production enum/mask writers.
fn terminal(
    tag: u8,
    detail: u16,
    mask: u16,
    stage: u8,
    substep: u32,
    values: [f64; 12],
) -> Vec<u8> {
    let mut bytes = ZERO[..ZERO.len() - 4].to_vec();
    bytes.extend((62 + 8 * mask.count_ones()).to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    for value in [-0_f64, -1., 1., 0.75, 0.25, 0.5] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(tag);
    bytes.extend(detail.to_le_bytes());
    bytes.push(stage);
    bytes.extend(substep.to_le_bytes());
    bytes.extend(mask.to_le_bytes());
    for (index, value) in values.into_iter().enumerate() {
        if mask & (1 << index) != 0 {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn independent_goldens_pin_every_reason_weather_and_all_truncation_points() {
    let fixtures: &[&[u8]] = &[
        ZERO,
        include_bytes!("fixtures/v6_successful_121.fsreplay"),
        include_bytes!("fixtures/v6_custom_both.fsreplay"),
        include_bytes!("fixtures/v6_clear.fsreplay"),
        include_bytes!("fixtures/v6_cloud.fsreplay"),
        include_bytes!("fixtures/v6_fog.fsreplay"),
        include_bytes!("fixtures/v6_rain.fsreplay"),
        include_bytes!("fixtures/v6_snow.fsreplay"),
        include_bytes!("fixtures/v6_storm.fsreplay"),
        include_bytes!("fixtures/v6_terminal_raw.fsreplay"),
        include_bytes!("fixtures/v6_terminal_wind.fsreplay"),
        include_bytes!("fixtures/v6_terminal_altitude.fsreplay"),
        include_bytes!("fixtures/v6_terminal_envelope.fsreplay"),
        include_bytes!("fixtures/v6_terminal_tip.fsreplay"),
        include_bytes!("fixtures/v6_terminal_power_map.fsreplay"),
        include_bytes!("fixtures/v6_terminal_propeller_map.fsreplay"),
        include_bytes!("fixtures/v6_terminal_aero.fsreplay"),
        include_bytes!("fixtures/v6_terminal_budget.fsreplay"),
        include_bytes!("fixtures/v6_terminal_disk_nonpositive.fsreplay"),
        include_bytes!("fixtures/v6_terminal_disk_below.fsreplay"),
        include_bytes!("fixtures/v6_terminal_disk_derived.fsreplay"),
        include_bytes!("fixtures/v6_terminal_static_thrust.fsreplay"),
        include_bytes!("fixtures/v6_terminal_static_power.fsreplay"),
        include_bytes!("fixtures/v6_terminal_static_floor.fsreplay"),
        include_bytes!("fixtures/v6_terminal_static_derived.fsreplay"),
        include_bytes!("fixtures/v6_terminal_adverse.fsreplay"),
        include_bytes!("fixtures/v6_terminal_transverse.fsreplay"),
        include_bytes!("fixtures/v6_terminal_both_inflows.fsreplay"),
        include_bytes!("fixtures/v6_terminal_scale_underflow.fsreplay"),
        include_bytes!("fixtures/v6_terminal_scale_load.fsreplay"),
        include_bytes!("fixtures/v6_terminal_negative_aero.fsreplay"),
        include_bytes!("fixtures/v6_terminal_negative_budget.fsreplay"),
        include_bytes!("fixtures/v6_terminal_negative_component.fsreplay"),
        include_bytes!("fixtures/v6_terminal_negative_derivative.fsreplay"),
        include_bytes!("fixtures/v6_terminal_negative_ground.fsreplay"),
    ];
    assert_eq!(fixtures.len(), 35);
    for (index, bytes) in fixtures.iter().copied().enumerate() {
        let mut input = bytes;
        let record = NearStaticTurbopropRecording::read_from(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(encoded(&record), bytes, "fixture {index}");
        for cut in 0..bytes.len() {
            assert!(read(&bytes[..cut]).is_err(), "fixture {index}, cut {cut}");
        }
        assert!(ReplayFile::read_from(&mut &bytes[..]).is_err());
        assert!(ModelReplayFile::read_from(&mut &bytes[..]).is_err());
        assert!(JetRecording::read_from(&mut &bytes[..]).is_err());
        assert!(TurbopropRecording::read_from(&mut &bytes[..]).is_err());
        let mut suffix = bytes.to_vec();
        suffix.extend([71, 72]);
        let mut slice = suffix.as_slice();
        NearStaticTurbopropRecording::read_from(&mut slice).unwrap();
        assert_eq!(slice, [71, 72]);
    }
}

#[test]
fn independent_successful_witness_pins_periodic_checkpoint_and_final_tail() {
    let record = read(include_bytes!("fixtures/v6_successful_121.fsreplay")).unwrap();
    assert_eq!(record.controls().len(), 121);
    assert_eq!(record.controls()[0].throttle().to_bits(), 0_f64.to_bits());
    assert_eq!(
        record.controls()[120].throttle().to_bits(),
        (120_f64 / 128.).to_bits()
    );
    let checkpoints = record.checkpoints();
    assert_eq!(
        checkpoints.iter().map(|c| c.frame).collect::<Vec<_>>(),
        [120, 121]
    );
    for (checkpoint, expected) in checkpoints
        .iter()
        .zip([[0.25_f64, 190., 0.4], [0.375, 195., 0.5]])
    {
        let state = checkpoint.state;
        assert_eq!(
            state.turbine_fraction.get().to_bits(),
            expected[0].to_bits()
        );
        assert_eq!(state.shaft_rad_s.get().to_bits(), expected[1].to_bits());
        assert_eq!(state.blade_pitch_rad.get().to_bits(), expected[2].to_bits());
    }
    assert_eq!(record.final_state(), &checkpoints[1].state);
    assert_ne!(record.final_state(), &checkpoints[0].state);
}

#[test]
fn earlier_v5_bytes_and_writer_version_remain_exact_and_exclude_new_codes() {
    for bytes in [
        include_bytes!("fixtures/v5_zero.fsreplay").as_slice(),
        include_bytes!("fixtures/v5_successful_121.fsreplay").as_slice(),
        include_bytes!("fixtures/v5_storm.fsreplay").as_slice(),
        include_bytes!("fixtures/v5_terminal_disk_below.fsreplay").as_slice(),
    ] {
        let record = TurbopropRecording::read_from(&mut &bytes[..]).unwrap();
        let mut output = Vec::new();
        record.write_to(&mut output).unwrap();
        assert_eq!(output, bytes);
        assert!(read(bytes).is_err());
    }
    // Even if an attacker changes only the version, the v5 identity gate stays closed.
    let mut fake_v5 = ZERO.to_vec();
    fake_v5[8..10].copy_from_slice(&5_u16.to_le_bytes());
    assert!(TurbopropRecording::read_from(&mut fake_v5.as_slice()).is_err());
    assert_eq!(flightsim_sim::replay_v5::TURBOPROP_FORMAT_VERSION, 5);
    assert_eq!(
        flightsim_sim::replay_v6::NEAR_STATIC_TURBOPROP_FORMAT_VERSION,
        6
    );
    assert_eq!(
        flightsim_sim::replay_v6::MAX_NEAR_STATIC_TURBOPROP_RECORDING_BYTES,
        49_100_927
    );
}

#[test]
fn counts_lengths_and_all_engine_numeric_boundaries_reject_without_repair() {
    let base = real_zero();
    let n = 14 + u32_at(&base, 10) as usize;
    for (offset, value) in [
        (10, 4097),
        (n, MAX_FRAMES + 1),
        (n + 4, 8335),
        (n + 4, 0),
        (n + 8, 1),
    ] {
        let mut bytes = base.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(read(&bytes).is_err(), "offset {offset}");
    }
    for (index, values) in [
        (
            13,
            vec![-f64::EPSILON, 1. + f64::EPSILON, f64::NAN, f64::INFINITY],
        ),
        (14, vec![0., 19.999, 1000.001, f64::NAN, f64::INFINITY]),
        (
            15,
            vec![
                -f64::EPSILON,
                std::f64::consts::FRAC_PI_2 + f64::EPSILON,
                f64::NAN,
                f64::INFINITY,
            ],
        ),
    ] {
        for value in values {
            for state in [n - 128, n + 12] {
                let mut bytes = base.clone();
                bytes[state + 8 * index..state + 8 * (index + 1)]
                    .copy_from_slice(&value.to_le_bytes());
                assert!(
                    read(&bytes).is_err(),
                    "state {state}, scalar {index}: {value}"
                );
            }
        }
    }
}

#[test]
fn unknown_nonzero_identity_tuples_are_inspectable_but_never_reproduced() {
    let base = real_zero();
    let identity = 18 + u32_at(&base, 14) as usize;
    for (offset, width) in [(0, 2), (2, 2), (4, 2), (6, 4)] {
        let mut bytes = base.clone();
        bytes[identity + offset] = 9;
        let record = read(&bytes).unwrap();
        assert_eq!(encoded(&record), bytes);
        assert!(NearStaticTurbopropReplayPlayer::new(config(), record).is_err());
        bytes[identity + offset..identity + offset + width].fill(0);
        assert!(read(&bytes).is_err());
    }
    for revision in [0_u32, 2, u32::MAX] {
        let mut bytes = base.clone();
        bytes[identity + 18..identity + 22].copy_from_slice(&revision.to_le_bytes());
        assert!(read(&bytes).is_err());
    }
}

#[test]
fn unknown_data_fingerprints_remain_inspectable_but_not_reproducible() {
    let mut environment = NearStaticTurbopropEnvironment::default();
    environment.terrain = NearStaticTurbopropTerrain::BundledGlobal;
    environment.conditions = environment.conditions.with_world_climate(
        true,
        Some(flightsim_world::ClimateDate::from_annual_phase(0.25).unwrap()),
    );
    let sim = NearStaticTurbopropSimulation::from_supported_state(
        config(),
        turboprop_common::initial(),
        environment,
        ControlInputs::neutral(),
    )
    .unwrap();
    let base = encoded(&NearStaticTurbopropRecorder::new(&sim).unwrap().finish());
    let environment = 18 + u32_at(&base, 14) as usize + 18 + 4;
    for delta in [88, 104] {
        let mut bytes = base.clone();
        bytes[environment + delta] ^= 0x80;
        let record = read(&bytes).unwrap();
        assert_eq!(encoded(&record), bytes);
        assert!(NearStaticTurbopropReplayPlayer::new(config(), record).is_err());
    }
}

#[test]
fn explicit_new_power_details_are_not_enum_discriminants() {
    for (code, expected) in [
        (1, StaticPowerBound::NonpositiveThrust),
        (2, StaticPowerBound::NonpositivePower),
        (3, StaticPowerBound::BelowStaticFloor),
        (4, StaticPowerBound::InvalidDerivedBound),
    ] {
        let bytes = terminal(9, code, 0x1ff, 0, 0, DIAGNOSTICS);
        let record = read(&bytes).unwrap();
        assert_eq!(
            record.terminal().unwrap().failure.reason,
            TurbopropFailureReason::NearStaticPowerBound(expected)
        );
        assert_eq!(encoded(&record), bytes);
    }
}

#[test]
fn independent_terminal_pins_every_diagnostic_field_in_wire_order() {
    let record = read(include_bytes!(
        "fixtures/v6_terminal_negative_component.fsreplay"
    ))
    .unwrap();
    let event = record.terminal().unwrap();
    assert_eq!(event.failure.substep, 1);
    assert_eq!(
        event.failure.stage,
        flightsim_fdm::turboprop::TurbopropStage::K2
    );
    let values = event.failure.diagnostics.values();
    let decoded = [
        values.pressure_ratio.unwrap().0,
        values.temperature_ratio.unwrap().0,
        values.mach.unwrap().0,
        values.advance_ratio.unwrap().0,
        values.blade_pitch.unwrap().get(),
        values.relative_shaft.unwrap().get(),
        values.absolute_spin.unwrap().get(),
        values.tip_mach.unwrap().0,
        values.crossflow_ratio.unwrap(),
        values.hover_velocity.unwrap().get(),
        values.adverse_inflow_ratio.unwrap(),
        values.transverse_inflow_ratio.unwrap(),
    ];
    let expected: [f64; 12] = [
        0.95, 1.05, 0.2, -0.005, 0.3, 180., 180.01, 0.6, 0.01, 20., 0.05, 0.025,
    ];
    assert_eq!(decoded.map(f64::to_bits), expected.map(f64::to_bits));
}

#[test]
fn terminal_tags_details_stages_masks_and_lengths_are_closed() {
    for (tag, details) in [
        (1, vec![0, 17, u16::MAX]),
        (8, vec![0, 4, u16::MAX]),
        (9, vec![0, 5, u16::MAX]),
        (10, vec![0, 1, 2, 3, 4, 5, 7, 8, 11, 15, 16, u16::MAX]),
        (11, vec![1, 2, u16::MAX]),
    ] {
        for detail in details {
            assert!(
                read(&terminal(tag, detail, 0x1ff, 0, 0, DIAGNOSTICS)).is_err(),
                "tag {tag}, detail {detail}"
            );
        }
    }
    for tag in [0, 12, 255] {
        assert!(read(&terminal(tag, 0, 0, 0, 0, DIAGNOSTICS)).is_err());
    }
    for (stage, substep) in [(6, 0), (255, 0), (0, 1), (1, 8), (5, u32::MAX)] {
        assert!(read(&terminal(9, 1, 0x1ff, stage, substep, DIAGNOSTICS)).is_err());
    }
    for (tag, detail, rejected_masks) in [
        (9, 1, vec![0, 0x30, 0x77, 0x1f7, 0xfff]),
        (
            10,
            6,
            vec![0, 0x30, 0x77, 0x1f7, 0x1ff, 0x3ff, 0x7ff, 0xeff, 0x1fff],
        ),
        (11, 0, vec![0, 0x30, 0x77, 0x1ef, 0x3ff, 0xfff]),
    ] {
        for mask in rejected_masks {
            assert!(
                read(&terminal(tag, detail, mask, 0, 0, DIAGNOSTICS)).is_err(),
                "tag {tag}, mask {mask:x}"
            );
        }
    }
    let base = terminal(9, 1, 0x1ff, 0, 0, DIAGNOSTICS);
    let length_offset = ZERO.len() - 4;
    for length in [1_u32, 61, 63, 133, 135, 159, u32::MAX] {
        let mut bytes = base.clone();
        bytes[length_offset..length_offset + 4].copy_from_slice(&length.to_le_bytes());
        assert!(read(&bytes).is_err(), "length {length}");
    }
}

#[test]
fn full_diagnostic_group_requires_negative_j_and_exact_fixed_domain_status() {
    let above = f64::from_bits(0.1_f64.to_bits() + 1);
    for (adverse, transverse, detail) in [(above, 0.1, 6), (0.1, above, 9), (above, above, 10)] {
        let mut values = DIAGNOSTICS;
        values[10] = adverse;
        values[11] = transverse;
        let bytes = terminal(10, detail, 0xfff, 0, 0, values);
        assert_eq!(encoded(&read(&bytes).unwrap()), bytes);
        for wrong in [6, 9, 10].into_iter().filter(|d| *d != detail) {
            assert!(read(&terminal(10, wrong, 0xfff, 0, 0, values)).is_err());
        }
    }
    for j in [-0_f64, 0., f64::from_bits(1), 0.001] {
        let mut values = DIAGNOSTICS;
        values[3] = j;
        for (tag, detail, mask) in [(9, 1, 0x1ff), (11, 0, 0x1ff), (1, 14, 0xfff)] {
            assert!(read(&terminal(tag, detail, mask, 0, 0, values)).is_err());
        }
    }
    for (tag, detail, stage) in [(6, 2, 1), (7, 0, 5), (1, 15, 3), (1, 6, 5), (8, 1, 2)] {
        assert!(read(&terminal(tag, detail, 0x1ff, stage, 0, DIAGNOSTICS)).is_err());
    }
    for (tag, detail, stage) in [(6, 2, 1), (7, 0, 5), (1, 14, 2), (1, 15, 3), (1, 6, 5)] {
        let mut values = DIAGNOSTICS;
        values[10] = 0.1;
        values[11] = 0.1;
        assert!(read(&terminal(tag, detail, 0xfff, stage, 0, values)).is_ok());
        values[10] = above;
        assert!(read(&terminal(tag, detail, 0xfff, stage, 0, values)).is_err());
    }
    assert!(read(&terminal(10, 6, 0xfff, 0, 0, DIAGNOSTICS)).is_err());
}

#[test]
fn diagnostics_preserve_signed_zero_subnormals_and_absence_without_underflow_repair() {
    let tiny = f64::from_bits(1);
    for transverse in [0_f64, -0., tiny] {
        let mut values = DIAGNOSTICS;
        values[3] = -tiny;
        values[9] = tiny;
        values[10] = tiny;
        values[11] = transverse;
        let bytes = terminal(1, 14, 0xfff, 0, 0, values);
        let record = read(&bytes).unwrap();
        let diagnostics = record.terminal().unwrap().failure.diagnostics.values();
        assert_eq!(
            diagnostics.hover_velocity.unwrap().get().to_bits(),
            tiny.to_bits()
        );
        assert_eq!(
            diagnostics.adverse_inflow_ratio.unwrap().to_bits(),
            tiny.to_bits()
        );
        assert_eq!(
            diagnostics.transverse_inflow_ratio.unwrap().to_bits(),
            transverse.to_bits()
        );
        assert_eq!(encoded(&record), bytes);
    }
    for mask in [0x1f7, 0x1ff] {
        let bytes = terminal(11, 0, mask, 0, 0, DIAGNOSTICS);
        let record = read(&bytes).unwrap();
        let diagnostics = record.terminal().unwrap().failure.diagnostics.values();
        assert_eq!(diagnostics.advance_ratio.is_some(), mask == 0x1ff);
        assert!(diagnostics.hover_velocity.is_none());
        assert!(diagnostics.adverse_inflow_ratio.is_none());
        assert!(diagnostics.transverse_inflow_ratio.is_none());
        assert_eq!(encoded(&record), bytes);
    }
    for index in 0..12 {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut values = DIAGNOSTICS;
            values[index] = invalid;
            assert!(
                read(&terminal(1, 14, 0xfff, 0, 0, values)).is_err(),
                "index {index}"
            );
        }
    }
    for (index, invalid) in [
        (9, 0_f64),
        (9, -0.),
        (9, -tiny),
        (10, 0.),
        (10, -0.),
        (10, -tiny),
        (11, -tiny),
    ] {
        let mut values = DIAGNOSTICS;
        values[index] = invalid;
        assert!(
            read(&terminal(1, 14, 0xfff, 0, 0, values)).is_err(),
            "index {index}: {invalid}"
        );
    }
    for zero in [0_f64, -0.] {
        let mut values = DIAGNOSTICS;
        values[3] = zero;
        let bytes = terminal(6, 2, 0x1ff, 0, 0, values);
        assert_eq!(encoded(&read(&bytes).unwrap()), bytes);
    }
}
