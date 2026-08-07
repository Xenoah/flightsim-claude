//! # flightsim-app
//!
//! 統合バイナリ。プラグインを組み立てて実行するだけで、**ロジックは持たない**。
//!
//! ## 結線を再実装しない
//!
//! 地形 → 接地平面 → FDM は `flightsim_sim::Simulation` が持つ（ADR-0006）。
//! ここはそれを 1 フレームぶん進めて、結果を描画と HUD へ配るだけ。
//! 同じ結線が 2 箇所にあると、片方だけ直されて挙動が食い違う。
//!
//! ## 起動
//!
//! ```bash
//! # 地形タイルの上を飛ぶ
//! cargo run -p flightsim-app --release -- --tiles data/tiles --start 35.553,139.781
//!
//! # タイルが無ければ海面 0 m の上を飛ぶ
//! cargo run -p flightsim-app --release
//! ```

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::camera::Exposure;
use bevy::pbr::{Atmosphere, ScatteringMedium};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use flightsim_core::{Degrees, Geodetic, Meters, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_input::{CameraRig, FlightsimInputPlugin, PilotControls, ViewMode};
use flightsim_render::{
    CameraWorldPosition, FlightsimRenderPlugin, RenderOrigin, RenderSet, SunDirection,
    TerrainRenderConfig, TerrainTiles, WorldOrientation, WorldPosition,
    terrain::{TerrainTile, despawn_tile, spawn_tile},
    update_terrain_selection,
};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_ui::{FlightsimUiPlugin, HudState};
use flightsim_world::{
    DiskTileSource, LodSelector, MemoryTileSource, Terrain, TileCache, TileId, TileSource,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// 実行時に差し替えられる地形供給元。
type BoxedSource = Box<dyn TileSource + Send + Sync>;

/// 起動時の設定。環境変数と引数から作る。
#[derive(Resource, Debug, Clone)]
struct Startup {
    tiles: Option<PathBuf>,
    start: Geodetic,
    heading: Radians,
    min_level: u8,
    max_level: u8,
    /// 指定すると、起動から一定時間後に 1 枚だけ撮って保存する。
    ///
    /// 描画は自動テストが極めて難しい領域なので、**目視で確かめる手段**を
    /// 用意しておく。無いと「動いてはいるが絵が出ていない」に気付けない。
    screenshot: Option<PathBuf>,
    /// 撮るまでの待ち時間。地形の読み込みが進むのを待つ。
    screenshot_delay: f64,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            tiles: None,
            // 羽田空港のあたり。地形が無ければ海面 0 m。
            start: Geodetic::from_degrees(35.553, 139.781, 0.0),
            heading: Degrees(50.0).to_radians(),
            min_level: 8,
            max_level: 13,
            screenshot: None,
            screenshot_delay: 5.0,
        }
    }
}

/// シミュレーション本体。
#[derive(Resource)]
struct FlightSimulation(Simulation<BoxedSource>);

/// 地形描画の作業用状態。
#[derive(Resource)]
struct TerrainStreaming {
    selector: LodSelector,
    source: BoxedSource,
    cache: TileCache,
    live: HashMap<TileId, ()>,
    material: Handle<StandardMaterial>,
}

/// 機体の実体につける印。
#[derive(Component, Debug, Clone, Copy)]
struct Aircraft;

fn main() {
    let startup = parse_arguments();

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "flightsim-claude".to_owned(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            FlightsimRenderPlugin,
            FlightsimInputPlugin,
            FlightsimUiPlugin,
        ))
        .insert_resource(startup)
        .init_resource::<CameraRig>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                advance_simulation,
                update_camera.after(advance_simulation),
                publish_hud.after(advance_simulation),
            )
                .before(RenderSet::Rebase),
        )
        .add_systems(Update, stream_terrain.in_set(RenderSet::Terrain))
        .add_systems(Update, (capture_screenshot, report_terrain))
        .run();
}

