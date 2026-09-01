//! やり直しの検査。
//!
//! **遊びのループを閉じるための機能。** 失敗しても再起動せずに続けられる
//! ことが要点なので、「本当に最初と同じ状態に戻るか」を外から確かめる。

use flightsim_core::{Geodetic, Meters, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

fn start() -> Geodetic {
    Geodetic::from_degrees(35.55, 139.78, 0.0)
}

fn parked() -> Simulation<MemoryTileSource> {
    Simulation::parked(
        AircraftConfig::light_single(),
        start(),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    )
}

/// 離陸してしばらく飛ぶ。
fn fly_a_while(simulation: &mut Simulation<MemoryTileSource>, frames: u32) {
    let controls = ControlInputs::neutral()
        .with_throttle(1.0)
        .with_elevator(0.2);
    for _ in 0..frames {
        simulation.advance(Seconds(1.0 / 60.0), controls);
    }
}

#[test]
fn restarting_puts_the_aircraft_back_where_it_started() {
    let mut simulation = parked();
    let original = *simulation.state();

    fly_a_while(&mut simulation, 1_800);
    // 30 秒で実測 98 m 上昇する。**やり直しの前に本当に離陸していること**を
    // 確かめてから戻す。地上を転がっただけで通ると、検査の意味が無い。
    let flown = simulation.state().geodetic();
    assert!(
        flown.altitude.get() - original.altitude().get() > 20.0,
        "the test must actually leave the runway before restarting"
    );

    simulation.restart_parked_at(start(), Radians::ZERO);

    // 同じ手順で作った状態なので、ビット単位で一致するはず。
    // ゆるく見ると「だいたい滑走路の上」で通ってしまう。
    assert_eq!(simulation.state().position.0, original.position.0);
    assert_eq!(simulation.state().velocity, original.velocity);
    assert_eq!(simulation.state().orientation, original.orientation);
    assert_eq!(
        simulation.state().angular_velocity,
        original.angular_velocity
    );
    assert!(
        simulation.on_ground(),
        "a restart puts it back on the ground"
    );
}

#[test]
fn restarting_clears_the_flight_record() {
    // **記録を残すと、失敗を繰り返すほど距離と着陸回数が増える。**
    // 「何回やり直したか」を測る数字になってしまう。
    let mut simulation = parked();
    fly_a_while(&mut simulation, 900);
    assert!(
        simulation.log().distance > Meters(100.0),
        "the test flight must cover ground before we check that it is cleared"
    );

    simulation.restart_parked_at(start(), Radians::ZERO);

    let log = simulation.log();
    assert_eq!(log.distance, Meters(0.0));
    assert_eq!(log.airborne_time, Seconds(0.0));
    assert_eq!(log.peak_agl, Meters(0.0));
    assert_eq!(log.landings, 0);
    assert_eq!(simulation.touchdown_count(), 0);
    assert!(simulation.last_touchdown().is_none());
    assert_eq!(simulation.elapsed(), Seconds(0.0));
}

#[test]
fn a_restart_recovers_from_a_diverged_state() {
    // **発散したら再起動しかない、では遊びのループが閉じない。**
    let mut simulation = parked();
    let broken = RigidBodyState {
        velocity: glam::DVec3::new(f64::NAN, 0.0, 0.0),
        ..*simulation.state()
    };
    simulation.restart_at(broken);
    simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    assert!(simulation.diverged(), "the broken state should diverge");

    simulation.restart_parked_at(start(), Radians::ZERO);
    assert!(
        !simulation.diverged(),
        "a restart must clear the divergence"
    );

    fly_a_while(&mut simulation, 300);
    assert!(
        !simulation.diverged(),
        "and the aircraft must fly normally afterwards"
    );
}

#[test]
fn restarting_twice_lands_in_the_same_place() {
    // 2 回目のやり直しが 1 回目と違うと、練習にならない。
    let mut simulation = parked();
    fly_a_while(&mut simulation, 300);
    simulation.restart_parked_at(start(), Radians::ZERO);
    let first = *simulation.state();

    fly_a_while(&mut simulation, 300);
    simulation.restart_parked_at(start(), Radians::ZERO);

    assert_eq!(simulation.state().position.0, first.position.0);
    assert_eq!(simulation.state().orientation, first.orientation);
}

#[test]
fn flying_after_a_restart_repeats_the_first_attempt() {
    // やり直したうえで同じ操作をすれば、同じ軌跡になること。
    // **ここが崩れると、やり直しは「似た状況」でしかない。**
    let mut first = parked();
    fly_a_while(&mut first, 600);
    let attempt = *first.state();

    let mut second = parked();
    fly_a_while(&mut second, 600);
    second.restart_parked_at(start(), Radians::ZERO);
    fly_a_while(&mut second, 600);

    assert_eq!(
        second.state().position.0,
        attempt.position.0,
        "the retry diverged from the first attempt"
    );
    assert_eq!(second.state().orientation, attempt.orientation);
}

#[test]
fn restarting_in_the_air_keeps_the_given_state() {
    // 進入練習のやり直しは滑走路上ではなく、進入の途中から。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.50, 139.78, 500.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(40.0, 0.0, 2.0),
    );
    let mut simulation = parked();
    fly_a_while(&mut simulation, 300);
    simulation.restart_at(state);

    assert_eq!(simulation.state().position.0, state.position.0);
    assert_eq!(simulation.state().velocity, state.velocity);
    assert_eq!(simulation.log().distance, Meters(0.0));
    assert!(
        !simulation.on_ground(),
        "restarting at 500 m must not report the aircraft as parked"
    );
}
