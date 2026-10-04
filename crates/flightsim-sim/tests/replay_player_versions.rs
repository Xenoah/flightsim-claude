//! Timing parity and exact source preservation across both player APIs.
use flightsim_core::{Attitude, Geodetic, Ned, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::replay::{
    Conditions, CurrentConditions, CurrentRecorder, EnvironmentConditions, Player, Recorder,
    ReplayFile, ReplayFilePlayer,
};

#[test]
fn versioned_player_preserves_format_identity_and_every_byte_during_operations() {
    for bytes in [
        include_bytes!("fixtures/legacy_v1.fsreplay").as_slice(),
        include_bytes!("fixtures/legacy_v2_disabled.fsreplay").as_slice(),
        include_bytes!("fixtures/legacy_v2_world.fsreplay").as_slice(),
        include_bytes!("fixtures/v3_legacy_weather.fsreplay").as_slice(),
    ] {
        let mut reader = bytes;
        let file = ReplayFile::read_from(&mut reader).unwrap();
        let version = file.format_version();
        let identity = file.aircraft_identity();
        let mut player = ReplayFilePlayer::new(file);
        player.set_paused(true);
        player.accumulate(Seconds(90.0));
        assert!(player.next_due().is_none());
        player.step_once();
        player.seek(0);
        player.set_speed(8.0);
        player.set_paused(false);
        player.accumulate(Seconds(2.0));
        while player.next_due().is_some() {}
        let mut actual = Vec::new();
        player.recording().write_to(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        assert_eq!(player.recording().format_version(), version);
        assert_eq!(player.recording().aircraft_identity(), identity);
    }
}

#[test]
fn legacy_and_current_players_have_identical_budget_pause_speed_seek_and_end_rules() {
    let config = AircraftConfig::light_single();
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.0, 140.0, 900.0),
        Attitude::from_degrees(0.0, 3.0, 270.0),
        Ned::new(40.0, 0.0, 0.0),
    );
    let mut old = Recorder::new(Conditions::default().with_aircraft(&config));
    let mut new = CurrentRecorder::new(CurrentConditions::for_aircraft(
        &config,
        EnvironmentConditions::default(),
    ));
    for index in 0..300 {
        let dt = Seconds(if index % 7 == 0 { 0.0 } else { 0.013 });
        let controls = ControlInputs::neutral().with_throttle(f64::from(index % 10) / 10.0);
        old.record(dt, controls, Some(&state));
        new.record(dt, controls, Some(&state));
    }
    let mut old = Player::new(old.finish());
    let mut new = ReplayFilePlayer::new(ReplayFile::V3(new.finish()));
    for speed in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0, 0.1, 8.0] {
        old.set_speed(speed);
        new.set_speed(speed);
        assert_eq!(old.speed().to_bits(), new.speed().to_bits());
        for paused in [true, false] {
            old.set_paused(paused);
            new.set_paused(paused);
            for dt in [f64::NAN, f64::INFINITY, -1.0, 0.0, f64::MAX, 0.019] {
                old.accumulate(Seconds(dt));
                new.accumulate(Seconds(dt));
                for _ in 0..32 {
                    assert_eq!(old.next_due(), new.next_due());
                    assert_eq!(old.cursor(), new.cursor());
                }
            }
        }
        assert_eq!(old.seek(257), new.seek(257));
        assert_eq!(old.step_once(), new.step_once());
        assert_eq!(old.seek(0), new.seek(0));
    }
    while !old.is_finished() {
        assert_eq!(old.step_once(), new.step_once());
    }
    assert!(new.is_finished());
    assert_eq!(old.frame_count(), new.frame_count());
    assert!(old.step_once().is_none() && new.step_once().is_none());
    assert_eq!(old.seek(u32::MAX), new.seek(u32::MAX));
}