/// `--tiles <DIR>` と `--start <LAT,LON>` を読む。
///
/// clap を入れるほどの規模ではない。増えたら入れる。
fn parse_arguments() -> Startup {
    let mut startup = Startup::default();
    let mut arguments = std::env::args().skip(1);

    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--tiles" => startup.tiles = arguments.next().map(PathBuf::from),
            "--start" => {
                if let Some(text) = arguments.next() {
                    let parts: Vec<f64> = text
                        .split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    if let [latitude, longitude] = parts.as_slice() {
                        startup.start = Geodetic::from_degrees(*latitude, *longitude, 0.0);
                    } else {
                        warn!("--start expects `lat,lon`; ignoring `{text}`");
                    }
                }
            }
            "--heading" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse::<f64>().ok()) {
                    startup.heading = Degrees(value).to_radians();
                }
            }
            "--max-level" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse::<u8>().ok()) {
                    startup.max_level = value;
                }
            }
            "--screenshot" => startup.screenshot = arguments.next().map(PathBuf::from),
            "--screenshot-delay" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse::<f64>().ok()) {
                    startup.screenshot_delay = value;
                }
            }
            other => warn!("unknown argument `{other}`"),
        }
    }
    startup
}

/// 供給元を 2 つ作る。シミュレーション用と描画用で別のキャッシュを持たせる。
///
/// 同じ `Terrain` を共有すると、描画のタイル読み込みが物理のキャッシュを
/// 押し出して、接地判定のたびにディスクへ行くことになる。
fn make_source(startup: &Startup) -> BoxedSource {
    startup.tiles.as_ref().map_or_else(
        || Box::new(MemoryTileSource::new()) as BoxedSource,
        |path| Box::new(DiskTileSource::new(path)) as BoxedSource,
    )
}

/// 描画に使う投影。
///
/// 遠クリップ面は 400 km。巡航高度 3 km で地平線は約 195 km なので、
/// 地平線の先まで描ける余裕を持たせてある。
#[allow(
    clippy::cast_possible_truncation,
    reason = "画角はラジアン、遠クリップ面は 400 km。どちらも f32 で表現できる"
)]
fn perspective() -> PerspectiveProjection {
    PerspectiveProjection {
        fov: Degrees(60.0).to_radians().get() as f32,
        near: 0.5,
        far: flightsim_render::default_far_plane().get() as f32,
        ..default()
    }
}

fn setup(
    mut commands: Commands,
    startup: Res<Startup>,
    config: Res<TerrainRenderConfig>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    match &startup.tiles {
        Some(path) => info!("terrain: {}", path.display()),
        None => info!("terrain: none — the whole world is at sea level"),
    }
    info!(
        "start: {:.5}, {:.5}",
        startup.start.latitude_degrees(),
        startup.start.longitude_degrees()
    );

    // --- シミュレーション ---

    let terrain = Terrain::new(
        make_source(&startup),
        64 * 1024 * 1024,
        startup.min_level..=startup.max_level,
    );
    let simulation = Simulation::parked(
        AircraftConfig::light_single(),
        startup.start,
        startup.heading,
        terrain,
        GroundSampler::default(),
    );

    let camera_position = simulation.state().geodetic();
    commands.insert_resource(RenderOrigin::new(camera_position));
    commands.insert_resource(CameraWorldPosition(camera_position));

    // --- 機体 ---

    commands.spawn((
        Aircraft,
        WorldPosition(simulation.state().position),
        WorldOrientation(simulation.state().orientation),
        Transform::default(),
        Name::new("aircraft"),
    ));
    commands.insert_resource(FlightSimulation(simulation));

    // --- 地形 ---

    commands.insert_resource(TerrainStreaming {
        selector: LodSelector::new(
            config.screen_space_error,
            1_080.0,
            Degrees(60.0).to_radians(),
            config.max_level,
            config.root_geometric_error,
        ),
        source: make_source(&startup),
        cache: TileCache::new(config.cache_bytes),
        live: HashMap::new(),
        material: materials.add(flightsim_render::default_terrain_material()),
    });

    // --- カメラと空 ---

    let sun = SunDirection::default();
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(perspective()),
        // 大気散乱は HDR カメラを要求する（ADR-0007）。
        // 引数は散乱項の LUT 解像度（falloff, phase）。
        // 64 段あれば地平線付近の色の変化が段付きに見えない。
        Atmosphere::earthlike(media.add(ScatteringMedium::earthlike(64, 64))),
        // 屋外の明るさ。既定のままだと地表が白飛びする。
        Exposure::SUNLIGHT,
        Transform::default(),
        Name::new("camera"),
    ));

    commands.spawn((
        DirectionalLight {
            // `Exposure::SUNLIGHT` は直射日光（10 万 lux 級）に合わせた露出。
            // ここを `FULL_DAYLIGHT`（2 万 lux）にすると 5 倍暗く、
            // **空だけ明るくて地面が真っ黒**という絵になる。実際にそうなった。
            illuminance: bevy::light::light_consts::lux::RAW_SUNLIGHT,
            shadows_enabled: true,
            ..default()
        },
        Transform::default().looking_to(flightsim_render::sun_light_direction(sun), Vec3::Y),
        Name::new("sun"),
    ));
}

