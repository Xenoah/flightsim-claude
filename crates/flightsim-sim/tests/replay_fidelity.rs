//! リプレイが**実際に同じ飛行を再現するか**の検査。
//!
//! 形式の往復だけでは足りない。バイト列が戻っても、それを流し直した結果が
//! 元と違えば、リプレイは嘘をつく。ここでは記録 → 再生を実際に回し、
//! 軌跡が一致することを見る。

use flightsim_core::{Geodetic, Meters, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState, Turbulence};
use flightsim_sim::replay::{Conditions, Player, Recorder, Recording};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

/// 記録・再生の 1 フレーム時間。**一定にしない。**
///
/// 実機では毎フレーム違う。可変フレーム時間でも一致することを見たいので、
/// 決定論的にばらつかせる。
fn frame_time(index: u32) -> Seconds {
    let jitter = f64::from(index % 7) * 0.0008;
    Seconds(1.0 / 60.0 + jitter)
}

/// 決定論的な操縦入力列。人の操作の代わり。
fn controls(index: u32) -> ControlInputs {
    let phase = f64::from(index) * 0.02;
    ControlInputs::neutral()
        .with_throttle(0.8)
        .with_elevator((phase.sin() * 0.15).clamp(-1.0, 1.0))
        .with_aileron((phase * 0.7).cos() * 0.1)
}

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

fn start() -> Geodetic {
    Geodetic::from_degrees(35.55, 139.78, 0.0)
}

fn new_simulation() -> Simulation<MemoryTileSource> {
    Simulation::parked(
        AircraftConfig::light_single(),
        start(),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    )
}

/// 記録しながら `frames` フレーム飛ぶ。記録と最終状態を返す。
fn record_a_flight(frames: u32) -> (Recording, RigidBodyState) {
    let config = AircraftConfig::light_single();
    let mut simulation = new_simulation();
    let mut recorder = Recorder::new(
        Conditions {
            start: start(),
            heading: Radians::ZERO,
            turbulence: Turbulence::light(7),
            ..Conditions::default()
        }
        .with_aircraft(&config),
    );
    simulation.set_turbulence(Turbulence::light(7));

    for index in 0..frames {
        let dt = frame_time(index);
        let input = controls(index);
        recorder.record(dt, input, Some(simulation.state()));
        simulation.advance(dt, input);
    }
    assert!(
        !simulation.diverged(),
        "the recorded flight must stay finite"
    );
    (recorder.finish(), *simulation.state())
}

/// 記録を最初から流し直し、最終状態を返す。
fn replay_to_end(recording: &Recording) -> RigidBodyState {
    let mut simulation = new_simulation();
    simulation.set_turbulence(recording.conditions().turbulence);
    let mut player = Player::new(recording.clone());
    while let Some(frame) = player.step_once() {
        simulation.advance(frame.frame_time, frame.controls);
    }
    *simulation.state()
}

fn separation(left: &RigidBodyState, right: &RigidBodyState) -> f64 {
    (left.position.0 - right.position.0).length()
}

#[test]
fn replaying_a_recording_retraces_the_same_flight() {
    // **これが通らなければリプレイに意味がない。**
    let (recording, flown) = record_a_flight(600);
    let replayed = replay_to_end(&recording);

    // 同じビルド・同じ入力列なので、ビット単位で一致するはず。
    // ゆるい許容にすると「ほぼ同じ」で通ってしまい、決定論が崩れても気付けない。
    assert_eq!(
        flown.position.0,
        replayed.position.0,
        "the replay drifted from the recorded flight by {} m",
        separation(&flown, &replayed)
    );
    assert_eq!(flown.velocity, replayed.velocity);
    assert_eq!(flown.orientation, replayed.orientation);
    assert_eq!(flown.angular_velocity, replayed.angular_velocity);
}

#[test]
fn the_flight_actually_went_somewhere() {
    // 上の一致検査は、機体が動いていなくても通ってしまう。
    // **何も起きない飛行で決定論を主張しない。**
    let (recording, flown) = record_a_flight(600);
    let travelled = (flown.position.0 - start().to_ecef().0).length();
    assert!(
        travelled > 100.0,
        "the test flight only moved {travelled} m; it does not exercise the replay"
    );
    assert!(recording.duration().get() > 9.0);
}

