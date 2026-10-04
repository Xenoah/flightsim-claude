use flightsim_fdm as cedar_witness_fdm;
#[path = "../../flightsim-fdm/tests/support/cedar_boundary_witness.rs"]
mod cedar_boundary_witness;
mod near_static_turboprop_common;
use flightsim_core::{MetersPerSecond, Radians, RadiansPerSecond, Seconds};
use flightsim_fdm::{
    ControlInputs,
    turboprop::{TurbopropEnvelope, TurbopropStage, near_static::TurbopropFailureReason},
};
use flightsim_sim::{
    near_static_turboprop_simulation::{
        NEAR_STATIC_TURBOPROP_FIXED_DT, NearStaticTurbopropEnvironment,
        NearStaticTurbopropSimulation, NearStaticTurbopropSnapshot,
    },
    replay_v6::{
        NearStaticTurbopropRecorder, NearStaticTurbopropRecording, NearStaticTurbopropReplayPlayer,
        snapshot_bits_equal, state_bits_equal,
    },
};
use near_static_turboprop_common::{config, initial, wrap_forward};
fn encoded(record: &NearStaticTurbopropRecording) -> Vec<u8> {
    let mut b = Vec::new();
    record.write_to(&mut b).unwrap();
    b
}
/// Opt-in audit artifacts; ordinary test runs do not write replay files.
fn export_qa_file(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("FLIGHTSIM_V6_QA_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}
fn control(i: u32) -> ControlInputs {
    ControlInputs::new(
        f64::from(i % 17) * 0.01,
        -0.02,
        0.01,
        if i < 240 { 0.4 } else { 0.6 },
        0.1,
    )
}
fn record(steps: u32) -> (NearStaticTurbopropRecording, NearStaticTurbopropSnapshot) {
    let mut sim = NearStaticTurbopropSimulation::from_supported_state(
        config(),
        initial(),
        NearStaticTurbopropEnvironment::default(),
        control(0),
    )
    .unwrap();
    let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    for i in 0..steps {
        let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(i));
        assert!(report.terminal().is_none(), "{:?}", report.terminal());
        recorder.record(&report).unwrap();
    }
    (recorder.finish(), sim.snapshot())
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn final_offset(b: &[u8]) -> usize {
    let n = 14 + u32_at(b, 10) as usize;
    let k = n + 4 + 48 * u32_at(b, n) as usize;
    k + 4 + (u32_at(b, k) as usize - 1) * 132 + 4
}
#[test]
fn numerical_full_state_replay_rewind_and_bounded_seek_restore_history() {
    let (r, snapshot) = record(481);
    let bytes = encoded(&r);
    assert_eq!(
        r.checkpoints().iter().map(|c| c.frame).collect::<Vec<_>>(),
        [120, 240, 360, 480, 481]
    );
    let mut p = NearStaticTurbopropReplayPlayer::new(
        config(),
        NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap(),
    )
    .unwrap();
    while !p.finished() {
        assert!(p.advance(Seconds(0.19)).unwrap() <= 240);
    }
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
    p.set_paused(true);
    assert_eq!(p.seek_to(481).unwrap(), 240);
    assert!(p.seeking());
    assert_eq!(p.continue_seek().unwrap(), 240);
    assert_eq!(p.continue_seek().unwrap(), 1);
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
    assert!(p.paused());
    p.seek_to(17).unwrap();
    let (_, at17) = record(17);
    assert!(snapshot_bits_equal(&at17, &p.simulation().snapshot()));
    p.restart().unwrap();
    assert_eq!(p.cursor(), 0);
    assert!(state_bits_equal(p.simulation().state(), &initial()));
}
#[test]
fn controls_and_all_engine_states_are_cadence_independent() {
    let mut expected = None;
    for hz in [30, 60, 144] {
        let mut sim = NearStaticTurbopropSimulation::from_state(
            config(),
            initial(),
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
        let mut c = 0_u32;
        for _ in 0..hz * 4 {
            let r = sim.advance_with_controller(Seconds(1. / f64::from(hz)), &mut c, |i, _, _| {
                let result = control(*i);
                *i += 1;
                result
            });
            assert!(r.terminal().is_none(), "{:?}", r.terminal());
            rec.record(&r).unwrap();
        }
        assert_eq!(c, 480);
        let value = (sim.snapshot(), encoded(&rec.finish()));
        if let Some(old) = &expected {
            assert_eq!(&value, old);
        } else {
            expected = Some(value);
        }
    }
}
#[test]
fn initial_domain_terminal_rolls_back_controller_time_history_and_unused_budget() {
    let mut state = initial();
    state.shaft_rad_s = RadiansPerSecond(270.);
    let mut sim = NearStaticTurbopropSimulation::from_state(
        config(),
        state,
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    assert!(
        NearStaticTurbopropSimulation::from_supported_state(
            config(),
            state,
            NearStaticTurbopropEnvironment::default(),
            control(0)
        )
        .is_err()
    );
    let before = sim.snapshot();
    let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let mut c = 5;
    let empty = sim.advance_with_controller(Seconds::ZERO, &mut c, |c, _, _| {
        *c += 1;
        control(0)
    });
    assert_eq!(empty.attempted_steps(), 0);
    assert_eq!(c, 5);
    let r = sim.advance_with_controller(Seconds(0.251), &mut c, |c, _, _| {
        *c += 1;
        control(0)
    });
    assert_eq!(r.attempted_steps(), 1);
    assert_eq!(c, 5);
    assert!(snapshot_bits_equal(&before, &sim.snapshot()));
    assert_eq!(sim.accumulated(), Seconds::ZERO);
    recorder.record(&r).unwrap();
    let r = recorder.finish();
    assert_eq!(r.controls().len(), 0);
    assert!(r.terminal().is_some());
    let p = NearStaticTurbopropReplayPlayer::new(config(), r).unwrap();
    assert!(p.finished());
    assert!(snapshot_bits_equal(&before, &p.simulation().snapshot()));
}
#[test]
fn later_stage_failure_preserves_authentic_full_state_and_replays_terminal() {
    let original = config();
    let mut envelope = *original.envelope().definition();
    envelope.relative_shaft_rad_s = [179.5, 260.];
    let cfg = wrap_forward(
        flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
            original.airframe().clone(),
            original.turbine().clone(),
            original.propeller().forward_map().clone(),
            *original.governor(),
            original.aero().clone(),
            TurbopropEnvelope::from_definition(envelope).unwrap(),
        )
        .unwrap(),
    );
    let mut sim = NearStaticTurbopropSimulation::from_state(
        cfg.clone(),
        initial(),
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let mut before = sim.snapshot();
    for _ in 0..1000 {
        before = sim.snapshot();
        let r = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, ControlInputs::neutral());
        rec.record(&r).unwrap();
        if r.terminal().is_some() {
            break;
        }
    }
    let event = sim
        .terminal()
        .expect("narrow shaft envelope stops deceleration");
    assert!(before.committed_steps > 0);
    assert_ne!(event.failure.stage, TurbopropStage::Initial);
    assert!(matches!(
        event.failure.reason,
        TurbopropFailureReason::OutsideOperatingEnvelope(_)
    ));
    assert!(snapshot_bits_equal(&before, &sim.snapshot()));
    let r = rec.finish();
    assert_eq!(r.duration(), before.elapsed);
    let mut p = NearStaticTurbopropReplayPlayer::new(cfg, r).unwrap();
    while !p.finished() {
        p.advance(Seconds(0.25)).unwrap();
    }
    assert!(snapshot_bits_equal(&before, &p.simulation().snapshot()));
}
#[test]
fn engine_checkpoint_drift_rolls_back_entire_attempt_and_drops_budget() {
    for index in 13..16 {
        let (r, _) = record(2);
        let mut bytes = encoded(&r);
        let off = final_offset(&bytes) + 8 * index;
        let value = f64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
        bytes[off..off + 8].copy_from_slice(&f64::from_bits(value.to_bits() + 1).to_le_bytes());
        let record = NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
        let mut p = NearStaticTurbopropReplayPlayer::new(config(), record).unwrap();
        p.advance(NEAR_STATIC_TURBOPROP_FIXED_DT).unwrap();
        let before = p.simulation().snapshot();
        assert!(p.advance(Seconds(0.25)).is_err());
        assert!(p.faulted());
        assert_eq!(p.cursor(), 1);
        assert!(snapshot_bits_equal(&before, &p.simulation().snapshot()));
    }
}
#[test]
fn recorder_rejects_noncontiguous_reports_and_preserves_prior_export() {
    let mut a = NearStaticTurbopropSimulation::from_state(
        config(),
        initial(),
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&a).unwrap();
    let r = a.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
    rec.record(&r).unwrap();
    let before = encoded(&rec.export());
    let mut initial = initial();
    initial.blade_pitch_rad = Radians(0.31);
    let mut b = NearStaticTurbopropSimulation::from_state(
        config(),
        initial,
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    b.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
    let unrelated = b.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(1));
    assert!(rec.record(&unrelated).is_err());
    assert!(rec.closed());
    assert_eq!(encoded(&rec.export()), before);
}
#[test]
fn wind_climate_weather_and_contact_history_reconstruct_from_zero() {
    use flightsim_sim::{
        Wind,
        near_static_turboprop_simulation::NearStaticTurbopropTerrain,
        weather::{WeatherPreset, WeatherScenario, WeatherSelection},
    };
    let mut env = NearStaticTurbopropEnvironment::default();
    env.terrain = NearStaticTurbopropTerrain::BundledGlobal;
    env.conditions = env.conditions.with_world_climate(
        true,
        Some(flightsim_world::ClimateDate::from_annual_phase(0.25).unwrap()),
    );
    env.conditions.wind = Wind {
        from: Radians(1.),
        speed: MetersPerSecond(1.),
    };
    env.conditions.turbulence = flightsim_fdm::Turbulence::light(1234);
    env.weather = WeatherSelection::Modeled(
        WeatherScenario::from_preset(WeatherPreset::Rain, initial().rigid_body.geodetic(), 456)
            .unwrap(),
    );
    let mut sim =
        NearStaticTurbopropSimulation::from_supported_state(config(), initial(), env, control(0))
            .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    for i in 0..241 {
        let r = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(i));
        assert!(r.terminal().is_none(), "{:?}", r.terminal());
        rec.record(&r).unwrap();
    }
    let snapshot = sim.snapshot();
    let r = rec.finish();
    assert_eq!(r.conditions().environment.weather, env.weather);
    let mut p = NearStaticTurbopropReplayPlayer::new(config(), r).unwrap();
    assert_eq!(p.seek_to(241).unwrap(), 240);
    assert_eq!(p.continue_seek().unwrap(), 1);
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}

#[test]
fn terminal_scratch_success_or_diagnostic_mismatch_never_commits_a_step() {
    let mut outside = initial();
    outside.shaft_rad_s = RadiansPerSecond(270.);
    let mut sim = NearStaticTurbopropSimulation::from_state(
        config(),
        outside,
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    rec.record(&sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0)))
        .unwrap();
    let terminal = encoded(&rec.finish());
    let event_length = u32_at(&terminal, terminal.len() - 110 - 4) as usize;
    assert_eq!(event_length, 110);
    // Append structurally valid but false evidence at a supported final state.
    let (r, expected) = record(1);
    let mut bytes = encoded(&r);
    bytes.truncate(bytes.len() - 4);
    bytes.extend(&u32::try_from(event_length).unwrap().to_le_bytes());
    let mut event = terminal[terminal.len() - event_length..].to_vec();
    event[..4].copy_from_slice(&1_u32.to_le_bytes());
    bytes.extend(event);
    let r = NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut p = NearStaticTurbopropReplayPlayer::new(config(), r).unwrap();
    assert!(p.advance(Seconds(0.25)).is_err());
    assert!(p.faulted());
    assert_eq!(p.cursor(), 1);
    assert!(snapshot_bits_equal(&expected, &p.simulation().snapshot()));
    assert!(p.simulation().terminal().is_none());
    // One ULP of an already valid diagnostic remains structurally admissible,
    // but must be rejected as mismatched physical evidence at cursor zero.
    let mut changed = terminal.clone();
    let last = changed.len() - 8;
    let x = u64::from_le_bytes(changed[last..].try_into().unwrap());
    changed[last..].copy_from_slice(&(x + 1).to_le_bytes());
    let r = NearStaticTurbopropRecording::read_from(&mut changed.as_slice()).unwrap();
    assert!(NearStaticTurbopropReplayPlayer::new(config(), r).is_err());
}
#[test]
fn terminal_attempt_at_cursor_240_shares_the_seek_work_cap() {
    let (before, _) = record(240);
    let (after, _) = record(241);
    let mach = |s: &flightsim_fdm::turboprop::TurbopropState| {
        flightsim_fdm::Atmosphere::standard()
            .sample(s.rigid_body.altitude())
            .mach(MetersPerSecond(s.rigid_body.velocity.length()))
    };
    let original = config();
    let mut e = *original.envelope().definition();
    e.mach[1] = (mach(before.final_state()) + mach(after.final_state())) / 2.;
    assert!(mach(before.final_state()) < e.mach[1] && mach(after.final_state()) > e.mach[1]);
    let cfg = wrap_forward(
        flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
            original.airframe().clone(),
            original.turbine().clone(),
            original.propeller().forward_map().clone(),
            *original.governor(),
            original.aero().clone(),
            TurbopropEnvelope::from_definition(e).unwrap(),
        )
        .unwrap(),
    );
    let mut sim = NearStaticTurbopropSimulation::from_state(
        cfg.clone(),
        initial(),
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    for i in 0..=240 {
        let r = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(i));
        rec.record(&r).unwrap();
        if i < 240 {
            assert!(r.terminal().is_none());
        } else {
            assert!(r.terminal().is_some());
        }
    }
    let snapshot = sim.snapshot();
    let r = rec.finish();
    assert_eq!(r.controls().len(), 240);
    let mut p = NearStaticTurbopropReplayPlayer::new(cfg, r).unwrap();
    assert_eq!(p.seek_to(240).unwrap(), 240);
    assert!(p.seeking());
    assert!(!p.finished());
    assert!(p.simulation().terminal().is_none());
    assert_eq!(p.continue_seek().unwrap(), 1);
    assert!(p.finished());
    assert!(!p.seeking());
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}

