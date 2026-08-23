//! 傾斜地に spawn した機体が裏返らないこと。
//!
//! # 何の再現なのか
//!
//! 合成地形の山腹（標高約 1 400 m、傾斜あり）へ spawn した直後、HUD が
//! BNK -175.5°・AGL -4 ft・GND を表示した。**AGL が負**なことから、初期状態で
//! 車輪が地面へめり込み、その反力で機体が跳ね上げられていた。
//!
//! [`flightsim_sim::parked_state`] は接地平面の勾配を見ずに「基準点の標高 + 脚の高さ」へ
//! 水平姿勢で機体を置く。傾斜地ではこれで上り側の車輪がめり込む。原因の修正は
//! `flightsim-fdm` の接地反力側（脚の弾性力を有限にし、伸長速度に上限を設けた）で、
//! ここでは **地形 → 接地平面 → FDM の結線を通しても裏返らないこと**を確かめる。
//!
//! 実データは要らない。傾斜が一定の合成タイルを 1 枚焼けば足りる。

use flightsim_core::{Geodetic, LocalFrame, Meters, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::dem::HeightGrid;
use flightsim_world::{DemTile, MemoryTileSource, Terrain, TileId};

/// 山腹に見立てた地点。
const SPAWN_LATITUDE: f64 = 35.55;
const SPAWN_LONGITUDE: f64 = 139.33;
const SPAWN_ELEVATION: f64 = 1_400.0;

/// タイルのレベル。高いほど 1 枚が小さく、急斜面でも標高の振れ幅が小さく済む。
const TILE_LEVEL: u8 = 13;
const GRID_SIZE: u32 = 65;

/// 姿勢の許容偏位。裏返りの検出が目的なので、実測値そのものではなく余裕を見た値。
const MAXIMUM_ATTITUDE_EXCURSION_DEGREES: f64 = 30.0;

/// spawn 地点を通る一定勾配の平面を 1 枚のタイルに焼く。
///
/// 標高は spawn 地点を原点とする局所 NED での北・東オフセットから決める。
/// **緯度経度の差をそのまま距離として使わない。** 経度 1° の距離は緯度で変わる。
fn sloped_tile(slope_degrees: f64, slope_bearing_degrees: f64) -> (TileId, DemTile) {
    let tangent = slope_degrees.to_radians().tan();
    let bearing = slope_bearing_degrees.to_radians();
    let (slope_north, slope_east) = (tangent * bearing.cos(), tangent * bearing.sin());

    let centre = Geodetic::from_degrees(SPAWN_LATITUDE, SPAWN_LONGITUDE, 0.0);
    let frame = LocalFrame::new(centre);
    let id = TileId::containing(TILE_LEVEL, centre);
    let bounds = id.bounds();
    let steps = f64::from(GRID_SIZE - 1);

    let mut samples = Vec::with_capacity((GRID_SIZE as usize).pow(2));
    for row in 0..GRID_SIZE {
        for column in 0..GRID_SIZE {
            // 格子の先頭行が最北端。
            let position = Geodetic::new(
                Radians(bounds.north.get() - f64::from(row) / steps * bounds.height().get()),
                Radians(bounds.west.get() + f64::from(column) / steps * bounds.width().get()),
                Meters::ZERO,
            );
            let offset = frame.ecef_to_ned_position(position.to_ecef());
            let elevation =
                SPAWN_ELEVATION + slope_north * offset.north() + slope_east * offset.east();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "タイルの標高は f32 で保持する形式。1 400 m 付近で 1e-4 m の分解能があり十分"
            )]
            samples.push(elevation as f32);
        }
    }

    (
        id,
        DemTile::new(bounds, HeightGrid::new(GRID_SIZE, GRID_SIZE, samples)),
    )
}

fn terrain_on_a_slope(slope_degrees: f64, slope_bearing_degrees: f64) -> Terrain<MemoryTileSource> {
    let (id, tile) = sloped_tile(slope_degrees, slope_bearing_degrees);
    let mut source = MemoryTileSource::new();
    source.insert(id, tile);
    Terrain::new(source, 8 * 1024 * 1024, 0..=14)
}

struct Outcome {
    maximum_tilt_degrees: f64,
    maximum_bank_degrees: f64,
    maximum_pitch_degrees: f64,
    /// 車輪の対地高度の最小値。負なら地面へめり込んでいる。
    minimum_wheel_clearance: f64,
    terrain_missing: bool,
}