/// 1 描画フレームぶんシミュレーションを進める。
fn advance_simulation(
    time: Res<Time>,
    controls: Res<PilotControls>,
    mut simulation: ResMut<FlightSimulation>,
    mut camera_position: ResMut<CameraWorldPosition>,
    mut aircraft: Query<(&mut WorldPosition, &mut WorldOrientation), With<Aircraft>>,
) {
    let report = simulation.0.advance(
        Seconds(f64::from(time.delta_secs())),
        controls.to_control_inputs(),
    );
    if report.diverged {
        // 発散した状態で飛び続けても意味が無い。原因が分かるよう一度だけ出す。
        error!("the simulation diverged; the aircraft state is no longer trustworthy");
        return;
    }

    // 描画には補間した状態を使う。**物理へは書き戻さない**（ADR-0004）。
    let interpolated = simulation.0.interpolated();
    for (mut position, mut orientation) in &mut aircraft {
        position.0 = interpolated.position;
        orientation.0 = interpolated.orientation;
    }
    camera_position.0 = interpolated.geodetic;
}

/// 視点モードに応じてカメラを置く。
fn update_camera(
    time: Res<Time>,
    mode: Res<ViewMode>,
    origin: Res<RenderOrigin>,
    simulation: Res<FlightSimulation>,
    mut rig: ResMut<CameraRig>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    aircraft: Query<&Transform, (With<Aircraft>, Without<Camera3d>)>,
) {
    if mode.is_changed() {
        // 切り替えた瞬間にカメラが数キロ飛んでいくのを防ぐ。
        rig.reset();
    }

    let Ok(aircraft) = aircraft.single() else {
        return;
    };
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };

    let interpolated = simulation.0.interpolated();
    let dt = Seconds(f64::from(time.delta_secs()));

    match *mode {
        ViewMode::Cockpit => {
            // 機体に固定。平滑化しない（頭が揺れると酔う）。
            #[allow(
                clippy::cast_possible_truncation,
                reason = "目線オフセットは数メートル。f32 で十分"
            )]
            let offset = Vec3::new(
                rig.eye_offset[0].get() as f32,
                -rig.eye_offset[2].get() as f32,
                -rig.eye_offset[1].get() as f32,
            );
            camera.translation = aircraft.translation + aircraft.rotation * offset;
            camera.rotation = aircraft.rotation * flightsim_render::body_to_camera_rotation();
        }
        ViewMode::Chase => {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "追従距離は数十メートル。f32 で十分"
            )]
            let behind =
                aircraft.rotation * Vec3::new(-(rig.chase_distance.get() as f32), 0.0, 0.0);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "追従高さは数十メートル。f32 で十分"
            )]
            let target = aircraft.translation + behind + Vec3::Y * (rig.chase_height.get() as f32);
            camera.translation = rig.follow(target, dt);
            camera.look_at(aircraft.translation, Vec3::Y);
        }
        ViewMode::Free => {
            // 機体の斜め上に固定。切り離した観測点。
            let target = aircraft.translation + Vec3::new(120.0, 60.0, 120.0);
            camera.translation = rig.follow(target, dt);
            camera.look_at(aircraft.translation, Vec3::Y);
        }
        ViewMode::Tower => {
            // 出発地点の地上から見る。打ち直しがあるので毎フレーム作り直す。
            let tower = origin.0.to_render(
                Geodetic::new(
                    interpolated.geodetic.latitude,
                    interpolated.geodetic.longitude,
                    Meters::ZERO,
                )
                .to_ecef(),
            );
            camera.translation = Vec3::new(tower.x, tower.y + 25.0, tower.z + 200.0);
            camera.look_at(aircraft.translation, Vec3::Y);
        }
    }
}

