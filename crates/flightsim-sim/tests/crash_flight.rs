//! 実際に飛ばして墜落させる検査。
//!
//! `crash.rs` の単体検査は境界の判定だけを見る。ここでは
//! **接地の経路を通って本当に墜落になるか**を、飛ばして確かめる。

use flightsim_core::{Geodetic, Meters, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{CrashCause, CrashLimits, GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

/// 指定した高さ・姿勢から静止落下させる。
fn drop_from(height: f64, roll: f64, pitch: f64) -> Simulation<MemoryTileSource> {
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, height),
        flightsim_core::Attitude::new(
            Radians(roll.to_radians()),
            Radians(pitch.to_radians()),
            Radians::ZERO,
        ),
        Ned::new(0.0, 0.0, 0.0),
    );
    Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    )
}

/// 接地するか諦めるまで回す。
fn settle(simulation: &mut Simulation<MemoryTileSource>) {
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        if simulation.crashed() || simulation.touchdown_count() > 0 {
            break;
        }
    }
}

#[test]
fn falling_from_height_wrecks_the_aircraft() {
    // 脚高 1 m を引くと約 9 m の落下。自由落下で √(2·9.81·9) = 13.3 m/s。
    // **限界の 5 m/s を明らかに超える。**
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    settle(&mut simulation);

    let crash = simulation
        .crash()
        .expect("a 9 m fall must wreck the aircraft");
    match crash.cause {
        CrashCause::SinkRate { sink_rate } => assert!(
            sink_rate.get() > 5.0,
            "the recorded sink rate {sink_rate} should exceed the limit"
        ),
        other => panic!("expected a sink-rate crash, got {other:?}"),
    }
    assert!(simulation.crashed());
}

#[test]
fn a_hard_but_survivable_arrival_is_not_a_crash() {
    // **普通に降りて壊れるようでは遊べない。** 評価表で「危険」と出る
    // 領域（3 m/s 超）でも、機体はまだ壊れないこと。
    //
    // 落下だけで穏やかな接地は作れない。空中と判定されるには脚の下に
    // 0.5 m 以上の隙間が要る（`AIRBORNE_CLEARANCE`）ので、
    // 脚高 1 m + 0.6 m から落とす。0.55 m 落ちて √(2·9.81·0.55) = 3.3 m/s。
    let mut simulation = drop_from(1.6, 0.0, 0.0);
    settle(&mut simulation);

    assert_eq!(
        simulation.touchdown_count(),
        1,
        "it must actually touch down"
    );
    let sink_rate = simulation
        .last_touchdown()
        .expect("it touched down")
        .sink_rate;
    assert!(
        (3.0..5.0).contains(&sink_rate.get()),
        "the test must land in the hard-but-survivable band, got {sink_rate}"
    );
    assert!(
        !simulation.crashed(),
        "{sink_rate} must not wreck the aircraft"
    );
}

#[test]
fn a_wrecked_aircraft_stops_moving() {
    // 転がり続けると「まだ飛べる」ように見えて、失敗が失敗として伝わらない。
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    settle(&mut simulation);
    assert!(simulation.crashed());

    let resting = *simulation.state();
    let elapsed = simulation.elapsed();
    for _ in 0..300 {
        let report = simulation.advance(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral().with_throttle(1.0),
        );
        assert_eq!(report.steps, 0, "a wrecked aircraft must not step");
    }
    assert_eq!(simulation.state().position.0, resting.position.0);
    assert_eq!(simulation.elapsed(), elapsed, "and time must not advance");
}

#[test]
fn a_crash_is_not_reported_as_a_divergence() {
    // **「操縦が下手だった」と「計算が壊れた」は別の失敗。**
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    settle(&mut simulation);
    assert!(simulation.crashed());
    assert!(
        !simulation.diverged(),
        "a crash is a normal outcome, not a numerical failure"
    );
    let report = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    assert!(!report.diverged);
}

#[test]
fn restarting_after_a_crash_gives_a_flyable_aircraft() {
    // **墜落したら再起動しかない、では遊びのループが閉じない。**
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    settle(&mut simulation);
    assert!(simulation.crashed());

    simulation.restart_parked_at(Geodetic::from_degrees(35.55, 139.78, 0.0), Radians::ZERO);
    assert!(!simulation.crashed(), "a restart must clear the wreck");
    assert!(simulation.crash().is_none());

    let controls = ControlInputs::neutral()
        .with_throttle(1.0)
        .with_elevator(0.2);
    for _ in 0..1_800 {
        simulation.advance(Seconds(1.0 / 60.0), controls);
    }
    assert!(
        simulation.agl() > Meters(20.0),
        "and the aircraft must fly again, got {} m AGL",
        simulation.agl().get()
    );
}

#[test]
fn the_none_limits_let_a_hard_arrival_through() {
    // 回帰テストと検証用の逃げ道が効くこと。
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    simulation.set_crash_limits(CrashLimits::NONE);
    settle(&mut simulation);

    assert!(!simulation.crashed());
    assert_eq!(simulation.touchdown_count(), 1);
}

#[test]
fn the_touchdown_record_survives_the_crash() {
    // 何が起きたかを見るのに要る。**墜落したら記録も消える、では
    // 「なぜ壊れたか」が分からない。**
    let mut simulation = drop_from(10.0, 0.0, 0.0);
    settle(&mut simulation);

    let touchdown = simulation
        .last_touchdown()
        .expect("the impact is still a touchdown");
    assert!(touchdown.sink_rate.get() > 5.0);
    assert_eq!(simulation.touchdown_count(), 1);
}