#[test]
fn a_recording_survives_a_round_trip_through_bytes() {
    let (recording, _) = record_a_flight(300);
    let mut bytes = Vec::new();
    recording
        .write_to(&mut bytes)
        .expect("writing to a Vec cannot fail");
    let restored = Recording::read_from(&mut &bytes[..]).expect("the round trip must read back");

    assert_eq!(&restored, &recording);
    // ファイルから読んだ記録でも、同じ飛行になること。
    assert_eq!(
        replay_to_end(&restored).position.0,
        replay_to_end(&recording).position.0
    );
}

#[test]
fn keyframes_are_placed_and_match_the_flight() {
    let (recording, _) = record_a_flight(600);
    assert!(
        recording.keyframes().len() >= 5,
        "600 frames at an interval of 120 should hold at least 5 keyframes, got {}",
        recording.keyframes().len()
    );

    // 検査点で実際にずれが 0 であること。ここがずれるなら記録側が壊れている。
    let mut simulation = new_simulation();
    simulation.set_turbulence(recording.conditions().turbulence);
    for (index, frame) in recording.frames().iter().enumerate() {
        let number = u32::try_from(index).expect("the test recording is short");
        if let Some(drift) = recording.drift_at(number, simulation.state()) {
            assert_eq!(drift, Meters(0.0), "keyframe {number} disagrees");
        }
        simulation.advance(frame.frame_time, frame.controls);
    }
}

#[test]
fn seeking_backward_lands_on_the_same_state_as_flying_there() {
    // 後退シークはキーフレームから積分し直す。**近道した結果が
    // 通しで飛んだ結果と違えば、シークは別の飛行を見せている。**
    let (recording, _) = record_a_flight(600);

    // 通しで 500 フレーム目まで飛ばす。
    let mut straight = new_simulation();
    straight.set_turbulence(recording.conditions().turbulence);
    for frame in recording.frames().iter().take(500) {
        straight.advance(frame.frame_time, frame.controls);
    }

    // シークして、キーフレームから 500 まで流し直す。
    let mut player = Player::new(recording.clone());
    let plan = player.seek(500).expect("the recording has keyframes");
    assert!(
        plan.replay_from < 500 && plan.frames_to_replay() <= 120,
        "the seek should start from a nearby keyframe, got {plan:?}"
    );

    let mut sought = Simulation::from_state(
        AircraftConfig::light_single(),
        plan.state,
        flat_world(),
        GroundSampler::default(),
    );
    sought.set_turbulence(recording.conditions().turbulence);
    while let Some(frame) = player.step_once() {
        if player.cursor() > plan.target {
            break;
        }
        sought.advance(frame.frame_time, frame.controls);
    }

    let gap = separation(straight.state(), sought.state());
    // ビット一致は求めない。**キーフレームから再開すると固定ステップの
    // アキュムレータが 0 に戻る**ので、サブステップの割れ方が本編と変わる。
    // 実測 0.17 m（600 フレーム中の 500 フレーム目、約 8.7 秒地点）。
    // 1 m を超えたら割れ方以外の何かが違っている。
    assert!(
        gap < 1.0,
        "seeking to frame 500 landed {gap} m away from flying there"
    );
}

#[test]
fn a_replay_recorded_with_another_aircraft_is_refused() {
    // **黙って別の機体で再生させない。** 軌跡が変わり、リプレイが嘘になる。
    let (recording, _) = record_a_flight(10);
    let same = AircraftConfig::light_single();
    recording
        .check_reproducible_with(&same)
        .expect("the same aircraft must be accepted");

    let mut heavier = AircraftConfig::light_single();
    heavier.mass_properties = flightsim_fdm::MassProperties::new(
        flightsim_core::Kilograms(1_200.0),
        1_285.0,
        1_825.0,
        2_667.0,
        0.0,
    );
    let error = recording
        .check_reproducible_with(&heavier)
        .expect_err("a different aircraft must be refused");
    let message = error.to_string();
    assert!(
        message.contains("fingerprint"),
        "the error should say what does not match, got: {message}"
    );
}

#[test]
fn renaming_an_aircraft_does_not_break_its_replays() {
    // 指紋は飛び方を決める数値だけで作る。名前を直しただけで
    // 過去の記録が全部読めなくなるのは筋が悪い。
    let (recording, _) = record_a_flight(10);
    let mut renamed = AircraftConfig::light_single();
    renamed.name = "Trainer".to_owned();
    recording
        .check_reproducible_with(&renamed)
        .expect("a rename must not invalidate the recording");
}