/// 斜面に spawn して `seconds` 秒ぶん、60 Hz のフレームで進める。
///
/// # Panics
///
/// 状態が非有限になった時点で落とす。
fn spawn_on_slope(slope_degrees: f64, slope_bearing_degrees: f64, seconds: f64) -> Outcome {
    let config = AircraftConfig::light_single();
    let gear_height = flightsim_sim::gear_height(&config).get();
    let mut terrain = terrain_on_a_slope(slope_degrees, slope_bearing_degrees);
    let sampler = GroundSampler::default();
    let start = Geodetic::from_degrees(SPAWN_LATITUDE, SPAWN_LONGITUDE, 0.0);

    // 駐機ブレーキ。無ければ 15° 斜面（tan = 0.27）を転がり落ちるのが正しい挙動で、
    // 姿勢の判定にならない。制動摩擦 0.715 は 25° 斜面まで保持できる。
    let controls = ControlInputs::neutral().with_brakes(1.0);
    let frame_time = Seconds(1.0 / 60.0);

    let mut simulation = Simulation::parked(
        config,
        start,
        Radians::ZERO,
        terrain_on_a_slope(slope_degrees, slope_bearing_degrees),
        sampler,
    );

    let mut maximum_tilt_degrees: f64 = 0.0;
    let mut maximum_bank_degrees: f64 = 0.0;
    let mut maximum_pitch_degrees: f64 = 0.0;
    let mut minimum_wheel_clearance = f64::INFINITY;
    let mut terrain_missing = false;

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "テストの反復回数。60 Hz × 秒数で 1 000 未満"
    )]
    let frames = (seconds * 60.0).round() as u32;
    for frame in 0..frames {
        let report = simulation.advance(frame_time, controls);
        assert!(
            !report.diverged,
            "the simulation diverged at frame {frame} on a {slope_degrees}° slope"
        );
        terrain_missing |= report.terrain_missing;

        let state = simulation.state();
        assert!(state.is_finite(), "non-finite state at frame {frame}");

        let attitude = state.attitude();
        maximum_bank_degrees = maximum_bank_degrees.max(attitude.roll.get().to_degrees().abs());
        maximum_pitch_degrees = maximum_pitch_degrees.max(attitude.pitch.get().to_degrees().abs());

        let body_down = state
            .local_frame()
            .ecef_to_ned_vector(state.orientation * glam::DVec3::Z);
        maximum_tilt_degrees =
            maximum_tilt_degrees.max(body_down.down().clamp(-1.0, 1.0).acos().to_degrees());

        // 重心の対地高度ではなく**車輪の対地高度**で見る。この機体で 1 m ずれる。
        let ground = terrain
            .elevation_at(Geodetic::new(
                state.geodetic().latitude,
                state.geodetic().longitude,
                Meters::ZERO,
            ))
            .expect("the synthetic tile covers the spawn area");
        minimum_wheel_clearance =
            minimum_wheel_clearance.min(state.altitude().get() - gear_height - ground.get());
    }

    Outcome {
        maximum_tilt_degrees,
        maximum_bank_degrees,
        maximum_pitch_degrees,
        minimum_wheel_clearance,
        terrain_missing,
    }
}

fn assert_upright(outcome: &Outcome, what: &str) {
    assert!(
        !outcome.terrain_missing,
        "{what}: the synthetic tile was not found"
    );
    assert!(
        outcome.maximum_bank_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: banked to {:.1}°",
        outcome.maximum_bank_degrees
    );
    assert!(
        outcome.maximum_pitch_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: pitched to {:.1}°",
        outcome.maximum_pitch_degrees
    );
    // オイラー角は裏返ると ±180° へ飛ぶので、局所鉛直との角度でも見る。
    assert!(
        outcome.maximum_tilt_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: tilted {:.1}° from the local vertical",
        outcome.maximum_tilt_degrees
    );
}

#[test]
fn spawning_on_a_fifteen_degree_hillside_does_not_flip_the_aircraft() {
    for bearing in [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
        let outcome = spawn_on_slope(15.0, bearing, 10.0);
        assert_upright(&outcome, &format!("15° hillside rising toward {bearing}°"));
    }
}

#[test]
fn spawning_on_a_twenty_five_degree_hillside_does_not_flip_the_aircraft() {
    // 25° はもう着陸できる面ではない。裏返らないことだけを見る。
    for bearing in [0.0, 90.0, 180.0, 270.0] {
        let outcome = spawn_on_slope(25.0, bearing, 10.0);
        assert_upright(&outcome, &format!("25° hillside rising toward {bearing}°"));
    }
}

#[test]
fn a_hillside_spawn_settles_onto_the_wheels_instead_of_sinking_through() {
    // HUD の AGL が -4 ft（-1.2 m）だった。車輪が地面へ大きくめり込まないこと。
    // 脚の静的沈み込み 0.028 m と斜面上での接地平面の近似ぶんの余裕を見る。
    for bearing in [0.0, 90.0, 180.0, 270.0] {
        let outcome = spawn_on_slope(15.0, bearing, 10.0);
        assert!(
            outcome.minimum_wheel_clearance > -0.20,
            "15° hillside rising toward {bearing}°: the wheels sank to \
             {:.3} m below the terrain",
            outcome.minimum_wheel_clearance
        );
    }
}

#[test]
fn a_hillside_spawn_is_deterministic() {
    let run = || {
        let config = AircraftConfig::light_single();
        let mut simulation = Simulation::parked(
            config,
            Geodetic::from_degrees(SPAWN_LATITUDE, SPAWN_LONGITUDE, 0.0),
            Radians::ZERO,
            terrain_on_a_slope(15.0, 30.0),
            GroundSampler::default(),
        );
        let controls = ControlInputs::neutral().with_brakes(1.0);
        let mut samples = Vec::new();
        for frame in 0..600 {
            simulation.advance(Seconds(1.0 / 60.0), controls);
            if frame % 30 == 0 {
                samples.push(*simulation.state());
            }
        }
        samples
    };

    assert_eq!(run(), run(), "the hillside spawn is not deterministic");
}
