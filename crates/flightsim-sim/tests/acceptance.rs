//! M1 の完了条件を検査する。
//!
//! 「ヘッドレスで実地形の上を物理的に妥当に飛ぶ軌跡を出力できること」が
//! 満たされているかを、焼いたタイルをディスクに置いて実際に飛ばして確かめる。
//!
//! ここで使うタイルはテスト内で生成する。CI に実データ（数百 GB）は置けないが、
//! **実行時の経路（ディスク → 解析 → キャッシュ → 標高 → 接地平面 → FDM）は
//! 本物を通す。**

use flightsim_core::{Degrees, Geodetic, Meters, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::{CircuitPlan, GroundSampler, Phase, SimulationOptions, Trajectory, fly};
use flightsim_world::dem::io::{tile_relative_path, write_tile};
use flightsim_world::{DiskTileSource, HeightGrid, MemoryTileSource, Terrain, TileId};
use std::path::{Path, PathBuf};

/// テスト毎に独立した一時ディレクトリ。
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flightsim-sim-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&path).expect("the temporary directory should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

const TILE_LEVEL: u8 = 11;
const GRID_SIZE: u32 = 65;

/// 解析的な標高関数からタイルを焼く。
///
/// 隣接タイルは境界の格子点座標が厳密に一致するため、同じ関数から焼けば
/// **境界で標高が一致する**。継ぎ目の検査はこの性質に依存している。
fn bake_around<F>(directory: &Path, centre: Geodetic, radius_tiles: i64, elevation: F)
where
    F: Fn(Geodetic) -> f64,
{
    let origin = TileId::containing(TILE_LEVEL, centre);
    let columns = i64::from(TileId::columns(TILE_LEVEL));
    let rows = i64::from(TileId::rows(TILE_LEVEL));

    for dy in -radius_tiles..=radius_tiles {
        for dx in -radius_tiles..=radius_tiles {
            let x = (i64::from(origin.x) + dx).rem_euclid(columns);
            let y = i64::from(origin.y) + dy;
            if y < 0 || y >= rows {
                continue;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "rem_euclid と範囲検査で 0..columns / 0..rows に収まっている"
            )]
            let id = TileId::new(TILE_LEVEL, x as u32, y as u32);

            let bounds = id.bounds();
            let steps = f64::from(GRID_SIZE - 1);
            let mut samples = Vec::with_capacity((GRID_SIZE as usize).pow(2));
            for row in 0..GRID_SIZE {
                for column in 0..GRID_SIZE {
                    let position = Geodetic::new(
                        Radians(
                            bounds.north.get() - f64::from(row) / steps * bounds.height().get(),
                        ),
                        Radians(
                            bounds.west.get() + f64::from(column) / steps * bounds.width().get(),
                        ),
                        Meters::ZERO,
                    );
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で十分"
                    )]
                    samples.push(elevation(position) as f32);
                }
            }

            let path = directory.join(tile_relative_path(id));
            std::fs::create_dir_all(path.parent().expect("has a parent")).expect("tile directory");
            let mut bytes = Vec::new();
            write_tile(
                &mut bytes,
                id,
                &HeightGrid::new(GRID_SIZE, GRID_SIZE, samples),
            )
            .expect("the synthetic grid should encode");
            std::fs::write(&path, bytes).expect("the tile should be writable");
        }
    }
}

fn terrain_from(directory: &Path) -> Terrain<DiskTileSource> {
    Terrain::new(
        DiskTileSource::new(directory),
        64 * 1024 * 1024,
        8..=TILE_LEVEL,
    )
}

fn plan() -> CircuitPlan {
    CircuitPlan {
        runway_heading: Radians::ZERO,
        outbound_heading: Degrees(90.0).to_radians(),
        ..CircuitPlan::default()
    }
}

fn options() -> SimulationOptions {
    SimulationOptions {
        max_duration: Seconds(600.0),
        sample_interval: Seconds(0.5),
        ..SimulationOptions::default()
    }
}

