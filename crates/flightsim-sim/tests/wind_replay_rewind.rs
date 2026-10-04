//! Nondefault physical weather survives the current recording boundaries and
//! reconstruction. Clear/Rain change recorded visual data only, for both models.

use flightsim_core::{Attitude, Degrees, Geodetic, MetersPerSecond, Ned, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState, Turbulence};
use flightsim_sim::{
    GroundSampler, Simulation, Wind,
    aircraft_profile::AircraftProfileV2,
    jet_scenarios::solve_jet_trim,
    model_simulation::{JetEnvironment, JetSimulation, JetSnapshot},
    replay::{
        CurrentConditions, CurrentRecorder, EnvironmentConditions, ReplayFile, ReplayFilePlayer,
    },
    replay_v4::{JetRecorder, JetRecording, JetReplayPlayer, state_bits_equal},
    weather::{WeatherPreset, WeatherScenario, WeatherSelection},
};
use flightsim_world::{MemoryTileSource, Terrain};

const TURBULENCE_SEED: u64 = 0x0123_4567_89ab_cdef;
const STEPS: u32 = 480;

fn weather(preset: WeatherPreset) -> WeatherSelection {
    WeatherSelection::Modeled(
        WeatherScenario::from_preset(
            preset,
            Geodetic::from_degrees(35.55, 139.78, 0.0),
            0xfedc_ba98_7654_3210,
        )
        .unwrap(),
    )
}

fn environment(initial: RigidBodyState) -> EnvironmentConditions {
    EnvironmentConditions {
        start: initial.geodetic(),
        heading: initial.attitude().yaw,
        wind: Wind {
            from: Degrees(310.0).to_radians(),
            speed: MetersPerSecond(7.25),
        },
        turbulence: Turbulence::moderate(TURBULENCE_SEED),
        start_epoch: 2_461_317.5,
        time_rate: 60.0,
        ..EnvironmentConditions::default()
    }
}

fn assert_physical_environment(actual: EnvironmentConditions, expected: EnvironmentConditions) {
    assert_eq!(
        actual.wind.from.get().to_bits(),
        expected.wind.from.get().to_bits()
    );
    assert_eq!(
        actual.wind.speed.get().to_bits(),
        expected.wind.speed.get().to_bits()
    );
    assert_eq!(
        actual.turbulence.intensity.get().to_bits(),
        expected.turbulence.intensity.get().to_bits()
    );
    assert_eq!(actual.turbulence.seed, TURBULENCE_SEED);
}

fn legacy_simulation(
    initial: RigidBodyState,
    conditions: EnvironmentConditions,
) -> Simulation<MemoryTileSource> {
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        initial,
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    sim.set_wind(conditions.wind);
    sim.set_turbulence(conditions.turbulence);
    sim.set_climate(conditions.climate_date).unwrap();
    sim
}

fn legacy_recording(preset: WeatherPreset, hz: u32) -> (ReplayFile, Simulation<MemoryTileSource>) {
    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    );
    let environment = environment(initial);
    let mut conditions =
        CurrentConditions::for_aircraft(&AircraftConfig::light_single(), environment);
    conditions.weather = weather(preset);
    let mut recorder = CurrentRecorder::new(conditions);
    let mut sim = legacy_simulation(initial, environment);
    let controls = ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65);
    for _ in 0..hz * 4 {
        let report = sim.advance_with_controls(Seconds(1.0 / f64::from(hz)), |dt, state| {
            recorder.record(dt, controls, Some(state));
            controls
        });
        assert!(!report.diverged && !sim.crashed());
    }
    assert_eq!(recorder.frame_count(), STEPS);
    (ReplayFile::V3(recorder.finish()), sim)
}

fn assert_legacy_equal(
    actual: &Simulation<MemoryTileSource>,
    expected: &Simulation<MemoryTileSource>,
) {
    assert!(state_bits_equal(actual.state(), expected.state()));
    assert_eq!(
        actual.elapsed().get().to_bits(),
        expected.elapsed().get().to_bits()
    );
    assert_eq!(actual.log(), expected.log());
    assert_eq!(actual.crash(), expected.crash());
    assert_eq!(actual.touchdown_count(), expected.touchdown_count());
}

fn finish_legacy(player: &mut ReplayFilePlayer, sim: &mut Simulation<MemoryTileSource>, hz: u32) {
    // Bounded even if a regression stops the player's cursor from advancing.
    for _ in 0..hz * 5 {
        player.accumulate(Seconds(1.0 / f64::from(hz)));
        while let Some(frame) = player.next_due() {
            let report = sim.advance(frame.frame_time, frame.controls);
            assert_eq!(report.steps, 1);
            assert!(!report.diverged && !sim.crashed());
        }
        if player.is_finished() {
            break;
        }
    }
    assert!(player.is_finished());
    assert_eq!(player.cursor(), STEPS);
}

