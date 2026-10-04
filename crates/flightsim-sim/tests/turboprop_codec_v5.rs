mod turboprop_common;
use flightsim_fdm::ControlInputs;
use flightsim_sim::{
    replay::{MAX_FRAMES, ReplayFile},
    replay_v4::{JetRecording, ModelReplayFile},
    replay_v5::{TurbopropRecorder, TurbopropRecording, TurbopropReplayPlayer},
    turboprop_simulation::{TurbopropEnvironment, TurbopropSimulation},
};
use turboprop_common::{config, initial};
fn encoded(r: &TurbopropRecording) -> Vec<u8> {
    let mut b = Vec::new();
    r.write_to(&mut b).unwrap();
    b
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn real_zero() -> Vec<u8> {
    let sim = TurbopropSimulation::from_state(config(), initial(), TurbopropEnvironment::default())
        .unwrap();
    encoded(&TurbopropRecorder::new(&sim).unwrap().finish())
}
#[test]
fn independent_goldens_pin_every_reason_weather_shape_and_old_reader_exclusion() {
    let fixtures: [&[u8]; 21] = [
        include_bytes!("fixtures/v5_zero.fsreplay"),
        include_bytes!("fixtures/v5_successful_121.fsreplay"),
        include_bytes!("fixtures/v5_custom_both.fsreplay"),
        include_bytes!("fixtures/v5_clear.fsreplay"),
        include_bytes!("fixtures/v5_cloud.fsreplay"),
        include_bytes!("fixtures/v5_fog.fsreplay"),
        include_bytes!("fixtures/v5_rain.fsreplay"),
        include_bytes!("fixtures/v5_snow.fsreplay"),
        include_bytes!("fixtures/v5_storm.fsreplay"),
        include_bytes!("fixtures/v5_terminal_raw.fsreplay"),
        include_bytes!("fixtures/v5_terminal_wind.fsreplay"),
        include_bytes!("fixtures/v5_terminal_altitude.fsreplay"),
        include_bytes!("fixtures/v5_terminal_envelope.fsreplay"),
        include_bytes!("fixtures/v5_terminal_tip.fsreplay"),
        include_bytes!("fixtures/v5_terminal_power_map.fsreplay"),
        include_bytes!("fixtures/v5_terminal_propeller_map.fsreplay"),
        include_bytes!("fixtures/v5_terminal_aero.fsreplay"),
        include_bytes!("fixtures/v5_terminal_budget.fsreplay"),
        include_bytes!("fixtures/v5_terminal_disk_nonpositive.fsreplay"),
        include_bytes!("fixtures/v5_terminal_disk_below.fsreplay"),
        include_bytes!("fixtures/v5_terminal_disk_derived.fsreplay"),
    ];
    for bytes in fixtures {
        let mut input = bytes;
        let record = TurbopropRecording::read_from(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(encoded(&record), bytes);
        for cut in 0..bytes.len() {
            assert!(
                TurbopropRecording::read_from(&mut &bytes[..cut]).is_err(),
                "cut {cut}"
            );
        }
        let mut input = bytes;
        assert!(ReplayFile::read_from(&mut input).is_err());
        let mut input = bytes;
        assert!(ModelReplayFile::read_from(&mut input).is_err());
        let mut input = bytes;
        assert!(JetRecording::read_from(&mut input).is_err());
        let mut suffix = bytes.to_vec();
        suffix.extend([71, 72]);
        let mut slice = suffix.as_slice();
        TurbopropRecording::read_from(&mut slice).unwrap();
        assert_eq!(slice, [71, 72]);
    }
}
#[test]
fn earlier_golden_bytes_and_writer_versions_stay_exact() {
    for bytes in [
        include_bytes!("fixtures/v4_zero.fsreplay").as_slice(),
        include_bytes!("fixtures/v4_terminal_zero.fsreplay").as_slice(),
        include_bytes!("fixtures/v4_storm.fsreplay").as_slice(),
    ] {
        let mut input = bytes;
        let record = ModelReplayFile::read_from(&mut input).unwrap();
        let mut output = Vec::new();
        record.write_to(&mut output).unwrap();
        assert_eq!(output, bytes);
        let mut input = bytes;
        assert!(TurbopropRecording::read_from(&mut input).is_err());
    }
    assert_eq!(flightsim_sim::replay_v4::MODEL_FORMAT_VERSION, 4);
    assert!(!flightsim_sim::model_identity::ModelIdentity::for_turboprop(&config()).supported());
}
#[test]
fn counts_lengths_identity_and_all_engine_numeric_boundaries_reject() {
    let base = real_zero();
    let n = 14 + u32_at(&base, 10) as usize;
    let initial = n - 128;
    let final_state = n + 12;
    for (offset, value) in [
        (10, 4097),
        (n, MAX_FRAMES + 1),
        (n + 4, 8335),
        (n + 4, 0),
        (n + 8, 1),
    ] {
        let mut b = base.clone();
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            TurbopropRecording::read_from(&mut b.as_slice()).is_err(),
            "offset {offset}"
        );
    }
    let identity = 18 + u32_at(&base, 14) as usize;
    for offset in [
        identity,
        identity + 2,
        identity + 4,
        identity + 6,
        identity + 18,
    ] {
        let mut b = base.clone();
        b[offset] = 9;
        assert!(
            TurbopropRecording::read_from(&mut b.as_slice()).is_err(),
            "identity{offset}"
        );
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
            for state in [initial, final_state] {
                let mut b = base.clone();
                b[state + 8 * index..state + 8 * (index + 1)].copy_from_slice(&value.to_le_bytes());
                assert!(
                    TurbopropRecording::read_from(&mut b.as_slice()).is_err(),
                    "state{state} scalar{index} {value}"
                );
            }
        }
    }
}
#[test]
fn terminal_codes_masks_lengths_and_diagnostic_signs_are_closed() {
    let base = include_bytes!("fixtures/v5_terminal_disk_below.fsreplay");
    let off = base.len() - 134;
    for (offset, value) in [
        (52, 0),
        (52, 9),
        (53, 0),
        (53, 4),
        (55, 6),
        (56, 8),
        (60, 0),
        (61, 2),
        (61, 128),
    ] {
        let mut b = base.to_vec();
        b[off + offset] = value;
        assert!(
            TurbopropRecording::read_from(&mut b.as_slice()).is_err(),
            "offset{offset} value{value}"
        );
    }
    for length in [1_u32, 61, 63, 133, 135, u32::MAX] {
        let mut b = base.to_vec();
        b[off - 4..off].copy_from_slice(&length.to_le_bytes());
        assert!(TurbopropRecording::read_from(&mut b.as_slice()).is_err());
    }
    for i in 0..9 {
        let mut b = base.to_vec();
        b[off + 62 + 8 * i..off + 70 + 8 * i].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(TurbopropRecording::read_from(&mut b.as_slice()).is_err());
    }
    for i in [0, 1, 2, 4, 5, 7, 8] {
        let mut b = base.to_vec();
        b[off + 62 + 8 * i..off + 70 + 8 * i].copy_from_slice(&(-1_f64).to_le_bytes());
        assert!(TurbopropRecording::read_from(&mut b.as_slice()).is_err());
    }
}
#[test]
fn unknown_data_fingerprints_remain_inspectable_but_not_reproducible() {
    let mut env = TurbopropEnvironment::default();
    env.terrain = flightsim_sim::turboprop_simulation::TurbopropTerrain::BundledGlobal;
    env.conditions = env.conditions.with_world_climate(
        true,
        Some(flightsim_world::ClimateDate::from_annual_phase(0.25).unwrap()),
    );
    let sim = TurbopropSimulation::from_supported_state(
        config(),
        initial(),
        env,
        ControlInputs::neutral(),
    )
    .unwrap();
    let base = encoded(&TurbopropRecorder::new(&sim).unwrap().finish());
    let environment = 18 + u32_at(&base, 14) as usize + 18 + 4;
    for delta in [88, 104] {
        let mut bytes = base.clone();
        bytes[environment + delta] ^= 0x80;
        let r = TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
        assert_eq!(encoded(&r), bytes);
        assert!(TurbopropReplayPlayer::new(config(), r).is_err());
    }
}
