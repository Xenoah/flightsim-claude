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
use bevy::camera::primitives::Aabb;
use bevy::pbr::{Atmosphere, ScatteringMedium};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use flightsim_core::{Degrees, Geodetic, Meters, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_input::{CameraRig, FlightsimInputPlugin, PilotControls, ViewMode};
use flightsim_render::{
    CameraWorldPosition, FlightsimRenderPlugin, ModelAxis, ModelFit, RenderOrigin, RenderSet,
    SunDirection, TerrainRenderConfig, TerrainTiles, WorldOrientation, WorldPosition,
    extents_in_model_space,
    terrain::{TerrainTile, despawn_tile, spawn_tile},
    update_terrain_selection,
};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_ui::{FlightsimUiPlugin, HudState};
use flightsim_world::Runway;
use flightsim_world::{
    DiskTileSource, LodSelector, MemoryTileSource, Terrain, TileCache, TileId, TileSource,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// 実行時に差し替えられる地形供給元。
type BoxedSource = Box<dyn TileSource + Send + Sync>;

/// 同梱している機体モデル。`assets/` からの相対パス。
///
/// 引数を何も付けずに起動したときに使う。**箱のプレースホルダより、
/// 実際の機体が出るほうが「動いている」と分かりやすい。**
const BUNDLED_MODEL: &str = "aircraft/light_single.glb";

/// 同梱モデルの軸（前、上）。
///
/// **glTF の慣習（-Z 前方）とは違う。** これを `ModelFit` の既定にしては
/// いけない。他所から持ってきたモデルまで -X 前方として扱われてしまう。
/// **同梱ぶん専用の実測値**として、ここにだけ置く。
const BUNDLED_MODEL_AXES: (ModelAxis, ModelAxis) = (ModelAxis::NegativeX, ModelAxis::PositiveY);

/// 引数の解釈中に出た指摘。
///
/// # なぜ溜めるのか
///
/// **`parse_arguments` は `LogPlugin` より前に走る。** そこで `warn!` を呼んでも
/// 購読者がまだ居らず、**何も出ない**。実際、`--bogus-flag` を渡しても無言だった。
/// 溜めておいて、ログが立ち上がってから出す。
#[derive(Resource, Debug, Default, Clone)]
struct StartupDiagnostics(Vec<String>);

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
    /// 起動時の視点。実行中は `C` で切り替えられる。
    view: ViewMode,
    /// 機体の 3D モデル。`assets/` からの相対パス。
    ///
    /// `None` は箱のプレースホルダ（`--no-model`）。既定は同梱モデル。
    model: Option<String>,
    /// モデルの座標系を機体軸へ合わせる補正。
    model_fit: ModelFit,
    /// 定常風。`--wind <方位>/<ノット>` で指定する（航空の慣習で from）。
    wind: flightsim_sim::Wind,
    /// 開始時刻（地方平均太陽時）。`None` なら render 側の既定。
    ///
    /// **地方平均太陽時にするのは、経度がどこでも「9 時なら朝」だから。**
    /// UTC で指定させると、飛ぶ場所によって同じ時刻が昼にも夜にもなる。
    start_hour: Option<(u8, u8)>,
    /// 時間加速の倍率。
    time_rate: f64,
    /// 指定すると、地面から この高さ（m）の空中に静止 spawn する。
    ///
    /// **着陸評価の結線を実際に確かめるための開発用。** 落下して接地する
    /// までの数秒で、接地記録 → 評価 → 表示の経路が全部通る。手で
    /// 飛ばさないと着陸できないのでは、この経路を検証できない。
    drop_height: Option<f64>,
    /// 見つかった `assets/` の実体。**Bevy に渡したものと同じ**でなければ、
    /// 「見つかった」と言った直後に `Path not found` が出る。
    assets: Option<PathBuf>,
}

impl Default for Startup {
    fn default() -> Self {
        // 既定は合成飛行場の滑走路上。**中心線に乗った状態で始まる**ので、
        // スロットルを開ければそのまま離陸できる。以前の既定
        // （35.553,139.781）は滑走路から横に 75 m 外れていた。
        let runway = Runway::synthetic();
        Self {
            tiles: None,
            start: runway.takeoff_start(),
            heading: runway.heading,
            min_level: 8,
            max_level: 13,
            screenshot: None,
            screenshot_delay: 5.0,
            view: ViewMode::default(),
            // 既定は同梱モデル。軸も同梱ぶんの実測値に合わせる。
            model: Some(BUNDLED_MODEL.to_owned()),
            model_fit: ModelFit {
                forward: BUNDLED_MODEL_AXES.0,
                up: BUNDLED_MODEL_AXES.1,
                ..ModelFit::default()
            },
            wind: flightsim_sim::Wind::CALM,
            start_hour: None,
            time_rate: 1.0,
            drop_height: None,
            assets: None,
        }
    }
}

/// シミュレーション本体。
#[derive(Resource)]
struct FlightSimulation(Simulation<BoxedSource>);

/// 着陸評価に使う滑走路。
///
/// 現状は合成飛行場ひとつ。空港データベースが入ったら差し替える。
#[derive(Resource, Debug, Clone)]
struct ActiveRunway(Runway);

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

/// 機体の**外形**につける印。プレースホルダでも glTF モデルでも付ける。
///
/// コックピット視点では目線が胴体の内側に入るので、外形をそのまま描くと
/// **視界が自分の機体で塞がる**。内装モデルはまだ無いので、外形を隠す。
#[derive(Component, Debug, Clone, Copy)]
struct ExteriorModel;

/// 読み込み待ちのモデル。寸法が分かった時点で倍率を決める。
///
/// glTF は非同期に読み込まれるので、spawn した瞬間には大きさが分からない。
/// **固定倍率で決め打ちすると、モデルを差し替えるたびに手で直すことになる。**
#[derive(Component, Debug, Clone, Copy)]
struct PendingModelFit(ModelFit);

fn main() {
    let (mut startup, mut diagnostics) = parse_arguments();

    // アセットの置き場所を先に決める。**Bevy の既定はこのリポジトリを指さない。**
    let mut asset_plugin = AssetPlugin::default();
    match assets_directory() {
        Some(directory) => {
            startup.assets = Some(directory.clone());
            asset_plugin.file_path = directory.to_string_lossy().into_owned();
        }
        None => diagnostics
            .0
            .push("could not find an `assets/` directory; models will not load".to_owned()),
    }

    // 時刻は地方平均太陽時で受ける。**経度がどこでも「9 時なら朝」**になる。
    let clock = {
        let mut clock = startup.start_hour.map_or_else(
            flightsim_render::TimeOfDay::default,
            |(hour, minute)| {
                flightsim_render::TimeOfDay::at_local_mean_solar_time(
                    flightsim_render::UtcDateTime::new(2026, 6, 21, hour, minute, 0.0),
                    startup.start.longitude,
                )
            },
        );
        clock.rate = flightsim_render::TimeRate(startup.time_rate);
        clock
    };

    App::new()
        .add_plugins(DefaultPlugins.set(asset_plugin).set(WindowPlugin {
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
        .insert_resource(clock)
        .insert_resource(startup)
        .insert_resource(diagnostics)
        .init_resource::<CameraRig>()
        // 指摘を先に出す。**設定の誤りは、その結果より前に見えるべき。**
        .add_systems(Startup, (report_arguments, setup).chain())
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
        .add_systems(
            Update,
            (
                capture_screenshot,
                report_terrain,
                fit_loaded_model,
                update_model_visibility,
                report_landings.after(advance_simulation),
            ),
        )
        .run();
}

/// `--tiles <DIR>` と `--start <LAT,LON>` を読む。
///
/// clap を入れるほどの規模ではない。増えたら入れる。
///
/// **指摘は `warn!` せず溜めて返す。** この関数は `LogPlugin` より前に走るので、
/// ここで出しても購読者が居らず何も表示されない（[`StartupDiagnostics`]）。
fn parse_arguments() -> (Startup, StartupDiagnostics) {
    let mut startup = Startup::default();
    let mut notes = Vec::new();

    // モデル関連は最後にまとめて決める。**軸の既定が「どのモデルか」で変わる**ため、
    // 引数を読んだ順に確定させられない。
    let mut requested_model = None;
    let mut placeholder = false;
    let mut forward = None;
    let mut up = None;

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
                        notes.push(format!("--start expects `lat,lon`; ignoring `{text}`"));
                    }
                }
            }
            "--heading" => match arguments.next() {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) => startup.heading = Degrees(value).to_radians(),
                    Err(_) => notes.push(format!("--heading expects degrees; ignoring `{text}`")),
                },
                None => notes.push("--heading needs a value".to_owned()),
            },
            "--max-level" => match arguments.next() {
                Some(text) => match text.parse::<u8>() {
                    Ok(value) => startup.max_level = value,
                    Err(_) => {
                        notes.push(format!("--max-level expects a number; ignoring `{text}`"))
                    }
                },
                None => notes.push("--max-level needs a value".to_owned()),
            },
            "--view" => {
                if let Some(name) = arguments.next() {
                    startup.view = match name.to_lowercase().as_str() {
                        "cockpit" => ViewMode::Cockpit,
                        "chase" => ViewMode::Chase,
                        "free" => ViewMode::Free,
                        "tower" => ViewMode::Tower,
                        other => {
                            notes.push(format!("unknown view `{other}`; keeping the default"));
                            startup.view
                        }
                    };
                }
            }
            "--model" => match arguments.next() {
                Some(path) => requested_model = Some(path),
                None => notes.push("--model needs a path".to_owned()),
            },
            // 箱のプレースホルダに戻す。同梱モデルが既定になったので、
            // **戻す手段が無いと寸法の食い違いを比べられない。**
            "--no-model" => placeholder = true,
            "--model-forward" | "--model-up" => {
                let Some(text) = arguments.next() else {
                    notes.push(format!("{flag} needs an axis"));
                    continue;
                };
                match ModelAxis::parse(&text) {
                    Ok(axis) => {
                        if flag == "--model-forward" {
                            forward = Some(axis);
                        } else {
                            up = Some(axis);
                        }
                    }
                    Err(error) => notes.push(error.to_string()),
                }
            }
            // 航空の慣習に合わせて `270/10`（西から 10 kt）。
            "--wind" => match arguments.next() {
                Some(text) => match parse_wind(&text) {
                    Ok(wind) => startup.wind = wind,
                    Err(message) => notes.push(message),
                },
                None => notes.push("--wind needs `<bearing>/<knots>`, e.g. 270/10".to_owned()),
            },
            // 地方平均太陽時。`--time 05:30` で日の出前から始まる。
            "--time" => match arguments.next() {
                Some(text) => match parse_clock(&text) {
                    Ok(clock) => startup.start_hour = Some(clock),
                    Err(message) => notes.push(message),
                },
                None => notes.push("--time needs `HH:MM`".to_owned()),
            },
            "--time-rate" => match arguments.next() {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) if value.is_finite() && value >= 0.0 => startup.time_rate = value,
                    _ => notes.push(format!(
                        "--time-rate expects a non-negative number, got `{text}`"
                    )),
                },
                None => notes.push("--time-rate needs a multiplier".to_owned()),
            },
            "--drop" => match arguments.next() {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) if value > 0.0 => startup.drop_height = Some(value),
                    _ => notes.push(format!(
                        "--drop expects metres above ground; ignoring `{text}`"
                    )),
                },
                None => notes.push("--drop needs a height in metres".to_owned()),
            },
            "--screenshot" => startup.screenshot = arguments.next().map(PathBuf::from),
            "--screenshot-delay" => match arguments.next() {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) => startup.screenshot_delay = value,
                    Err(_) => {
                        notes.push(format!(
                            "--screenshot-delay expects seconds; ignoring `{text}`"
                        ));
                    }
                },
                None => notes.push("--screenshot-delay needs a value".to_owned()),
            },
            other => notes.push(format!("unknown argument `{other}`")),
        }
    }

    let (model, fit) = resolve_model(requested_model, placeholder, forward, up, &mut notes);
    startup.model = model;
    startup.model_fit = fit;

    (startup, StartupDiagnostics(notes))
}

