//! 壊しにいくテスト。
//!
//! 受け入れ条件（`acceptance.rs`）は「意図した使い方が動くこと」を見る。
//! こちらは**意図していない使い方で壊れないこと**を見る。
//!
//! 描画層を載せる前にここを固めておく。GUI が絡むと再現も切り分けも一気に難しくなり、
//! 「描画のバグ」に見えるものが実は下層の欠陥だった、という事態になりやすい。

use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::{CircuitPlan, GroundSampler, SimulationOptions, fly};
use flightsim_world::dem::HeightGrid;
use flightsim_world::{DemTile, MemoryTileSource, Terrain, TileId};

fn plan() -> CircuitPlan {
    CircuitPlan::default()
}

fn flat_terrain_at(id: TileId, elevation: f64) -> Terrain<MemoryTileSource> {
    let mut source = MemoryTileSource::new();
    source.insert(
        id,
        DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(elevation))),
    );
    Terrain::new(source, 8 * 1024 * 1024, 0..=20)
}

fn empty_terrain() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 0..=14)
}

/// 軌跡の全サンプルが有限であることを確かめる。
fn assert_all_finite(trajectory: &flightsim_sim::Trajectory, context: &str) {
    assert!(!trajectory.diverged, "{context}: the trajectory diverged");
    for sample in &trajectory.samples {
        for (name, value) in [
            ("latitude", sample.position.latitude.get()),
            ("longitude", sample.position.longitude.get()),
            ("altitude", sample.position.altitude.get()),
            ("ground_elevation", sample.ground_elevation.get()),
            ("agl", sample.agl.get()),
            ("roll", sample.attitude.roll.get()),
            ("pitch", sample.attitude.pitch.get()),
            ("yaw", sample.attitude.yaw.get()),
            ("airspeed", sample.airspeed.get()),
            ("vertical_speed", sample.vertical_speed.get()),
            ("aileron", sample.controls.aileron()),
            ("elevator", sample.controls.elevator()),
            ("throttle", sample.controls.throttle()),
        ] {
            assert!(
                value.is_finite(),
                "{context}: `{name}` was {value} at t = {}",
                sample.time
            );
        }
    }
}

// --- 極 ---

#[test]
fn flying_at_the_poles_does_not_produce_non_finite_state() {
    // 極では「北」が縮退する。接地平面の有限差分がそこで壊れないこと。
    for latitude in [89.999, 90.0, -89.999, -90.0] {
        let start = Geodetic::from_degrees(latitude, 0.0, 0.0);
        let trajectory = fly(
            &AircraftConfig::light_single(),
            &plan(),
            start,
            &mut empty_terrain(),
            &GroundSampler::default(),
            &SimulationOptions {
                max_duration: Seconds(120.0),
                ..SimulationOptions::default()
            },
        );
        assert_all_finite(&trajectory, &format!("at latitude {latitude}°"));
    }
}

#[test]
fn the_ground_plane_at_a_pole_is_finite_and_bounded() {
    // 極でも勾配が発散しないこと。NaN が出ると接地反力から全状態へ伝播する。
    let sampler = GroundSampler::default();
    for latitude in [90.0, -90.0, 89.9999999] {
        let position = Geodetic::from_degrees(latitude, 137.0, 0.0);
        let id = TileId::containing(6, position);
        let mut terrain = flat_terrain_at(id, 250.0);

        let plane = sampler.sample(&mut terrain, position);
        assert!(
            plane.slope.is_finite(),
            "slope at latitude {latitude}° was {:?}",
            plane.slope
        );
        assert!(
            plane.elevation.get().is_finite(),
            "elevation at latitude {latitude}° was {}",
            plane.elevation
        );
    }
}

// --- 日付変更線 ---

#[test]
fn flying_across_the_dateline_keeps_the_terrain_lookup_working() {
    // 経度 +180° と -180° は同じ場所。ここで地形が消えると、
    // 太平洋のど真ん中で地面が無くなる。
    let start = Geodetic::from_degrees(0.0, 179.99, 0.0);
    let id = TileId::containing(6, start);
    let mut terrain = flat_terrain_at(id, 100.0);

    let sampler = GroundSampler::default();
    let plane = sampler.sample(&mut terrain, start);
    assert!(
        plane.from_terrain,
        "terrain was lost right before the dateline"
    );

    // タイルの東端をまたいだ直後も、同じタイルの中なら拾えること。
    let bounds = id.bounds();
    let just_inside = Geodetic::new(
        start.latitude,
        Radians(bounds.east.get() - bounds.width().get() * 1e-9),
        Meters::ZERO,
    );
    assert!(
        sampler.sample(&mut terrain, just_inside).from_terrain,
        "terrain was lost at the eastern edge of its own tile"
    );
}