fn assert_sane(trajectory: &Trajectory) {
    assert!(!trajectory.diverged, "the trajectory diverged");
    for sample in &trajectory.samples {
        assert!(
            sample.position.altitude.get().is_finite()
                && sample.airspeed.get().is_finite()
                && sample.attitude.is_finite(),
            "a non-finite value appeared at t = {}",
            sample.time
        );
    }
}

// --- 受け入れ条件 1: 実地形の上で離陸 → 旋回 → 着陸 ---

#[test]
fn a_full_circuit_completes_over_baked_terrain() {
    let workspace = TempDir::new("circuit");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    // 起伏のある地形。平坦な板の上を飛んだだけでは統合の検証にならない。
    bake_around(workspace.path(), start, 4, |position| {
        400.0
            + 60.0 * (position.longitude.get() * 3_000.0).sin()
            + 40.0 * (position.latitude.get() * 3_000.0).cos()
    });

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &options(),
    );

    assert_sane(&trajectory);
    assert_eq!(
        trajectory.final_phase,
        Phase::Complete,
        "the circuit stopped at {:?} after {} — phases: {:?}",
        trajectory.final_phase,
        trajectory.duration,
        trajectory.phases_visited()
    );
    assert_eq!(
        trajectory.phases_visited(),
        vec![
            Phase::TakeoffRoll,
            Phase::Climb,
            Phase::Cruise,
            Phase::Turn,
            Phase::Approach,
            Phase::Flare,
            Phase::Rollout,
            Phase::Complete,
        ]
    );
    assert_eq!(
        trajectory.steps_without_terrain, 0,
        "the aircraft left the baked area; the test terrain is too small"
    );
}

#[test]
fn the_aircraft_actually_leaves_the_ground_and_returns() {
    let workspace = TempDir::new("airborne");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |_| 400.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &options(),
    );

    assert_sane(&trajectory);
    assert!(
        trajectory.peak_agl().get() > 250.0,
        "the aircraft only reached {} AGL",
        trajectory.peak_agl()
    );

    let last = trajectory.samples.last().expect("samples were recorded");
    assert!(
        last.wheel_clearance.get().abs() < 0.5,
        "the aircraft ended {} above the ground rather than on it",
        last.wheel_clearance
    );
    assert!(
        last.airspeed.get() < 2.0,
        "the aircraft was still doing {} at the end",
        last.airspeed
    );
}

#[test]
fn the_touchdown_is_survivable() {
    // 沈下率 3 m/s は軽single機の設計限界（10 ft/s）。それを超える接地は
    // 「着陸した」とは言えない。
    let workspace = TempDir::new("touchdown");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |_| 250.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &options(),
    );
    assert_sane(&trajectory);

    let worst_sink = trajectory
        .samples
        .iter()
        .filter(|sample| matches!(sample.phase, Phase::Flare | Phase::Rollout))
        .map(|sample| -sample.vertical_speed.get())
        .fold(f64::NEG_INFINITY, f64::max);

    assert!(
        worst_sink < 3.0,
        "touched down at {worst_sink:.2} m/s, beyond the 3 m/s design limit"
    );
}

// --- 受け入れ条件 2: 地形標高が反映されている ---