/// `05:30` のような時刻を読む。
fn parse_clock(text: &str) -> Result<(u8, u8), String> {
    let Some((hour, minute)) = text.split_once(':') else {
        return Err(format!("--time expects `HH:MM`, got `{text}`"));
    };
    let hour: u8 = hour
        .trim()
        .parse()
        .map_err(|_| format!("--time hour `{hour}` is not a number"))?;
    let minute: u8 = minute
        .trim()
        .parse()
        .map_err(|_| format!("--time minute `{minute}` is not a number"))?;
    if hour > 23 || minute > 59 {
        return Err(format!("--time `{text}` is not a valid clock time"));
    }
    Ok((hour, minute))
}

/// `270/10` のような風の指定を読む。
///
/// 航空の慣習どおり「どちら**から**吹くか」と**ノット**で受ける。
/// 内部は SI なので、ここで一度だけ変換する（CLAUDE.md の単位規約）。
fn parse_wind(text: &str) -> Result<flightsim_sim::Wind, String> {
    let Some((bearing, knots)) = text.split_once('/') else {
        return Err(format!("--wind expects `<bearing>/<knots>`, got `{text}`"));
    };
    let bearing: f64 = bearing
        .trim()
        .parse()
        .map_err(|_| format!("--wind bearing `{bearing}` is not a number"))?;
    let knots: f64 = knots
        .trim()
        .parse()
        .map_err(|_| format!("--wind speed `{knots}` is not a number"))?;

    if !bearing.is_finite() || !knots.is_finite() || knots < 0.0 {
        return Err(format!("--wind `{text}` is out of range"));
    }
    Ok(flightsim_sim::Wind {
        from: Degrees(bearing).to_radians(),
        speed: flightsim_core::Knots(knots).to_meters_per_second(),
    })
}

