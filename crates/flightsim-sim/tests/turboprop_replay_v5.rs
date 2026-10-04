mod turboprop_common;
use flightsim_core::{MetersPerSecond, Radians, RadiansPerSecond, Seconds};
use flightsim_fdm::{
    ControlInputs,
    turboprop::{
        TurbopropAircraftConfig, TurbopropEnvelope, TurbopropFailureReason, TurbopropStage,
    },
};
use flightsim_sim::{
    replay_v5::{
        TurbopropRecorder, TurbopropRecording, TurbopropReplayPlayer, snapshot_bits_equal,
        state_bits_equal,
    },
    turboprop_simulation::{
        TURBOPROP_FIXED_DT, TurbopropEnvironment, TurbopropSimulation, TurbopropSnapshot,
    },
};
use turboprop_common::{config, initial};
fn encoded(record: &TurbopropRecording) -> Vec<u8> {
    let mut b = Vec::new();
    record.write_to(&mut b).unwrap();
    b
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
fn record(steps: u32) -> (TurbopropRecording, TurbopropSnapshot) {
    let mut sim = TurbopropSimulation::from_supported_state(
        config(),
        initial(),
        TurbopropEnvironment::default(),
        control(0),
    )
    .unwrap();
    let mut recorder = TurbopropRecorder::new(&sim).unwrap();
    for i in 0..steps {
        let report = sim.advance(TURBOPROP_FIXED_DT, control(i));
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
    let mut p = TurbopropReplayPlayer::new(
        config(),
        TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap(),
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
        let mut sim =
            TurbopropSimulation::from_state(config(), initial(), TurbopropEnvironment::default())
                .unwrap();
        let mut rec = TurbopropRecorder::new(&sim).unwrap();
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
    let mut sim =
        TurbopropSimulation::from_state(config(), state, TurbopropEnvironment::default()).unwrap();
    assert!(
        TurbopropSimulation::from_supported_state(
            config(),
            state,
            TurbopropEnvironment::default(),
            control(0)
        )
        .is_err()
    );
    let before = sim.snapshot();
    let mut recorder = TurbopropRecorder::new(&sim).unwrap();
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
    let p = TurbopropReplayPlayer::new(config(), r).unwrap();
    assert!(p.finished());
    assert!(snapshot_bits_equal(&before, &p.simulation().snapshot()));
}
#[test]
fn later_stage_failure_preserves_authentic_full_state_and_replays_terminal() {
    let original = config();
    let mut envelope = *original.envelope().definition();
    envelope.relative_shaft_rad_s = [179.5, 260.];
    let cfg = TurbopropAircraftConfig::new(
        original.airframe().clone(),
        original.turbine().clone(),
        original.propeller().clone(),
        *original.governor(),
        original.aero().clone(),
        TurbopropEnvelope::from_definition(envelope).unwrap(),
    )
    .unwrap();
    let mut sim =
        TurbopropSimulation::from_state(cfg.clone(), initial(), TurbopropEnvironment::default())
            .unwrap();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    let mut before = sim.snapshot();
    for _ in 0..1000 {
        before = sim.snapshot();
        let r = sim.advance(TURBOPROP_FIXED_DT, ControlInputs::neutral());
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
    let mut p = TurbopropReplayPlayer::new(cfg, r).unwrap();
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
        let record = TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
        let mut p = TurbopropReplayPlayer::new(config(), record).unwrap();
        p.advance(TURBOPROP_FIXED_DT).unwrap();
        let before = p.simulation().snapshot();
        assert!(p.advance(Seconds(0.25)).is_err());
        assert!(p.faulted());
        assert_eq!(p.cursor(), 1);
        assert!(snapshot_bits_equal(&before, &p.simulation().snapshot()));
    }
}
#[test]
fn recorder_rejects_noncontiguous_reports_and_preserves_prior_export() {
    let mut a =
        TurbopropSimulation::from_state(config(), initial(), TurbopropEnvironment::default())
            .unwrap();
    let mut rec = TurbopropRecorder::new(&a).unwrap();
    let r = a.advance(TURBOPROP_FIXED_DT, control(0));
    rec.record(&r).unwrap();
    let before = encoded(&rec.export());
    let mut initial = initial();
    initial.blade_pitch_rad = Radians(0.31);
    let mut b = TurbopropSimulation::from_state(config(), initial, TurbopropEnvironment::default())
        .unwrap();
    b.advance(TURBOPROP_FIXED_DT, control(0));
    let unrelated = b.advance(TURBOPROP_FIXED_DT, control(1));
    assert!(rec.record(&unrelated).is_err());
    assert!(rec.closed());
    assert_eq!(encoded(&rec.export()), before);
}
#[test]
fn wind_climate_weather_and_contact_history_reconstruct_from_zero() {
    use flightsim_sim::{
        Wind,
        turboprop_simulation::TurbopropTerrain,
        weather::{WeatherPreset, WeatherScenario, WeatherSelection},
    };
    let mut env = TurbopropEnvironment::default();
    env.terrain = TurbopropTerrain::BundledGlobal;
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
        TurbopropSimulation::from_supported_state(config(), initial(), env, control(0)).unwrap();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    for i in 0..241 {
        let r = sim.advance(TURBOPROP_FIXED_DT, control(i));
        assert!(r.terminal().is_none(), "{:?}", r.terminal());
        rec.record(&r).unwrap();
    }
    let snapshot = sim.snapshot();
    let r = rec.finish();
    assert_eq!(r.conditions().environment.weather, env.weather);
    let mut p = TurbopropReplayPlayer::new(config(), r).unwrap();
    assert_eq!(p.seek_to(241).unwrap(), 240);
    assert_eq!(p.continue_seek().unwrap(), 1);
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}

#[test]
fn terminal_scratch_success_or_diagnostic_mismatch_never_commits_a_step() {
    let mut outside = initial();
    outside.shaft_rad_s = RadiansPerSecond(270.);
    let mut sim =
        TurbopropSimulation::from_state(config(), outside, TurbopropEnvironment::default())
            .unwrap();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    rec.record(&sim.advance(TURBOPROP_FIXED_DT, control(0)))
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
    let r = TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut p = TurbopropReplayPlayer::new(config(), r).unwrap();
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
    let r = TurbopropRecording::read_from(&mut changed.as_slice()).unwrap();
    assert!(TurbopropReplayPlayer::new(config(), r).is_err());
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
    let cfg = TurbopropAircraftConfig::new(
        original.airframe().clone(),
        original.turbine().clone(),
        original.propeller().clone(),
        *original.governor(),
        original.aero().clone(),
        TurbopropEnvelope::from_definition(e).unwrap(),
    )
    .unwrap();
    let mut sim =
        TurbopropSimulation::from_state(cfg.clone(), initial(), TurbopropEnvironment::default())
            .unwrap();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    for i in 0..=240 {
        let r = sim.advance(TURBOPROP_FIXED_DT, control(i));
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
    let mut p = TurbopropReplayPlayer::new(cfg, r).unwrap();
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
    let mut sim =
        TurbopropSimulation::from_state(config(), state, TurbopropEnvironment::default()).unwrap();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    for i in 0..120 {
        let r = sim.advance(TURBOPROP_FIXED_DT, control(i));
        assert!(r.terminal().is_none(), "{:?}", r.terminal());
        rec.record(&r).unwrap();
    }
    let snapshot = sim.snapshot();
    assert!(snapshot.touchdown_count > 0);
    assert!(snapshot.last_touchdown.is_some());
    assert!(snapshot.log.airborne_time.get() > 0.);
    let mut p = TurbopropReplayPlayer::new(config(), rec.finish()).unwrap();
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
    let mut propeller = original.propeller().definition().clone();
    propeller.cells = vec![
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0., cp: 0.1 },
        PropellerCellDefinition { ct: 0., cp: 0.1 },
    ];
    let cfg = TurbopropAircraftConfig::new(
        original.airframe().clone(),
        original.turbine().clone(),
        PropellerMap::from_definition(propeller).unwrap(),
        *original.governor(),
        original.aero().clone(),
        *original.envelope(),
    )
    .unwrap();
    let mut state = initial();
    state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., 1000.),
        Attitude::default(),
        Ned::new(180. / std::f64::consts::PI, 0., 0.),
    );
    let mut sim =
        TurbopropSimulation::from_state(cfg.clone(), state, TurbopropEnvironment::default())
            .unwrap();
    let snapshot = sim.snapshot();
    let mut rec = TurbopropRecorder::new(&sim).unwrap();
    let report = sim.advance(TURBOPROP_FIXED_DT, control(0));
    let event = report.terminal().unwrap();
    assert_eq!(
        event.failure.reason,
        TurbopropFailureReason::PropellerPowerBound(PropellerPowerBound::BelowIdealDisk)
    );
    assert!((event.failure.diagnostics.values().advance_ratio.unwrap().0 - 1.).abs() < 1e-12);
    assert!(snapshot_bits_equal(&snapshot, &sim.snapshot()));
    rec.record(&report).unwrap();
    let bytes = encoded(&rec.finish());
    let r = TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
    let p = TurbopropReplayPlayer::new(cfg, r).unwrap();
    assert!(p.finished());
    assert!(snapshot_bits_equal(&snapshot, &p.simulation().snapshot()));
}
#[test]
fn parked_start_retains_explicit_engine_bits_without_warmup_and_restart_is_atomic() {
    use flightsim_core::Geodetic;
    use flightsim_sim::aircraft_profile_v3::AircraftProfileV3;
    let profile = AircraftProfileV3::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
    ))
    .unwrap();
    let engine = profile.running_start();
    let mut sim = TurbopropSimulation::parked(
        profile.configuration().clone(),
        Geodetic::from_degrees(35., 139., 0.),
        Radians(0.7),
        TurbopropEnvironment::default(),
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
    let mut sim =
        TurbopropSimulation::from_state(config(), initial(), TurbopropEnvironment::default())
            .unwrap();
    for _ in 0..120 {
        let r = sim.advance(
            TURBOPROP_FIXED_DT,
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
fn terminal_only_report_must_match_all_full_state_scalars() {
    let mut state = initial();
    state.shaft_rad_s = RadiansPerSecond(270.);
    let sim =
        TurbopropSimulation::from_state(config(), state, TurbopropEnvironment::default()).unwrap();
    let mut recorder = TurbopropRecorder::new(&sim).unwrap();
    let prior = encoded(&recorder.export());
    state.shaft_rad_s = RadiansPerSecond(280.);
    let mut other =
        TurbopropSimulation::from_state(config(), state, TurbopropEnvironment::default()).unwrap();
    let report = other.advance(TURBOPROP_FIXED_DT, control(0));
    assert!(report.terminal().is_some());
    assert!(recorder.record(&report).is_err());
    assert!(recorder.closed());
    assert_eq!(encoded(&recorder.export()), prior);
}

#[test]
fn visual_time_overflow_closes_before_any_report_append() {
    let mut environment = TurbopropEnvironment::default();
    environment.conditions.time_rate = 1e308;
    let mut sim = TurbopropSimulation::from_state(config(), initial(), environment).unwrap();
    let mut recorder = TurbopropRecorder::new(&sim).unwrap();
    let prefix = encoded(&recorder.export());
    let report = sim.advance(TURBOPROP_FIXED_DT, control(0));
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
        let a =
            TurbopropSimulation::from_state(config(), initial(), TurbopropEnvironment::default())
                .unwrap();
        let mut rec = TurbopropRecorder::new(&a).unwrap();
        let prior = encoded(&rec.export());
        let mut env = TurbopropEnvironment::default();
        let mut cfg = config();
        match foreign {
            0 => env.conditions.wind.speed = MetersPerSecond(1000.),
            1 => {
                let mut e = *cfg.envelope().definition();
                e.relative_shaft_rad_s[1] = 179.;
                let mut g = *cfg.governor().definition();
                g.reference_rad_s = 170.;
                cfg = TurbopropAircraftConfig::new(
                    cfg.airframe().clone(),
                    cfg.turbine().clone(),
                    cfg.propeller().clone(),
                    flightsim_fdm::turboprop::SampledGovernor::from_definition(g).unwrap(),
                    cfg.aero().clone(),
                    TurbopropEnvelope::from_definition(e).unwrap(),
                )
                .unwrap();
            }
            // Signed zero is part of exact environment provenance even though
            // this particular numerical wind vector is physically unchanged.
            _ => env.conditions.wind.speed = MetersPerSecond(-0.),
        }
        let mut b = TurbopropSimulation::from_state(cfg, initial(), env).unwrap();
        assert!(state_bits_equal(a.state(), b.state()));
        let report = b.advance(TURBOPROP_FIXED_DT, control(0));
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
        let cfg = TurbopropAircraftConfig::new(
            original.airframe().clone(),
            TurbinePowerTable::from_definition(turbine).unwrap(),
            original.propeller().clone(),
            *original.governor(),
            original.aero().clone(),
            TurbopropEnvelope::from_definition(envelope).unwrap(),
        )
        .unwrap();
        let mut state = initial();
        state.rigid_body = flightsim_fdm::RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0., 0., 1000.),
            Attitude::default(),
            Ned::new(40., 0., 0.),
        );
        state.turbine_fraction = TurbineFraction::new(fraction).unwrap();
        state.blade_pitch_rad = Radians(pitch);
        let mut sim =
            TurbopropSimulation::from_state(cfg.clone(), state, TurbopropEnvironment::default())
                .unwrap();
        let before = sim.snapshot();
        let mut recorder = TurbopropRecorder::new(&sim).unwrap();
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
        let player = TurbopropReplayPlayer::new(cfg, recorder.finish()).unwrap();
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
        let mut env = TurbopropEnvironment::default();
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
        let next = crossflow(TURBOPROP_FIXED_DT);
        let original = config();
        let mut e = *original.envelope().definition();
        // Fixed witness: zero-time ratio .00254737687272323 exceeds the
        // cap, prospective +dt ratio .00254727921162822 is supported.
        e.maximum_crossflow_tip_ratio = 0.002547328042175725;
        assert!(next < e.maximum_crossflow_tip_ratio && at_zero > e.maximum_crossflow_tip_ratio);
        let cfg = TurbopropAircraftConfig::new(
            original.airframe().clone(),
            original.turbine().clone(),
            original.propeller().clone(),
            *original.governor(),
            original.aero().clone(),
            TurbopropEnvelope::from_definition(e).unwrap(),
        )
        .unwrap();
        let mut sim = TurbopropSimulation::from_state(cfg.clone(), state, env).unwrap();
        let mut rec = TurbopropRecorder::new(&sim).unwrap();
        let report = sim.advance(TURBOPROP_FIXED_DT, control(0));
        assert!(report.terminal().is_none());
        assert!(
            TurbopropSimulation::from_supported_state(cfg.clone(), state, env, control(0)).is_err()
        );
        rec.record(&report).unwrap();
        let expected = sim.snapshot();
        let mut p = TurbopropReplayPlayer::new(cfg, rec.finish()).unwrap();
        while !p.finished() {
            p.advance(TURBOPROP_FIXED_DT).unwrap();
        }
        assert!(snapshot_bits_equal(&expected, &p.simulation().snapshot()));
    }
}

