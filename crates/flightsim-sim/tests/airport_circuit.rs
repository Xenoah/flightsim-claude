//! M2 完了条件の受け入れ検査: 「1 空港周辺で離陸 → 旋回 → 着陸が通ること」。
//!
//! 合成飛行場（`Runway::synthetic()`）の離陸開始点から場周を飛ぶ。
//! 地形は滑走路標高と同じ平地（合成 DEM のタイルは CI に無いため）。
//! 滑走路の contains / offsets は高度を見ないので、この単純化で
//! 「空港周辺に降りたか」の判定は損なわれない。
//!
//! **自動操縦は滑走路へ戻る横方向誘導を持たない**（M3 の空港運用側）。
//! ここで確かめるのは、滑走路から離陸し、旋回し、空港の周辺に無事に
//! 降りられること。滑走路上への精密着陸はプレイヤーの腕の見せ所。

use flightsim_core::{Degrees, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::{CircuitPlan, GroundSampler, Phase, SimulationOptions, Wind, fly};
use flightsim_world::{MemoryTileSource, Runway, Terrain};

#[test]
fn a_circuit_from_the_synthetic_runway_completes() {
    let runway = Runway::synthetic();
    let config = AircraftConfig::light_single();

    // 平地（標高 0）。滑走路の contains は高度非依存。
    let mut terrain = Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12);

    let plan = CircuitPlan {
        runway_heading: runway.heading,
        // 左場周: 離陸方位から 90° 左へ。
        outbound_heading: Radians(runway.heading.get() - Degrees(90.0).to_radians().get()),
        ..CircuitPlan::default()
    };

    let trajectory = fly(
        &config,
        &plan,
        runway.takeoff_start(),
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(600.0),
            ..SimulationOptions::default()
        },
    );
    assert!(!trajectory.diverged, "the flight diverged");

    // 全フェーズを通ったこと。
    let phases = trajectory.phases_visited();
    for expected in [
        Phase::TakeoffRoll,
        Phase::Climb,
        Phase::Cruise,
        Phase::Turn,
        Phase::Approach,
        Phase::Flare,
        Phase::Rollout,
    ] {
        assert!(
            phases.contains(&expected),
            "the circuit never reached {expected:?}; visited {phases:?}"
        );
    }

    // ちゃんと飛んだこと（場周高度に届いた）。
    assert!(
        trajectory.peak_agl().get() >= 250.0,
        "the aircraft only reached {} AGL",
        trajectory.peak_agl()
    );

    // 空港の周辺に降りたこと。場周 1 周ぶんの半径として 10 km を上限にする。
    let last = trajectory
        .samples
        .last()
        .expect("the trajectory has samples");
    let distance = runway.center().great_circle_distance(last.position);
    assert!(
        distance.get() < 10_000.0,
        "the aircraft ended up {distance} from the airport"
    );

    // 最後は地上で静止に近いこと。
    assert!(
        last.wheel_clearance.get() < Meters(0.5).get(),
        "the aircraft is still {} above the ground",
        last.wheel_clearance
    );
    assert!(
        last.ground_speed.get() < 5.0,
        "the aircraft is still rolling at {}",
        last.ground_speed
    );

    // NaN が出ていないこと。
    assert!(
        trajectory
            .samples
            .iter()
            .all(|sample| sample.position.latitude.get().is_finite()
                && sample.airspeed.get().is_finite()
                && sample.agl.get().is_finite()),
        "the trajectory contains non-finite states"
    );
}

#[test]
fn the_circuit_still_completes_in_a_crosswind() {
    // **横風でも一周できること。** 自動操縦は偏流修正を持たないので
    // 風下へ流されるが、それでも離陸・上昇・旋回・進入・接地の
    // フェーズを全部通り、機体が壊れずに止まること。
    // ここが崩れると、風を入れたせいでゲームが成立しなくなる。
    let runway = Runway::synthetic();
    let config = AircraftConfig::light_single();
    let mut terrain = Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12);

    let plan = CircuitPlan {
        runway_heading: runway.heading,
        outbound_heading: Radians(runway.heading.get() - Degrees(90.0).to_radians().get()),
        ..CircuitPlan::default()
    };

    // 滑走路方位 50° に対して 140°（真横）から 6 m/s。
    let trajectory = fly(
        &config,
        &plan,
        runway.takeoff_start(),
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(600.0),
            wind: Wind {
                from: Radians(runway.heading.get() + Degrees(90.0).to_radians().get()),
                speed: MetersPerSecond(6.0),
            },
            ..SimulationOptions::default()
        },
    );

    assert!(!trajectory.diverged, "the crosswind flight diverged");

    let phases = trajectory.phases_visited();
    for expected in [
        Phase::TakeoffRoll,
        Phase::Climb,
        Phase::Approach,
        Phase::Flare,
        Phase::Rollout,
    ] {
        assert!(
            phases.contains(&expected),
            "the crosswind circuit never reached {expected:?}; visited {phases:?}"
        );
    }

    let last = trajectory.samples.last().expect("samples");
    assert!(
        last.wheel_clearance.get() < 0.5,
        "the aircraft never settled, {} above the ground",
        last.wheel_clearance
    );
    assert!(
        trajectory
            .samples
            .iter()
            .all(|sample| sample.airspeed.get().is_finite() && sample.agl.get().is_finite()),
        "the crosswind trajectory contains non-finite states"
    );
}

#[test]
fn a_headwind_shortens_the_takeoff_roll() {
    // 外部の真値: 向かい風は離陸滑走距離を縮める。対気速度が先に立つため。
    // **これが逆なら風の符号が間違っている。**
    let runway = Runway::synthetic();
    let config = AircraftConfig::light_single();
    let plan = CircuitPlan {
        runway_heading: runway.heading,
        outbound_heading: Radians(runway.heading.get() - Degrees(90.0).to_radians().get()),
        ..CircuitPlan::default()
    };

    let roll_distance = |wind: Wind| {
        let mut terrain = Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12);
        let trajectory = fly(
            &config,
            &plan,
            runway.takeoff_start(),
            &mut terrain,
            &GroundSampler::default(),
            &SimulationOptions {
                max_duration: Seconds(120.0),
                wind,
                ..SimulationOptions::default()
            },
        );
        // 離陸滑走を抜けた最初のサンプルまでの移動距離。
        let start = trajectory.samples.first().expect("samples").position;
        trajectory
            .samples
            .iter()
            .find(|sample| sample.phase != Phase::TakeoffRoll)
            .map(|sample| start.great_circle_distance(sample.position).get())
            .expect("the aircraft should leave the takeoff roll")
    };

    // 滑走路方位そのものから吹く = 真向かい風。
    let headwind = roll_distance(Wind {
        from: runway.heading,
        speed: MetersPerSecond(8.0),
    });
    let calm = roll_distance(Wind::CALM);

    assert!(
        headwind < calm * 0.9,
        "an 8 m/s headwind should shorten the roll from {calm:.0} m, got {headwind:.0} m"
    );
}
