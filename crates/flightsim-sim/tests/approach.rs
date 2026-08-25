//! 最終進入から始める状態の検査。
//!
//! **着陸だけを練習したいのに毎回場周を一周させるのは辛い。**
//! ゲームの核が着陸の腕なら、そこへすぐ入れる道が要る。
//!
//! 検査は `approach_state` の内部式ではなく、**返された状態を外から測って**
//! 行う（同じ式を写すと符号の取り違えを検出できない）。

use flightsim_core::{Degrees, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs};
use flightsim_sim::{GroundSampler, Simulation, approach_state};
use flightsim_world::{MemoryTileSource, Runway, Terrain};

fn runway() -> Runway {
    Runway::synthetic()
}

/// 標準の 3 度進入、1 海里手前、35 m/s。
fn standard() -> flightsim_fdm::RigidBodyState {
    approach_state(
        &runway(),
        Meters(1852.0),
        Degrees(3.0).to_radians(),
        MetersPerSecond(35.0),
    )
}

#[test]
fn the_aircraft_starts_on_a_three_degree_glideslope() {
    // 幾何の外部値: 3 度の進入角なら、末端から 1 海里（1852 m）で
    // 高さ 1852 * tan(3 度) = 97.1 m（318 ft）。
    let state = standard();
    let height = state.altitude().get() - runway().elevation.get();
    let expected = 1852.0 * 3.0_f64.to_radians().tan();
    assert!(
        (height - expected).abs() < 1.0,
        "on a 3 degree slope one mile out the aircraft should be {expected:.1} m up, got {height:.1}"
    );
}

#[test]
fn the_aircraft_starts_before_the_threshold_not_past_it() {
    // 符号を取り違えると、滑走路の**先**から進入することになる。
    let runway = runway();
    let state = standard();
    let along = runway.longitudinal_offset(state.geodetic()).get();
    assert!(
        along < 0.0,
        "the start should be before the threshold, got {along:.0} m along the runway"
    );
    assert!(
        (along + 1852.0).abs() < 20.0,
        "the start should be about one mile out, got {along:.0} m"
    );
}

#[test]
fn the_aircraft_starts_on_the_centreline() {
    let runway = runway();
    let state = standard();
    let across = runway.lateral_offset(state.geodetic()).get();
    assert!(
        across.abs() < 5.0,
        "the start should be on the centreline, got {across:.1} m off"
    );
}

#[test]
fn the_aircraft_starts_pointing_at_the_runway() {
    let runway = runway();
    let heading = standard().attitude().yaw.wrap_positive().get();
    let expected = runway.heading.wrap_positive().get();
    assert!(
        (heading - expected).abs() < 1e-6,
        "the start should face the runway heading"
    );
}

#[test]
fn the_aircraft_starts_descending_at_the_glideslope_rate() {
    // 35 m/s で 3 度なら降下率 35 * sin(3 度) = 1.83 m/s（360 ft/min）。
    // 実機の標準的な進入降下率とほぼ同じで、外部値として使える。
    let state = standard();
    let sink = -state.vertical_speed().get();
    let expected = 35.0 * 3.0_f64.to_radians().sin();
    assert!(
        (sink - expected).abs() < 0.05,
        "a 35 m/s approach on 3 degrees should sink at {expected:.2} m/s, got {sink:.2}"
    );
}

#[test]
fn the_horizontal_speed_matches_the_requested_airspeed() {
    let state = standard();
    let ground_speed = state.ground_speed().get();
    let expected = 35.0 * 3.0_f64.to_radians().cos();
    assert!(
        (ground_speed - expected).abs() < 0.05,
        "the horizontal component should be {expected:.2} m/s, got {ground_speed:.2}"
    );
}

#[test]
fn broken_arguments_fall_back_to_something_flyable() {
    // 引数が壊れていても NaN の状態を作らない。飛べる既定へ倒す。
    for (distance, slope, speed) in [
        (f64::NAN, 3.0, 35.0),
        (-500.0, 3.0, 35.0),
        (1852.0, f64::NAN, 35.0),
        (1852.0, 3.0, f64::NAN),
        (1852.0, 3.0, -10.0),
    ] {
        let state = approach_state(
            &runway(),
            Meters(distance),
            Degrees(slope).to_radians(),
            MetersPerSecond(speed),
        );
        assert!(
            state.is_finite(),
            "approach_state({distance}, {slope}, {speed}) produced a non-finite state"
        );
        assert!(
            state.altitude().get() > runway().elevation.get(),
            "the fallback must still be airborne"
        );
    }
}

#[test]
fn flying_the_approach_hands_off_reaches_the_runway_area() {
    // **結線の検査。** この状態から操縦せずに降ろすと、滑走路の近くへ
    // 落ちること。ここが的外れなら、位置か速度の向きが間違っている。
    let runway = runway();
    let state = approach_state(
        &runway,
        Meters(1852.0),
        Degrees(3.0).to_radians(),
        MetersPerSecond(35.0),
    );
    let terrain = Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12);
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        terrain,
        GroundSampler::default(),
    );

    // 中間のスロットルで滑らせる。操縦はしない。
    let controls = ControlInputs::neutral().with_throttle(0.4);
    for _ in 0..(120 * 90) {
        simulation.advance(Seconds(1.0 / 120.0), controls);
        if simulation.touchdown_count() > 0 {
            break;
        }
    }

    let touchdown = simulation
        .last_touchdown()
        .expect("an unpowered approach must reach the ground");
    let from_runway = runway
        .center()
        .great_circle_distance(touchdown.position)
        .get();
    assert!(
        from_runway < 4_000.0,
        "the hands-off approach landed {from_runway:.0} m from the runway"
    );
}

#[test]
fn the_start_is_deterministic() {
    // 同じ引数なら必ず同じ状態。リプレイの前提（ADR-0004）。
    let a = standard();
    let b = standard();
    assert_eq!(a.position, b.position);
    assert_eq!(a.velocity, b.velocity);
    let _ = Radians::ZERO;
}