#[test]
fn visual_time_boundary_keeps_an_existing_nonempty_prefix_exportable() {
    let mut environment = TurbopropEnvironment::default();
    environment.conditions.start_epoch = flightsim_sim::replay::MAX_VISUAL_EPOCH - 1.;
    environment.conditions.time_rate = 0.6 * 86_400. / TURBOPROP_FIXED_DT.get();
    let mut sim = TurbopropSimulation::from_state(config(), initial(), environment).unwrap();
    let mut recorder = TurbopropRecorder::new(&sim).unwrap();
    recorder
        .record(&sim.advance(TURBOPROP_FIXED_DT, control(0)))
        .unwrap();
    let prefix = encoded(&recorder.export());
    assert_eq!(recorder.export().controls().len(), 1);
    let report = sim.advance(TURBOPROP_FIXED_DT, control(1));
    assert_eq!(report.committed_steps(), 1);
    assert!(recorder.record(&report).is_err());
    assert!(recorder.closed());
    assert_eq!(encoded(&recorder.export()), prefix);
    let mut player = TurbopropReplayPlayer::new(config(), recorder.finish()).unwrap();
    player.advance(TURBOPROP_FIXED_DT).unwrap();
    assert_eq!(player.cursor(), 1);
    assert_eq!(sim.snapshot().committed_steps, 2);
}
