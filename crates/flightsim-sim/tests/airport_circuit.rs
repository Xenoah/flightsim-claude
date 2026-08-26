//! M2 完了条件の受け入れ検査: 「1 空港周辺で離陸 → 旋回 → 着陸が通ること」。
//!
//! 合成飛行場（`Runway::synthetic()`）の離陸開始点から場周を飛ぶ。
//! 地形は滑走路標高と同じ平地（合成 DEM のタイルは CI に無いため）。
//! 滑走路の contains / offsets は高度を見ないので、この単純化で
//! 「空港周辺に降りたか」の判定は損なわれない。
//!
//! 自動操縦は滑走路の延長中心線を捕捉し、無風と横風の両方で
//! 滑走路矩形内に接地する。これを数値で回帰検査する。

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

    let plan = CircuitPlan::for_runway(runway);

    let trajectory = fly(
        &config,
        &plan,
        runway.takeoff_start(),
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(900.0),
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

    assert_touchdown_on_runway(&trajectory, runway);

    // ロールアウト後も空港の周辺に残ったこと。
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
fn the_circuit_still_completes_in_crosswinds_from_both_sides() {
    // **左右どちらの横風でも一周できること。** 方位を中心線の
    // 前方注視点へ更新し続けることで偏流を修正し、滑走路内に接地させる。
    let runway = Runway::synthetic();
    let config = AircraftConfig::light_single();
    let plan = CircuitPlan::for_runway(runway);

    for side in [-90.0_f64, 90.0] {
        let mut terrain = Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12);
        let trajectory = fly(
            &config,
            &plan,
            runway.takeoff_start(),
            &mut terrain,
            &GroundSampler::default(),
            &SimulationOptions {
                max_duration: Seconds(900.0),
                // 滑走路の左または右、真横から 6 m/s。
                wind: Wind {
                    from: Radians(runway.heading.get() + Degrees(side).to_radians().get()),
                    speed: MetersPerSecond(6.0),
                },
                ..SimulationOptions::default()
            },
        );

        assert!(
            !trajectory.diverged,
            "the {side:+.0}° crosswind flight diverged"
        );

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
                "the {side:+.0}° crosswind circuit never reached {expected:?}; visited {phases:?}"
            );
        }

        let last = trajectory.samples.last().expect("samples");
        assert!(
            last.wheel_clearance.get() < 0.5,
            "the {side:+.0}° crosswind aircraft never settled, {} above the ground",
            last.wheel_clearance
        );
        assert_touchdown_on_runway(&trajectory, runway);
        assert!(
            trajectory
                .samples
                .iter()
                .all(|sample| sample.airspeed.get().is_finite() && sample.agl.get().is_finite()),
            "the {side:+.0}° crosswind trajectory contains non-finite states"
        );
    }
}

fn assert_touchdown_on_runway(trajectory: &flightsim_sim::Trajectory, runway: Runway) {
    let touchdown = trajectory
        .samples
        .iter()
        .find(|sample| sample.phase == Phase::Rollout)
        .expect("the flight should enter rollout");
    let offsets = runway.offsets(touchdown.position);
    assert!(
        runway.contains(touchdown.position),
        "touchdown missed the runway: longitudinal {:.1} m, lateral {:.1} m (width {:.1} m)",
        offsets.longitudinal.get(),
        offsets.lateral.get(),
        runway.width.get()
    );
    assert!(
        offsets.lateral.get().abs() <= runway.width.get() * 0.35,
        "touchdown left too little edge margin: lateral {:.1} m for width {:.1} m",
        offsets.lateral.get(),
        runway.width.get()
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