#[test]
fn real_contact_touchdown_and_log_reconstruct_on_seek() {
    use flightsim_core::{Attitude, Geodetic, Ned};
    let mut state = initial();
    state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., 1.8),
        Attitude::default(),
        Ned::new(5., 0., 1.),
    );
    let mut sim = NearStaticTurbopropSimulation::from_state(
        config(),
        state,
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    for i in 0..120 {
        let r = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(i));
        assert!(r.terminal().is_none(), "{:?}", r.terminal());
        rec.record(&r).unwrap();
    }
    let snapshot = sim.snapshot();
    assert!(snapshot.touchdown_count > 0);
    assert!(snapshot.last_touchdown.is_some());
    assert!(snapshot.log.airborne_time.get() > 0.);
    let mut p = NearStaticTurbopropReplayPlayer::new(config(), rec.finish()).unwrap();
    p.seek_to(120).unwrap();
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
    p.seek_to(1).unwrap();
    assert_eq!(p.simulation().snapshot().touchdown_count, 0);
    p.seek_to(120).unwrap();
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}
#[test]
fn interior_propeller_power_bound_is_recordable_and_reproduces_exact_error() {
    use flightsim_core::{Attitude, Geodetic, Ned};
    use flightsim_fdm::turboprop::{PropellerCellDefinition, PropellerMap, PropellerPowerBound};
    let original = config();
    let mut propeller = original.propeller().forward_map().definition().clone();
    propeller.cells = vec![
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0., cp: 0.1 },
        PropellerCellDefinition { ct: 0., cp: 0.1 },
    ];
    let cfg = wrap_forward(
        flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
            original.airframe().clone(),
            original.turbine().clone(),
            PropellerMap::from_definition(propeller).unwrap(),
            *original.governor(),
            original.aero().clone(),
            *original.envelope(),
        )
        .unwrap(),
    );
    let mut state = initial();
    state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., 1000.),
        Attitude::default(),
        Ned::new(180. / std::f64::consts::PI, 0., 0.),
    );
    let mut sim = NearStaticTurbopropSimulation::from_state(
        cfg.clone(),
        state,
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    let snapshot = sim.snapshot();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
    let event = report.terminal().unwrap();
    assert_eq!(
        event.failure.reason,
        TurbopropFailureReason::PropellerPowerBound(PropellerPowerBound::BelowIdealDisk)
    );
    assert!((event.failure.diagnostics.values().advance_ratio.unwrap().0 - 1.).abs() < 1e-12);
    assert!(snapshot_bits_equal(&snapshot, &sim.snapshot()));
    rec.record(&report).unwrap();
    let bytes = encoded(&rec.finish());
    let r = NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
    let p = NearStaticTurbopropReplayPlayer::new(cfg, r).unwrap();
    assert!(p.finished());
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}
#[test]
fn parked_start_retains_explicit_engine_bits_without_warmup_and_restart_is_atomic() {
    use flightsim_core::Geodetic;
    use flightsim_sim::aircraft_profile_v4::AircraftProfileV4;
    let profile = AircraftProfileV4::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
    ))
    .unwrap();
    let engine = profile.running_start();
    let mut sim = NearStaticTurbopropSimulation::parked(
        profile.configuration().clone(),
        Geodetic::from_degrees(35., 139., 0.),
        Radians(0.7),
        NearStaticTurbopropEnvironment::default(),
        engine,
    )
    .unwrap();
    assert_eq!(sim.elapsed(), Seconds::ZERO);
    assert_eq!(
        sim.state().turbine_fraction.get().to_bits(),
        engine.turbine_fraction().get().to_bits()
    );
    assert_eq!(sim.state().shaft_rad_s, engine.shaft_speed());
    assert_eq!(sim.state().blade_pitch_rad, engine.blade_pitch());
    assert!(
        sim.snapshot()
            .gear_clearances
            .iter()
            .all(|c| c.get().abs() < 0.001)
    );
    let before = sim.snapshot();
    let mut invalid = *sim.state();
    invalid.shaft_rad_s = RadiansPerSecond(0.);
    assert!(sim.restart_at(invalid).is_err());
    assert!(snapshot_bits_equal(&before, &sim.snapshot()));
    assert!(
        sim.restart_parked_at(Geodetic::from_degrees(100., 0., 0.), Radians::ZERO, engine)
            .is_err()
    );
    assert!(snapshot_bits_equal(&before, &sim.snapshot()));
    sim.restart_parked_at(Geodetic::from_degrees(36., 140., 0.), Radians::ZERO, engine)
        .unwrap();
    assert_eq!(sim.snapshot().committed_steps, 0);
    assert_eq!(sim.snapshot().touchdown_count, 0);
    assert!(sim.terminal().is_none());
}
#[test]
fn turbine_response_matches_an_independent_analytic_oracle() {
    let mut sim = NearStaticTurbopropSimulation::from_state(
        config(),
        initial(),
        NearStaticTurbopropEnvironment::default(),
    )
    .unwrap();
    for _ in 0..120 {
        let r = sim.advance(
            NEAR_STATIC_TURBOPROP_FIXED_DT,
            ControlInputs::neutral().with_throttle(0.7),
        );
        assert!(r.terminal().is_none());
    }
    let expected = 0.7 + (0.4 - 0.7) * (-1_f64).exp();
    assert!((sim.state().turbine_fraction.get() - expected).abs() < 3e-14);
    assert_ne!(sim.state().shaft_rad_s.get().to_bits(), 180_f64.to_bits());
    assert_ne!(
        sim.state().blade_pitch_rad.get().to_bits(),
        0.3_f64.to_bits()
    );
}

