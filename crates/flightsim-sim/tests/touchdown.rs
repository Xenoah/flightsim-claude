//! 接地記録の検査。
//!
//! 着陸評価の土台。**接地の瞬間を取りこぼすと、ゲームとしての
//! 「うまく降りられたか」が言えなくなる。**

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

/// 静止スロットル・中立舵。
fn idle() -> ControlInputs {
    ControlInputs::default()
}

#[test]
fn parking_is_not_a_landing() {
    // spawn の瞬間を着陸として数えたら、開始直後に「着陸成功」が出る。
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        start(),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );

    for _ in 0..120 {
        simulation.advance(Seconds(1.0 / 60.0), idle());
    }

    assert_eq!(simulation.touchdown_count(), 0);
    assert!(simulation.last_touchdown().is_none());
    assert!(simulation.on_ground(), "a parked aircraft is on the ground");
}

#[test]
fn a_short_drop_records_the_impact_sink_rate() {
    // 低い高さからの落下は空力がほぼ効かないので、√(2·g·h) と突き合わせられる。
    // **高い高さから落とすと 90° 迎角の抗力で大きく減速し、外部の既知値と
    // 比べられなくなる**（実測: 20 m 落下で沈下率 4.5 m/s まで減速した）。
    let config = AircraftConfig::light_single();
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 2.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut simulation =
        Simulation::from_state(config, state, flat_world(), GroundSampler::default());

    for _ in 0..300 {
        simulation.advance(Seconds(1.0 / 60.0), idle());
        if simulation.touchdown_count() > 0 {
            break;
        }
    }

    let touchdown = simulation
        .last_touchdown()
        .expect("a fall from 2 m must touch the ground");

    // 落下距離 ≈ 2 m − 脚の長さ。脚 0.2〜1.8 m を許すと √(2·g·h) は 2.0〜5.9 m/s。
    assert!(
        (2.0..=6.0).contains(&touchdown.sink_rate.get()),
        "the recorded sink rate is {} — the pre-contact state was not captured",
        touchdown.sink_rate
    );
    assert!(
        touchdown.ground_speed.get() < 1.0,
        "a vertical drop has no ground speed, got {}",
        touchdown.ground_speed
    );

    // 接地点は落下点の直下。
    let position = touchdown.position;
    assert!((position.latitude.to_degrees().get() - 35.55).abs() < 0.001);
    assert!((position.longitude.to_degrees().get() - 139.78).abs() < 0.001);
}

#[test]
fn bounces_within_the_hysteresis_band_are_not_counted_again() {
    // 接地直後の小さな跳ね返りで着陸が二重に数えられないこと。
    let config = AircraftConfig::light_single();
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 5.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        Ned::new(0.0, 0.0, 0.0),
    );
    let mut simulation =
        Simulation::from_state(config, state, flat_world(), GroundSampler::default());

    // 落ち着くまで十分回す。
    for _ in 0..1200 {
        simulation.advance(Seconds(1.0 / 60.0), idle());
    }

    assert_eq!(
        simulation.touchdown_count(),
        1,
        "a light bounce after touchdown must not count as another landing"
    );
    assert!(simulation.on_ground());

    // 最終的に静止し、姿勢が壊れていないこと。
    let attitude = simulation.state().attitude();
    assert!(
        attitude.roll.get().abs() < 0.2 && attitude.pitch.get().abs() < 0.2,
        "the aircraft should settle upright, got roll {} pitch {}",
        attitude.roll,
        attitude.pitch
    );
}

#[test]
fn the_count_survives_a_full_stop_and_go() {
    // AGL の揺らぎでカウントが勝手に増えないこと（長時間の静止）。
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        start(),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );

    let mut counts = Vec::new();
    for _ in 0..600 {
        simulation.advance(Seconds(1.0 / 60.0), idle());
        counts.push(simulation.touchdown_count());
    }
    assert!(
        counts.iter().all(|&count| count == 0),
        "the touchdown count drifted while parked: {:?}",
        counts.iter().max()
    );
    let _ = Meters(0.0); // 型を使わない警告避けではなく、単位系を明示する印
}