/// どのモデルを、どの軸で使うかを決める。
///
/// - `--no-model` → プレースホルダ
/// - `--model <path>` → そのモデル。軸の既定は **glTF の慣習**（-Z 前方）
/// - どちらも無ければ同梱モデル。軸は [`BUNDLED_MODEL_AXES`]
///
/// **同梱モデルの軸を全体の既定にしない。** そうすると他所から持ってきた
/// モデルまで -X 前方として扱われ、横を向いた理由が分からなくなる。
///
/// 前と上が平行になった場合は、その組を捨てて既定へ戻す。**黙って妙な向きに
/// しない**（回転が一意に決まらず、機体が予測できない姿勢になる）。
fn resolve_model(
    requested: Option<String>,
    placeholder: bool,
    forward: Option<ModelAxis>,
    up: Option<ModelAxis>,
    notes: &mut Vec<String>,
) -> (Option<String>, ModelFit) {
    let fallback = ModelFit::default();

    if placeholder {
        if requested.is_some() {
            notes.push("--no-model overrides --model".to_owned());
        }
        return (None, fallback);
    }

    let (path, default_axes) = match requested {
        Some(path) => (path, (fallback.forward, fallback.up)),
        None => (BUNDLED_MODEL.to_owned(), BUNDLED_MODEL_AXES),
    };

    let chosen = (
        forward.unwrap_or(default_axes.0),
        up.unwrap_or(default_axes.1),
    );
    let fit = match ModelFit::new(chosen.0, chosen.1, fallback.target_length) {
        Ok(fit) => fit,
        Err(error) => {
            notes.push(format!("{error}; using the default axes instead"));
            ModelFit::new(default_axes.0, default_axes.1, fallback.target_length)
                .unwrap_or(fallback)
        }
    };
    (Some(path), fit)
}

