//! Exact application replay orchestration. State-only keyframes do not contain
//! the fixed-step remainder, turbulence clock, crash state or accumulated log.
//! Rewinds therefore replay from frame zero in bounded batches.

use bevy::prelude::*;
use flightsim_core::{Ecef, RenderFrame, Seconds};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use flightsim_sim::{Player, Recording, Simulation};
use flightsim_world::TileSource;

const FRAMES_PER_UPDATE: usize = 240;

/// File-format validity is broader than what the application can draw. Keep
/// this check at the app boundary instead of changing valid persisted bytes.
pub(crate) fn validate_replay_position(position: Ecef) -> Result<(), &'static str> {
    if !position.is_finite() || !position.0.length_squared().is_finite() {
        return Err("position has a nonfinite magnitude");
    }
    let geodetic = position.to_geodetic();
    if !geodetic.latitude.is_finite()
        || !geodetic.longitude.is_finite()
        || !geodetic.altitude.is_finite()
    {
        return Err("position has nonfinite geodetic coordinates");
    }
    let rendered = RenderFrame::new(geodetic).to_render(position);
    if !rendered.is_finite() || !rendered.length_squared().is_finite() {
        return Err("position exceeds the finite render-coordinate range");
    }
    Ok(())
}

pub(crate) fn validate_replay_state(state: &RigidBodyState) -> Result<(), &'static str> {
    validate_replay_position(state.position)?;
    if !state.is_finite()
        || !state.velocity.length_squared().is_finite()
        || !state.angular_velocity.length_squared().is_finite()
        || (state.orientation.length() - 1.0).abs() > 1e-12
    {
        return Err("state has nonfinite magnitudes or a nonunit attitude");
    }
    Ok(())
}

#[derive(Resource)]
pub(crate) struct ReplayPlayback {
    pub player: Player,
    pub elapsed: Seconds,
    pub total: Seconds,
    pub last_controls: ControlInputs,
    pub fault: Option<String>,
    seek_target: Option<u32>,
}

impl ReplayPlayback {
    pub fn new(recording: Recording) -> Self {
        Self {
            elapsed: Seconds(0.0),
            total: recording.duration(),
            last_controls: recording
                .frames()
                .first()
                .map_or_else(ControlInputs::neutral, |frame| frame.controls),
            player: Player::new(recording),
            fault: None,
            seek_target: None,
        }
    }

    pub fn initial_state(&self) -> RigidBodyState {
        self.player
            .recording()
            .keyframe_exactly_at(0)
            .expect("replay startup validates frame-zero state")
            .state
    }

    pub const fn is_seeking(&self) -> bool {
        self.seek_target.is_some()
    }

    pub fn audio_paused(&self) -> bool {
        self.player.is_paused()
            || self.is_seeking()
            || self.fault.is_some()
            || self.player.is_finished()
    }

    pub fn rewind<S: TileSource>(&mut self, simulation: &mut Simulation<S>) {
        let current_time = self.seek_target.map_or(self.elapsed.get(), |target| {
            self.player
                .recording()
                .frames()
                .iter()
                .take(target as usize)
                .map(|frame| frame.frame_time.get())
                .sum()
        });
        let wanted = (current_time - 10.0).max(0.0);
        let mut accumulated = 0.0;
        let mut target = 0;
        for frame in self.player.recording().frames() {
            if accumulated >= wanted {
                break;
            }
            accumulated += frame.frame_time.get();
            target += 1;
        }
        self.begin_seek(simulation, target);
    }

    fn begin_seek<S: TileSource>(&mut self, simulation: &mut Simulation<S>, target: u32) {
        let initial = self.initial_state();
        self.player.seek(0).expect("validated frame-zero keyframe");
        simulation.restart_at(initial);
        self.elapsed = Seconds(0.0);
        self.last_controls = self
            .player
            .recording()
            .frames()
            .first()
            .map_or_else(ControlInputs::neutral, |frame| frame.controls);
        self.fault = None;
        self.seek_target = Some(target.min(self.player.frame_count()));
    }

