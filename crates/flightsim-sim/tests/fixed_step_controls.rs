//! Pilot-state integration and recording share the actual 120 Hz FDM boundaries.
//! No renderer or second accumulator is involved in these regressions.

use flightsim_core::{Attitude, Geodetic, Ned, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, RECOMMENDED_FIXED_DT, RigidBodyState, Turbulence,
};
use flightsim_sim::replay::{Conditions, Player, Recorder, Recording};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

fn initial() -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    )
}

fn from_state(state: RigidBodyState) -> Simulation<MemoryTileSource> {
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    simulation.set_turbulence(Turbulence::moderate(7));
    simulation
}

fn recorder(state: RigidBodyState) -> Recorder {
    Recorder::new(
        Conditions {
            start: state.geodetic(),
            heading: Radians::ZERO,
            turbulence: Turbulence::moderate(7),
            ..Conditions::default()
        }
        .with_aircraft(&AircraftConfig::light_single()),
    )
}

fn changing_controls(step: u32) -> ControlInputs {
    let phase = f64::from(step) * 0.017;
    ControlInputs::neutral()
        .with_throttle(0.65 + phase.sin() * 0.1)
        .with_elevator(0.03 + phase.cos() * 0.025)
        .with_aileron((phase * 0.7).sin() * 0.015)
        .with_rudder((phase * 0.5).cos() * 0.01)
}

fn assert_same_physics(left: &Simulation<MemoryTileSource>, right: &Simulation<MemoryTileSource>) {
    assert_eq!(left.state(), right.state());
    assert_eq!(left.elapsed(), right.elapsed());
    assert_eq!(left.log(), right.log());
    assert_eq!(left.last_touchdown(), right.last_touchdown());
    assert_eq!(left.touchdown_count(), right.touchdown_count());
    assert_eq!(left.crash(), right.crash());
    assert_eq!(left.diverged(), right.diverged());
    assert_eq!(left.on_ground(), right.on_ground());
}

#[test]
fn constant_input_wrapper_is_identical_to_the_callback_path() {
    let mut constant = from_state(initial());
    let mut callback = from_state(initial());
    let controls = changing_controls(0);
    for _ in 0..30 {
        for seconds in [0.0, 1.0 / 144.0, 0.25, 0.017, 1.0 / 120.0] {
            let mut calls = 0;
            let report = callback.advance_with_controls(Seconds(seconds), |dt, _| {
                assert_eq!(dt, RECOMMENDED_FIXED_DT);
                calls += 1;
                controls
            });
            assert_eq!(constant.advance(Seconds(seconds), controls), report);
            assert_eq!(calls, report.steps);
            assert_same_physics(&constant, &callback);
            assert_eq!(constant.interpolated(), callback.interpolated());
        }
    }
}

#[test]
fn changing_controls_receive_every_actual_pre_step_state() {
    for hz in [30, 60, 144] {
        let mut grouped = from_state(initial());
        let mut reference = from_state(initial());
        let mut recorded = recorder(initial());
        let mut step = 0;
        for _ in 0..hz * 10 {
            let before = step;
            let report = grouped.advance_with_controls(Seconds(1.0 / f64::from(hz)), |dt, pre| {
                assert_eq!(dt, RECOMMENDED_FIXED_DT);
                assert_eq!(pre, reference.state(), "{hz} Hz, step {step}");
                let controls = changing_controls(step);
                recorded.record(dt, controls, Some(pre));
                assert_eq!(reference.advance(dt, controls).steps, 1);
                step += 1;
                controls
            });
            assert_eq!(report.steps, step - before);
            assert!(!report.diverged && !grouped.crashed());
            assert_same_physics(&grouped, &reference);
        }
        assert_eq!(recorded.frame_count(), step);
        assert_eq!(step, 1200, "ten seconds must align at every render cadence");
        assert_eq!(recorded.recording().duration(), grouped.elapsed());
    }
}

