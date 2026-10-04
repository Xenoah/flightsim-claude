use flightsim_core::{Attitude, Geodetic, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::subsonic::{
    JetAircraftConfig, JetFailureReason, JetStage, OperatingEnvelope, OperatingEnvelopeDefinition,
};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use flightsim_sim::{
    aircraft_profile::AircraftProfileV2,
    jet_scenarios::solve_jet_trim,
    model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation},
    replay::{MAX_FRAMES, ReplayFile},
    replay_v4::{JetRecorder, JetRecording, JetReplayPlayer, ModelReplayFile, state_bits_equal},
};

fn config() -> JetAircraftConfig {
    AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap()
    .configuration()
    .clone()
}
fn initial() -> RigidBodyState {
    solve_jet_trim(
        &config(),
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap()
    .state
}
fn new_sim() -> JetSimulation {
    JetSimulation::from_state(config(), initial(), JetEnvironment::default()).unwrap()
}
fn encoded(recording: &JetRecording) -> Vec<u8> {
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    bytes
}
fn recorded(steps: u32) -> (JetRecording, flightsim_sim::model_simulation::JetSnapshot) {
    let trim = solve_jet_trim(
        &config(),
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap();
    let mut sim =
        JetSimulation::from_state(config(), trim.state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    for _ in 0..steps {
        let report = sim.advance(JET_FIXED_DT, trim.controls);
        assert!(report.terminal().is_none());
        recorder.record(&report).unwrap();
    }
    (recorder.finish(), sim.snapshot())
}
fn u32_at(bytes: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap())
}
fn environment_offset(bytes: &[u8]) -> usize {
    18 + u32_at(bytes, 14) as usize + 18 + 4
}
fn final_offset(bytes: &[u8]) -> usize {
    let n_offset = 14 + u32_at(bytes, 10) as usize;
    let n = u32_at(bytes, n_offset) as usize;
    let k_offset = n_offset + 4 + 48 * n;
    let k = u32_at(bytes, k_offset) as usize;
    k_offset + 4 + (k - 1) * 108 + 4
}

#[test]
fn independent_golden_bytes_all_weather_shapes_and_terminal() {
    let fixtures: [&[u8]; 10] = [
        include_bytes!("fixtures/v4_zero.fsreplay"),
        include_bytes!("fixtures/v4_terminal_zero.fsreplay"),
        include_bytes!("fixtures/v4_terminal_no_query.fsreplay"),
        include_bytes!("fixtures/v4_custom_both.fsreplay"),
        include_bytes!("fixtures/v4_clear.fsreplay"),
        include_bytes!("fixtures/v4_cloud.fsreplay"),
        include_bytes!("fixtures/v4_fog.fsreplay"),
        include_bytes!("fixtures/v4_rain.fsreplay"),
        include_bytes!("fixtures/v4_snow.fsreplay"),
        include_bytes!("fixtures/v4_storm.fsreplay"),
    ];
    for bytes in fixtures {
        let mut input = bytes;
        let recording = JetRecording::read_from(&mut input).unwrap();
        assert_eq!(encoded(&recording), bytes);
        let mut input = bytes;
        assert!(ReplayFile::read_from(&mut input).is_err());
        let mut input = bytes;
        assert!(matches!(
            ModelReplayFile::read_from(&mut input).unwrap(),
            ModelReplayFile::V4(_)
        ));
        for cut in 0..bytes.len() {
            assert!(
                JetRecording::read_from(&mut &bytes[..cut]).is_err(),
                "truncated prefix{cut}"
            );
        }
    }
}
#[test]
fn old_formats_stay_separate_and_byte_exact() {
    for bytes in [
        include_bytes!("fixtures/legacy_v1.fsreplay").as_slice(),
        include_bytes!("fixtures/legacy_v2_disabled.fsreplay"),
        include_bytes!("fixtures/legacy_v2_world.fsreplay"),
        include_bytes!("fixtures/v3_rain.fsreplay"),
    ] {
        let mut input = bytes;
        let value = ModelReplayFile::read_from(&mut input).unwrap();
        assert!(matches!(&value, ModelReplayFile::Existing(_)));
        let mut out = Vec::new();
        value.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }
}
#[test]
fn bounded_conditions_and_counts_reject_before_following_payload() {
    let base = include_bytes!("fixtures/v4_zero.fsreplay").to_vec();
    for length in [0, 254, 4097, u32::MAX] {
        let mut bytes = base.clone();
        bytes[10..14].copy_from_slice(&length.to_le_bytes());
        let mut input = std::io::Cursor::new(bytes);
        assert!(JetRecording::read_from(&mut input).is_err());
        assert_eq!(input.position(), 14);
    }
    let end = 14 + u32_at(&base, 10) as usize;
    let mut bytes = base;
    bytes[end..end + 4].copy_from_slice(&(MAX_FRAMES + 1).to_le_bytes());
    let mut input = std::io::Cursor::new(bytes);
    assert!(JetRecording::read_from(&mut input).is_err());
    assert_eq!(input.position(), u64::try_from(end + 4).unwrap());
}
#[test]
fn invalid_tags_lengths_controls_and_final_state_reject() {
    let (recording, _) = recorded(1);
    let base = encoded(&recording);
    let identity = 18 + u32_at(&base, 14) as usize;
    let end = 14 + u32_at(&base, 10) as usize;
    for offset in [
        identity,
        identity + 2,
        identity + 4,
        identity + 6,
        identity + 18,
    ] {
        let mut bytes = base.clone();
        bytes[offset] = 99;
        assert!(JetRecording::read_from(&mut bytes.as_slice()).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, 1.01, -1.01] {
        let mut bytes = base.clone();
        bytes[end + 4..end + 12].copy_from_slice(&value.to_le_bytes());
        assert!(JetRecording::read_from(&mut bytes.as_slice()).is_err());
    }
    let mut bytes = base.clone();
    let p = final_offset(&bytes);
    bytes[p..p + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(JetRecording::read_from(&mut bytes.as_slice()).is_err());
    let mut bytes = base;
    let k = end + 4 + 48;
    bytes[k..k + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(JetRecording::read_from(&mut bytes.as_slice()).is_err());
}
#[test]
fn altered_dataset_identity_roundtrips_but_reproduction_rejects() {
    let (recording, _) = recorded(0);
    let original = encoded(&recording);
    let e = environment_offset(&original);
    for climate in [false, true] {
        let mut bytes = original.clone();
        let flags = if climate { 2_u64 } else { 1_u64 };
        bytes[e + 80..e + 88].copy_from_slice(&flags.to_le_bytes());
        if climate {
            bytes[e + 104..e + 112].copy_from_slice(&123_u64.to_le_bytes());
        } else {
            bytes[e + 88..e + 96].copy_from_slice(&123_u64.to_le_bytes());
            bytes[e + 112] = 1;
            bytes[e + 113..e + 121].copy_from_slice(&0_f64.to_le_bytes());
        }
        let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
        assert_eq!(encoded(&decoded), bytes);
        assert!(
            JetSimulation::from_state(
                config(),
                decoded.conditions().initial_state,
                decoded.conditions().environment
            )
            .is_err()
        );
        assert!(JetReplayPlayer::new(config(), decoded).is_err());
    }
}
#[test]
fn actual_held_controls_replay_and_seek_restore_all_history() {
    let (recording, expected) = recorded(720);
    let recording = JetRecording::read_from(&mut encoded(&recording).as_slice()).unwrap();
    assert_eq!(recording.checkpoints().len(), 6);
    let mut player = JetReplayPlayer::new(config(), recording).unwrap();
    player.set_paused(true);
    assert_eq!(player.advance(Seconds(10.)).unwrap(), 0);
    assert_eq!(player.cursor(), 0);
    assert_eq!(player.seek_to(720).unwrap(), 240);
    assert!(player.seeking());
    while player.seeking() {
        assert!(player.continue_seek().unwrap() <= 240);
    }
    assert!(player.finished());
    assert_eq!(player.simulation().snapshot(), expected);
    assert!(state_bits_equal(
        player.simulation().state(),
        &expected.state
    ));
    player.seek_to(130).unwrap();
    assert_eq!(player.cursor(), 130);
    assert!(!player.finished());
    player.restart().unwrap();
    assert_eq!(player.cursor(), 0);
    assert_eq!(player.simulation().elapsed(), Seconds::ZERO);
    player.set_paused(false);
    while !player.finished() {
        player.advance(Seconds(1. / 144.)).unwrap();
    }
    assert_eq!(player.simulation().snapshot(), expected);
}
#[test]
fn failure_zero_rolls_back_controller_clocks_contacts_and_records_one_event() {
    let mut state = initial();
    state.velocity *= 8.;
    let mut sim = JetSimulation::from_state(config(), state, JetEnvironment::default()).unwrap();
    let before = sim.snapshot();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    let mut controller = 7_u32;
    let no_step = sim.advance_with_controller(Seconds(1. / 1000.), &mut controller, |c, _, _| {
        *c += 1;
        ControlInputs::neutral()
    });
    assert_eq!(no_step.committed_steps(), 0);
    assert_eq!(controller, 7);
    let report = sim.advance_with_controller(Seconds(0.25), &mut controller, |c, _, _| {
        *c += 1;
        ControlInputs::neutral().with_throttle(0.5)
    });
    assert_eq!(report.committed_steps(), 0);
    assert!(report.terminal().is_some());
    assert_eq!(sim.snapshot(), before);
    assert!(state_bits_equal(sim.state(), &before.state));
    assert_eq!(controller, 7);
    assert_eq!(sim.accumulated(), Seconds::ZERO);
    recorder.record(&report).unwrap();
    let recording = recorder.finish();
    assert_eq!(recording.controls().len(), 0);
    assert_eq!(recording.duration(), Seconds::ZERO);
    assert_eq!(recording.terminal().unwrap().cursor, 0);
    let mut player = JetReplayPlayer::new(
        config(),
        JetRecording::read_from(&mut encoded(&recording).as_slice()).unwrap(),
    )
    .unwrap();
    assert!(player.finished());
    assert_eq!(player.simulation().snapshot(), before);
    player.set_paused(true);
    player.seek_to(0).unwrap();
    assert!(player.finished());
    player.restart().unwrap();
    assert!(player.finished());
    let again = sim.advance_with_controller(Seconds(1.), &mut controller, |_, _, _| {
        panic!("terminal must not request controls")
    });
    assert_eq!(again.committed_steps(), 0);
    assert!(again.terminal().is_none());
    sim.restart_at(initial()).unwrap();
    assert!(sim.terminal().is_none());
    assert_eq!(sim.elapsed(), Seconds::ZERO);
}
#[test]
fn terminal_reason_and_query_bits_are_verified_and_unexpected_success_faults() {
    let mut state = initial();
    state.velocity *= 8.;
    let mut sim = JetSimulation::from_state(config(), state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    recorder
        .record(&sim.advance(JET_FIXED_DT, ControlInputs::neutral()))
        .unwrap();
    let baseline = encoded(&recorder.finish());
    let l = baseline.len() - 85;
    for offset in [l + 52, l + 61] {
        let mut bytes = baseline.clone();
        if offset == l + 52 {
            bytes[offset] = 4;
        } else {
            let value = f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            bytes[offset..offset + 8].copy_from_slice(&(value + 0.0001).to_le_bytes());
        }
        let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
        assert!(JetReplayPlayer::new(config(), decoded).is_err());
    }
    let (valid, _) = recorded(0);
    let mut bytes = encoded(&valid);
    bytes.truncate(bytes.len() - 4);
    bytes.extend(85_u32.to_le_bytes());
    bytes.extend(&baseline[l..]);
    let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    assert!(JetReplayPlayer::new(config(), decoded).is_err());
}
#[test]
fn ordinary_state_drift_and_wrong_model_are_rejected() {
    let (recording, _) = recorded(121);
    let mut bytes = encoded(&recording);
    let offset = final_offset(&bytes);
    let x = f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    bytes[offset..offset + 8].copy_from_slice(&(x + 0.01).to_le_bytes());
    let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut player = JetReplayPlayer::new(config(), decoded).unwrap();
    assert!(player.seek_to(121).is_err());
    assert!(player.faulted());
    let original = config();
    let changed = JetAircraftConfig::new(
        original.airframe().clone(),
        original.thrust().clone(),
        original.aero().clone(),
        OperatingEnvelope::from_definition(OperatingEnvelopeDefinition {
            pressure_ratio: [0.02, 1.],
            temperature_ratio: [0.5, 1.5],
            mach: [0., 0.9],
        })
        .unwrap(),
    )
    .unwrap();
    assert!(JetReplayPlayer::new(changed, recording).is_err());
}
#[test]
fn accepted_near_unit_quaternion_at_frame_zero_remains_recordable() {
    let mut state = initial();
    state.orientation *= 1. + 5e-10;
    state.velocity *= 8.;
    let mut sim = JetSimulation::from_state(config(), state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    recorder
        .record(&sim.advance(JET_FIXED_DT, ControlInputs::neutral()))
        .unwrap();
    let recording = JetRecording::read_from(&mut encoded(&recorder.finish()).as_slice()).unwrap();
    assert!(
        JetReplayPlayer::new(config(), recording)
            .unwrap()
            .finished()
    );
}
#[test]
fn polar_dateline_and_high_altitude_initial_metadata_roundtrip() {
    for latitude in [-90., -45., 0., 45., 90.] {
        for longitude in [-180., 0., 180.] {
            for height in [0., 1000., 85999.999] {
                let state = RigidBodyState::from_geodetic(
                    Geodetic::from_degrees(latitude, longitude, height),
                    Attitude::new(
                        Radians(0.1),
                        Radians(std::f64::consts::FRAC_PI_2),
                        Radians(1.),
                    ),
                    Ned::new(0., 0., 0.),
                );
                let sim =
                    JetSimulation::from_state(config(), state, JetEnvironment::default()).unwrap();
                let recording = JetRecorder::new(&sim).unwrap().finish();
                let decoded = JetRecording::read_from(&mut encoded(&recording).as_slice()).unwrap();
                assert!(state_bits_equal(
                    &decoded.conditions().initial_state,
                    &state
                ));
            }
        }
    }
}
#[test]
fn recorder_rejects_midflight_and_fractional_start() {
    let mut sim = new_sim();
    sim.advance(Seconds(0.001), ControlInputs::neutral());
    assert!(JetRecorder::new(&sim).is_err());
    sim.advance(JET_FIXED_DT, ControlInputs::neutral());
    assert!(JetRecorder::new(&sim).is_err());
}
#[test]
fn same_fixed_inputs_are_exact_across_render_cadences() {
    let trim = solve_jet_trim(
        &config(),
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap();
    let mut expected = None;
    for hz in [30_u32, 60, 144] {
        let mut sim =
            JetSimulation::from_state(config(), trim.state, JetEnvironment::default()).unwrap();
        let mut recorder = JetRecorder::new(&sim).unwrap();
        let mut controller = 0_u32;
        for _ in 0..hz * 4 {
            let report = sim.advance_with_controller(
                Seconds(1. / f64::from(hz)),
                &mut controller,
                |step, _, _| {
                    *step += 1;
                    trim.controls
                        .with_throttle(if *step < 240 { 0.3 } else { 0.7 })
                },
            );
            assert!(report.terminal().is_none());
            recorder.record(&report).unwrap();
        }
        assert_eq!(controller, 480);
        let result = (sim.snapshot(), encoded(&recorder.finish()));
        if let Some(value) = &expected {
            assert_eq!(&result, value);
        } else {
            expected = Some(result);
        }
    }
}
#[test]
fn failed_step_after_successes_preserves_last_committed_time_and_log() {
    let original = config();
    let initial = initial();
    let mach = flightsim_fdm::Atmosphere::standard()
        .sample(initial.altitude())
        .mach(MetersPerSecond(50.));
    let cfg = JetAircraftConfig::new(
        original.airframe().clone(),
        original.thrust().clone(),
        original.aero().clone(),
        OperatingEnvelope::from_definition(OperatingEnvelopeDefinition {
            pressure_ratio: [0.01, 1.],
            temperature_ratio: [0.5, 1.5],
            mach: [0., mach + 0.001],
        })
        .unwrap(),
    )
    .unwrap();
    let mut sim =
        JetSimulation::from_state(cfg.clone(), initial, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    let mut before = sim.snapshot();
    for _ in 0..1000 {
        before = sim.snapshot();
        let report = sim.advance(JET_FIXED_DT, ControlInputs::neutral().with_throttle(1.));
        recorder.record(&report).unwrap();
        if report.terminal().is_some() {
            break;
        }
    }
    assert!(sim.terminal().is_some());
    assert!(before.committed_steps > 0);
    assert_eq!(sim.snapshot(), before);
    let recording = recorder.finish();
    assert_eq!(recording.controls().len(), before.committed_steps as usize);
    assert_eq!(recording.duration(), before.elapsed);
    let mut player = JetReplayPlayer::new(cfg, recording).unwrap();
    while !player.finished() {
        player.advance(Seconds(0.25)).unwrap();
    }
    assert_eq!(player.simulation().snapshot(), before);
    assert!(matches!(
        sim.terminal().unwrap().failure.reason,
        JetFailureReason::OutsideOperatingEnvelope(_)
    ));
    assert_ne!(sim.terminal().unwrap().failure.stage, JetStage::Initial);
}

#[test]
fn signed_zero_checkpoint_and_terminal_query_changes_are_mismatches() {
    let (recording, _) = recorded(1);
    let mut bytes = encoded(&recording);
    let end = final_offset(&bytes);
    let zero = (0..13)
        .find(|index| {
            f64::from_le_bytes(
                bytes[end + index * 8..end + index * 8 + 8]
                    .try_into()
                    .unwrap(),
            )
            .abs()
            .to_bits()
                == 0
        })
        .expect("symmetric flight has zero components");
    bytes[end + zero * 8 + 7] ^= 0x80;
    let recording = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut player = JetReplayPlayer::new(config(), recording).unwrap();
    assert!(player.seek_to(1).is_err());

    let original = config();
    let cfg = JetAircraftConfig::new(
        original.airframe().clone(),
        original.thrust().clone(),
        original.aero().clone(),
        OperatingEnvelope::from_definition(OperatingEnvelopeDefinition {
            pressure_ratio: [0.01, 1.],
            temperature_ratio: [0.5, 1.5],
            mach: [0.01, 0.9],
        })
        .unwrap(),
    )
    .unwrap();
    let mut state = initial();
    state.velocity = glam::DVec3::new(0., -0., 0.);
    state.angular_velocity.z = -0.;
    let mut sim = JetSimulation::from_state(cfg.clone(), state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    let control = ControlInputs::neutral()
        .with_aileron(-0.)
        .with_elevator(-0.)
        .with_rudder(-0.)
        .with_throttle(-0.)
        .with_flaps(-0.)
        .with_brakes(-0.);
    recorder
        .record(&sim.advance(JET_FIXED_DT, control))
        .unwrap();
    assert!(state_bits_equal(sim.state(), &state));
    let mut bytes = encoded(&recorder.finish());
    let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    assert_eq!(
        decoded.terminal().unwrap().controls.aileron().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        decoded.terminal().unwrap().controls.brakes().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(
        JetReplayPlayer::new(cfg.clone(), decoded)
            .unwrap()
            .finished()
    );
    let last = bytes.len() - 1;
    bytes[last] ^= 0x80;
    let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    assert!(JetReplayPlayer::new(cfg, decoded).is_err());
}

#[test]
fn terminal_at_cursor_240_is_deferred_by_work_cap_and_settled_without_time() {
    let base = config();
    let start = initial();
    let control = ControlInputs::neutral().with_throttle(1.);
    let mut probe =
        JetSimulation::from_state(base.clone(), start, JetEnvironment::default()).unwrap();
    for _ in 0..240 {
        assert!(probe.advance(JET_FIXED_DT, control).terminal().is_none());
    }
    let mach240 = flightsim_fdm::Atmosphere::standard()
        .sample(probe.state().altitude())
        .mach(probe.airspeed());
    probe.advance(JET_FIXED_DT, control);
    let mach241 = flightsim_fdm::Atmosphere::standard()
        .sample(probe.state().altitude())
        .mach(probe.airspeed());
    assert!(mach241 > mach240);
    let cfg = JetAircraftConfig::new(
        base.airframe().clone(),
        base.thrust().clone(),
        base.aero().clone(),
        OperatingEnvelope::from_definition(OperatingEnvelopeDefinition {
            pressure_ratio: [0.01, 1.],
            temperature_ratio: [0.5, 1.5],
            mach: [0., (mach240 + mach241) / 2.],
        })
        .unwrap(),
    )
    .unwrap();
    let mut sim = JetSimulation::from_state(cfg.clone(), start, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    for _ in 0..241 {
        recorder
            .record(&sim.advance(JET_FIXED_DT, control))
            .unwrap();
    }
    assert_eq!(sim.snapshot().committed_steps, 240);
    assert!(sim.terminal().is_some());
    let recording = recorder.finish();
    let expected = sim.snapshot();
    let mut player = JetReplayPlayer::new(cfg, recording).unwrap();
    player.set_paused(true);
    assert_eq!(player.seek_to(240).unwrap(), 240);
    assert!(player.seeking());
    assert!(!player.finished());
    assert!(player.simulation().terminal().is_none());
    assert_eq!(player.advance(Seconds::ZERO).unwrap(), 0);
    assert!(!player.finished());
    assert_eq!(player.continue_seek().unwrap(), 1);
    assert!(!player.seeking());
    assert!(player.finished());
    assert_eq!(player.simulation().snapshot(), expected);
    player.restart().unwrap();
    player.set_paused(false);
    for _ in 0..8 {
        player.advance(Seconds(0.25)).unwrap();
    }
    assert!(player.finished());
    assert_eq!(player.simulation().snapshot(), expected);
}

#[test]
fn unexpected_terminal_success_fault_preserves_cursor_and_committed_snapshot() {
    let (recording, expected) = recorded(1);
    let mut bytes = encoded(&recording);
    bytes.truncate(bytes.len() - 4);
    bytes.extend(61_u32.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    for _ in 0..6 {
        bytes.extend(0_f64.to_le_bytes());
    }
    bytes.push(6);
    bytes.extend(0_u16.to_le_bytes());
    bytes.push(0);
    bytes.extend(0_u32.to_le_bytes());
    bytes.push(0);
    let recording = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut player = JetReplayPlayer::new(config(), recording).unwrap();
    assert!(player.seek_to(1).is_err());
    assert!(player.faulted());
    assert_eq!(player.cursor(), 1);
    assert_eq!(player.simulation().snapshot(), expected);
    assert!(state_bits_equal(
        player.simulation().state(),
        &expected.state
    ));
    assert!(player.simulation().terminal().is_none());
}

#[test]
fn bundled_terrain_climate_wind_and_authored_weather_replay_at_exact_committed_clock() {
    use flightsim_sim::{
        Wind,
        model_simulation::JetTerrain,
        weather::{WeatherPreset, WeatherScenario, WeatherSelection},
    };
    let trim = solve_jet_trim(
        &config(),
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap();
    let mut environment = JetEnvironment::default();
    environment.terrain = JetTerrain::BundledGlobal;
    environment.conditions = environment.conditions.with_world_climate(
        true,
        Some(flightsim_world::ClimateDate::from_annual_phase(0.25).unwrap()),
    );
    environment.conditions.wind = Wind {
        from: Radians(1.0),
        speed: MetersPerSecond(3.),
    };
    environment.conditions.turbulence = flightsim_fdm::Turbulence::light(1234);
    environment.weather = WeatherSelection::Modeled(
        WeatherScenario::from_preset(WeatherPreset::Rain, Geodetic::from_degrees(0., 0., 0.), 456)
            .unwrap(),
    );
    let mut expected = None;
    for hz in [30_u32, 144] {
        let mut sim = JetSimulation::from_state(config(), trim.state, environment).unwrap();
        let mut recorder = JetRecorder::new(&sim).unwrap();
        for _ in 0..hz * 4 {
            let report = sim.advance(Seconds(1. / f64::from(hz)), trim.controls);
            assert!(report.terminal().is_none());
            recorder.record(&report).unwrap();
        }
        let snapshot = sim.snapshot();
        let recording = recorder.finish();
        let bytes = encoded(&recording);
        assert_eq!(
            recording.conditions().environment.weather,
            environment.weather
        );
        let mut replay = JetReplayPlayer::new(
            config(),
            JetRecording::read_from(&mut bytes.as_slice()).unwrap(),
        )
        .unwrap();
        while !replay.finished() {
            replay.advance(Seconds(0.025)).unwrap();
        }
        assert_eq!(replay.simulation().snapshot(), snapshot);
        let outcome = (snapshot, bytes);
        if let Some(previous) = &expected {
            assert_eq!(&outcome, previous);
        } else {
            expected = Some(outcome);
        }
    }
}