#[test]
fn flying_over_a_plateau_puts_the_aircraft_higher_than_over_a_plain() {
    // 地形が FDM へ渡っていなければ、どちらも同じ高度で接地してしまう。
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);

    let run_at = |elevation: f64, name: &str| -> Trajectory {
        let workspace = TempDir::new(name);
        bake_around(workspace.path(), start, 4, |_| elevation);
        let trajectory = fly(
            &AircraftConfig::light_single(),
            &plan(),
            start,
            &mut terrain_from(workspace.path()),
            &GroundSampler::default(),
            &options(),
        );
        assert_sane(&trajectory);
        trajectory
    };

    let plain = run_at(0.0, "plain");
    let plateau = run_at(1_500.0, "plateau");

    let plain_start = plain
        .samples
        .first()
        .expect("samples")
        .position
        .altitude
        .get();
    let plateau_start = plateau
        .samples
        .first()
        .expect("samples")
        .position
        .altitude
        .get();

    assert!(
        (plateau_start - plain_start - 1_500.0).abs() < 5.0,
        "the plateau start was {plateau_start} m and the plain {plain_start} m; \
         the 1500 m difference did not reach the aircraft"
    );

    for sample in &plateau.samples {
        assert!(
            (sample.ground_elevation.get() - 1_500.0).abs() < 5.0,
            "ground elevation was recorded as {} over the plateau",
            sample.ground_elevation
        );
    }
}

#[test]
fn sloping_ground_tilts_the_parked_aircraft() {
    // 勾配が FDM に渡っていなければ、坂の上でも機体は水平のままになる。
    let workspace = TempDir::new("slope");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    // 北へ 1 度あたり 4000 m 上がる（約 3.6% 勾配）。
    bake_around(workspace.path(), start, 4, |position| {
        400.0 + 4_000.0 * (position.latitude.get() - start.latitude.get()).to_degrees()
    });

    let mut terrain = terrain_from(workspace.path());
    let sampler = GroundSampler::default();
    let ground = sampler.sample(&mut terrain, start);

    assert!(ground.from_terrain);
    assert!(
        ground.slope.north() > 0.0,
        "a north-rising slope was reported as {:?}",
        ground.slope
    );

    // 静止させて少しだけ回すと、機体は斜面に沿って傾く。
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &CircuitPlan {
            // 離陸させずに接地状態を観察するため、回転速度を到達不能にする。
            rotate_speed: flightsim_core::MetersPerSecond(1_000.0),
            ..plan()
        },
        start,
        &mut terrain,
        &sampler,
        &SimulationOptions {
            max_duration: Seconds(4.0),
            sample_interval: Seconds(0.5),
            ..SimulationOptions::default()
        },
    );
    assert_sane(&trajectory);

    let settled = trajectory.samples.last().expect("samples");
    assert!(
        settled.attitude.pitch.get().abs() > 0.5_f64.to_radians(),
        "the aircraft stayed at {:.3}° pitch on a 3.6% slope",
        settled.attitude.pitch.to_degrees().get()
    );
}

// --- 受け入れ条件 3: タイル境界で標高が飛ばない ---

#[test]
fn ground_elevation_never_jumps_between_samples() {
    // タイルをまたぐたびに段差があると、機体が見えない縁石を踏み続ける。
    let workspace = TempDir::new("continuity");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |position| {
        300.0 + 30.0 * (position.longitude.get() * 2_000.0).sin()
    });

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &options(),
    );
    assert_sane(&trajectory);

    // 0.5 秒間隔で最大 60 m/s 進む。滑らかな地形なら標高差は小さいはず。
    for pair in trajectory.samples.windows(2) {
        let jump = (pair[1].ground_elevation.get() - pair[0].ground_elevation.get()).abs();
        assert!(
            jump < 20.0,
            "ground elevation jumped {jump:.2} m between t = {} and t = {} \
             (lon {:.5} → {:.5}); a tile seam is likely",
            pair[0].time,
            pair[1].time,
            pair[0].position.longitude_degrees(),
            pair[1].position.longitude_degrees()
        );
    }
}