fn record_changing_controls() -> (Recording, Simulation<MemoryTileSource>) {
    let mut simulation = from_state(initial());
    let mut recorded = recorder(initial());
    let mut step = 0;
    for frame in 0..600 {
        // Include zero-step frames, multi-step frames and fractional remainder.
        let dt = Seconds([1.0 / 144.0, 0.025, 0.0, 0.009][frame % 4]);
        let before = recorded.frame_count();
        let report = simulation.advance_with_controls(dt, |dt, pre| {
            let controls = changing_controls(step);
            recorded.record(dt, controls, Some(pre));
            step += 1;
            controls
        });
        assert_eq!(recorded.frame_count() - before, report.steps);
        assert!(!report.diverged && !simulation.crashed());
    }
    assert_eq!(recorded.frame_count(), step);
    (recorded.finish(), simulation)
}

fn replay_until(player: &mut Player, simulation: &mut Simulation<MemoryTileSource>, target: u32) {
    while player.cursor() < target {
        // The app bounds each seek update to 240 records; exactness must not
        // depend on where those update boundaries fall.
        for _ in 0..240 {
            if player.cursor() >= target {
                break;
            }
            if let Some(key) = player.recording().keyframe_exactly_at(player.cursor()) {
                assert_eq!(simulation.state(), &key.state);
            }
            let frame = player.step_once().expect("target is within recording");
            let report = simulation.advance(frame.frame_time, frame.controls);
            assert_eq!(report.steps, 1);
            assert!(!report.diverged);
        }
    }
}

#[test]
fn per_step_recording_round_trip_and_frame_zero_replay_are_exact() {
    let (recording, flown) = record_changing_controls();
    assert!(recording.frames().len() > 600);
    assert!(
        recording
            .frames()
            .windows(2)
            .all(|pair| pair[0].controls != pair[1].controls)
    );
    assert!(
        recording
            .frames()
            .iter()
            .all(|frame| frame.frame_time == RECOMMENDED_FIXED_DT)
    );
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    let restored = Recording::read_from(&mut &bytes[..]).unwrap();
    assert_eq!(restored, recording);
    restored
        .check_reproducible_with(&AircraftConfig::light_single())
        .unwrap();
    let mut player = Player::new(restored);
    let mut replayed = from_state(player.recording().keyframe_exactly_at(0).unwrap().state);
    let count = player.frame_count();
    replay_until(&mut player, &mut replayed, count);
    assert!(player.is_finished());
    assert_same_physics(&flown, &replayed);
    assert_eq!(player.recording().duration(), replayed.elapsed());
}

#[test]
fn seeking_by_restarting_frame_zero_restores_clocks_and_flight_log() {
    let (recording, _) = record_changing_controls();
    let mut player = Player::new(recording.clone());
    let mut sought = from_state(initial());
    let count = player.frame_count();
    replay_until(&mut player, &mut sought, count);
    for target in [503, 0, 721, 240, count] {
        let origin = player.seek(0).unwrap();
        sought.restart_at(origin.state);
        replay_until(&mut player, &mut sought, target);
        let mut straight = from_state(initial());
        let mut straight_player = Player::new(recording.clone());
        replay_until(&mut straight_player, &mut straight, target);
        assert_eq!(player.cursor(), target);
        assert_same_physics(&sought, &straight);
    }
}

#[test]
fn zero_step_pause_and_clamp_do_not_invent_input_or_recording_steps() {
    let mut simulation = from_state(initial());
    let mut recorded = recorder(initial());
    let half = RECOMMENDED_FIXED_DT * 0.5;
    let mut advance = |simulation: &mut Simulation<MemoryTileSource>, dt| {
        simulation.advance_with_controls(dt, |fixed, pre| {
            let controls = changing_controls(recorded.frame_count());
            recorded.record(fixed, controls, Some(pre));
            controls
        })
    };
    assert_eq!(advance(&mut simulation, half).steps, 0);
    for _ in 0..100 {
        assert_eq!(advance(&mut simulation, Seconds::ZERO).steps, 0);
    }
    assert_eq!(advance(&mut simulation, Seconds(-5.0)).steps, 0);
    assert_eq!(simulation.state(), &initial());
    assert_eq!(simulation.elapsed(), Seconds::ZERO);
    assert_eq!(advance(&mut simulation, half).steps, 1);
    assert_eq!(advance(&mut simulation, Seconds(60.0)).steps, 30);
    assert_eq!(advance(&mut simulation, Seconds::ZERO).steps, 0);
    assert_eq!(recorded.frame_count(), 31);
    assert_eq!(recorded.recording().duration(), simulation.elapsed());
}