#[test]
fn terminal_only_report_must_match_all_three_engine_state_scalars() {
    for engine_scalar in 0..3 {
        let mut state = initial();
        state.shaft_rad_s = RadiansPerSecond(270.);
        let sim = NearStaticTurbopropSimulation::from_state(
            config(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
        let prior = encoded(&recorder.export());
        match engine_scalar {
            0 => {
                state.turbine_fraction =
                    flightsim_fdm::turboprop::TurbineFraction::new(0.41).unwrap()
            }
            1 => state.shaft_rad_s = RadiansPerSecond(280.),
            _ => state.blade_pitch_rad = Radians(0.31),
        }
        let mut other = NearStaticTurbopropSimulation::from_state(
            config(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let report = other.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
        assert!(report.terminal().is_some());
        assert!(
            recorder.record(&report).is_err(),
            "foreign engine scalar {engine_scalar}"
        );
        assert!(recorder.closed());
        assert_eq!(encoded(&recorder.export()), prior);
    }
}

#[test]
fn visual_time_overflow_closes_before_any_report_append() {
    let mut environment = NearStaticTurbopropEnvironment::default();
    environment.conditions.time_rate = 1e308;
    let mut sim =
        NearStaticTurbopropSimulation::from_state(config(), initial(), environment).unwrap();
    let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let prefix = encoded(&recorder.export());
    let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
    assert_eq!(report.committed_steps(), 1);
    assert!(recorder.record(&report).is_err());
    assert!(recorder.closed());
    assert_eq!(encoded(&recorder.export()), prefix);
    let recording = recorder.finish();
    recording.validate().unwrap();
    assert!(recording.controls().is_empty());
}

#[test]
fn foreign_model_or_environment_reports_reject_even_with_identical_full_state() {
    for foreign in 0..3 {
        let a = NearStaticTurbopropSimulation::from_state(
            config(),
            initial(),
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let mut rec = NearStaticTurbopropRecorder::new(&a).unwrap();
        let prior = encoded(&rec.export());
        let mut env = NearStaticTurbopropEnvironment::default();
        let mut cfg = config();
        match foreign {
            0 => env.conditions.wind.speed = MetersPerSecond(1000.),
            1 => {
                let mut e = *cfg.envelope().definition();
                e.relative_shaft_rad_s[1] = 179.;
                let mut g = *cfg.governor().definition();
                g.reference_rad_s = 170.;
                cfg = wrap_forward(
                    flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
                        cfg.airframe().clone(),
                        cfg.turbine().clone(),
                        cfg.propeller().forward_map().clone(),
                        flightsim_fdm::turboprop::SampledGovernor::from_definition(g).unwrap(),
                        cfg.aero().clone(),
                        TurbopropEnvelope::from_definition(e).unwrap(),
                    )
                    .unwrap(),
                );
            }
            // Signed zero is part of exact environment provenance even though
            // this particular numerical wind vector is physically unchanged.
            _ => env.conditions.wind.speed = MetersPerSecond(-0.),
        }
        let mut b = NearStaticTurbopropSimulation::from_state(cfg, initial(), env).unwrap();
        assert!(state_bits_equal(a.state(), b.state()));
        let report = b.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
        if foreign < 2 {
            assert!(report.terminal().is_some());
        } else {
            assert_eq!(report.committed_steps(), 1);
        }
        assert!(rec.record(&report).is_err());
        assert!(rec.closed());
        assert_eq!(encoded(&rec.export()), prior);
    }
}

#[test]
fn k2_k3_k4_endpoint_and_later_substep_failures_roll_back_host_and_controller() {
    use flightsim_core::{Attitude, Geodetic, Ned};
    use flightsim_fdm::turboprop::{TurbineFraction, TurbinePowerTable};
    for (maximum, fraction, pitch, throttle, rise, stage, substep) in [
        (180.01, 0.4, 0.3, 1.0, 1.0, TurbopropStage::K2, 0),
        (180.0215, 0.4, 0.3, 1.0, 1.0, TurbopropStage::K3, 0),
        (180.03, 0.4, 0.3, 1.0, 1.0, TurbopropStage::K4, 0),
        // Independent FDM witness brackets K4=180.11406012828962 and
        // weighted endpoint=180.11406014776125 for this authored fixture.
        (
            180.11406013802542,
            0.2,
            0.1,
            0.0,
            1.0,
            TurbopropStage::Endpoint,
            0,
        ),
        (180.05, 0.4, 0.3, 1.0, 0.1, TurbopropStage::K4, 1),
    ] {
        let original = config();
        let mut envelope = *original.envelope().definition();
        envelope.relative_shaft_rad_s[1] = maximum;
        let mut turbine = original.turbine().definition().clone();
        turbine.rise_seconds = rise;
        let cfg = wrap_forward(
            flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
                original.airframe().clone(),
                TurbinePowerTable::from_definition(turbine).unwrap(),
                original.propeller().forward_map().clone(),
                *original.governor(),
                original.aero().clone(),
                TurbopropEnvelope::from_definition(envelope).unwrap(),
            )
            .unwrap(),
        );
        let mut state = initial();
        state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0., 0., 1000.),
            Attitude::default(),
            Ned::new(40., 0., 0.),
        );
        state.turbine_fraction = TurbineFraction::new(fraction).unwrap();
        state.blade_pitch_rad = Radians(pitch);
        let mut sim = NearStaticTurbopropSimulation::from_state(
            cfg.clone(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let before = sim.snapshot();
        let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
        let mut controller = 12;
        let report = sim.advance_with_controller(Seconds(0.25), &mut controller, |c, _, _| {
            *c += 1;
            ControlInputs::neutral().with_throttle(throttle)
        });
        let error = report.terminal().unwrap().failure;
        assert_eq!(
            (error.stage, error.substep),
            (stage, substep),
            "maximum {maximum}"
        );
        assert_eq!(report.attempted_steps(), 1);
        assert_eq!(controller, 12);
        assert!(snapshot_bits_equal(&before, &sim.snapshot()));
        assert_eq!(sim.accumulated(), Seconds::ZERO);
        recorder.record(&report).unwrap();
        let player = NearStaticTurbopropReplayPlayer::new(cfg, recorder.finish()).unwrap();
        assert!(player.finished());
        assert!(snapshot_bits_equal(
            &before,
            &player.simulation().snapshot()
        ));
    }
}

#[test]
fn recorded_first_attempt_uses_prospective_wind_clock_for_initial_admission() {
    use flightsim_fdm::{Atmosphere, Environment, Turbulence};
    use flightsim_sim::Wind;
    let state = initial();
    let position = state.rigid_body.geodetic();
    {
        let mut env = NearStaticTurbopropEnvironment::default();
        env.conditions.wind = Wind {
            from: Radians(1.),
            speed: MetersPerSecond(1.),
        };
        env.conditions.turbulence = Turbulence::light(0);
        let crossflow = |t| {
            let e = Environment::with_wind_ned(
                Atmosphere::standard(),
                position,
                env.conditions.wind.to_ned(),
            )
            .with_turbulence(env.conditions.turbulence, t, position);
            let relative =
                state.rigid_body.orientation.inverse() * (state.rigid_body.velocity - e.wind_ecef);
            relative.y.hypot(relative.z) / 180.
        };
        let at_zero = crossflow(Seconds::ZERO);
        let next = crossflow(NEAR_STATIC_TURBOPROP_FIXED_DT);
        let original = config();
        let mut e = *original.envelope().definition();
        // Fixed witness: zero-time ratio .00254737687272323 exceeds the
        // cap, prospective +dt ratio .00254727921162822 is supported.
        e.maximum_crossflow_tip_ratio = 0.002547328042175725;
        assert!(next < e.maximum_crossflow_tip_ratio && at_zero > e.maximum_crossflow_tip_ratio);
        let cfg = wrap_forward(
            flightsim_fdm::turboprop::TurbopropAircraftConfig::new(
                original.airframe().clone(),
                original.turbine().clone(),
                original.propeller().forward_map().clone(),
                *original.governor(),
                original.aero().clone(),
                TurbopropEnvelope::from_definition(e).unwrap(),
            )
            .unwrap(),
        );
        let mut sim = NearStaticTurbopropSimulation::from_state(cfg.clone(), state, env).unwrap();
        let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
        let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
        assert!(report.terminal().is_none());
        assert!(
            NearStaticTurbopropSimulation::from_supported_state(
                cfg.clone(),
                state,
                env,
                control(0)
            )
            .is_err()
        );
        rec.record(&report).unwrap();
        let expected = sim.snapshot();
        let mut p = NearStaticTurbopropReplayPlayer::new(cfg, rec.finish()).unwrap();
        while !p.finished() {
            p.advance(NEAR_STATIC_TURBOPROP_FIXED_DT).unwrap();
        }
        assert!(snapshot_bits_equal(&expected, &p.simulation().snapshot()));
    }
}

#[test]
fn visual_time_boundary_keeps_an_existing_nonempty_prefix_exportable() {
    let mut environment = NearStaticTurbopropEnvironment::default();
    environment.conditions.start_epoch = flightsim_sim::replay::MAX_VISUAL_EPOCH - 1.;
    environment.conditions.time_rate = 0.6 * 86_400. / NEAR_STATIC_TURBOPROP_FIXED_DT.get();
    let mut sim =
        NearStaticTurbopropSimulation::from_state(config(), initial(), environment).unwrap();
    let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    recorder
        .record(&sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0)))
        .unwrap();
    let prefix = encoded(&recorder.export());
    assert_eq!(recorder.export().controls().len(), 1);
    let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(1));
    assert_eq!(report.committed_steps(), 1);
    assert!(recorder.record(&report).is_err());
    assert!(recorder.closed());
    assert_eq!(encoded(&recorder.export()), prefix);
    let mut player = NearStaticTurbopropReplayPlayer::new(config(), recorder.finish()).unwrap();
    player.advance(NEAR_STATIC_TURBOPROP_FIXED_DT).unwrap();
    assert_eq!(player.cursor(), 1);
    assert_eq!(sim.snapshot().committed_steps, 2);
}