/// LOD 選択とタイルのストリーミング。
fn stream_terrain(
    mut commands: Commands,
    config: Res<TerrainRenderConfig>,
    camera_position: Res<CameraWorldPosition>,
    mut streaming: ResMut<TerrainStreaming>,
    mut tiles: ResMut<TerrainTiles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mesh_query: Query<&Mesh3d, With<TerrainTile>>,
) {
    let camera = camera_position.0.to_ecef();
    let streaming = &mut *streaming;

    let mut spawned: Vec<(TileId, Entity)> = Vec::new();
    let update = update_terrain_selection(
        &streaming.selector,
        &streaming.source,
        &mut streaming.cache,
        &mut streaming.live,
        camera,
        config.load_budget_per_frame,
        &mut |id, dem| {
            let entity = spawn_tile(
                &mut commands,
                &mut meshes,
                streaming.material.clone(),
                id,
                dem,
            );
            spawned.push((id, entity));
        },
    );

    for (id, entity) in spawned {
        tiles.insert(id, entity);
    }
    for id in update.despawned {
        despawn_tile(&mut commands, &mut meshes, &mesh_query, &mut tiles, id);
    }
}

/// 起動から一定時間後に 1 枚だけ撮る。
///
/// 撮ったあとは自動終了しない。呼び出し側が止めること
/// （終了処理まで自前で持つと、撮れなかったのか落ちたのかが分からなくなる）。
/// 地形の状況を定期的に報告する。
///
/// **タイル数が 0 のままでも必ず出す。** 絵が出ないときに
/// 「1 枚も spawn していないのか、出ているが見えていないのか」を切り分けられないと、
/// 原因の当たりが付けられない。変化時だけ出す作りにして実際に詰まった。
fn report_terrain(
    time: Res<Time>,
    tiles: Res<TerrainTiles>,
    camera_position: Res<CameraWorldPosition>,
    mut elapsed: Local<f64>,
) {
    *elapsed += f64::from(time.delta_secs());
    if *elapsed < 2.0 {
        return;
    }
    *elapsed = 0.0;

    info!(
        "terrain: {} tile(s) live, camera {:.4}, {:.4} at {:.0} m",
        tiles.len(),
        camera_position.0.latitude_degrees(),
        camera_position.0.longitude_degrees(),
        camera_position.0.altitude.get(),
    );
}

fn capture_screenshot(
    time: Res<Time>,
    startup: Res<Startup>,
    mut commands: Commands,
    mut elapsed: Local<f64>,
    mut done: Local<bool>,
) {
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    if *done {
        return;
    }

    *elapsed += f64::from(time.delta_secs());
    if *elapsed < startup.screenshot_delay {
        return;
    }

    *done = true;
    info!("capturing a screenshot to {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
}

/// HUD に値を配る。
fn publish_hud(
    simulation: Res<FlightSimulation>,
    controls: Res<PilotControls>,
    mode: Res<ViewMode>,
    mut hud: ResMut<HudState>,
) {
    let state = simulation.0.state();
    let interpolated = simulation.0.interpolated();
    let ground = simulation.0.ground();
    let agl = simulation.0.agl();

    *hud = HudState {
        airspeed: flightsim_core::MetersPerSecond(state.body_velocity().length()),
        altitude: state.altitude(),
        agl,
        vertical_speed: state.vertical_speed(),
        heading: interpolated.attitude.yaw,
        pitch: interpolated.attitude.pitch,
        roll: interpolated.attitude.roll,
        throttle: controls.throttle.value(),
        flaps: controls.flaps.value(),
        // 脚の長さぶん余裕を見る。重心の対地高度なので接地時でも 1 m 前後ある。
        on_ground: agl.get() < flightsim_sim::gear_height(simulation.0.config()).get() + 0.3,
        terrain_available: ground.from_terrain,
        view_mode: mode.name(),
    };
}