#[test]
fn invalid_pre_state_never_calls_controls_and_frozen_divergence_does_not_record() {
    let mut broken = initial();
    broken.velocity.x = f64::NAN;
    let mut simulation = from_state(broken);
    let mut calls = 0;
    for _ in 0..3 {
        let report = simulation.advance_with_controls(Seconds(0.25), |_, _| {
            calls += 1;
            ControlInputs::neutral()
        });
        assert!(report.diverged);
        assert_eq!(report.steps, 0);
    }
    assert_eq!(calls, 0);
    assert_eq!(simulation.elapsed(), Seconds::ZERO);
}

#[test]
fn a_step_that_diverges_counts_once_then_discards_the_remaining_budget() {
    let mut extreme = initial();
    extreme.angular_velocity = glam::DVec3::splat(1e200);
    assert!(extreme.is_finite());
    let mut simulation = from_state(extreme);
    let mut recorded = recorder(extreme);
    let report = simulation.advance_with_controls(Seconds(0.25), |dt, pre| {
        recorded.record(dt, ControlInputs::neutral(), Some(pre));
        ControlInputs::neutral()
    });
    assert!(report.diverged);
    assert_eq!(report.steps, 1);
    assert_eq!(simulation.elapsed(), RECOMMENDED_FIXED_DT);
    assert_eq!(recorded.frame_count(), 1);
    let report = simulation.advance_with_controls(Seconds(1.0), |_, _| {
        panic!("frozen divergence must not request another input")
    });
    assert_eq!(report.steps, 0);
    assert!(report.diverged);
    // A state-only recovery also must not resurrect the discarded frame budget.
    simulation.rewind_to(initial());
    assert_eq!(
        simulation
            .advance(Seconds::ZERO, ControlInputs::neutral())
            .steps,
        0
    );
}

#[test]
fn crash_within_one_long_frame_matches_single_steps_and_replay_exactly() {
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 2.0),
        Attitude::from_degrees(0.0, 0.0, 0.0),
        Ned::new(10.0, 0.0, 10.0),
    );
    let mut grouped = from_state(state);
    let mut single = from_state(state);
    let mut recorded = recorder(state);
    let mut calls = 0;
    let report = grouped.advance_with_controls(Seconds(0.249), |dt, pre| {
        assert_eq!(pre, single.state());
        let controls = ControlInputs::neutral();
        recorded.record(dt, controls, Some(pre));
        assert_eq!(single.advance(dt, controls).steps, 1);
        calls += 1;
        controls
    });
    assert!(grouped.crashed() && single.crashed());
    assert!(!report.diverged);
    assert!(report.steps > 0 && report.steps < 29);
    assert_eq!(report.steps, calls);
    assert_eq!(recorded.frame_count(), report.steps);
    assert_same_physics(&grouped, &single);
    assert_eq!(grouped.elapsed(), grouped.crash().unwrap().elapsed);
    assert_eq!(grouped.interpolated(), single.interpolated());
    assert_eq!(grouped.interpolated().position, grouped.state().position);
    for frame_time in [Seconds::ZERO, Seconds(0.25), Seconds(100.0)] {
        let report = grouped.advance_with_controls(frame_time, |dt, pre| {
            recorded.record(dt, ControlInputs::neutral(), Some(pre));
            panic!("frozen crash must not request another input")
        });
        assert_eq!(report.steps, 0);
    }
    assert_eq!(recorded.frame_count(), calls);
    assert_eq!(recorded.recording().duration(), grouped.elapsed());
    let mut bytes = Vec::new();
    recorded.finish().write_to(&mut bytes).unwrap();
    let recording = Recording::read_from(&mut &bytes[..]).unwrap();
    let mut player = Player::new(recording);
    let mut replayed = from_state(state);
    let count = player.frame_count();
    replay_until(&mut player, &mut replayed, count);
    assert_same_physics(&grouped, &replayed);
    grouped.rewind_to(initial());
    assert_eq!(
        grouped
            .advance(Seconds::ZERO, ControlInputs::neutral())
            .steps,
        0
    );
}