/// `assets/` の実体を探す。
///
/// # なぜ Bevy に任せられないのか
///
/// `bevy_asset` の起点は `BEVY_ASSET_ROOT` → `CARGO_MANIFEST_DIR` → 実行ファイルの隣、
/// の順で決まる。**どれもこのリポジトリの `assets/` を指さないことがある。**
///
/// - `cargo run -p flightsim-app` では `CARGO_MANIFEST_DIR` が `crates/flightsim-app`
///   になる。そこに `assets/` は無い。**文書に書いてあったこの起動方法は動いていなかった**
/// - 実行ファイルを直接叩くと `target/debug/assets/` を見る
///
/// そこで候補それぞれから上へ辿り、実在する `assets/` を見つけて
/// `AssetPlugin::file_path` に絶対パスで渡す。どこから起動しても同じ物を読む。
fn assets_directory() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") {
        candidates.push(PathBuf::from(root));
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(directory) = exe.parent()
    {
        candidates.push(directory.to_path_buf());
    }
    candidates.push(PathBuf::from("."));

    candidates.iter().find_map(|start| assets_above(start))
}

/// `start` から上へ辿って `assets/` を探す。
///
/// 探索を分けてあるのは、**ディレクトリを作れば検査できる**ようにするため。
fn assets_above(start: &std::path::Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(directory) = current {
        let candidate = directory.join("assets");
        if candidate.is_dir() {
            return Some(candidate);
        }
        current = directory.parent();
    }
    None
}