    /// Returns true only on the update that detects a numerical failure or a
    /// positional mismatch. A rejected replay remains visibly stopped.
    pub fn tick<S: TileSource>(
        &mut self,
        simulation: &mut Simulation<S>,
        elapsed: Seconds,
    ) -> bool {
        if self.fault.is_some() {
            return false;
        }
        if validate_replay_state(simulation.state()).is_err()
            || !self.elapsed.get().is_finite()
            || self.elapsed.get() < 0.0
            || !self.total.get().is_finite()
            || self.total.get() < 0.0
        {
            self.stop("REPLAY STOPPED: invalid state or duration".into());
            return true;
        }
        if !self.is_seeking() {
            self.player.accumulate(elapsed);
        }
        for _ in 0..FRAMES_PER_UPDATE {
            if let Some(target) = self.seek_target
                && self.player.cursor() >= target
            {
                self.seek_target = None;
                break;
            }
            let cursor = self.player.cursor();
            if let Some(frame) = self.player.recording().frames().get(cursor as usize)
                && (!frame.frame_time.get().is_finite()
                    || frame.frame_time.get() < 0.0
                    || !(self.elapsed.get() + frame.frame_time.get()).is_finite())
            {
                self.stop(format!(
                    "REPLAY STOPPED: invalid duration at frame {cursor}"
                ));
                return true;
            }
            if let Some(drift) = self.player.recording().drift_at(cursor, simulation.state())
                && (!drift.is_finite() || drift > Player::DIVERGENCE_LIMIT)
            {
                self.stop(format!(
                    "REPLAY MISMATCH: {:.1} m at frame {cursor}; check terrain/build",
                    drift.get()
                ));
                return true;
            }
            let frame = if self.is_seeking() {
                self.player.step_once()
            } else {
                self.player.next_due()
            };
            let Some(frame) = frame else {
                break;
            };
            let report = simulation.advance(frame.frame_time, frame.controls);
            self.elapsed = Seconds(self.elapsed.get() + frame.frame_time.get());
            self.last_controls = frame.controls;
            if report.diverged || validate_replay_state(simulation.state()).is_err() {
                self.stop(format!(
                    "REPLAY STOPPED: numerical failure at frame {cursor}"
                ));
                return true;
            }
            let pose = simulation.interpolated();
            if validate_replay_position(pose.position).is_err()
                || !pose.orientation.is_finite()
                || !pose.geodetic.latitude.is_finite()
                || !pose.geodetic.longitude.is_finite()
                || !pose.geodetic.altitude.is_finite()
            {
                self.stop(format!(
                    "REPLAY STOPPED: numerical failure at frame {cursor}"
                ));
                return true;
            }
        }
        false
    }