#[test]
fn negative_inflow_crosses_zero_and_reconstructs_every_host_field_over_601_steps() {
    use glam::DVec3;
    let mut start = initial();
    // The synthetic aircraft points vertically down and initially moves up.
    // Gravity carries the authored negative-J branch into positive flow.
    start.rigid_body.velocity = start.rigid_body.orientation * DVec3::new(-0.2, 0., 0.);
    let mut sim = NearStaticTurbopropSimulation::from_supported_state(
        config(),
        start,
        NearStaticTurbopropEnvironment::default(),
        control(0),
    )
    .unwrap();
    let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let mut negative_endpoints = 0;
    let mut positive_endpoints = 0;
    let mut snapshots = Vec::new();
    for i in 0..601 {
        let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(i));
        assert_eq!(
            report.committed_steps(),
            1,
            "step {i}: {:?}",
            report.terminal()
        );
        assert!(report.terminal().is_none());
        rec.record(&report).unwrap();
        let rigid = sim.state().rigid_body;
        let axial = (rigid.orientation.inverse() * rigid.velocity).x;
        let omega = sim.state().shaft_rad_s.get() + rigid.angular_velocity.x;
        let j = axial / (omega / std::f64::consts::TAU * 2.);
        if j < 0. {
            negative_endpoints += 1;
            assert!(j >= -0.01);
        } else if j > 0. {
            positive_endpoints += 1;
        }
        if [17, 241, 481, 601].contains(&(i + 1)) {
            snapshots.push((i + 1, sim.snapshot()));
        }
    }
    assert!(negative_endpoints >= 2);
    assert!(positive_endpoints > 590);
    let expected = sim.snapshot();
    assert_ne!(expected.state.shaft_rad_s, start.shaft_rad_s);
    assert_ne!(expected.state.blade_pitch_rad, start.blade_pitch_rad);
    let recording = rec.finish();
    let identity = recording.conditions().identity;
    assert_eq!(
        (
            identity.algorithm,
            identity.schema,
            identity.kind,
            identity.law_revision
        ),
        (1, 4, 3, 2)
    );
    assert_eq!(recording.conditions().simulation_revision, 1);
    assert_eq!(
        recording
            .checkpoints()
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        [120, 240, 360, 480, 600, 601]
    );
    let bytes = encoded(&recording);
    export_qa_file("near-static-negative-crossing-601-v6.fsreplay", &bytes);
    for hz in [30, 60, 144] {
        let mut player = NearStaticTurbopropReplayPlayer::new(
            config(),
            NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap(),
        )
        .unwrap();
        while !player.finished() {
            assert!(player.advance(Seconds(1. / f64::from(hz))).unwrap() <= 240);
        }
        assert!(snapshot_bits_equal(
            &expected,
            &player.simulation().snapshot()
        ));
        player.set_paused(true);
        for (cursor, snapshot) in snapshots.iter().rev() {
            assert!(player.seek_to(*cursor).unwrap() <= 240);
            while player.seeking() {
                assert!(player.continue_seek().unwrap() <= 240);
            }
            assert!(snapshot_bits_equal(
                snapshot,
                &player.simulation().snapshot()
            ));
        }
        player.restart().unwrap();
        assert_eq!(player.cursor(), 0);
        assert!(state_bits_equal(player.simulation().state(), &start));
    }
}

