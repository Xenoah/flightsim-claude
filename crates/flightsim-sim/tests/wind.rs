//! 定常風の検査。
//!
//! 外部の真値と突き合わせる:
//! - 駐機中の対気速度は風速そのもの（対地速度は 0）
//! - 横風は機体を風下へ流す
//! - from/to の符号: 270°（西）からの風は空気を**東へ**動かす

use flightsim_core::{Degrees, Geodetic, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{GroundSampler, Simulation, Wind};
use flightsim_world::{MemoryTileSource, Terrain};

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

#[test]
fn the_wind_vector_points_where_the_air_actually_moves() {
    // 航空の慣習は「どこから吹くか」。270°（西）から 10 m/s の風は
    // 空気が東（East 正）へ 10 m/s 動くこと。ここを逆にすると
    // 追い風と向かい風が入れ替わり、離陸距離の感覚が全部逆になる。
    let westerly = Wind {
        from: Degrees(270.0).to_radians(),
        speed: MetersPerSecond(10.0),
    };
    let ned = westerly.to_ned();
    assert!(
        (ned.east() - 10.0).abs() < 1e-9,
        "a westerly wind must move air eastward, got east = {}",
        ned.east()
    );
    assert!(ned.north().abs() < 1e-9);

    // 360°（北）からの風は空気を南へ。
    let northerly = Wind {
        from: Degrees(360.0).to_radians(),
        speed: MetersPerSecond(4.0),
    };
    assert!(northerly.to_ned().north() < -3.9);
}

#[test]
fn a_parked_aircraft_reads_the_wind_as_airspeed() {
    // 地上静止なら対気速度 = 風速。実機の吹き流しと同じ理屈で、
    // 風の結線が生きているかを一撃で確かめられる。
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.548, 139.775, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );
    simulation.set_wind(Wind {
        from: Degrees(0.0).to_radians(),
        speed: MetersPerSecond(12.0),
    });

    // ブレーキで静止を保つ。
    let controls = ControlInputs::neutral().with_brakes(1.0);
    for _ in 0..300 {
        simulation.advance(Seconds(1.0 / 60.0), controls);
    }

    let state = simulation.state();
    assert!(
        state.ground_speed().get() < 0.5,
        "the aircraft should stay parked, ground speed {}",
        state.ground_speed()
    );

    // 対気速度 = |対地速度ベクトル − 風ベクトル|。地上静止なら風速そのもの。
    let environment = flightsim_fdm::Environment::with_wind_ned(
        flightsim_fdm::Atmosphere::standard(),
        state.geodetic(),
        simulation.wind().to_ned(),
    );
    let airspeed = (state.velocity - environment.wind_ecef).length();
    assert!(
        (airspeed - 12.0).abs() < 1.0,
        "a parked aircraft in a 12 m/s wind should read about 12 m/s airspeed, got {airspeed}"
    );
}

#[test]
fn a_crosswind_shifts_the_early_trajectory_downwind() {
    // **「落ちる機体は風下へ流れ続ける」は誤り**だと実測で分かっている。
    // 垂直尾翼の風見安定で機首が風上を向き、滑空が始まると風上へ切り込む
    // （実測: 西風 15 m/s で 2 秒後 +1.9 m 東、10 秒後は -77 m 西・+128 m 前方）。
    // 外部真値にできるのは**初期の力積**だけ: 空力が姿勢を変える前の
    // 数秒間は、無風との差分が必ず風下（東）を向く。
    let make = |wind: Wind| {
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.6, 139.5, 300.0),
            flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
            Ned::new(0.0, 0.0, 0.0),
        );
        let mut simulation = Simulation::from_state(
            AircraftConfig::light_single(),
            state,
            flat_world(),
            GroundSampler::default(),
        );
        simulation.set_wind(wind);
        simulation
    };

    let mut windy = make(Wind {
        from: Degrees(270.0).to_radians(),
        speed: MetersPerSecond(15.0),
    });
    let mut calm = make(Wind::CALM);

    // 2 秒 = 姿勢の空力が支配的になる前。
    for _ in 0..120 {
        windy.advance(Seconds(1.0 / 60.0), ControlInputs::default());
        calm.advance(Seconds(1.0 / 60.0), ControlInputs::default());
    }

    let east_of = |simulation: &Simulation<MemoryTileSource>| {
        simulation.state().geodetic().longitude_degrees()
    };
    let difference_degrees = east_of(&windy) - east_of(&calm);
    let difference_metres =
        difference_degrees.to_radians() * 35.6_f64.to_radians().cos() * 6_378_137.0;
    assert!(
        difference_metres > 0.5,
        "in the first two seconds a westerly wind must push the aircraft east          relative to calm air, got {difference_metres:.2} m"
    );
}

#[test]
fn calm_wind_changes_nothing() {
    // Wind::CALM の経路が still_air と同じ結果になること（結線の回帰防止）。
    let make = || {
        Simulation::parked(
            AircraftConfig::light_single(),
            Geodetic::from_degrees(35.548, 139.775, 0.0),
            Radians::ZERO,
            flat_world(),
            GroundSampler::default(),
        )
    };
    let mut with_calm = make();
    with_calm.set_wind(Wind::CALM);
    let mut untouched = make();

    for _ in 0..300 {
        with_calm.advance(Seconds(1.0 / 60.0), ControlInputs::default());
        untouched.advance(Seconds(1.0 / 60.0), ControlInputs::default());
    }
    assert_eq!(
        with_calm.state().position,
        untouched.state().position,
        "setting a calm wind must be a no-op"
    );
}

#[test]
fn non_finite_wind_does_not_poison_the_state() {
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.548, 139.775, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );
    simulation.set_wind(Wind {
        from: Radians(f64::NAN),
        speed: MetersPerSecond(f64::INFINITY),
    });
    for _ in 0..120 {
        let report = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::default());
        assert!(!report.diverged, "non-finite wind poisoned the simulation");
    }
    assert!(simulation.state().is_finite());
}
