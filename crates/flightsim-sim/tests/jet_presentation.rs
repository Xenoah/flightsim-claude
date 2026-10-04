//! Native-jet adapter boundary: sim-owned placement, diagnostics and replay time.
use flightsim_core::{Attitude, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::subsonic::{AirframeDefinition, JetAircraftConfig, MachAeroSchedule};
use flightsim_fdm::{Atmosphere, ControlInputs, RigidBodyState, Turbulence};
use flightsim_sim::{
    Wind,
    aircraft_profile::AircraftProfileV2,
    jet_scenarios::solve_jet_trim,
    model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation, JetTerrain},
    replay::EnvironmentConditions,
    replay_v4::{JetRecorder, JetRecording, JetReplayPlayer, state_bits_equal},
};
use flightsim_world::ClimateDate;

fn config() -> JetAircraftConfig {
    AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap()
    .configuration()
    .clone()
}
fn trim() -> flightsim_sim::jet_scenarios::JetTrim {
    solve_jet_trim(
        &config(),
        Geodetic::from_degrees(0., 0., 1000.),
        MetersPerSecond(50.),
        MetersPerSecond::ZERO,
        0.,
    )
    .unwrap()
}
fn bytes(recording: &JetRecording) -> Vec<u8> {
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    bytes
}
fn recording(ticks: u32) -> JetRecording {
    let trim = trim();
    let mut sim =
        JetSimulation::from_state(config(), trim.state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    for _ in 0..ticks {
        let report = sim.advance(JET_FIXED_DT, trim.controls);
        assert!(report.terminal().is_none());
        recorder.record(&report).unwrap();
    }
    recorder.finish()
}

#[test]
fn parked_flat_and_global_are_recordable_pristine_three_wheel_starts() {
    for terrain in [
        JetTerrain::Flat {
            elevation: Meters(400.),
        },
        JetTerrain::BundledGlobal,
    ] {
        let env = JetEnvironment {
            terrain,
            conditions: EnvironmentConditions::default()
                .with_world_climate(terrain == JetTerrain::BundledGlobal, None),
            ..JetEnvironment::default()
        };
        for heading in [0., 0.8, 1.57, 3.8] {
            let sim = JetSimulation::parked(
                config(),
                Geodetic::from_degrees(46.6, 8.3, 12345.),
                Radians(heading),
                env,
            )
            .unwrap();
            let snapshot = sim.snapshot();
            assert_eq!(snapshot.elapsed, Seconds::ZERO);
            assert_eq!(snapshot.committed_steps, 0);
            assert_eq!(snapshot.state, snapshot.previous);
            assert!(!snapshot.airborne);
            assert!(
                snapshot
                    .gear_clearances
                    .iter()
                    .all(|c| c.get().abs() < 0.001)
            );
            if terrain == JetTerrain::BundledGlobal {
                assert!(
                    snapshot.ground.slope.north().abs() + snapshot.ground.slope.east().abs() > 1e-6
                );
            }
            let recording = JetRecorder::new(&sim).unwrap().export();
            let player = JetReplayPlayer::new(config(), recording).unwrap();
            assert_eq!(snapshot, player.simulation().snapshot());
        }
    }
}

#[test]
fn exact_pole_placement_uses_the_same_canonical_frame_as_sampled_ground() {
    let environment = JetEnvironment {
        terrain: JetTerrain::BundledGlobal,
        conditions: EnvironmentConditions::default().with_world_climate(true, None),
        ..JetEnvironment::default()
    };
    for latitude in [-90., 90.] {
        for heading in [0., 0.8, 1.57, 3.8] {
            let reference = JetSimulation::parked(
                config(),
                Geodetic::from_degrees(latitude, 0., 0.),
                Radians(heading),
                environment,
            )
            .unwrap();
            for longitude in [-180., -139.33, -90., 90., 139.33, 180.] {
                let sim = JetSimulation::parked(
                    config(),
                    Geodetic::from_degrees(latitude, longitude, 0.),
                    Radians(heading),
                    environment,
                )
                .unwrap_or_else(|error| {
                    panic!("pole {latitude}, longitude {longitude}, heading {heading}: {error}")
                });
                assert!(
                    sim.snapshot()
                        .gear_clearances
                        .iter()
                        .all(|c| c.get().abs() < 0.001)
                );
                // Core's canonical exact-pole longitude is zero. The caller's
                // physically ambiguous longitude must not select a second NED
                // basis for either the pose or its requested heading.
                assert!(state_bits_equal(sim.state(), reference.state()));
                let recorded = JetRecorder::new(&sim).unwrap().export();
                let player = JetReplayPlayer::new(config(), recorded).unwrap();
                assert_eq!(sim.snapshot(), player.simulation().snapshot());
            }
        }
    }
}

#[test]
fn parked_aligns_unequal_leg_lengths_and_rejects_degenerate_geometry() {
    let original = config();
    let mut definition = AirframeDefinition::from_config(original.airframe());
    definition.landing_gear[0].contact_m[2] += 0.25;
    let unequal = JetAircraftConfig::new(
        definition.to_config().unwrap(),
        original.thrust().clone(),
        original.aero().clone(),
        *original.envelope(),
    )
    .unwrap();
    let sim = JetSimulation::parked(
        unequal,
        Geodetic::from_degrees(0., 0., 0.),
        Radians::ZERO,
        JetEnvironment::default(),
    )
    .unwrap();
    assert!(sim.state().attitude().pitch.get().abs() > 0.05);
    assert!(
        sim.snapshot()
            .gear_clearances
            .iter()
            .all(|c| c.get().abs() < 0.001)
    );
    definition.landing_gear[1].contact_m = definition.landing_gear[0].contact_m;
    let degenerate = JetAircraftConfig::new(
        definition.to_config().unwrap(),
        original.thrust().clone(),
        original.aero().clone(),
        *original.envelope(),
    )
    .unwrap();
    assert!(
        JetSimulation::parked(
            degenerate,
            Geodetic::from_degrees(0., 0., 0.),
            Radians::ZERO,
            JetEnvironment::default()
        )
        .is_err()
    );
}

#[test]
fn failed_parked_restart_is_atomic_and_success_resets_history() {
    let mut sim =
        JetSimulation::from_state(config(), trim().state, JetEnvironment::default()).unwrap();
    sim.advance(Seconds(0.021), trim().controls);
    let before = sim.snapshot();
    let accumulated = sim.accumulated();
    assert!(
        sim.restart_parked_at(Geodetic::from_degrees(91., 0., 0.), Radians::ZERO)
            .is_err()
    );
    assert_eq!(before, sim.snapshot());
    assert_eq!(accumulated, sim.accumulated());
    assert!(
        sim.restart_parked_at(Geodetic::from_degrees(0., 0., 0.), Radians(f64::NAN))
            .is_err()
    );
    sim.restart_parked_at(Geodetic::from_degrees(0., 0., 0.), Radians::ZERO)
        .unwrap();
    assert_eq!(sim.elapsed(), Seconds::ZERO);
    assert_eq!(sim.accumulated(), Seconds::ZERO);
    assert!(JetRecorder::new(&sim).is_ok());
}

#[test]
fn presentation_uses_local_climate_wind_and_mach_schedule_without_mutation() {
    let original = config();
    let mut aero = original.aero().definition().clone();
    aero.knots[0].aero.stall_angle_rad = 0.1;
    aero.knots[1].aero.stall_angle_rad = 0.4;
    let config = JetAircraftConfig::new(
        original.airframe().clone(),
        original.thrust().clone(),
        MachAeroSchedule::from_definition(aero).unwrap(),
        *original.envelope(),
    )
    .unwrap();
    let start = Geodetic::from_degrees(35.55, 139.78, 1000.);
    let state = RigidBodyState::from_geodetic(
        start,
        Attitude::from_degrees(0., 4., 0.),
        Ned::new(50., 0., 0.),
    );
    let environment = JetEnvironment {
        conditions: EnvironmentConditions {
            wind: Wind {
                from: Radians::ZERO,
                speed: MetersPerSecond(10.),
            },
            turbulence: Turbulence::CALM,
            ..EnvironmentConditions::default().with_world_climate(false, ClimateDate::from_month(7))
        },
        ..JetEnvironment::default()
    };
    let sim = JetSimulation::from_state(config, state, environment).unwrap();
    let before = sim.snapshot();
    for _ in 0..3 {
        let presentation = sim.presentation();
        let physical_air = Atmosphere::with_temperature_offset(
            presentation.climate.unwrap().isa_temperature_offset.get(),
        )
        .sample(state.altitude());
        assert_eq!(presentation.atmosphere, physical_air);
        assert!((presentation.aero_angles.true_airspeed.get() - 60.).abs() < 1e-10);
        let mach = 60. / physical_air.speed_of_sound.get();
        assert!((presentation.mach.unwrap().0 - mach).abs() < 1e-12);
        let expected_stall = 0.1 + (0.4 - 0.1) * mach / 0.9;
        assert!(
            (presentation.aero_coefficients.unwrap().stall_angle.get() - expected_stall).abs()
                < 1e-12
        );
        assert!((presentation.stall_fraction - 4_f64.to_radians() / expected_stall).abs() < 1e-12);
        assert_eq!(sim.snapshot(), before);
        assert_eq!(sim.accumulated(), Seconds::ZERO);
    }
}

#[test]
fn live_interpolation_moves_only_presentation_and_terminal_uses_last_accepted_pose() {
    let mut sim =
        JetSimulation::from_state(config(), trim().state, JetEnvironment::default()).unwrap();
    sim.advance(Seconds(JET_FIXED_DT.get() * 1.5), trim().controls);
    let snapshot = sim.snapshot();
    let expected = snapshot
        .previous
        .position
        .as_vec()
        .lerp(snapshot.state.position.as_vec(), 0.5);
    assert!(sim.interpolated().position.as_vec().distance(expected) < 1e-9);
    assert_eq!(sim.snapshot(), snapshot);
    let mut state = trim().state;
    state.velocity *= 8.;
    sim.restart_at(state).unwrap();
    let presentation = sim.presentation();
    assert!(presentation.aero_coefficients.is_none());
    assert_eq!(presentation.stall_fraction.to_bits(), 0_f64.to_bits());
    assert!(
        sim.advance(JET_FIXED_DT, ControlInputs::neutral())
            .terminal()
            .is_some()
    );
    assert_eq!(sim.interpolated().position, sim.state().position);
}

#[test]
fn repeated_export_keeps_recording_open_and_final_checkpoints_exact() {
    let trim = trim();
    let mut sim =
        JetSimulation::from_state(config(), trim.state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    let empty = recorder.export();
    assert_eq!(empty.checkpoints().len(), 1);
    empty.validate().unwrap();
    for tick in 1..=121 {
        recorder
            .record(&sim.advance(JET_FIXED_DT, trim.controls))
            .unwrap();
        if [1, 119, 120, 121].contains(&tick) {
            let saved = recorder.export();
            saved.validate().unwrap();
            assert_eq!(saved.controls().len(), tick);
            assert_eq!(bytes(&saved), bytes(&recorder.export()));
            assert!(!recorder.closed());
            let mut player = JetReplayPlayer::new(config(), saved).unwrap();
            player.seek_to(u32::try_from(tick).unwrap()).unwrap();
            assert!(state_bits_equal(player.simulation().state(), sim.state()));
        }
    }
    assert_eq!(bytes(&recorder.export()), bytes(&recorder.finish()));
}

#[test]
fn terminal_zero_exports_repeatedly_without_an_ordinary_frame() {
    let mut state = trim().state;
    state.velocity *= 8.;
    let mut sim = JetSimulation::from_state(config(), state, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    recorder
        .record(&sim.advance(JET_FIXED_DT, ControlInputs::neutral().with_throttle(0.7)))
        .unwrap();
    let first = recorder.export();
    assert_eq!(bytes(&first), bytes(&recorder.export()));
    assert!(recorder.closed());
    assert!(first.controls().is_empty());
    assert_eq!(first.terminal().unwrap().cursor, 0);
    let player = JetReplayPlayer::new(config(), first).unwrap();
    assert!(player.finished());
    assert_eq!(player.last_controls(), ControlInputs::neutral());
}

#[test]
fn replay_speed_interpolation_seek_pause_and_end_share_one_authoritative_clock() {
    let recording = recording(480);
    let expected_bytes = bytes(&recording);
    let mut player = JetReplayPlayer::new(config(), recording).unwrap();
    player.set_speed(0.5);
    assert_eq!(player.advance(Seconds(JET_FIXED_DT.get() * 3.)).unwrap(), 1);
    let snapshot = player.simulation().snapshot();
    assert_eq!(player.simulation().accumulated(), Seconds::ZERO);
    let expected = snapshot
        .previous
        .position
        .as_vec()
        .lerp(snapshot.state.position.as_vec(), 0.5);
    assert!(player.interpolated().position.as_vec().distance(expected) < 1e-9);
    assert_eq!(player.presentation().pose, player.interpolated());
    assert_eq!(player.last_controls(), trim().controls);
    player.set_paused(true);
    assert_eq!(
        player.interpolated().position,
        player.simulation().state().position
    );
    assert_eq!(player.advance(Seconds(20.)).unwrap(), 0);
    assert_eq!(player.simulation().snapshot(), snapshot);
    assert_eq!(player.seek_to(400).unwrap(), 240);
    assert_eq!(player.seek_target(), Some(400));
    assert_eq!(player.continue_seek().unwrap(), 160);
    assert_eq!(player.seek_target(), None);
    assert_eq!(
        player.interpolated().position,
        player.simulation().state().position
    );
    player.set_paused(false);
    player.set_speed(f64::INFINITY);
    assert_eq!(player.speed().to_bits(), 8_f64.to_bits());
    assert_eq!(player.advance(Seconds(30.)).unwrap(), 80);
    assert!(player.finished());
    assert_eq!(
        player.interpolated().position,
        player.simulation().state().position
    );
    assert_eq!(bytes(player.recording()), expected_bytes);
    player.set_speed(f64::NEG_INFINITY);
    assert_eq!(player.speed().to_bits(), 0.1_f64.to_bits());
    player.set_speed(f64::NAN);
    assert_eq!(player.speed().to_bits(), 1_f64.to_bits());
    player.restart().unwrap();
    assert_eq!(player.last_controls(), ControlInputs::neutral());
    player.set_speed(8.);
    assert_eq!(player.advance(Seconds(0.25)).unwrap(), 240);
    assert_eq!(player.cursor(), 240);
    assert_eq!(player.advance(Seconds::ZERO).unwrap(), 0);
}