#[test]
fn a_flight_starting_on_the_dateline_stays_finite() {
    for longitude in [180.0, -180.0, 179.9999, -179.9999] {
        let start = Geodetic::from_degrees(0.0, longitude, 0.0);
        let trajectory = fly(
            &AircraftConfig::light_single(),
            &plan(),
            start,
            &mut empty_terrain(),
            &GroundSampler::default(),
            &SimulationOptions {
                max_duration: Seconds(120.0),
                ..SimulationOptions::default()
            },
        );
        assert_all_finite(&trajectory, &format!("at longitude {longitude}°"));
    }
}

// --- キャッシュ ---

#[test]
fn a_cache_too_small_for_one_tile_still_returns_terrain() {
    // insert 直後に evict されると elevation_at が None を返し、
    // 「タイルはあるのに地形が無い」という状態になる。
    let id = TileId::new(10, 500, 300);
    let mut source = MemoryTileSource::new();
    source.insert(
        id,
        DemTile::new(id.bounds(), HeightGrid::flat(65, 65, Meters(700.0))),
    );

    // 容量 0 は使用側の誤りとして TileCache が拒否する（意図的な事前条件）。
    // ここで見たいのは「1 タイルすら入らない有効な容量」での挙動。
    for capacity in [1_usize, 64, 1_024] {
        let mut terrain = Terrain::new(&source, capacity, 10..=10);
        let elevation = terrain.elevation_at(id.center());
        assert!(
            elevation.is_some(),
            "a {capacity}-byte cache lost a tile that was successfully loaded"
        );
        assert!(
            (elevation.expect("some").get() - 700.0).abs() < 1e-3,
            "a {capacity}-byte cache returned the wrong elevation"
        );
    }
}

#[test]
fn a_tiny_cache_does_not_change_the_trajectory() {
    // キャッシュ容量は性能の問題であって、結果を変えてはならない。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let id = TileId::containing(9, start);

    let build = |capacity: usize| {
        let mut source = MemoryTileSource::new();
        source.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(65, 65, Meters(200.0))),
        );
        Terrain::new(source, capacity, 9..=9)
    };

    let spacious = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut build(64 * 1024 * 1024),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(90.0),
            ..SimulationOptions::default()
        },
    );
    let cramped = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut build(1),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(90.0),
            ..SimulationOptions::default()
        },
    );

    assert_eq!(
        spacious, cramped,
        "the cache capacity changed the trajectory"
    );
}

// --- 長時間 ---

#[test]
fn an_hour_of_flight_stays_finite() {
    // 数値ドリフトの検査。quaternion のノルムが崩れると姿勢が壊れる。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &CircuitPlan {
            // 旋回まで行かせず、巡航を延々続けさせる。
            cruise_duration: Seconds(3_600.0),
            ..plan()
        },
        start,
        &mut empty_terrain(),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(3_600.0),
            sample_interval: Seconds(10.0),
            ..SimulationOptions::default()
        },
    );

    assert_all_finite(&trajectory, "after an hour");
    let last = trajectory.samples.last().expect("samples were recorded");
    assert!(
        last.agl.get() > 100.0 && last.agl.get() < 2_000.0,
        "after an hour the aircraft was at {} AGL; altitude hold drifted",
        last.agl
    );
}

// --- 設定の縁 ---

#[test]
fn a_sample_interval_finer_than_the_step_records_every_step() {
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut empty_terrain(),
        &GroundSampler::default(),
        &SimulationOptions {
            dt: Seconds(1.0 / 120.0),
            max_duration: Seconds(2.0),
            sample_interval: Seconds(1.0 / 10_000.0),
            ..SimulationOptions::default()
        },
    );

    assert_all_finite(&trajectory, "with a very fine sample interval");
    // 2 秒 × 120 Hz = 240 ステップ + 最終サンプル。
    assert!(
        trajectory.samples.len() >= 240,
        "only {} samples were recorded for 240 steps",
        trajectory.samples.len()
    );
}

#[test]
fn a_sample_interval_longer_than_the_run_still_records_something() {
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut empty_terrain(),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(5.0),
            sample_interval: Seconds(1_000.0),
            ..SimulationOptions::default()
        },
    );

    assert!(
        trajectory.samples.len() >= 2,
        "expected at least the first and last sample, got {}",
        trajectory.samples.len()
    );
    assert_all_finite(&trajectory, "with a coarse sample interval");
}

#[test]
fn a_non_finite_sample_interval_does_not_hang_or_panic() {
    // NaN との比較は常に false。記録が一切走らない経路になる。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut empty_terrain(),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(5.0),
            sample_interval: Seconds(f64::NAN),
            ..SimulationOptions::default()
        },
    );
    // 最終サンプルだけは必ず残ること。
    assert!(!trajectory.samples.is_empty());
    assert_all_finite(&trajectory, "with a NaN sample interval");
}

#[test]
fn peak_agl_is_meaningful_even_for_a_very_short_run() {
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut empty_terrain(),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(1.0 / 120.0),
            ..SimulationOptions::default()
        },
    );
    assert!(
        trajectory.peak_agl().get().is_finite(),
        "peak AGL was {} for a one-step run",
        trajectory.peak_agl()
    );
}