#[test]
fn genuine_near_static_domain_and_invalid_scale_terminals_replay_exactly() {
    use flightsim_core::{Attitude, Geodetic, Ned};
    use flightsim_fdm::turboprop::AxisStatus;
    use glam::{DQuat, DVec3};
    for (velocity, stage, adverse, transverse) in [
        (
            DVec3::new(-0.5, 0., 0.),
            TurbopropStage::Initial,
            true,
            false,
        ),
        (
            DVec3::new(-0.1, 1., 0.),
            TurbopropStage::Initial,
            false,
            true,
        ),
        (DVec3::new(-0.1, 0., 0.42), TurbopropStage::K2, false, true),
        (DVec3::new(-0.1, 0., 0.40), TurbopropStage::K4, false, true),
        (
            DVec3::new(-f64::from_bits(1), 0., 0.),
            TurbopropStage::Initial,
            false,
            false,
        ),
        (
            DVec3::new(-0.1, f64::from_bits(1), 0.),
            TurbopropStage::Initial,
            false,
            false,
        ),
    ] {
        let mut state = initial();
        state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0., 0., 1000.),
            Attitude::default(),
            Ned::new(0., 0., 0.),
        );
        if !adverse && !transverse {
            state.rigid_body.orientation = DQuat::IDENTITY;
        }
        state.rigid_body.velocity = state.rigid_body.orientation * velocity;
        let mut sim = NearStaticTurbopropSimulation::from_state(
            config(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let before = sim.snapshot();
        let mut rec = NearStaticTurbopropRecorder::new(&sim).unwrap();
        let mut controller = 7;
        let report = sim.advance_with_controller(Seconds(0.25), &mut controller, |c, _, _| {
            *c += 1;
            ControlInputs::neutral()
        });
        let error = report
            .terminal()
            .expect("genuine negative-flow rejection")
            .failure;
        assert_eq!((error.stage, error.substep), (stage, 0));
        assert_eq!(controller, 7);
        assert_eq!(report.attempted_steps(), 1);
        assert_eq!(sim.accumulated(), Seconds::ZERO);
        assert!(snapshot_bits_equal(&before, &sim.snapshot()));
        if adverse || transverse {
            let TurbopropFailureReason::OutsideNearStaticDomain(domain) = error.reason else {
                panic!("wrong negative-flow failure: {error:?}")
            };
            assert_eq!(domain.adverse_inflow == AxisStatus::Above, adverse);
            assert_eq!(domain.transverse_inflow == AxisStatus::Above, transverse);
            let values = error.diagnostics.values();
            assert!(values.advance_ratio.unwrap().0 < 0.);
            assert!(values.hover_velocity.unwrap().get() > 0.);
            assert_eq!(values.adverse_inflow_ratio.unwrap() > 0.1, adverse);
            assert_eq!(values.transverse_inflow_ratio.unwrap() > 0.1, transverse);
        } else {
            assert_eq!(error.reason, TurbopropFailureReason::InvalidNearStaticScale);
            let values = error.diagnostics.values();
            if velocity.y > 0. {
                // Nonzero negative J was established, but positive transverse
                // velocity divided by the hover scale underflows to zero.
                assert_eq!(velocity.y.to_bits(), 1);
                assert!(values.advance_ratio.unwrap().0 < 0.);
            } else {
                assert!(values.advance_ratio.is_none());
            }
            assert!(values.hover_velocity.is_none());
            assert!(values.adverse_inflow_ratio.is_none());
            assert!(values.transverse_inflow_ratio.is_none());
        }
        rec.record(&report).unwrap();
        let bytes = encoded(&rec.finish());
        if !adverse && !transverse {
            // J-underflow precedes the propeller query (0x1f7). Transverse
            // ratio underflow follows it (0x1ff), before the scale triple.
            let expected_mask: u16 = if velocity.y > 0. { 0x1ff } else { 0x1f7 };
            let event_length = 62 + 8 * usize::try_from(expected_mask.count_ones()).unwrap();
            let event = bytes.len() - event_length;
            assert_eq!(
                u16::from_le_bytes(bytes[event + 60..event + 62].try_into().unwrap()),
                expected_mask
            );
        }
        let recording = NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
        let player = NearStaticTurbopropReplayPlayer::new(config(), recording).unwrap();
        assert!(player.finished());
        assert_eq!(player.simulation().terminal().unwrap().failure, error);
        assert!(snapshot_bits_equal(
            &before,
            &player.simulation().snapshot()
        ));
    }
}