#[test]
fn elevation_matches_across_a_tile_seam() {
    // 継ぎ目そのものを狙って、両側で標高が一致することを直接確かめる。
    let workspace = TempDir::new("seam");
    let centre = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), centre, 2, |position| {
        500.0 + 200.0 * position.longitude.get().sin() + 100.0 * position.latitude.get().cos()
    });

    let mut terrain = terrain_from(workspace.path());
    let tile = TileId::containing(TILE_LEVEL, centre);
    let bounds = tile.bounds();
    let step = bounds.width().get() * 1e-7;

    for fraction in [0.1, 0.25, 0.5, 0.75, 0.9] {
        let latitude = Radians(bounds.north.get() - fraction * bounds.height().get());
        let west_side = terrain
            .elevation_at(Geodetic::new(
                latitude,
                Radians(bounds.east.get() - step),
                Meters::ZERO,
            ))
            .expect("the western tile is baked");
        let east_side = terrain
            .elevation_at(Geodetic::new(
                latitude,
                Radians(bounds.east.get() + step),
                Meters::ZERO,
            ))
            .expect("the eastern tile is baked");

        assert!(
            (west_side.get() - east_side.get()).abs() < 0.5,
            "at {fraction} down the seam the two sides read {west_side} and {east_side}"
        );
    }
}

// --- 受け入れ条件 4: 決定論 ---

#[test]
fn the_same_scenario_produces_a_bit_identical_trajectory() {
    // 再現しない軌跡では回帰テストもリプレイも成立しない（ADR-0004）。
    let workspace = TempDir::new("determinism");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |position| {
        350.0 + 80.0 * (position.longitude.get() * 2_500.0).sin()
    });

    let run = || {
        fly(
            &AircraftConfig::light_single(),
            &plan(),
            start,
            &mut terrain_from(workspace.path()),
            &GroundSampler::default(),
            &options(),
        )
    };

    let first = run();
    let second = run();

    assert_eq!(
        first.samples.len(),
        second.samples.len(),
        "the two runs recorded different numbers of samples"
    );
    assert_eq!(
        first, second,
        "two identical scenarios produced different trajectories"
    );
}

#[test]
fn a_warm_terrain_cache_does_not_change_the_trajectory() {
    // キャッシュが答えを変えると決定論が崩れる。
    let workspace = TempDir::new("warm-cache");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |position| {
        350.0 + 50.0 * (position.latitude.get() * 2_500.0).cos()
    });

    let mut terrain = terrain_from(workspace.path());
    let cold = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain,
        &GroundSampler::default(),
        &options(),
    );
    // 同じ Terrain を使い回す。今度はキャッシュが埋まった状態で始まる。
    let warm = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain,
        &GroundSampler::default(),
        &options(),
    );

    assert_eq!(cold, warm, "the warm cache changed the trajectory");
}

// --- 受け入れ条件 5: タイルが無い領域 ---

#[test]
fn flying_where_no_tiles_exist_still_works() {
    // 海上。地形が無いのは異常ではない。
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        Geodetic::from_degrees(0.0, -140.0, 0.0),
        &mut Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=TILE_LEVEL),
        &GroundSampler::default(),
        &options(),
    );

    assert_sane(&trajectory);
    assert_eq!(trajectory.final_phase, Phase::Complete);
    assert!(trajectory.steps_without_terrain > 0);
    for sample in &trajectory.samples {
        assert!(
            !sample.terrain_available,
            "terrain was reported over open ocean"
        );
        assert!(
            sample.ground_elevation.get().abs() < 1e-9,
            "sea level should be exactly 0 m, got {}",
            sample.ground_elevation
        );
    }
}

#[test]
fn leaving_the_baked_area_mid_flight_does_not_break_the_run() {
    // 焼いた範囲を出た瞬間に破綻しないこと。運用上は普通に起きる。
    let workspace = TempDir::new("edge");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    // 出発点まわりだけ焼く。場周飛行の途中で必ず外へ出る。
    bake_around(workspace.path(), start, 0, |_| 300.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &options(),
    );

    assert_sane(&trajectory);
    assert!(
        trajectory.steps_without_terrain > 0,
        "the aircraft never left the baked tile; the test is not exercising the edge"
    );
    assert!(
        trajectory.samples.iter().any(|s| s.terrain_available),
        "the aircraft never had terrain at all"
    );
}