#[test]
fn v3_wind_and_seed_are_exact_across_cadence_rewind_and_visual_weather() {
    let (clear, clear_sim) = legacy_recording(WeatherPreset::Clear, 30);
    let (rain, rain_sim) = legacy_recording(WeatherPreset::Rain, 144);
    assert_ne!(clear.weather(), rain.weather());
    assert_eq!(clear.frames(), rain.frames());
    assert_eq!(clear.keyframes(), rain.keyframes());
    assert_legacy_equal(&clear_sim, &rain_sim);

    for (recording, expected) in [(clear, clear_sim), (rain, rain_sim)] {
        let mut bytes = Vec::new();
        recording.write_to(&mut bytes).unwrap();
        let decoded = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.format_version(), 3);
        assert_eq!(decoded.weather(), recording.weather());
        assert_physical_environment(decoded.environment(), recording.environment());
        let mut rewritten = Vec::new();
        decoded.write_to(&mut rewritten).unwrap();
        assert_eq!(rewritten, bytes);
        let initial = decoded.keyframe_exactly_at(0).unwrap().state;
        let mut sim = legacy_simulation(initial, decoded.environment());
        let mut player = ReplayFilePlayer::new(decoded);
        finish_legacy(&mut player, &mut sim, 165);
        assert_legacy_equal(&sim, &expected);

        // The legacy seek plan stores a pose, not the turbulence clock/history.
        // Reconstruct from frame zero, as the app does, rather than resetting a
        // midflight keyframe to t=0 and silently changing the gust realization.
        let origin = player.seek(0).unwrap();
        assert_eq!(origin.replay_from, 0);
        sim.restart_at(origin.state);
        assert_eq!(sim.elapsed(), Seconds::ZERO);
        for _ in 0..137 {
            let frame = player.step_once().unwrap();
            sim.advance(frame.frame_time, frame.controls);
        }
        assert_eq!(player.cursor(), 137);
        let midway_state = *sim.state();
        let midway_time = sim.elapsed();
        let origin = player.seek(0).unwrap();
        sim.restart_at(origin.state);
        for _ in 0..137 {
            let frame = player.step_once().unwrap();
            sim.advance(frame.frame_time, frame.controls);
        }
        assert!(state_bits_equal(sim.state(), &midway_state));
        assert_eq!(sim.elapsed().get().to_bits(), midway_time.get().to_bits());
        finish_legacy(&mut player, &mut sim, 30);
        assert_legacy_equal(&sim, &expected);
    }
}

#[test]
fn v4_seeded_wind_rewind_restores_history_and_clear_rain_preserve_physics() {
    let config = AircraftProfileV2::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
    ))
    .unwrap()
    .configuration()
    .clone();
    // Trim is solved once in still air; the same controls are held in every
    // physical weather case. There is no ongoing controller or weather retrim.
    let trim = solve_jet_trim(
        &config,
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        MetersPerSecond(50.0),
        MetersPerSecond::ZERO,
        0.0,
    )
    .unwrap();
    let mut previous: Option<(JetSnapshot, JetRecording)> = None;
    for (preset, hz) in [(WeatherPreset::Clear, 30), (WeatherPreset::Rain, 144)] {
        let environment = JetEnvironment {
            conditions: environment(trim.state),
            weather: weather(preset),
            ..JetEnvironment::default()
        };
        let mut sim = JetSimulation::from_state(config.clone(), trim.state, environment).unwrap();
        let mut recorder = JetRecorder::new(&sim).unwrap();
        for _ in 0..hz * 4 {
            let report = sim.advance(Seconds(1.0 / f64::from(hz)), trim.controls);
            assert!(report.terminal().is_none(), "{:?}", report.terminal());
            recorder.record(&report).unwrap();
        }
        let snapshot = sim.snapshot();
        assert_eq!(snapshot.committed_steps, STEPS);
        let recording = recorder.finish();
        assert_eq!(recording.controls().len(), usize::try_from(STEPS).unwrap());
        if let Some((prior_snapshot, prior_recording)) = &previous {
            assert_eq!(&snapshot, prior_snapshot);
            assert!(state_bits_equal(&snapshot.state, &prior_snapshot.state));
            assert_eq!(recording.controls(), prior_recording.controls());
            assert_eq!(recording.checkpoints(), prior_recording.checkpoints());
            assert_ne!(
                recording.conditions().environment.weather,
                prior_recording.conditions().environment.weather
            );
        }
        let mut bytes = Vec::new();
        recording.write_to(&mut bytes).unwrap();
        let decoded = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
        assert_physical_environment(
            decoded.conditions().environment.conditions,
            environment.conditions,
        );
        assert_eq!(
            decoded.conditions().environment.weather,
            environment.weather
        );
        let mut rewritten = Vec::new();
        decoded.write_to(&mut rewritten).unwrap();
        assert_eq!(rewritten, bytes);
        let mut player = JetReplayPlayer::new(config.clone(), decoded).unwrap();
        for _ in 0..165 * 5 {
            player.advance(Seconds(1.0 / 165.0)).unwrap();
            if player.finished() {
                break;
            }
        }
        assert!(player.finished());
        assert_eq!(player.simulation().snapshot(), snapshot);
        player.set_paused(true);
        player.seek_to(137).unwrap();
        assert_eq!(player.cursor(), 137);
        let midway = player.simulation().snapshot();
        player.restart().unwrap();
        assert_eq!(player.simulation().elapsed(), Seconds::ZERO);
        player.seek_to(137).unwrap();
        assert_eq!(player.simulation().snapshot(), midway);
        assert!(state_bits_equal(player.simulation().state(), &midway.state));
        assert!(player.seek_to(STEPS).unwrap() <= 240);
        for _ in 0..3 {
            if !player.seeking() {
                break;
            }
            assert!(player.continue_seek().unwrap() <= 240);
        }
        assert!(!player.seeking() && player.finished());
        assert_eq!(player.simulation().snapshot(), snapshot);
        assert!(state_bits_equal(
            player.simulation().state(),
            &snapshot.state
        ));
        previous = Some((snapshot, recording));
    }
}
