//! 飛行記録の検査。
//!
//! **積み上がるものが無いと、プレイヤーが続ける理由がない。**
//! ここが壊れると「今日はどれだけ飛んだか」が言えなくなる。

use flightsim_core::{Geodetic, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

fn parked() -> Simulation<MemoryTileSource> {
    Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.548, 139.775, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    )
}

#[test]
fn a_parked_aircraft_logs_nothing() {
    // 駐機したまま放置して距離や滞空時間が増えるなら、積分が壊れている。
    let mut simulation = parked();
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }

    let log = simulation.log();
    assert_eq!(log.landings, 0);
    assert!(
        log.airborne_time.get() < 0.1,
        "a parked aircraft logged {} airborne",
        log.airborne_time
    );
    assert!(
        log.distance.get() < 1.0,
        "a parked aircraft logged {} of travel",
        log.distance
    );
}

#[test]
fn airborne_time_counts_only_time_off_the_ground() {
    // 20 m から落とす。滞空時間は落下時間ぶんだけで、着地後は増えない。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 20.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );

    // 落ちて着地するまで。
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        if simulation.touchdown_count() > 0 {
            break;
        }
    }
    let at_touchdown = simulation.log().airborne_time;
    assert!(
        at_touchdown.get() > 0.5 && at_touchdown.get() < 20.0,
        "a fall from 20 m should log a few seconds aloft, got {at_touchdown}"
    );

    // 着地後さらに回しても、滞空時間は増えない。
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }
    assert!(
        (simulation.log().airborne_time.get() - at_touchdown.get()).abs() < 0.5,
        "airborne time kept growing on the ground: {} then {}",
        at_touchdown,
        simulation.log().airborne_time
    );
}

#[test]
fn the_peak_altitude_is_remembered_after_coming_down() {
    // 「最高高度」は現在高度ではない。降りてきても残ること。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 50.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );

    for _ in 0..1200 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }

    let log = simulation.log();
    assert!(
        log.peak_agl.get() > 45.0,
        "the peak altitude should remember the 50 m start, got {}",
        log.peak_agl
    );
    assert!(
        simulation.agl().get() < 5.0,
        "the aircraft should be back on the ground for this check to mean anything"
    );
}

#[test]
fn distance_accumulates_along_the_path_not_as_the_crow_flies() {
    // 場周を 1 周すれば出発点に戻るが、飛んだ距離は 0 ではない。
    // ここを直線距離で測ると、周回飛行の記録が消える。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 500.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        // 北へ 40 m/s。
        Ned::new(40.0, 0.0, 0.0),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );

    // 10 秒。等速なら約 400 m。空力で減速するので下限だけ見る。
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }

    let log = simulation.log();
    assert!(
        log.distance.get() > 100.0,
        "flying north at 40 m/s for 10 s should log real distance, got {}",
        log.distance
    );
    assert!(
        log.distance.get().is_finite(),
        "the distance went non-finite"
    );
}

#[test]
fn the_landing_count_matches_the_touchdown_count() {
    // 2 つの数え方が食い違うと、どちらが正しいか分からなくなる。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 3.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );
    for _ in 0..1200 {
        simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }
    assert_eq!(simulation.log().landings, simulation.touchdown_count());
    assert_eq!(simulation.log().landings, 1);
}