    pub(crate) fn stop(&mut self, message: String) {
        error!("{message}");
        self.fault = Some(message);
        self.seek_target = None;
        self.player.set_paused(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Attitude, Geodetic, Ned, Radians};
    use flightsim_fdm::{AircraftConfig, Turbulence};
    use flightsim_sim::{GroundSampler, Recorder, replay::Conditions};
    use flightsim_world::{MemoryTileSource, Terrain};

    fn simulation(state: RigidBodyState) -> Simulation<MemoryTileSource> {
        let mut sim = Simulation::from_state(
            AircraftConfig::light_single(),
            state,
            Terrain::new(MemoryTileSource::new(), 1024, 8..=12),
            GroundSampler::default(),
        );
        sim.set_turbulence(Turbulence::moderate(97));
        sim
    }

    fn record() -> (Recording, RigidBodyState) {
        let initial = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 2000.0),
            Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
            Ned::new(45.0, 0.0, 0.0),
        );
        let mut sim = simulation(initial);
        let mut recorder = Recorder::new(Conditions {
            turbulence: Turbulence::moderate(97),
            ..Conditions::default()
        });
        for index in 0..900 {
            let dt = Seconds(0.013 + f64::from(index % 7) * 0.002);
            let controls = ControlInputs::neutral()
                .with_throttle(0.65)
                .with_elevator(0.04);
            recorder.record(dt, controls, Some(sim.state()));
            sim.advance(dt, controls);
        }
        assert!(!sim.diverged());
        (recorder.finish(), *sim.state())
    }

    #[test]
    fn in_memory_invalid_durations_stop_before_poisoning_physics() {
        let (valid, _) = record();
        let initial = valid.keyframe_exactly_at(0).unwrap().state;
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let mut recorder = Recorder::new(Conditions::default());
            recorder.record(Seconds(bad), ControlInputs::neutral(), Some(&initial));
            let mut replay = ReplayPlayback::new(recorder.finish());
            let mut sim = simulation(initial);
            assert!(replay.tick(&mut sim, Seconds(1.0)));
            assert!(replay.fault.is_some());
            assert_eq!(*sim.state(), initial);
            assert!(sim.interpolated().position.0.is_finite());
        }
    }

    #[test]
    fn invalid_elapsed_or_total_stops_without_consuming_a_frame() {
        let (recording, _) = record();
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            for invalid_elapsed in [false, true] {
                let mut replay = ReplayPlayback::new(recording.clone());
                let mut sim = simulation(replay.initial_state());
                if invalid_elapsed {
                    replay.elapsed = Seconds(value);
                } else {
                    replay.total = Seconds(value);
                }
                assert!(replay.tick(&mut sim, Seconds(1.0)));
                assert_eq!(replay.player.cursor(), 0);
                assert!(replay.player.is_paused());
            }
        }
    }

    #[test]
    fn invalid_state_magnitudes_and_attitudes_stop_before_interpolation() {
        let (recording, _) = record();
        let initial = recording.keyframe_exactly_at(0).unwrap().state;
        let mut states = [initial; 6];
        states[0].velocity.x = f64::NAN;
        states[1].velocity.x = f64::MAX;
        states[2].angular_velocity.x = f64::MAX;
        states[3].orientation = bevy::math::DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
        states[4].orientation = bevy::math::DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0);
        states[5].orientation = bevy::math::DQuat::from_xyzw(0.0, 0.0, 0.0, f64::MAX);
        for state in states {
            let mut replay = ReplayPlayback::new(recording.clone());
            let mut sim = simulation(state);
            assert!(replay.tick(&mut sim, Seconds(0.0)));
            assert_eq!(replay.player.cursor(), 0);
            assert!(replay.fault.as_ref().unwrap().contains("invalid state"));
        }
    }

    #[test]
    fn render_overflow_positions_stop_before_any_frame_is_consumed() {
        let (recording, _) = record();
        for magnitude in [1e30, 1e39] {
            let mut state = recording.keyframe_exactly_at(0).unwrap().state;
            state.position = Ecef::new(magnitude, 0.0, 0.0);
            assert!(state.is_finite() && state.position.0.length_squared().is_finite());
            let mut sim = simulation(state);
            let mut replay = ReplayPlayback::new(recording.clone());
            assert!(replay.tick(&mut sim, Seconds::ZERO));
            assert_eq!(replay.player.cursor(), 0);
            assert!(replay.fault.as_ref().unwrap().contains("invalid state"));
        }
    }

    #[test]
    fn normal_geographic_extremes_remain_usable_in_the_app() {
        for latitude in [-90.0, 0.0, 90.0] {
            for longitude in [-180.0, 0.0, 180.0] {
                for altitude in [-500.0, 0.0, 100_000.0] {
                    let state = RigidBodyState::from_geodetic(
                        Geodetic::from_degrees(latitude, longitude, altitude),
                        Attitude::from_degrees(0.0, 0.0, 0.0),
                        Ned::new(40.0, 0.0, 0.0),
                    );
                    assert!(validate_replay_state(&state).is_ok());
                }
            }
        }
    }

    #[test]
    fn numerical_failure_after_advancing_is_stopped_before_interpolation() {
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, 1e19),
            Attitude::from_degrees(0.0, 0.0, 0.0),
            Ned::new(0.0, 0.0, -1e22),
        );
        assert!(validate_replay_state(&state).is_ok());
        let mut recorder = Recorder::new(Conditions::default());
        recorder.record(Seconds(1.0 / 120.0), ControlInputs::neutral(), Some(&state));
        let mut replay = ReplayPlayback::new(recorder.finish());
        let mut sim = simulation(state);
        assert!(replay.tick(&mut sim, Seconds(1.0)));
        assert_eq!(replay.player.cursor(), 1);
        assert!(replay.fault.as_ref().unwrap().contains("numerical failure"));
        assert!(replay.player.is_paused());
    }

    #[test]
    fn nonfinite_drift_stops_even_when_the_current_simulation_is_valid() {
        let (recording, _) = record();
        let initial = recording.keyframe_exactly_at(0).unwrap().state;
        let mut corrupt_keyframe = initial;
        corrupt_keyframe.position.0.x = f64::NAN;
        let mut recorder = Recorder::new(Conditions::default());
        recorder.record(
            Seconds(0.01),
            ControlInputs::neutral(),
            Some(&corrupt_keyframe),
        );
        let mut replay = ReplayPlayback::new(recorder.finish());
        let mut sim = simulation(initial);
        assert!(replay.tick(&mut sim, Seconds(1.0)));
        assert_eq!(replay.player.cursor(), 0);
        assert_eq!(*sim.state(), initial);
        assert!(replay.fault.as_ref().unwrap().contains("MISMATCH"));
    }

    #[test]
    fn elapsed_addition_overflow_stops_before_consuming_the_frame() {
        let (recording, _) = record();
        let initial = recording.keyframe_exactly_at(0).unwrap().state;
        let mut recorder = Recorder::new(Conditions::default());
        recorder.record(Seconds(f64::MAX), ControlInputs::neutral(), Some(&initial));
        let mut replay = ReplayPlayback::new(recorder.finish());
        replay.elapsed = Seconds(f64::MAX);
        let mut sim = simulation(initial);
        assert!(replay.tick(&mut sim, Seconds(1.0)));
        assert_eq!(replay.player.cursor(), 0);
        assert_eq!(*sim.state(), initial);
    }

    #[test]
    fn zero_duration_frames_are_valid_and_cannot_exceed_the_update_budget() {
        let (recording, _) = record();
        let initial = recording.keyframe_exactly_at(0).unwrap().state;
        let mut recorder = Recorder::new(Conditions::default());
        for _ in 0..1_000 {
            recorder.record(Seconds::ZERO, ControlInputs::neutral(), Some(&initial));
        }
        let mut replay = ReplayPlayback::new(recorder.finish());
        let mut sim = simulation(initial);
        assert!(!replay.tick(&mut sim, Seconds::ZERO));
        assert_eq!(replay.player.cursor(), 240);
        assert_eq!(*sim.state(), initial);
        while !replay.player.is_finished() {
            assert!(!replay.tick(&mut sim, Seconds::ZERO));
        }
        let final_state = *sim.state();
        assert!(!replay.tick(&mut sim, Seconds(100.0)));
        assert_eq!(*sim.state(), final_state);
        assert_eq!(replay.elapsed, Seconds::ZERO);
        assert!(replay.audio_paused());
    }

    #[test]
    fn audio_mutes_all_nonplaying_replay_states() {
        let (recording, _) = record();
        let mut replay = ReplayPlayback::new(recording);
        assert!(!replay.audio_paused());
        replay.player.set_paused(true);
        assert!(replay.audio_paused());
        replay.player.set_paused(false);
        replay.seek_target = Some(1);
        assert!(replay.audio_paused());
        replay.seek_target = None;
        replay.fault = Some("test".into());
        assert!(replay.audio_paused());
        replay.fault = None;
        while replay.player.step_once().is_some() {}
        assert!(replay.audio_paused());
    }

    #[test]
    fn airborne_initial_state_and_variable_time_replay_match_exactly() {
        let (recording, flown) = record();
        let mut replay = ReplayPlayback::new(recording);
        assert!(replay.initial_state().altitude().get() > 1900.0);
        let mut sim = simulation(replay.initial_state());
        while !replay.player.is_finished() {
            assert!(!replay.tick(&mut sim, Seconds(1.0)));
        }
        assert_eq!(*sim.state(), flown);
    }

    #[test]
    fn rewind_restores_clock_remainder_and_log_in_bounded_batches() {
        let (recording, _) = record();
        let initial = recording.keyframe_exactly_at(0).unwrap().state;
        let mut reference = simulation(initial);
        for frame in recording.frames().iter().take(517) {
            reference.advance(frame.frame_time, frame.controls);
        }
        let mut replay = ReplayPlayback::new(recording);
        let mut sim = simulation(initial);
        while !replay.player.is_finished() {
            replay.tick(&mut sim, Seconds(1.0));
        }
        replay.player.set_paused(true);
        replay.begin_seek(&mut sim, 517);
        replay.tick(&mut sim, Seconds(900.0));
        assert_eq!(replay.player.cursor(), 240);
        assert!(replay.is_seeking());
        while replay.is_seeking() {
            assert!(!replay.tick(&mut sim, Seconds(900.0)));
        }
        assert_eq!(replay.player.cursor(), 517);
        assert!(replay.player.is_paused());
        assert_eq!(*sim.state(), *reference.state());
        assert_eq!(sim.elapsed(), reference.elapsed());
        assert_eq!(sim.log(), reference.log());
        let state = *sim.state();
        replay.tick(&mut sim, Seconds(100.0));
        assert_eq!(*sim.state(), state);
    }

    #[test]
    fn mismatch_stops_and_is_reported_before_wrong_inputs_advance() {
        let (recording, _) = record();
        let mut replay = ReplayPlayback::new(recording);
        let mut state = replay.initial_state();
        state.position.0.x += 100.0;
        let mut sim = simulation(state);
        assert!(replay.tick(&mut sim, Seconds(1.0)));
        assert!(replay.fault.as_ref().unwrap().contains("MISMATCH"));
        assert_eq!(replay.player.cursor(), 0);
        assert!(replay.player.is_paused());
        replay.player.set_paused(false);
        assert!(!replay.tick(&mut sim, Seconds(1.0)));
        assert_eq!(*sim.state(), state);
        replay.begin_seek(&mut sim, 0);
        replay.tick(&mut sim, Seconds(1.0));
        assert!(replay.fault.is_none());
        assert_eq!(*sim.state(), replay.initial_state());
    }
}