/// その視点で機体の外形を描くか。
///
/// コックピットからは自分の機体は見えない。**外形を描くと視界が塞がる。**
const fn shows_exterior(mode: ViewMode) -> bool {
    !matches!(mode, ViewMode::Cockpit)
}

/// 視点に応じて機体の外形を出し入れする。
fn update_model_visibility(
    mode: Res<ViewMode>,
    mut models: Query<&mut Visibility, With<ExteriorModel>>,
) {
    let wanted = if shows_exterior(*mode) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut models {
        // 毎フレーム書き込むと変更検知が無駄に走る。
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// 新しい接地を着陸評価へ流す。
///
/// `touchdown_count` の増分で「新しい接地」を検出する。bool の
/// 「今のフレームで接地したか」だと、読み損ねたフレームで取りこぼす。
fn report_landings(
    mut commands: Commands,
    simulation: Res<FlightSimulation>,
    runway: Res<ActiveRunway>,
    mut seen: Local<u32>,
) {
    let count = simulation.0.touchdown_count();
    if count == *seen {
        return;
    }
    *seen = count;

    let Some(touchdown) = simulation.0.last_touchdown() else {
        return;
    };

    // 滑走路方位との差。反対向きの着陸（逆進入）も正しい着陸なので、
    // 正方位と逆方位の近いほうを取る。
    let error_to = |target: flightsim_core::Radians| {
        let mut difference =
            (touchdown.heading.get() - target.get()) % (2.0 * std::f64::consts::PI);
        if difference > std::f64::consts::PI {
            difference -= 2.0 * std::f64::consts::PI;
        }
        if difference < -std::f64::consts::PI {
            difference += 2.0 * std::f64::consts::PI;
        }
        difference
    };
    let forward = error_to(runway.0.heading);
    let reverse = error_to(runway.0.reciprocal_heading());
    let heading_error = if forward.abs() <= reverse.abs() {
        forward
    } else {
        reverse
    };

    commands.insert_resource(flightsim_ui::LandingReport {
        sink_rate: touchdown.sink_rate,
        ground_speed: touchdown.ground_speed,
        bank: touchdown.bank,
        on_runway: Some(runway.0.contains(touchdown.position)),
        heading_error: Some(flightsim_core::Radians(heading_error)),
    });
}

/// 引数の指摘を出す。
///
/// [`parse_arguments`] が溜めたものを、ログが立ち上がってから出す。
fn report_arguments(diagnostics: Res<StartupDiagnostics>) {
    for note in &diagnostics.0 {
        warn!("{note}");
    }
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

#[allow(
    clippy::too_many_arguments,
    reason = "Bevy の Startup system は必要なリソースを引数で受け取るしかない。分割すると初期化の順序を自分で管理することになり、かえって壊れやすい"
)]
fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    startup: Res<Startup>,
    config: Res<TerrainRenderConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    lighting: Res<flightsim_render::SunLighting>,
    sun: Res<SunDirection>,
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
    let simulation = match startup.drop_height {
        // 開発用: 空中に静止 spawn して落とす。接地記録 → 評価 → 表示の
        // 経路を、手で飛ばさずに通すため。
        Some(height) => {
            let sampler = GroundSampler::default();
            let mut probe = Terrain::new(
                make_source(&startup),
                8 * 1024 * 1024,
                startup.min_level..=startup.max_level,
            );
            let ground = sampler.sample(&mut probe, startup.start);
            let state = flightsim_fdm::RigidBodyState::from_geodetic(
                Geodetic::new(
                    startup.start.latitude,
                    startup.start.longitude,
                    Meters(ground.elevation.get() + height),
                ),
                flightsim_core::Attitude::new(
                    flightsim_core::Radians::ZERO,
                    flightsim_core::Radians::ZERO,
                    startup.heading,
                ),
                flightsim_core::Ned::new(0.0, 0.0, 0.0),
            );
            Simulation::from_state(
                AircraftConfig::light_single(),
                state,
                terrain,
                GroundSampler::default(),
            )
        }
        None => Simulation::parked(
            AircraftConfig::light_single(),
            startup.start,
            startup.heading,
            terrain,
            GroundSampler::default(),
        ),
    };

    let mut simulation = simulation;
    simulation.set_wind(startup.wind);

    commands.insert_resource(startup.view);

    // --- 滑走路 ---

    // 見た目は**実際の地面の高さ**に置く。地形タイルがあれば彫られた 8 m、
    // 無ければ海面 0 m。着陸評価（contains / offsets）は高度を見ないので、
    // 見た目の高さを地面へ合わせても評価はずれない。
    let runway = Runway::synthetic();
    let ground_elevation = simulation.ground().elevation;
    let visual_threshold = Geodetic::new(
        runway.threshold.latitude,
        runway.threshold.longitude,
        ground_elevation,
    );
    let (runway_mesh, runway_origin) = flightsim_render::runway::runway_mesh(
        visual_threshold,
        runway.heading,
        runway.length,
        runway.width,
    );
    commands.spawn((
        flightsim_render::terrain_mesh_bundle(
            meshes.add(runway_mesh),
            materials.add(flightsim_render::default_terrain_material()),
            runway_origin,
        ),
        Name::new("runway"),
    ));
    commands.insert_resource(ActiveRunway(runway));

    let camera_position = simulation.state().geodetic();
    commands.insert_resource(RenderOrigin::new(camera_position));
    commands.insert_resource(CameraWorldPosition(camera_position));

    // --- 機体 ---

    // モデルがあれば glTF、無ければ箱のプレースホルダ。
    // **どちらの場合も子は機体軸で置く。** 親の Transform が
    // 機体軸 → 描画座標を担うので、子はそのままでよい。
    // 指定されたモデルが実在するか先に確かめる。**無いまま起動すると機体が
    // 消え、「動いてはいるが絵が出ていない」状態になる。**
    let model = startup.model.as_ref().and_then(|path| {
        // **Bevy へ渡したのと同じ場所で確かめる。** 別の探索を書くと、
        // 「見つかった」と言った直後に Bevy が Path not found を出す。
        match startup
            .assets
            .as_ref()
            .map(|directory| directory.join(path))
        {
            Some(file) if file.is_file() => Some((path.clone(), file)),
            Some(file) => {
                warn!("aircraft model was not found; using the placeholder");
                warn!("  looked for: {}", file.display());
                None
            }
            None => {
                warn!("no `assets/` directory was found; using the placeholder");
                None
            }
        }
    });

    let parts: Vec<Entity> = match &model {
        Some((path, found)) => {
            info!("aircraft model: {}", found.display());
            vec![
                commands
                    .spawn((
                        SceneRoot(
                            asset_server.load(GltfAssetLabel::Scene(0).from_asset(path.clone())),
                        ),
                        // 回転は今すぐ決まるが、倍率はモデルの寸法が要る。
                        // 読み込みが終わるまで待つ（`fit_loaded_model`）。
                        Transform::from_rotation(startup.model_fit.rotation()),
                        PendingModelFit(startup.model_fit),
                        ExteriorModel,
                        Visibility::default(),
                        Name::new("aircraft model"),
                    ))
                    .id(),
            ]
        }
        None => flightsim_render::placeholder_parts(simulation.config())
            .into_iter()
            .map(|part| {
                commands
                    .spawn((
                        Mesh3d(meshes.add(part.mesh)),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: part.color,
                            perceptual_roughness: 0.5,
                            ..default()
                        })),
                        ExteriorModel,
                        part.transform,
                        Name::new(part.name),
                    ))
                    .id()
            })
            .collect(),
    };

    commands
        .spawn((
            Aircraft,
            WorldPosition(simulation.state().position),
            WorldOrientation(simulation.state().orientation),
            Transform::default(),
            Visibility::default(),
            Name::new("aircraft"),
        ))
        .add_children(&parts);

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

    // 太陽。**`SunLight` の印が要る。** これが無いと向きと照度が時刻に
    // 追随しない（光源だけ固定のまま空だけ動く、という絵になる）。
    commands.spawn(flightsim_render::sun_light_bundle(&lighting, *sun));
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
/// 読み込みが終わったモデルの倍率を決める。
///
/// glTF の読み込みは非同期なので、子のメッシュが揃うまで寸法が分からない。
/// 揃った時点で全体の AABB を測り、目標全長に合わせる。
fn fit_loaded_model(
    mut commands: Commands,
    pending: Query<(Entity, &PendingModelFit, &GlobalTransform)>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut transforms: Query<&mut Transform>,
) {
    for (entity, fit, model_global) in &pending {
        // **モデル自身の座標系で測る。** 描画フレームの軸のまま測ると、
        // 得られるのは回転後の箱を包む箱で、方位によって倍率が変わる。
        let into_model = model_global.affine().inverse();
        let parts = children
            .iter_descendants(entity)
            .filter_map(|descendant| bounds.get(descendant).ok())
            .map(|(aabb, global)| (*aabb, global.affine()));

        let Some(extents) = extents_in_model_space(into_model, parts) else {
            // まだ読み込まれていない。次のフレームで再挑戦する。
            continue;
        };

        let scale = fit.0.scale_for(extents);
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.scale = Vec3::splat(scale);
        }
        info!(
            "aircraft model fitted: {:.2} m along its length → scale {scale:.4}",
            (extents * fit.0.forward.to_vec3()).length()
        );
        commands.entity(entity).remove::<PendingModelFit>();
    }
}

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
        // **対地速度ではなく対気速度。** 風が入ると両者は一致せず、
        // 失速も揚力も対気速度で決まる。
        airspeed: simulation.0.airspeed(),
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
        wind_from: simulation.0.wind().from,
        wind_speed: simulation.0.wind().speed,
        log: {
            // sim の記録を ui の型へ詰め替える。**ui は sim に依存しない**
            // ので（依存は一方向）、変換はここが引き受ける。
            let log = simulation.0.log();
            flightsim_ui::FlightSummary {
                airborne_time: log.airborne_time,
                distance: log.distance,
                peak_agl: log.peak_agl,
                landings: log.landings,
            }
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- モデルと軸の決め方 ---

    fn resolve(
        requested: Option<&str>,
        placeholder: bool,
        forward: Option<ModelAxis>,
        up: Option<ModelAxis>,
    ) -> (Option<String>, ModelFit, Vec<String>) {
        let mut notes = Vec::new();
        let (model, fit) = resolve_model(
            requested.map(ToOwned::to_owned),
            placeholder,
            forward,
            up,
            &mut notes,
        );
        (model, fit, notes)
    }

    #[test]
    fn with_no_arguments_the_bundled_model_is_used_with_its_own_axes() {
        // 同梱モデルは glTF の慣習と違う軸を持つ。既定で横を向いては困る。
        let (model, fit, notes) = resolve(None, false, None, None);
        assert_eq!(model.as_deref(), Some(BUNDLED_MODEL));
        assert_eq!(fit.forward, BUNDLED_MODEL_AXES.0);
        assert_eq!(fit.up, BUNDLED_MODEL_AXES.1);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn another_model_falls_back_to_the_gltf_convention_not_the_bundled_axes() {
        // **ここが肝。** 同梱モデルの軸を全体の既定にしてしまうと、他所から
        // 持ってきたモデルまで -X 前方として扱われ、横を向いた理由が分からなくなる。
        let (model, fit, _) = resolve(Some("other/plane.glb"), false, None, None);
        assert_eq!(model.as_deref(), Some("other/plane.glb"));
        assert_eq!(fit.forward, ModelFit::default().forward);
        assert_eq!(fit.up, ModelFit::default().up);
        assert_ne!(
            fit.forward, BUNDLED_MODEL_AXES.0,
            "the bundled model's axes leaked into an unrelated model"
        );
    }

    #[test]
    fn explicit_axes_win_over_both_defaults() {
        let (_, fit, _) = resolve(
            Some("other/plane.glb"),
            false,
            Some(ModelAxis::PositiveZ),
            Some(ModelAxis::PositiveY),
        );
        assert_eq!(fit.forward, ModelAxis::PositiveZ);
        assert_eq!(fit.up, ModelAxis::PositiveY);
    }

    #[test]
    fn parallel_axes_are_rejected_rather_than_producing_an_arbitrary_rotation() {
        // 前と上が平行だと回転が一意に決まらない。黙って妙な姿勢にしない。
        let (_, fit, notes) = resolve(
            None,
            false,
            Some(ModelAxis::PositiveX),
            Some(ModelAxis::NegativeX),
        );
        assert_eq!(fit.forward, BUNDLED_MODEL_AXES.0, "should fall back");
        assert!(
            notes.iter().any(|note| note.contains("perpendicular")),
            "the reason should be stated: {notes:?}"
        );
    }

    #[test]
    fn asking_for_the_placeholder_gives_no_model() {
        let (model, _, notes) = resolve(None, true, None, None);
        assert_eq!(model, None);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn combining_no_model_with_a_model_says_which_one_won() {
        // 黙って片方を捨てると、指定したモデルが出ない理由が分からない。
        let (model, _, notes) = resolve(Some("other/plane.glb"), true, None, None);
        assert_eq!(model, None);
        assert!(!notes.is_empty(), "the conflict should be reported");
    }

    // --- 視点と外形 ---

    #[test]
    fn the_aircraft_is_hidden_from_the_cockpit() {
        // 目線が胴体の内側に入るので、外形を描くと視界が自分の機体で塞がる。
        assert!(!shows_exterior(ViewMode::Cockpit));
    }

    #[test]
    fn every_outside_view_shows_the_aircraft() {
        // 外から見ているのに機体が消えたら、飛んでいるのか分からない。
        for mode in [ViewMode::Chase, ViewMode::Free, ViewMode::Tower] {
            assert!(shows_exterior(mode), "{} hides the aircraft", mode.name());
        }
    }

    // --- assets/ の探索 ---

    #[test]
    fn the_assets_directory_is_found_from_a_subdirectory() {
        // cargo run では CARGO_MANIFEST_DIR が crates/flightsim-app になる。
        // **そこに assets/ は無い。** 上へ辿って見つけられること。
        let root = std::env::temp_dir().join(format!(
            "flightsim-assets-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let nested = root.join("crates/flightsim-app");
        std::fs::create_dir_all(&nested).expect("temp dirs");
        std::fs::create_dir_all(root.join("assets")).expect("assets dir");

        let found = assets_above(&nested).expect("the assets/ above should be found");
        assert_eq!(found, root.join("assets"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tree_without_assets_reports_nothing_rather_than_guessing() {
        // 無いのに在ると答えると、直後に Bevy が Path not found を出す。
        let empty = std::env::temp_dir().join(format!(
            "flightsim-empty-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(empty.join("a/b")).expect("temp dirs");
        // 一時ディレクトリの上に assets/ が無いことが前提。あれば検査を飛ばす。
        if assets_above(&empty).is_none() {
            assert_eq!(assets_above(&empty.join("a/b")), None);
        }
        std::fs::remove_dir_all(&empty).ok();
    }
}