#[test]
fn a_corrupt_tile_does_not_stop_the_flight() {
    // 壊れたタイル 1 枚で飛行全体が止まってはいけない。
    let workspace = TempDir::new("corrupt");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 4, |_| 300.0);

    // 出発点のタイルを壊す。
    let broken = TileId::containing(TILE_LEVEL, start);
    std::fs::write(
        workspace.path().join(tile_relative_path(broken)),
        b"not a tile",
    )
    .expect("overwrite");

    let mut terrain = terrain_from(workspace.path());
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain,
        &GroundSampler::default(),
        &options(),
    );

    assert_sane(&trajectory);
    assert!(
        !terrain.load_failures().is_empty(),
        "the corrupt tile should have been reported"
    );
}

// --- 出力 ---

#[test]
fn the_csv_has_one_row_per_sample_and_a_header() {
    let workspace = TempDir::new("csv");
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    bake_around(workspace.path(), start, 2, |_| 300.0);

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        start,
        &mut terrain_from(workspace.path()),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(20.0),
            ..options()
        },
    );

    let mut csv = Vec::new();
    trajectory
        .write_csv(&mut csv)
        .expect("writing to a Vec cannot fail");
    let text = String::from_utf8(csv).expect("the CSV must be valid UTF-8");

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), trajectory.samples.len() + 1);
    assert!(lines[0].starts_with("time_s,phase,latitude_deg"));

    let columns = lines[0].split(',').count();
    for (index, line) in lines.iter().enumerate().skip(1) {
        assert_eq!(
            line.split(',').count(),
            columns,
            "row {index} has a different number of columns than the header"
        );
    }
}

// --- パイプライン全体 ---

#[test]
fn tiles_baked_by_the_tilegen_pipeline_can_be_flown_over() {
    // 焼く側と読む側が同じ形式に合意していることを、実際に飛んで確かめる。
    // 片方だけ変えたときにここが落ちる。
    use flightsim_tilegen::testing::GeoTiffBuilder;
    use flightsim_tilegen::{RasterSet, TileGenOptions, generate_tiles};

    let workspace = TempDir::new("pipeline");
    let (west, north) = (139.70, 35.62);
    let (size, pixel) = (512_u32, 1.0 / 3600.0);

    // 一定標高 300 m のラスタ。
    let bytes = GeoTiffBuilder::new(size, size, vec![300.0_f32; (size * size) as usize])
        .origin(west, north)
        .pixel_size(pixel, pixel)
        .build();
    let source = workspace.path().join("terrain.tif");
    std::fs::write(&source, bytes).expect("writing the synthetic raster");

    let rasters = RasterSet::load(&[source]).expect("the raster should open");
    let coverage = rasters.coverage().expect("one raster");
    let tiles = workspace.path().join("tiles");
    let report = generate_tiles(
        &rasters,
        coverage,
        TILE_LEVEL..=TILE_LEVEL,
        &TileGenOptions::default(),
        &tiles,
        false,
    )
    .expect("baking should succeed");
    assert!(report.tiles_written > 0);

    // 焼いた範囲の中心から少しだけ飛ぶ。
    let centre = Geodetic::new(
        Radians((coverage.north().get() + coverage.south().get()) * 0.5),
        Radians((coverage.east().get() + coverage.west().get()) * 0.5),
        Meters::ZERO,
    );
    let mut terrain = terrain_from(&tiles);

    let elevation = terrain
        .elevation_at(centre)
        .expect("the baked tiles cover the centre");
    assert!(
        (elevation.get() - 300.0).abs() < 1.0,
        "the baked terrain reads {elevation} where the raster says 300 m"
    );

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan(),
        centre,
        &mut terrain,
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(30.0),
            ..options()
        },
    );

    assert_sane(&trajectory);
    assert!(
        terrain.load_failures().is_empty(),
        "tiles we just baked failed to load"
    );
    assert!(
        trajectory.samples.iter().any(|s| s.terrain_available),
        "the baked tiles were never picked up by the runtime loader"
    );
}