// --- 地形の縁 ---

#[test]
fn starting_below_sea_level_works() {
    // 死海は -430 m。負の標高で接地判定が壊れないこと。
    let start = Geodetic::from_degrees(31.5, 35.5, 0.0);
    let id = TileId::containing(9, start);
    let mut terrain = flat_terrain_at(id, -430.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(60.0),
            ..SimulationOptions::default()
        },
    );

    assert_all_finite(&trajectory, "below sea level");
    let first = trajectory.samples.first().expect("samples");
    assert!(
        (first.ground_elevation.get() + 430.0).abs() < 1.0,
        "ground elevation was {} where the terrain says -430 m",
        first.ground_elevation
    );
}

#[test]
fn starting_on_a_high_plateau_works() {
    // チベット高原は 4500 m。空気が薄く推力も揚力も落ちる。
    let start = Geodetic::from_degrees(33.0, 88.0, 0.0);
    let id = TileId::containing(9, start);
    let mut terrain = flat_terrain_at(id, 4_500.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(120.0),
            ..SimulationOptions::default()
        },
    );

    assert_all_finite(&trajectory, "on a high plateau");
    let first = trajectory.samples.first().expect("samples");
    assert!((first.ground_elevation.get() - 4_500.0).abs() < 1.0);
}

#[test]
fn the_coarsest_and_finest_tile_levels_both_work() {
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    for level in [0_u8, 1, 20, flightsim_world::tile::MAX_LEVEL] {
        let id = TileId::containing(level, start);
        let mut source = MemoryTileSource::new();
        source.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(150.0))),
        );
        let mut terrain = Terrain::new(source, 1024 * 1024, level..=level);

        let elevation = terrain.elevation_at(start);
        assert!(elevation.is_some(), "level {level} lost its tile",);
        assert!((elevation.expect("some").get() - 150.0).abs() < 1e-3);
    }
}

// --- 決定論 ---

#[test]
fn determinism_holds_across_many_repeats() {
    // 2 回だけでは、たまたま一致した可能性を排除できない。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let id = TileId::containing(9, start);

    let run = || {
        fly(
            &AircraftConfig::light_single(),
            &plan(),
            start,
            &mut flat_terrain_at(id, 300.0),
            &GroundSampler::default(),
            &SimulationOptions {
                max_duration: Seconds(120.0),
                ..SimulationOptions::default()
            },
        )
    };

    let reference = run();
    for attempt in 1..=8 {
        assert_eq!(reference, run(), "run {attempt} differed from the first");
    }
}

#[test]
fn a_different_probe_distance_changes_nothing_on_flat_ground() {
    // 平坦地では探査距離によらず同じ平面が出るはず。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let id = TileId::containing(9, start);

    for distance in [1.0, 10.0, 50.0] {
        let mut terrain = flat_terrain_at(id, 300.0);
        let plane = GroundSampler::new(Meters(distance), Meters::ZERO).sample(&mut terrain, start);
        assert!(
            plane.slope.north().abs() < 1e-6 && plane.slope.east().abs() < 1e-6,
            "a {distance} m probe reported slope {:?} on flat ground",
            plane.slope
        );
        assert!((plane.elevation.get() - 300.0).abs() < 1e-3);
    }
}

// --- 速度域 ---

#[test]
fn an_aircraft_that_flies_itself_off_still_gets_managed() {
    // 零迎角でも揚力係数は正なので、回転速度に達しなくても速度だけで浮く。
    // そのとき TakeoffRoll のままだと、翼は水平固定・高度は無管理のまま
    // 上昇し続ける。浮いた時点で上昇フェーズへ移ること。
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let id = TileId::containing(9, start);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &CircuitPlan {
            rotate_speed: MetersPerSecond(10_000.0),
            ..plan()
        },
        start,
        &mut flat_terrain_at(id, 300.0),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(180.0),
            ..SimulationOptions::default()
        },
    );

    assert_all_finite(&trajectory, "with an unreachable rotate speed");
    assert_ne!(
        trajectory.final_phase,
        flightsim_sim::Phase::TakeoffRoll,
        "the aircraft got airborne but stayed in the takeoff roll phase,          where nothing manages its altitude or heading"
    );
    assert!(
        trajectory
            .phases_visited()
            .contains(&flightsim_sim::Phase::Climb),
        "phases visited: {:?}",
        trajectory.phases_visited()
    );
}

#[test]
fn a_zero_pattern_altitude_does_not_break_the_phase_machine() {
    let start = Geodetic::from_degrees(35.0, 139.0, 0.0);
    let id = TileId::containing(9, start);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &CircuitPlan {
            pattern_altitude_agl: Meters::ZERO,
            ..plan()
        },
        start,
        &mut flat_terrain_at(id, 300.0),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(300.0),
            ..SimulationOptions::default()
        },
    );
    assert_all_finite(&trajectory, "with a zero pattern altitude");
}