#[test]
fn a_negative_row_change_invalidates_player_identity_and_report_provenance() {
    use flightsim_fdm::turboprop::near_static::TurbopropAircraftConfig;
    let original = config();
    let mut rows = original.propeller().negative_rows().clone();
    rows[0][0].cp = 0.031;
    let foreign =
        TurbopropAircraftConfig::from_forward(original.forward_config().clone(), rows).unwrap();
    let (recording, _) = record(1);
    assert!(NearStaticTurbopropReplayPlayer::new(foreign.clone(), recording).is_err());
    for terminal_only in [false, true] {
        let mut state = initial();
        if terminal_only {
            state.shaft_rad_s = RadiansPerSecond(270.);
        }
        let host = NearStaticTurbopropSimulation::from_state(
            original.clone(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let mut recorder = NearStaticTurbopropRecorder::new(&host).unwrap();
        let prefix = encoded(&recorder.export());
        let mut foreign_host = NearStaticTurbopropSimulation::from_state(
            foreign.clone(),
            state,
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let report = foreign_host.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
        assert_eq!(report.terminal().is_some(), terminal_only);
        assert_eq!(report.committed_steps(), usize::from(!terminal_only));
        assert!(recorder.record(&report).is_err());
        assert!(recorder.closed());
        assert_eq!(encoded(&recorder.export()), prefix);
    }
}

#[test]
fn signed_zero_and_subnormal_full_state_bits_survive_export_and_empty_admission() {
    use flightsim_fdm::turboprop::TurbineFraction;
    let mut state = initial();
    state.turbine_fraction = TurbineFraction::new(f64::from_bits(1)).unwrap();
    state.rigid_body.angular_velocity.x = -0.;
    state.rigid_body.angular_velocity.y = f64::from_bits(1);
    let sim = NearStaticTurbopropSimulation::from_supported_state(
        config(),
        state,
        NearStaticTurbopropEnvironment::default(),
        control(0),
    )
    .unwrap();
    let recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let bytes = encoded(&recorder.finish());
    let recording = NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
    assert_eq!(encoded(&recording), bytes);
    assert!(state_bits_equal(
        &recording.conditions().initial_state,
        &state
    ));
    assert!(state_bits_equal(recording.final_state(), &state));
    let player = NearStaticTurbopropReplayPlayer::new(config(), recording).unwrap();
    assert!(player.finished());
    assert!(snapshot_bits_equal(
        &sim.snapshot(),
        &player.simulation().snapshot()
    ));
}

#[test]
fn unchanged_state_reports_still_require_exact_terrain_weather_and_clock_origin() {
    use flightsim_core::Meters;
    use flightsim_fdm::Turbulence;
    use flightsim_sim::{
        near_static_turboprop_simulation::NearStaticTurbopropTerrain,
        weather::{WeatherPreset, WeatherScenario, WeatherSelection},
    };
    for field in 0..5 {
        let host = NearStaticTurbopropSimulation::from_state(
            config(),
            initial(),
            NearStaticTurbopropEnvironment::default(),
        )
        .unwrap();
        let mut recorder = NearStaticTurbopropRecorder::new(&host).unwrap();
        let prefix = encoded(&recorder.export());
        let mut environment = NearStaticTurbopropEnvironment::default();
        match field {
            0 => {
                environment.terrain = NearStaticTurbopropTerrain::Flat {
                    elevation: Meters(0.01),
                }
            }
            1 => environment.conditions.turbulence = Turbulence::light(37),
            2 => {
                environment.weather = WeatherSelection::Modeled(
                    WeatherScenario::from_preset(
                        WeatherPreset::Rain,
                        initial().rigid_body.geodetic(),
                        456,
                    )
                    .unwrap(),
                )
            }
            3 => environment.conditions.time_rate = 2.,
            _ => environment.conditions.start_epoch += 1.,
        }
        let mut foreign =
            NearStaticTurbopropSimulation::from_state(config(), initial(), environment).unwrap();
        assert!(state_bits_equal(host.state(), foreign.state()));
        let report = foreign.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, control(0));
        assert_eq!(report.committed_steps(), 1);
        assert!(recorder.record(&report).is_err(), "foreign field {field}");
        assert!(recorder.closed());
        assert_eq!(encoded(&recorder.export()), prefix);
    }
}

#[test]
fn v6_inspects_unknown_nonzero_identity_but_only_reproduces_exact_schema4_law2() {
    let (recording, _) = record(1);
    let bytes = encoded(&recording);
    assert!(
        flightsim_sim::replay_v5::TurbopropRecording::read_from(&mut bytes.as_slice()).is_err()
    );
    let id_offset = 18 + u32_at(&bytes, 14) as usize;
    for (offset, value) in [
        (8, 5_u16),          // Wire version.
        (id_offset + 18, 2), // Unsupported host law, low u16.
    ] {
        let mut changed = bytes.clone();
        changed[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        assert!(NearStaticTurbopropRecording::read_from(&mut changed.as_slice()).is_err());
    }
    // Wire inspection preserves an unknown nonzero physical tuple exactly.
    // Only the executable player requires algorithm1/schema4/kind3/law2.
    for (offset, value) in [
        (id_offset, 2_u16), // Identity algorithm.
        (id_offset + 2, 3), // Old schema.
        (id_offset + 4, 2), // Jet kind.
        (id_offset + 6, 1), // Old FDM law, low u16.
    ] {
        let mut changed = bytes.clone();
        changed[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        let readable = NearStaticTurbopropRecording::read_from(&mut changed.as_slice()).unwrap();
        assert_eq!(encoded(&readable), changed);
        assert!(NearStaticTurbopropReplayPlayer::new(config(), readable).is_err());
    }
    let mut changed = bytes;
    changed[id_offset + 10] ^= 1;
    let readable = NearStaticTurbopropRecording::read_from(&mut changed.as_slice()).unwrap();
    assert!(NearStaticTurbopropReplayPlayer::new(config(), readable).is_err());
}

#[test]
fn original_cedar_bare_fdm_boundary_retains_exact_law1_rejection_and_law2_success() {
    use flightsim_fdm::{Environment, turboprop as law1};
    use flightsim_sim::model_identity::ModelIdentity;
    use near_static_turboprop_common::{boundary_state, cedar, cedar_forward};
    let state = boundary_state();
    let controls = ControlInputs::neutral().with_brakes(1.);
    let forward = cedar_forward();
    assert_eq!(
        ModelIdentity::for_turboprop(&forward).fingerprint,
        0xf83d_082e_8148_71c0
    );
    let environment = Environment::still_air();
    assert!(environment.ground_reference().is_none());
    assert_eq!(environment.wind_ecef.to_array().map(f64::to_bits), [0; 3]);
    assert_eq!(environment.atmosphere.temperature_offset().to_bits(), 0);
    assert_eq!(environment.ground_elevation.get().to_bits(), 0);
    assert_eq!(environment.ground_slope().north().to_bits(), 0);
    assert_eq!(environment.ground_slope().east().to_bits(), 0);
    let mut pure = law1::TurbopropFlightDynamics::new(forward.clone(), state).unwrap();
    let failure = pure
        .step(NEAR_STATIC_TURBOPROP_FIXED_DT, controls, &environment)
        .unwrap_err();
    assert_eq!((failure.stage, failure.substep), (TurbopropStage::K2, 2));
    cedar_boundary_witness::assert_rejection(
        &forward,
        state,
        controls,
        &environment,
        failure,
        -4.878_198_680_520_810_5e-6,
    );
    assert!(state_bits_equal(pure.state(), &state));
    let mut law2 = law1::near_static::TurbopropFlightDynamics::new(cedar(), state).unwrap();
    let accepted = law2
        .step(NEAR_STATIC_TURBOPROP_FIXED_DT, controls, &environment)
        .unwrap();
    assert_eq!(accepted.substeps, cedar_boundary_witness::SUBSTEPS);
    assert!(!state_bits_equal(law2.state(), &state));
}

#[test]
fn cedar_held_plane_fdm_witness_matches_exact_v5_terminal_and_law2_replay() {
    use flightsim_fdm::{Environment, turboprop as law1};
    use flightsim_sim::{model_identity::ModelIdentity, replay_v5, turboprop_simulation as host1};
    use near_static_turboprop_common::{boundary_state, cedar, cedar_forward};
    let diagnostic_bits = |error: law1::TurbopropStepError| {
        let d = error.diagnostics.values();
        [
            d.pressure_ratio.map(|v| v.0),
            d.temperature_ratio.map(|v| v.0),
            d.mach.map(|v| v.0),
            d.advance_ratio.map(|v| v.0),
            d.blade_pitch.map(|v| v.get()),
            d.relative_shaft.map(|v| v.get()),
            d.absolute_spin.map(|v| v.get()),
            d.tip_mach.map(|v| v.0),
            d.crossflow_ratio,
        ]
        .map(|v| v.map(f64::to_bits))
    };
    let state = boundary_state();
    let controls = ControlInputs::neutral().with_brakes(1.);
    let forward = cedar_forward();
    assert_eq!(
        ModelIdentity::for_turboprop(&forward).fingerprint,
        0xf83d_082e_8148_71c0
    );

    // Bare still-air FDM recomputes its local ground frame at each stage. The
    // unchanged host instead holds the frame-zero sampled plane for the whole
    // attempt. These are distinct exact environments, with distinct J bits.
    // Verify the host context independently through the old FDM before v5
    // replay; preserve the original bare-FDM input and same-runtime witness above.
    let mut old = host1::TurbopropSimulation::from_state(
        forward.clone(),
        state,
        host1::TurbopropEnvironment::default(),
    )
    .unwrap();
    let old_before = old.snapshot();
    let ground = old_before.ground;
    let position = state.rigid_body.geodetic();
    let ground_bits = [
        ground.reference.latitude.get(),
        ground.reference.longitude.get(),
        ground.reference.altitude.get(),
        ground.elevation.get(),
        ground.slope.north(),
        ground.slope.east(),
    ]
    .map(f64::to_bits);
    // Historical Linux GNU output; other platforms still compare every
    // held-plane word with the original state's same-runtime geodetic result.
    if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        assert_eq!(
            ground_bits,
            [0x3fe3_8c45_e13d_5a30, 0x4003_6876_b19e_2081, 0, 0, 0, 0]
        );
    }
    assert_eq!(
        ground_bits,
        [
            position.latitude.get().to_bits(),
            position.longitude.get().to_bits(),
            0,
            0,
            0,
            0,
        ]
    );
    assert!(ground.from_terrain);
    let held_environment = Environment::still_air().with_ground_plane(
        ground.reference,
        ground.elevation,
        ground.slope,
    );
    let reference = held_environment.ground_reference().unwrap();
    assert_eq!(
        [
            reference.latitude.get(),
            reference.longitude.get(),
            reference.altitude.get(),
            held_environment.ground_elevation.get(),
            held_environment.ground_slope().north(),
            held_environment.ground_slope().east()
        ]
        .map(f64::to_bits),
        ground_bits,
    );
    assert_eq!(
        held_environment.wind_ecef.to_array().map(f64::to_bits),
        [0; 3]
    );
    assert_eq!(
        held_environment.atmosphere.temperature_offset().to_bits(),
        0
    );
    eprintln!(
        "Cedar held-plane [reference latitude,longitude,altitude,elevation,north slope,east slope] bits: {ground_bits:016x?}; calm wind bits [0,0,0], ISA offset bits 0"
    );
    let mut held_fdm = law1::TurbopropFlightDynamics::new(forward.clone(), state).unwrap();
    let held_failure = held_fdm
        .step(NEAR_STATIC_TURBOPROP_FIXED_DT, controls, &held_environment)
        .unwrap_err();
    assert_eq!(
        (held_failure.stage, held_failure.substep),
        (TurbopropStage::K2, 2)
    );
    cedar_boundary_witness::assert_rejection(
        &forward,
        state,
        controls,
        &held_environment,
        held_failure,
        -4.878_198_680_954_174e-6,
    );
    assert!(state_bits_equal(held_fdm.state(), &state));
    let mut old_recorder = replay_v5::TurbopropRecorder::new(&old).unwrap();
    let report = old.advance(host1::TURBOPROP_FIXED_DT, controls);
    let old_failure = report
        .terminal()
        .expect("law1 host retains domain rejection")
        .failure;
    assert!(
        matches!(old_failure.reason, law1::TurbopropFailureReason::OutsidePropellerDomain(d)
        if d.advance_ratio == law1::AxisStatus::Below)
    );
    assert_eq!(
        (old_failure.stage, old_failure.substep),
        (TurbopropStage::K2, 2)
    );
    assert_eq!(old_failure, held_failure);
    assert_eq!(diagnostic_bits(old_failure), diagnostic_bits(held_failure));
    assert!(replay_v5::snapshot_bits_equal(&old_before, &old.snapshot()));
    old_recorder.record(&report).unwrap();
    let old_recording = old_recorder.finish();
    assert_eq!(
        (
            old_recording.conditions().identity.schema,
            old_recording.conditions().identity.law_revision
        ),
        (3, 1)
    );
    let mut old_bytes = Vec::new();
    old_recording.write_to(&mut old_bytes).unwrap();
    export_qa_file("near-static-held-plane-law1-v5.fsreplay", &old_bytes);
    let old_player = replay_v5::TurbopropReplayPlayer::new(
        forward,
        replay_v5::TurbopropRecording::read_from(&mut old_bytes.as_slice()).unwrap(),
    )
    .unwrap();
    assert!(old_player.finished());
    assert_eq!(
        old_player.simulation().terminal().unwrap().failure,
        old_failure
    );
    assert_eq!(
        diagnostic_bits(old_player.simulation().terminal().unwrap().failure),
        diagnostic_bits(held_failure)
    );
    assert!(replay_v5::snapshot_bits_equal(
        &old_before,
        &old_player.simulation().snapshot()
    ));
    assert!(NearStaticTurbopropRecording::read_from(&mut old_bytes.as_slice()).is_err());

    let mut new = NearStaticTurbopropSimulation::from_supported_state(
        cedar(),
        state,
        NearStaticTurbopropEnvironment::default(),
        controls,
    )
    .unwrap();
    assert!(state_bits_equal(old.state(), new.state()));
    let mut rec = NearStaticTurbopropRecorder::new(&new).unwrap();
    let report = new.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, controls);
    assert!(report.terminal().is_none(), "{:?}", report.terminal());
    assert_eq!(report.committed_steps(), 1);
    rec.record(&report).unwrap();
    let expected = new.snapshot();
    let recording = rec.finish();
    assert_eq!(
        (
            recording.conditions().identity.schema,
            recording.conditions().identity.law_revision
        ),
        (4, 2)
    );
    let bytes = encoded(&recording);
    export_qa_file("near-static-held-plane-law2-v6.fsreplay", &bytes);
    let mut player = NearStaticTurbopropReplayPlayer::new(
        cedar(),
        NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap(),
    )
    .unwrap();
    player.advance(NEAR_STATIC_TURBOPROP_FIXED_DT).unwrap();
    assert!(player.finished());
    assert!(snapshot_bits_equal(
        &expected,
        &player.simulation().snapshot()
    ));
}

#[test]
fn cedar_braked_idle_release_and_throttle_replay_all_host_history() {
    use near_static_turboprop_common::{boundary_state, cedar};
    let cfg = cedar();
    let mut sim = NearStaticTurbopropSimulation::from_supported_state(
        cfg.clone(),
        boundary_state(),
        NearStaticTurbopropEnvironment::default(),
        ControlInputs::neutral().with_brakes(1.),
    )
    .unwrap();
    let mut recorder = NearStaticTurbopropRecorder::new(&sim).unwrap();
    let mut snapshots = vec![(0, sim.snapshot())];
    let mut negative_endpoints = 0;
    let mut negative_span = None;
    for i in 0..3600 {
        // Twenty seconds of braked idle, then ten seconds of released brakes
        // with an explicit linear throttle command ending at 0.4.
        let controls = if i < 2400 {
            ControlInputs::neutral().with_brakes(1.)
        } else {
            ControlInputs::neutral().with_throttle(0.4 * f64::from(i - 2400) / 1199.)
        };
        let report = sim.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, controls);
        recorder.record(&report).unwrap();
        if report.terminal().is_some() {
            // Preserve and replay authentic terminal evidence before reporting
            // a domain exit; never repair the state, map, wind or controls.
            break;
        }
        let rigid = sim.state().rigid_body;
        let axial = (rigid.orientation.inverse() * rigid.velocity).x;
        if axial < 0. {
            negative_endpoints += 1;
            let span = negative_span.get_or_insert((i + 1, i + 1));
            span.1 = i + 1;
        }
        if [1, 120, 481, 1200, 2400, 3000, 3600].contains(&(i + 1)) {
            snapshots.push((i + 1, sim.snapshot()));
        }
    }
    let expected = sim.snapshot();
    let expected_terminal = sim.terminal();
    let recording = recorder.finish();
    let bytes = encoded(&recording);
    export_qa_file("near-static-cedar-3600-v6.fsreplay", &bytes);
    let mut player = NearStaticTurbopropReplayPlayer::new(
        cfg,
        NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap(),
    )
    .unwrap();
    while !player.finished() {
        assert!(player.advance(Seconds(0.25)).unwrap() <= 240);
    }
    assert!(snapshot_bits_equal(
        &expected,
        &player.simulation().snapshot()
    ));
    assert_eq!(player.simulation().terminal(), expected_terminal);
    player.set_paused(true);
    for (cursor, snapshot) in snapshots.iter().rev() {
        assert!(player.seek_to(*cursor).unwrap() <= 240);
        while player.seeking() {
            assert!(player.continue_seek().unwrap() <= 240);
        }
        assert!(snapshot_bits_equal(
            snapshot,
            &player.simulation().snapshot()
        ));
    }
    assert!(
        expected_terminal.is_none(),
        "authentic law2 domain exit after {} committed steps; negative endpoints {negative_endpoints}, span {negative_span:?}: {expected_terminal:?}",
        expected.committed_steps,
    );
    assert_eq!(expected.committed_steps, 3600);
    assert!(
        negative_endpoints > 0,
        "the continued boundary must actually enter negative inflow"
    );
    assert_eq!(recording.controls().len(), 3600);
    assert_ne!(expected.state.shaft_rad_s, boundary_state().shaft_rad_s);
    assert_ne!(
        expected.state.blade_pitch_rad,
        boundary_state().blade_pitch_rad
    );
}
