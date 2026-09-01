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
//!
//! # 雲量 60%、雲底 900 m、雲頂 1700 m（雲中視程 250 m）
//! cargo run -p flightsim-app --release -- \
//!     --cloud-cover 0.6 --cloud-base 900 --cloud-top 1700 --cloud-visibility 250
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
use flightsim_core::{Attitude, Degrees, Geodetic, LocalFrame, Meters, Ned, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_input::{CameraRig, FlightsimInputPlugin, PilotControls, ViewMode};
use flightsim_render::{
    CameraWorldPosition, CloudLayer, FlightsimRenderPlugin, ModelAxis, ModelFit, RenderOrigin,
    RenderSet, SunDirection, TerrainRenderConfig, TerrainTiles, WorldOrientation, WorldPosition,
    extents_in_model_space,
    terrain::{TerrainTile, despawn_tile, spawn_tile},
    update_terrain_selection,
};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_ui::{DataAttribution, FlightsimUiPlugin, HudState};
use flightsim_world::{
    AirportApron, AirportDatabase, AirportGroundLight, AirportHoldingPosition, AirportTaxiway,
    DiskTileSource, GroundLightKind, LodSelector, MemoryTileSource, Runway, RunwaySide, Terrain,
    TileCache, TileId, TileSource,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// 進入練習を始めるときのスロットル。
///
/// 進入速度を保つのに要る出力。**0 で始めると突っ込む**ので、
/// 練習モードでは操縦入力も進入の形に揃える。
const APPROACH_THROTTLE: f64 = 0.45;

/// 進入練習を始めるときのフラップ。実機の進入形態に倣って全開。
const APPROACH_FLAPS: f64 = 1.0;

/// Active runway と同じ空港とみなして描画する誘導路の探索半径。
///
/// FSAP は空港 relation へ依存せず中心線だけを保持する。地域抽出全域を描くと、遠方の
/// 空港まで一度に GPU へ載るため、滑走路中心から 15 km の実用的な境界で一度だけ絞る。
const ACTIVE_AIRPORT_RADIUS: Meters = Meters(15_000.0);

/// 1 本の誘導路から描画用に展開する中心線点数の上限。
///
/// 舗装 mesh と灯火配置はいずれも 4,096 点を境界にしている。DEM を引いた後で弾くと、
/// 不正に巨大な way のために先にメモリと地形探索を消費するため、app の入口でも揃えて守る。
const MAX_TAXIWAY_SURFACE_POINTS: usize =
    flightsim_render::taxiway_lights::MAX_TAXIWAY_LIGHT_PATH_POINTS;

/// OSM 明示灯と中心線から作った灯火を同一点とみなす ECEF 距離。
const GROUND_LIGHT_DEDUP_DISTANCE: f64 = 0.25;

/// 難易度。
///
/// # 何を変えて、何を変えないか
///
/// 変えるのは**環境の厳しさ**（風・乱流）と**案内の量**（チュートリアル）だけ。
///
/// **着陸の採点は変えない。** 難易度で甘くすると、同じ操縦に違う点が付く。
/// 上達したのか設定を下げただけなのかが分からなくなり、点が意味を失う。
/// 沈下率 1 m/s の接地は、初心者が出しても熟練者が出しても同じ 1 m/s。
///
/// 風と乱流は `--wind` / `--turbulence` で個別に上書きできる。
/// **難易度は既定値を決めるだけ**で、明示指定を打ち消さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Difficulty {
    /// 無風・無乱流。案内を出す。**初めて飛ぶ人向け。**
    Beginner,
    /// 弱い乱流。案内を出す。
    #[default]
    Normal,
    /// 横風と中程度の乱流。案内を出さない。
    Realistic,
}

impl Difficulty {
    /// 名前から読む。
    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "beginner" | "easy" => Some(Self::Beginner),
            "normal" => Some(Self::Normal),
            "realistic" | "hard" => Some(Self::Realistic),
            _ => None,
        }
    }

    /// この難易度が既定にする風。
    ///
    /// `Realistic` は滑走路に対して斜め 40 度の風にする。**真横だと
    /// 着陸できず、真正面だと横風の練習にならない。**
    fn default_wind(self, runway_heading: Radians) -> flightsim_sim::Wind {
        match self {
            Self::Beginner | Self::Normal => flightsim_sim::Wind::CALM,
            Self::Realistic => flightsim_sim::Wind {
                from: Radians(runway_heading.get() + Degrees(40.0).to_radians().get()),
                speed: flightsim_core::Knots(12.0).to_meters_per_second(),
            },
        }
    }

    /// この難易度が既定にする乱流。
    const fn default_turbulence(self) -> flightsim_fdm::Turbulence {
        match self {
            Self::Beginner => flightsim_fdm::Turbulence::CALM,
            Self::Normal => flightsim_fdm::Turbulence::light(1),
            Self::Realistic => flightsim_fdm::Turbulence::moderate(1),
        }
    }

    /// チュートリアルの案内を出すか。
    const fn shows_tutorial(self) -> bool {
        !matches!(self, Self::Realistic)
    }

    /// 表示名。**ASCII のみ**（既定フォントの都合）。
    const fn name(self) -> &'static str {
        match self {
            Self::Beginner => "beginner",
            Self::Normal => "normal",
            Self::Realistic => "realistic",
        }
    }
}

/// やり直したときに戻る場所。
///
/// **`setup` が作った開始状態をそのまま覚えておく。** 引数から作り直すと、
/// 空港 DB の解決や地形のサンプリングをもう一度通ることになり、
/// 「同じところに戻る」保証が計算の再現性頼みになる。
#[derive(Resource, Debug, Clone, Copy)]
enum StartCondition {
    /// 滑走路上の静止。標高と勾配はやり直しのたびに引き直す。
    Parked {
        /// 滑走路上の開始位置。
        position: Geodetic,
        /// 滑走路の方位。
        heading: flightsim_core::Radians,
    },
    /// 空中の任意状態。進入練習と落下試験。
    InFlight(flightsim_fdm::RigidBodyState),
}

/// 飛行の記録。**常に回している。**
///
/// 「今のを保存したい」と思うのは飛んだ**後**なので、押してから
/// 記録を始める作りでは間に合わない。1 フレーム 56 バイトなので、
/// 上限（約 4.6 時間）まで溜めても数十 MB。
#[derive(Resource)]
struct FlightRecorder(flightsim_sim::Recorder);

/// 再生中の状態。**再生していないときはリソースごと存在しない。**
#[derive(Resource)]
struct ReplayPlayback {
    player: flightsim_sim::Player,
    /// 再生済みの飛行時間。表示に使う。
    elapsed: Seconds,
    /// 記録全体の長さ。
    total: Seconds,
    /// 直近に流した操縦入力。
    ///
    /// **HUD と計器はこれを見る。** ここを渡さないと、機体が加速しているのに
    /// スロットル 0% と表示され、計器が嘘をつく（スクリーンショットで発覚）。
    last_controls: flightsim_fdm::ControlInputs,
}

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

/// OSM 空港 DB を実際に使う場合だけ表示する帰属。
///
/// 既定フォントに無い `©` は使わず `(c)` とする。詳細な URL と ODbL の説明は
/// 配布物の `ATTRIBUTION.md` にある。
const OSM_AIRPORT_ATTRIBUTION: &str = "Airport data: (c) OpenStreetMap contributors (ODbL)";

/// 起動時に選ばれた滑走路の出所。
///
/// OSM の DB を指定しても、読み込み失敗や空 DB なら合成滑走路へ戻る。その場合に
/// 帰属表示を出すと「使っていないデータの出所」を主張することになるため、結果を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunwaySource {
    Synthetic,
    OpenStreetMap { way_id: i64 },
}

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
    /// オフライン変換済みの OSM 空港 DB（`.fsairports`）。
    ///
    /// 生の PBF は実行時に読まない（ADR-0003 / ADR-0008）。
    airports: Option<PathBuf>,
    start: Geodetic,
    /// `--start` が明示されたか。
    ///
    /// 空港 DB だけを指定した場合は、選んだ滑走路上から始める。明示された場合は
    /// その地点を nearest 検索と spawn の両方に使うため、値だけでは区別できない。
    start_was_explicit: bool,
    heading: Radians,
    /// `--heading` が明示されたか。未指定なら選んだ滑走路方位へ揃える。
    heading_was_explicit: bool,
    /// 開始・進入・描画・灯火・着陸評価が共有する滑走路。
    ///
    /// `main` で空港 DB を解決した後は、各 system が別々に選び直してはならない。
    runway: Runway,
    /// Active runway の周囲にある OSM 誘導路。起動時に一度だけ地域 DB から絞る。
    taxiways: Vec<AirportTaxiway>,
    /// Active runway の周囲にある OSM エプロン。
    aprons: Vec<AirportApron>,
    /// Active runway の周囲にある OSM 待機位置。
    holding_positions: Vec<AirportHoldingPosition>,
    /// Active runway の周囲にある OSM 明示灯火。
    ground_lights: Vec<AirportGroundLight>,
    runway_source: RunwaySource,
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
    /// 乱流。`--turbulence light|moderate|severe` で指定する。
    turbulence: flightsim_fdm::Turbulence,
    /// 難易度。風・乱流・案内の既定を決める。
    difficulty: Difficulty,
    /// `--replay <FILE>` で再生する記録。指定があれば操縦を受け付けない。
    replay: Option<PathBuf>,
    /// `--wind` が明示されたか。**難易度の既定で上書きしないため。**
    wind_was_given: bool,
    /// `--turbulence` が明示されたか。
    turbulence_was_given: bool,
    /// 開始時刻（地方平均太陽時）。`None` なら render 側の既定。
    ///
    /// **地方平均太陽時にするのは、経度がどこでも「9 時なら朝」だから。**
    /// UTC で指定させると、飛ぶ場所によって同じ時刻が昼にも夜にもなる。
    start_hour: Option<(u8, u8)>,
    /// 時間加速の倍率。
    time_rate: f64,
    /// 描画する雲レイヤー。既定は快晴（雲量 0）。
    clouds: CloudLayer,
    /// 指定すると、地面から この高さ（m）の空中に静止 spawn する。
    ///
    /// **着陸評価の結線を実際に確かめるための開発用。** 落下して接地する
    /// までの数秒で、接地記録 → 評価 → 表示の経路が全部通る。手で
    /// 飛ばさないと着陸できないのでは、この経路を検証できない。
    drop_height: Option<f64>,
    /// 指定すると、最終進入の途中（末端から この海里）から始める。
    ///
    /// **着陸だけを練習したいのに毎回場周を一周させるのは辛い。**
    /// ゲームの核が着陸の腕なら、そこへすぐ入れる道が要る。
    approach: Option<f64>,
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
            airports: None,
            start: runway.takeoff_start(),
            start_was_explicit: false,
            heading: runway.heading,
            heading_was_explicit: false,
            runway,
            taxiways: Vec::new(),
            aprons: Vec::new(),
            holding_positions: Vec::new(),
            ground_lights: Vec::new(),
            runway_source: RunwaySource::Synthetic,
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
            turbulence: flightsim_fdm::Turbulence::CALM,
            difficulty: Difficulty::default(),
            replay: None,
            wind_was_given: false,
            turbulence_was_given: false,
            start_hour: None,
            time_rate: 1.0,
            clouds: CloudLayer::default(),
            drop_height: None,
            approach: None,
            assets: None,
        }
    }
}

/// シミュレーション本体。
#[derive(Resource)]
struct FlightSimulation(Simulation<BoxedSource>);

/// 着陸評価に使う滑走路。
///
/// 合成滑走路または OSM DB から起動時に選んだ 1 本。開始・進入・描画・灯火も
/// 同じ値を使い、system ごとに選び直さない。
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
    resolve_airport_database(&mut startup, &mut diagnostics);
    // **記録の条件は空港より後に当てる。** 空港の解決が開始位置と方位を
    // 書き換えるので、先に当てると上書きされて別の場所を再生してしまう。
    let recording = resolve_replay(&mut startup, &mut diagnostics);

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
        // 再生では記録された暦上の一点へ合わせる。**時分だけ合わせても
        // 日付がずれれば太陽高度が変わり、昼夜も影の向きも別物になる。**
        if let Some(recording) = recording.as_ref()
            && recording.conditions().start_epoch > 0.0
        {
            clock.utc = flightsim_render::JulianDate(recording.conditions().start_epoch);
        }
        clock
    };
    let clouds = startup.clouds;
    let conditions = recording_conditions(&startup, &clock);
    let playback = recording.map(|recording| ReplayPlayback {
        elapsed: Seconds(0.0),
        total: recording.duration(),
        last_controls: flightsim_fdm::ControlInputs::neutral(),
        player: flightsim_sim::Player::new(recording),
    });
    let data_attribution = match startup.runway_source {
        RunwaySource::Synthetic => DataAttribution::default(),
        RunwaySource::OpenStreetMap { .. } => DataAttribution::new(OSM_AIRPORT_ATTRIBUTION),
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(asset_plugin).set(WindowPlugin {
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
        flightsim_audio::FlightAudioPlugin,
    ))
    .insert_resource(clock)
    .insert_resource(clouds)
    .insert_resource(data_attribution)
    .insert_resource(startup)
    .insert_resource(diagnostics)
    .insert_resource(FlightRecorder(flightsim_sim::Recorder::new(conditions)))
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
            adjust_time_rate,
            toggle_tutorial,
            update_airport_lights,
            control_replay.before(advance_simulation),
            control_flight.before(advance_simulation),
            publish_crash.after(advance_simulation),
            publish_sound.after(advance_simulation),
            publish_replay_status.after(advance_simulation),
        ),
    );

    if let Some(playback) = playback {
        app.insert_resource(playback);
    }

    app.run();
}

/// `--replay` のファイルを読み、記録された条件を起動設定へ写す。
///
/// **読めなければ再生しない。** 条件だけ当てて中身を捨てると、
/// 「再生したつもりが普通に飛べる」という一番分かりにくい状態になる。
fn resolve_replay(
    startup: &mut Startup,
    diagnostics: &mut StartupDiagnostics,
) -> Option<flightsim_sim::Recording> {
    let path = startup.replay.clone()?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            diagnostics
                .0
                .push(format!("could not open `{}`: {error}", path.display()));
            startup.replay = None;
            return None;
        }
    };
    let recording = match flightsim_sim::Recording::read_from(&mut std::io::BufReader::new(file)) {
        Ok(recording) => recording,
        Err(error) => {
            diagnostics
                .0
                .push(format!("could not read `{}`: {error}", path.display()));
            startup.replay = None;
            return None;
        }
    };

    // 記録した機体で再生できるか。**違う機体なら別の軌跡になる。**
    if let Err(error) = recording.check_reproducible_with(&AircraftConfig::light_single()) {
        diagnostics
            .0
            .push(format!("`{}` cannot be replayed: {error}", path.display()));
        startup.replay = None;
        return None;
    }

    // 条件を写す。ここを飛ばすと、同じ入力を流しても別の飛行になる。
    let conditions = recording.conditions();
    startup.start = conditions.start;
    startup.heading = conditions.heading;
    startup.wind = conditions.wind;
    startup.turbulence = conditions.turbulence;
    startup.time_rate = conditions.time_rate;
    // 時刻は `clock` を直接置き換える（下記）。start_hour は触らない。
    // 進入練習と落下試験は開始状態を作り替えてしまう。再生では使わない。
    startup.approach = None;
    startup.drop_height = None;
    Some(recording)
}

/// これから飛ぶぶんの記録条件。
fn recording_conditions(
    startup: &Startup,
    clock: &flightsim_render::TimeOfDay,
) -> flightsim_sim::replay::Conditions {
    flightsim_sim::replay::Conditions {
        start: startup.start,
        heading: startup.heading,
        wind: startup.wind,
        turbulence: startup.turbulence,
        start_epoch: clock.utc.get(),
        time_rate: startup.time_rate,
        ..flightsim_sim::replay::Conditions::default()
    }
    .with_aircraft(&AircraftConfig::light_single())
}

/// 保存先を決める。既にある名前は上書きしない。
///
/// **上書きすると、直前の良い着陸が次のフライトで消える。**
fn next_replay_path() -> PathBuf {
    for index in 1..1_000 {
        let path = PathBuf::from(format!("flight-{index:03}.fsreplay"));
        if !path.exists() {
            return path;
        }
    }
    // 999 本溜まっているなら、もう名前で管理する話ではない。
    PathBuf::from("flight-overflow.fsreplay")
}

/// `--tiles <DIR>`、`--airports <FILE>`、`--start <LAT,LON>` を読む。
///
/// clap を入れるほどの規模ではない。増えたら入れる。
///
/// **指摘は `warn!` せず溜めて返す。** この関数は `LogPlugin` より前に走るので、
/// ここで出しても購読者が居らず何も表示されない（[`StartupDiagnostics`]）。
fn parse_arguments() -> (Startup, StartupDiagnostics) {
    parse_arguments_from(std::env::args().skip(1))
}

/// 次のトークンが別の long option でなければ、値として取り出す。
///
/// 値の無い option が後続 option を飲み込むと、後続側が警告なしで消える。
/// 負の数値は `-1` のようにハイフン 1 つなので、値として渡す。
fn next_argument_value<I>(arguments: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = String>,
{
    arguments
        .peek()
        .is_some_and(|value| !value.starts_with("--"))
        .then(|| arguments.next().expect("peeked argument must exist"))
}

/// 引数列を解釈する。テストから実プロセスの引数を差し替えられる入口。
fn parse_arguments_from(
    arguments: impl IntoIterator<Item = String>,
) -> (Startup, StartupDiagnostics) {
    let mut startup = Startup::default();
    let mut notes = Vec::new();

    // 雲の値は順不同で指定できるよう、全部読んでから組として検証する。
    let mut cloud_cover = startup.clouds.cover;
    let mut cloud_base = startup.clouds.base;
    let mut cloud_top = startup.clouds.top;
    let mut cloud_visibility = startup.clouds.visibility;
    let mut cloud_arguments_valid = true;

    // モデル関連は最後にまとめて決める。**軸の既定が「どのモデルか」で変わる**ため、
    // 引数を読んだ順に確定させられない。
    let mut requested_model = None;
    let mut placeholder = false;
    let mut forward = None;
    let mut up = None;

    let mut arguments = arguments.into_iter().peekable();

    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--tiles" => match next_argument_value(&mut arguments) {
                Some(path) => startup.tiles = Some(PathBuf::from(path)),
                None => notes.push("--tiles needs a directory".to_owned()),
            },
            "--replay" => match next_argument_value(&mut arguments) {
                Some(path) => startup.replay = Some(PathBuf::from(path)),
                None => notes.push("--replay needs a file".to_owned()),
            },
            "--difficulty" => match next_argument_value(&mut arguments) {
                Some(text) => match Difficulty::parse(&text) {
                    Some(level) => startup.difficulty = level,
                    None => notes.push(format!(
                        "unknown difficulty `{text}`; expected beginner, normal or realistic"
                    )),
                },
                None => notes.push("--difficulty needs a level".to_owned()),
            },
            "--airports" => match next_argument_value(&mut arguments) {
                Some(path) => startup.airports = Some(PathBuf::from(path)),
                None => notes.push("--airports needs a .fsairports file".to_owned()),
            },
            "--start" => {
                if let Some(text) = next_argument_value(&mut arguments) {
                    match parse_start_position(&text) {
                        Ok(position) => {
                            startup.start = position;
                            startup.start_was_explicit = true;
                        }
                        Err(message) => notes.push(message),
                    }
                } else {
                    notes.push("--start needs `lat,lon`".to_owned());
                }
            }
            "--heading" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) if value.is_finite() => {
                        startup.heading = Degrees(value).to_radians();
                        startup.heading_was_explicit = true;
                    }
                    _ => notes.push(format!(
                        "--heading expects finite degrees; ignoring `{text}`"
                    )),
                },
                None => notes.push("--heading needs a value".to_owned()),
            },
            "--max-level" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<u8>() {
                    Ok(value) => startup.max_level = value,
                    Err(_) => {
                        notes.push(format!("--max-level expects a number; ignoring `{text}`"))
                    }
                },
                None => notes.push("--max-level needs a value".to_owned()),
            },
            "--view" => {
                if let Some(name) = next_argument_value(&mut arguments) {
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
                } else {
                    notes.push("--view needs a view name".to_owned());
                }
            }
            "--model" => match next_argument_value(&mut arguments) {
                Some(path) => requested_model = Some(path),
                None => notes.push("--model needs a path".to_owned()),
            },
            // 箱のプレースホルダに戻す。同梱モデルが既定になったので、
            // **戻す手段が無いと寸法の食い違いを比べられない。**
            "--no-model" => placeholder = true,
            "--model-forward" | "--model-up" => {
                let Some(text) = next_argument_value(&mut arguments) else {
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
            "--wind" => match next_argument_value(&mut arguments) {
                Some(text) => match parse_wind(&text) {
                    Ok(wind) => {
                        startup.wind = wind;
                        startup.wind_was_given = true;
                    }
                    Err(message) => notes.push(message),
                },
                None => notes.push("--wind needs `<bearing>/<knots>`, e.g. 270/10".to_owned()),
            },
            "--turbulence" => match next_argument_value(&mut arguments) {
                Some(text) => match text.to_lowercase().as_str() {
                    // 種は固定。**同じ指定なら毎回同じ大気**になり、
                    // 「さっきの着陸が難しかったのは運か腕か」を切り分けられる。
                    "calm" | "none" => {
                        startup.turbulence = flightsim_fdm::Turbulence::CALM;
                        startup.turbulence_was_given = true;
                    }
                    "light" => {
                        startup.turbulence = flightsim_fdm::Turbulence::light(1);
                        startup.turbulence_was_given = true;
                    }
                    "moderate" => {
                        startup.turbulence = flightsim_fdm::Turbulence::moderate(1);
                        startup.turbulence_was_given = true;
                    }
                    "severe" => {
                        startup.turbulence = flightsim_fdm::Turbulence::severe(1);
                        startup.turbulence_was_given = true;
                    }
                    other => notes.push(format!(
                        "unknown turbulence `{other}`; expected calm, light, moderate or severe"
                    )),
                },
                None => notes.push("--turbulence needs a level".to_owned()),
            },
            // 地方平均太陽時。`--time 05:30` で日の出前から始まる。
            "--time" => match next_argument_value(&mut arguments) {
                Some(text) => match parse_clock(&text) {
                    Ok(clock) => startup.start_hour = Some(clock),
                    Err(message) => notes.push(message),
                },
                None => notes.push("--time needs `HH:MM`".to_owned()),
            },
            "--time-rate" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) if value.is_finite() && value >= 0.0 => startup.time_rate = value,
                    _ => notes.push(format!(
                        "--time-rate expects a non-negative number, got `{text}`"
                    )),
                },
                None => notes.push("--time-rate needs a multiplier".to_owned()),
            },
            "--cloud-cover" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f32>() {
                    Ok(value) => cloud_cover = value,
                    Err(_) => {
                        cloud_arguments_valid = false;
                        notes.push(format!(
                            "--cloud-cover expects a number from 0 to 1, got `{text}`"
                        ));
                    }
                },
                None => {
                    cloud_arguments_valid = false;
                    notes.push("--cloud-cover needs a value from 0 to 1".to_owned());
                }
            },
            "--cloud-base" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) => cloud_base = Meters(value),
                    Err(_) => {
                        cloud_arguments_valid = false;
                        notes.push(format!(
                            "--cloud-base expects an ellipsoid height in metres, got `{text}`"
                        ));
                    }
                },
                None => {
                    cloud_arguments_valid = false;
                    notes.push("--cloud-base needs a height in metres".to_owned());
                }
            },
            "--cloud-top" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) => cloud_top = Meters(value),
                    Err(_) => {
                        cloud_arguments_valid = false;
                        notes.push(format!(
                            "--cloud-top expects an ellipsoid height in metres, got `{text}`"
                        ));
                    }
                },
                None => {
                    cloud_arguments_valid = false;
                    notes.push("--cloud-top needs a height in metres".to_owned());
                }
            },
            "--cloud-visibility" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) => cloud_visibility = Meters(value),
                    Err(_) => {
                        cloud_arguments_valid = false;
                        notes.push(format!("--cloud-visibility expects metres, got `{text}`"))
                    }
                },
                None => {
                    cloud_arguments_valid = false;
                    notes.push("--cloud-visibility needs a distance in metres".to_owned());
                }
            },
            // 着陸練習。`--approach` だけなら 1 海里、値を付ければその距離。
            "--approach" => match next_argument_value(&mut arguments) {
                None => startup.approach = Some(1.0),
                Some(text) => match text.parse::<f64>() {
                    Ok(distance) if distance.is_finite() && distance > 0.0 => {
                        startup.approach = Some(distance);
                    }
                    _ => {
                        notes.push(format!("--approach expects miles out, got `{text}`"));
                    }
                },
            },
            "--drop" => match next_argument_value(&mut arguments) {
                Some(text) => match text.parse::<f64>() {
                    Ok(value) if value > 0.0 => startup.drop_height = Some(value),
                    _ => notes.push(format!(
                        "--drop expects metres above ground; ignoring `{text}`"
                    )),
                },
                None => notes.push("--drop needs a height in metres".to_owned()),
            },
            "--screenshot" => match next_argument_value(&mut arguments) {
                Some(path) => startup.screenshot = Some(PathBuf::from(path)),
                None => notes.push("--screenshot needs a PNG path".to_owned()),
            },
            "--screenshot-delay" => match next_argument_value(&mut arguments) {
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

    apply_difficulty(&mut startup);

    if cloud_arguments_valid {
        match CloudLayer::try_new(cloud_cover, cloud_base, cloud_top, cloud_visibility, 1) {
            Ok(clouds) => startup.clouds = clouds,
            Err(error) => notes.push(format!(
                "invalid cloud layer ({error}); using the clear default"
            )),
        }
    } else {
        notes.push("invalid cloud arguments; using the clear default".to_owned());
    }

    (startup, StartupDiagnostics(notes))
}

/// `latitude,longitude` を有限な度として読む。
///
/// `filter_map` で数値だけ拾うと `35,nope,139` が誤って 2 要素として通るため、
/// 各フィールドを独立に検査する。
fn parse_start_position(text: &str) -> Result<Geodetic, String> {
    let fields: Vec<&str> = text.split(',').collect();
    let [latitude, longitude] = fields.as_slice() else {
        return Err(format!("--start expects `lat,lon`; ignoring `{text}`"));
    };
    let latitude = latitude
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("--start latitude is not a number; ignoring `{text}`"))?;
    let longitude = longitude
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("--start longitude is not a number; ignoring `{text}`"))?;

    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(format!(
            "--start expects latitude -90..90 and longitude -180..180; ignoring `{text}`"
        ));
    }

    Ok(Geodetic::from_degrees(latitude, longitude, 0.0))
}

/// 指定された実行時空港 DB から最寄り滑走路を一度だけ選ぶ。
///
/// ここは `App::new()` より前なので、失敗はログへ直接出さず diagnostics に溜める。
/// 選択を setup や着陸評価で繰り返すと、開始位置と表示対象が別滑走路になり得る。
fn resolve_airport_database(startup: &mut Startup, diagnostics: &mut StartupDiagnostics) {
    let Some(path) = startup.airports.clone() else {
        return;
    };

    match AirportDatabase::read_from_path(&path) {
        Ok(database) => {
            if apply_nearest_airport(startup, database).is_none() {
                diagnostics.0.push(format!(
                    "airport database `{}` contains no runways; using the synthetic runway",
                    path.display()
                ));
            }
        }
        Err(error) => diagnostics.0.push(format!(
            "could not read airport database `{}` ({error}); using the synthetic runway",
            path.display()
        )),
    }
}

/// 最寄り滑走路を [`Startup`] の唯一の active runway として適用する。
///
/// 戻り値は OSM way ID。空 DB または不正な検索地点なら `None` で、設定は変えない。
fn apply_nearest_airport(startup: &mut Startup, database: AirportDatabase) -> Option<i64> {
    let selected = database.nearest(startup.start)?;
    let runway = selected.runway;
    let source_way_id = selected.source_way_id;
    let mut ground = database.into_ground_features();
    ground
        .taxiways
        .retain(|taxiway| taxiway_is_near_runway(taxiway, runway, ACTIVE_AIRPORT_RADIUS));
    ground
        .aprons
        .retain(|apron| apron_is_near_runway(apron, runway, ACTIVE_AIRPORT_RADIUS));
    ground
        .holding_positions
        .retain(|holding| point_is_near_runway(holding.position(), runway, ACTIVE_AIRPORT_RADIUS));
    ground
        .ground_lights
        .retain(|light| point_is_near_runway(light.position(), runway, ACTIVE_AIRPORT_RADIUS));

    startup.runway = runway;
    startup.taxiways = ground.taxiways;
    startup.aprons = ground.aprons;
    startup.holding_positions = ground.holding_positions;
    startup.ground_lights = ground.ground_lights;
    startup.runway_source = RunwaySource::OpenStreetMap {
        way_id: source_way_id,
    };
    if !startup.start_was_explicit {
        startup.start = runway.takeoff_start();
    }
    if !startup.heading_was_explicit {
        startup.heading = runway.heading;
    }

    Some(source_way_id)
}

/// 空港 relation が無くても、active runway 周辺だけを描画対象へ絞る。
fn taxiway_is_near_runway(taxiway: &AirportTaxiway, runway: Runway, radius: Meters) -> bool {
    let centre = runway.center().to_ecef();
    let points = taxiway.points();
    points
        .iter()
        .any(|point| centre.distance_to(point.to_ecef()).get() <= radius.get())
        || points.windows(2).any(|segment| {
            point_to_segment_distance(
                centre.as_vec(),
                segment[0].to_ecef().as_vec(),
                segment[1].to_ecef().as_vec(),
            ) <= radius.get()
        })
}

/// 面の頂点がすべて外でも、面自体が探索円を横切れば active airport に含める。
fn apron_is_near_runway(apron: &AirportApron, runway: Runway, radius: Meters) -> bool {
    let centre = runway.center().to_ecef().as_vec();
    apron.triangles().iter().any(|triangle| {
        point_to_triangle_distance(
            centre,
            triangle[0].to_ecef().as_vec(),
            triangle[1].to_ecef().as_vec(),
            triangle[2].to_ecef().as_vec(),
        ) <= radius.get()
    })
}

fn point_is_near_runway(point: Geodetic, runway: Runway, radius: Meters) -> bool {
    runway.center().to_ecef().distance_to(point.to_ecef()).get() <= radius.get()
}

fn point_to_segment_distance(
    point: bevy::math::DVec3,
    start: bevy::math::DVec3,
    end: bevy::math::DVec3,
) -> f64 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if !length_squared.is_finite() || length_squared <= f64::EPSILON {
        return point.distance(start);
    }
    let fraction = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * fraction)
}

/// 点と三角形の最短距離。射影が面内なら面まで、外なら三辺までを返す。
fn point_to_triangle_distance(
    point: bevy::math::DVec3,
    a: bevy::math::DVec3,
    b: bevy::math::DVec3,
    c: bevy::math::DVec3,
) -> f64 {
    let ab = b - a;
    let ac = c - a;
    let normal = ab.cross(ac);
    let normal_squared = normal.length_squared();
    if normal_squared.is_finite() && normal_squared > f64::EPSILON {
        let projected = point - normal * ((point - a).dot(normal) / normal_squared);
        let dot_00 = ab.dot(ab);
        let dot_01 = ab.dot(ac);
        let dot_11 = ac.dot(ac);
        let relative = projected - a;
        let denominator = dot_00 * dot_11 - dot_01 * dot_01;
        if denominator.abs() > f64::EPSILON {
            let u = (dot_11 * relative.dot(ab) - dot_01 * relative.dot(ac)) / denominator;
            let v = (dot_00 * relative.dot(ac) - dot_01 * relative.dot(ab)) / denominator;
            let tolerance = 1.0e-10;
            if u >= -tolerance && v >= -tolerance && u + v <= 1.0 + tolerance {
                return point.distance(projected);
            }
        }
    }

    point_to_segment_distance(point, a, b)
        .min(point_to_segment_distance(point, b, c))
        .min(point_to_segment_distance(point, c, a))
}

/// 測地点から指定方位の右へ移した位置。
///
/// 方位から NED への変換は `flightsim-core` の [`Attitude`] に集約し、app で
/// 測地変換を再実装しない。
fn point_right_of_heading(point: Geodetic, heading: Radians, distance: Meters) -> Geodetic {
    let body_to_ned = Attitude::new(Radians::ZERO, Radians::ZERO, heading).to_quaternion();
    let right_ned = body_to_ned * bevy::math::DVec3::Y * distance.get();
    LocalFrame::new(point)
        .ned_to_ecef_position(Ned(right_ned))
        .to_geodetic()
}

/// 待機位置から滑走路へ向かう接近方位。向きが分からない標識は安全側で省略する。
fn holding_approach_heading(heading: Radians, runway_side: RunwaySide) -> Option<Radians> {
    match runway_side {
        RunwaySide::Forward => Some(heading),
        RunwaySide::Backward => {
            Some(Radians(heading.get() + core::f64::consts::PI).wrap_positive())
        }
        RunwaySide::Unknown => None,
    }
}

type GroundLightCell = (GroundLightKind, i64, i64, i64);

/// 明示灯を優先しつつ、空間的に同じ灯火を除く bounded accumulator。
///
/// セル幅を重複距離と同じにすれば、重複候補は自身を含む 27 セルだけに存在する。
/// セルは候補の絞り込みにだけ使い、最終判定は ECEF の実距離で行う。
struct GroundLightAccumulator {
    limit: usize,
    lights: Vec<(Geodetic, GroundLightKind)>,
    ecef_positions: Vec<bevy::math::DVec3>,
    cells: HashMap<GroundLightCell, Vec<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroundLightInsert {
    Inserted,
    Duplicate,
    AtCapacity,
    AllocationFailed,
}

impl GroundLightAccumulator {
    fn try_new(limit: usize) -> Result<Self, std::collections::TryReserveError> {
        let mut lights = Vec::new();
        lights.try_reserve_exact(limit)?;
        let mut ecef_positions = Vec::new();
        ecef_positions.try_reserve_exact(limit)?;
        let mut cells = HashMap::new();
        cells.try_reserve(limit)?;
        Ok(Self {
            limit,
            lights,
            ecef_positions,
            cells,
        })
    }

    fn insert(&mut self, point: Geodetic, kind: GroundLightKind) -> GroundLightInsert {
        let ecef = point.to_ecef().as_vec();
        let (_, x, y, z) = ground_light_cell(ecef, kind);
        let maximum_distance_squared = GROUND_LIGHT_DEDUP_DISTANCE.powi(2);

        for delta_x in -1_i64..=1 {
            for delta_y in -1_i64..=1 {
                for delta_z in -1_i64..=1 {
                    let Some(neighbour_x) = x.checked_add(delta_x) else {
                        continue;
                    };
                    let Some(neighbour_y) = y.checked_add(delta_y) else {
                        continue;
                    };
                    let Some(neighbour_z) = z.checked_add(delta_z) else {
                        continue;
                    };
                    let Some(indices) =
                        self.cells
                            .get(&(kind, neighbour_x, neighbour_y, neighbour_z))
                    else {
                        continue;
                    };
                    if indices.iter().any(|&index| {
                        self.ecef_positions[index].distance_squared(ecef)
                            <= maximum_distance_squared
                    }) {
                        return GroundLightInsert::Duplicate;
                    }
                }
            }
        }

        // 重複を先に判定する。上限到達後の duplicate も capacity を消費せず、
        // 明示灯と fallback の順序を変えても unique 数にだけ上限が掛かる。
        if self.lights.len() == self.limit {
            return GroundLightInsert::AtCapacity;
        }

        let index = self.lights.len();
        let indices = self.cells.entry((kind, x, y, z)).or_default();
        if indices.try_reserve(1).is_err() {
            return GroundLightInsert::AllocationFailed;
        }
        self.lights.push((point, kind));
        self.ecef_positions.push(ecef);
        indices.push(index);
        GroundLightInsert::Inserted
    }

    fn len(&self) -> usize {
        self.lights.len()
    }

    fn into_lights(self) -> Vec<(Geodetic, GroundLightKind)> {
        self.lights
    }
}

fn ground_light_cell(ecef: bevy::math::DVec3, kind: GroundLightKind) -> GroundLightCell {
    let scaled = ecef / GROUND_LIGHT_DEDUP_DISTANCE;
    debug_assert!(scaled.is_finite());
    debug_assert!(
        scaled.abs().max_element() < 9.0e18_f64,
        "有効な地球近傍の測地座標は i64 セル範囲に収まる"
    );
    #[allow(
        clippy::cast_possible_truncation,
        reason = "有限な地球近傍 ECEF を 0.25 m セルへ切り下げ、i64 範囲も直前で確認する"
    )]
    (
        kind,
        scaled.x.floor() as i64,
        scaled.y.floor() as i64,
        scaled.z.floor() as i64,
    )
}

fn valid_taxiway_surface_point_count(count: usize) -> bool {
    (2..=MAX_TAXIWAY_SURFACE_POINTS).contains(&count)
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

/// 難易度の既定値を、明示指定されていない項目にだけ入れる。
///
/// **`--wind` / `--turbulence` を打ち消さない。** 難易度は「何も言わなかった
/// ときにどうするか」を決めるだけで、利用者の明示指定より強くはない。
///
/// 着陸の採点には触れない（[`Difficulty`] の doc を参照）。
fn apply_difficulty(startup: &mut Startup) {
    if !startup.wind_was_given {
        startup.wind = startup.difficulty.default_wind(startup.heading);
    }
    if !startup.turbulence_was_given {
        startup.turbulence = startup.difficulty.default_turbulence();
    }
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

/// 太陽高度に応じて滑走路灯・誘導路灯・警戒灯を点け消しする。
///
/// 材質の `emissive` を直接動かす。**灯火ごとにエンティティを持たない**
/// （色と chunk ごとに束ねてある）ので、灯火数に比例する entity 更新は起こらない。
fn update_airport_lights(
    sun: Res<SunDirection>,
    lights: Query<(
        &MeshMaterial3d<StandardMaterial>,
        &flightsim_render::runway_lights::AirportLights,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut previous: Local<Option<f32>>,
) {
    let fraction = flightsim_render::runway_lights::light_intensity_fraction(sun.elevation);

    // 変わっていなければ何もしない。毎フレーム材質を書き換えると
    // GPU への再アップロードが走る。
    if previous.is_some_and(|value| (value - fraction).abs() < 1e-4) {
        return;
    }
    *previous = Some(fraction);

    for (handle, marker) in &lights {
        let Some(material) = materials.get_mut(&handle.0) else {
            continue;
        };
        // **現在値から逆算しない。** 一度 0 にすると二度と戻らないうえ、
        // 誤差が溜まる。常に「基準色 × 比率」で計算する。
        material.emissive = marker.emissive_at(fraction);
    }
}

/// `H` でチュートリアルの表示を切り替える。
///
/// 時間加速と同じく、キー入力（`flightsim-input`）と表示状態
/// （`flightsim-ui`）は同階層で直接依存できないため app に置く。
///
/// **上級者の邪魔をしないための逃げ道。** 状態機械は裏で動き続けるので、
/// 戻したときは古い段階ではなく今の段階が出る。
fn toggle_tutorial(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visibility: ResMut<flightsim_ui::TutorialVisibility>,
) {
    if keyboard.just_pressed(KeyCode::KeyH) {
        visibility.toggle();
    }
}

/// 時間加速をキーで変える。
///
/// # なぜ app に置くのか
///
/// 時刻は `flightsim-render` の `TimeOfDay` が持ち、キー入力は
/// `flightsim-input` が扱う。**この 2 つは同階層なので直接依存できない**
/// （CLAUDE.md 規約 2 の横断禁止）。両方を知っているのは app だけ。
///
/// `,` で遅く、`.` で速く。日の出を待つのに実時間を使わせない。
fn adjust_time_rate(
    keyboard: Res<ButtonInput<KeyCode>>,
    paused: Res<flightsim_ui::Paused>,
    mut clock: ResMut<flightsim_render::TimeOfDay>,
) {
    // 一時停止中は触らせない。ここで倍率を変えると、再開時に復元する値と
    // 食い違って**押した設定が黙って消える**。
    if paused.is_paused() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Period) {
        clock.rate = clock.rate.faster();
        info!("time rate: {}x", clock.rate.0);
    }
    if keyboard.just_pressed(KeyCode::Comma) {
        clock.rate = clock.rate.slower();
        info!("time rate: {}x", clock.rate.0);
    }
}

/// `Esc` で一時停止、`R` でやり直し。
///
/// # なぜ app に置くのか
///
/// キー入力は `flightsim-input`、状態は `flightsim-sim`、表示は
/// `flightsim-ui`。**この 3 つは直接依存できない**（規約 2）。
///
/// 再生中は効かない。記録を止めたり巻き戻したりするのは `F5` / `F8` の
/// 仕事で、そちらと二重に効くと状態が食い違う。
#[expect(
    clippy::too_many_arguments,
    reason = "やり直しは機体・操縦・記録・時刻・案内・評価を同時に戻す。まとめると誰が何を戻すのか読めなくなる"
)]
fn control_flight(
    keyboard: Res<ButtonInput<KeyCode>>,
    start: Res<StartCondition>,
    playback: Option<Res<ReplayPlayback>>,
    mut paused: ResMut<flightsim_ui::Paused>,
    mut simulation: ResMut<FlightSimulation>,
    mut controls: ResMut<PilotControls>,
    mut recorder: ResMut<FlightRecorder>,
    mut clock: ResMut<flightsim_render::TimeOfDay>,
    mut tutorial: ResMut<flightsim_ui::TutorialState>,
    mut landing: ResMut<flightsim_ui::LandingReportState>,
    // 止める直前の時間加速。**再開で 1 倍に戻さないため。**
    // `--time-rate 60` で夜明けを待っていた人を、一時停止のたびに
    // 実時間へ引き戻すことになる。
    mut rate_before_pause: Local<Option<flightsim_render::TimeRate>>,
) {
    if playback.is_some() {
        return;
    }

    // 墜落中は止められない。**止まっているものを止めても何も起きず、
    // 帯が 2 枚重なるだけ。** やり直し（`R`）は効く。
    if keyboard.just_pressed(KeyCode::Escape) && !simulation.0.crashed() {
        paused.toggle();
        // 止めたのに日が暮れるのはおかしい。時刻も一緒に止める。
        if paused.is_paused() {
            *rate_before_pause = Some(clock.rate);
            clock.rate = flightsim_render::TimeRate::PAUSED;
        } else {
            clock.rate = rate_before_pause
                .take()
                .unwrap_or(flightsim_render::TimeRate::REAL_TIME);
        }
        info!(
            "{}",
            if paused.is_paused() {
                "paused"
            } else {
                "resumed"
            }
        );
    }

    if keyboard.just_pressed(KeyCode::KeyR) {
        restart_flight(
            &start,
            &mut simulation,
            &mut controls,
            &mut recorder,
            &mut tutorial,
            &mut landing,
        );
        // やり直したら止まったままにしない。**押してから Esc を探させない。**
        // 時間加速は止める前のものへ戻す。やり直しは時刻の設定を変える操作ではない。
        if paused.is_paused() {
            clock.rate = rate_before_pause
                .take()
                .unwrap_or(flightsim_render::TimeRate::REAL_TIME);
        }
        paused.0 = false;
        info!("restarted");
    }
}

/// 開始状態へ戻す。
///
/// **記録も評価も案内も一緒に戻す。** 機体だけ戻すと、飛んだ距離や
/// 直前の着陸評価が残り、やり直したのに前回の結果が画面に出続ける。
fn restart_flight(
    start: &StartCondition,
    simulation: &mut FlightSimulation,
    controls: &mut PilotControls,
    recorder: &mut FlightRecorder,
    tutorial: &mut flightsim_ui::TutorialState,
    landing: &mut flightsim_ui::LandingReportState,
) {
    match *start {
        StartCondition::Parked { position, heading } => {
            simulation.0.restart_parked_at(position, heading);
            *controls = PilotControls::default();
        }
        StartCondition::InFlight(state) => {
            simulation.0.restart_at(state);
            // 進入の途中から始まるので、`setup` と同じ出力とフラップで置く。
            // 全閉・フラップ 0 で放り出すと即座に落ちる。
            let mut fresh = PilotControls::default();
            fresh.throttle.set_absolute(APPROACH_THROTTLE);
            fresh.flaps.set_absolute(APPROACH_FLAPS);
            *controls = fresh;
        }
    }

    // **記録は捨てる。** やり直しは記録に残らないので、残したまま続けると
    // 再生時に同じ入力を流しても別の飛行になる。
    let conditions = recorder.0.recording().conditions().clone();
    recorder.0 = flightsim_sim::Recorder::new(conditions);

    *tutorial = flightsim_ui::TutorialState::default();
    *landing = flightsim_ui::LandingReportState::default();
}

/// リプレイの保存と再生操作。
///
/// # なぜ app に置くのか
///
/// キー入力は `flightsim-input`、記録は `flightsim-sim`、表示は
/// `flightsim-ui`。**この 3 つは直接依存できない**（規約 2）。
/// 3 つとも知っているのは app だけ。
///
/// - `F9` — ここまでの飛行を保存する（再生中は効かない）
/// - `F5` — 一時停止・再開
/// - `F6` / `F7` — 遅く / 速く
/// - `F8` — 10 秒戻る
fn control_replay(
    keyboard: Res<ButtonInput<KeyCode>>,
    recorder: Res<FlightRecorder>,
    mut simulation: ResMut<FlightSimulation>,
    playback: Option<ResMut<ReplayPlayback>>,
) {
    let Some(mut playback) = playback else {
        if keyboard.just_pressed(KeyCode::F9) {
            save_recording(recorder.0.recording());
        }
        return;
    };

    if keyboard.just_pressed(KeyCode::F5) {
        let paused = !playback.player.is_paused();
        playback.player.set_paused(paused);
        info!("replay: {}", if paused { "paused" } else { "resumed" });
    }
    if keyboard.just_pressed(KeyCode::F6) {
        let speed = playback.player.speed() / 2.0;
        playback.player.set_speed(speed);
        info!("replay speed: x{:.2}", playback.player.speed());
    }
    if keyboard.just_pressed(KeyCode::F7) {
        let speed = playback.player.speed() * 2.0;
        playback.player.set_speed(speed);
        info!("replay speed: x{:.2}", playback.player.speed());
    }
    if keyboard.just_pressed(KeyCode::F8) {
        rewind_replay(&mut playback, &mut simulation);
    }
}

/// 10 秒ぶん戻して、キーフレームから流し直す。
///
/// **積分は戻せない。** キーフレームまで巻き戻し、そこから目標まで
/// 一気に回し直す。10 秒ぶんの空回しは 1 フレームで終わる。
fn rewind_replay(playback: &mut ReplayPlayback, simulation: &mut FlightSimulation) {
    let recording = playback.player.recording();
    // フレーム時間は一定ではないので、時間から番号を数え直す。
    let target_time = (playback.elapsed.get() - 10.0).max(0.0);
    let mut accumulated = 0.0;
    let mut target = 0_u32;
    for (index, frame) in recording.frames().iter().enumerate() {
        if accumulated >= target_time {
            target = u32::try_from(index).unwrap_or(u32::MAX);
            break;
        }
        accumulated += frame.frame_time.get();
        target = u32::try_from(index).unwrap_or(u32::MAX);
    }

    let Some(plan) = playback.player.seek(target) else {
        warn!("replay: this recording has no keyframes; cannot rewind");
        return;
    };
    simulation.0.rewind_to(plan.state);

    // キーフレームから目標まで詰める。ここは表示せずに一気に回す。
    let mut elapsed = 0.0;
    for frame in playback
        .player
        .recording()
        .frames()
        .iter()
        .take(plan.replay_from as usize)
    {
        elapsed += frame.frame_time.get();
    }
    while playback.player.cursor() < plan.target {
        let Some(frame) = playback.player.step_once() else {
            break;
        };
        simulation.0.advance(frame.frame_time, frame.controls);
        playback.last_controls = frame.controls;
        elapsed += frame.frame_time.get();
    }
    playback.elapsed = Seconds(elapsed);
    info!("replay: rewound to {:.1} s", elapsed);
}

/// 記録をファイルへ書く。
fn save_recording(recording: &flightsim_sim::Recording) {
    if recording.frames().is_empty() {
        warn!("nothing to save yet");
        return;
    }
    let path = next_replay_path();
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(error) => {
            error!("could not create `{}`: {error}", path.display());
            return;
        }
    };
    let mut writer = std::io::BufWriter::new(file);
    match recording.write_to(&mut writer) {
        // **flush を確かめる。** BufWriter は drop 時の失敗を握り潰すので、
        // ここを省くと「保存しました」と言った直後に中身が欠ける。
        Ok(()) => match std::io::Write::flush(&mut writer) {
            Ok(()) => info!(
                "saved {} frames ({:.0} s) to {}",
                recording.frames().len(),
                recording.duration().get(),
                path.display()
            ),
            Err(error) => error!("could not finish writing `{}`: {error}", path.display()),
        },
        Err(error) => error!("could not write `{}`: {error}", path.display()),
    }
}

/// 失速警報を出し始める、失速角に対する迎角の割合。
///
/// 実機の失速警報は失速速度の 5〜10 kt 手前で鳴る。迎角で言えば失速角の
/// 手前で、**余裕を持って鳴らないと警報の意味がない**（鳴った時点で
/// 失速していては回復操作が間に合わない）。
const STALL_WARNING_FRACTION: f64 = 0.85;

/// 警報を止める割合。**鳴り始める点より低くする。**
///
/// 同じ値にすると、境界上で迎角が揺れるたびに鳴ったり止まったりして
/// 耳障りなうえ、本当に近いのかが分からなくなる。
const STALL_WARNING_RELEASE: f64 = 0.78;

/// 機体の状態を音へ渡す。
///
/// `flightsim-audio` は `flightsim-sim` に依存できない（依存は一方向）ので、
/// 値を取り出すのは app の仕事。
fn publish_sound(
    simulation: Res<FlightSimulation>,
    controls: Res<PilotControls>,
    paused: Res<flightsim_ui::Paused>,
    playback: Option<Res<ReplayPlayback>>,
    mut sound: ResMut<flightsim_audio::AircraftSound>,
    mut warning: Local<bool>,
) {
    // 再生中は流している側の出力を映す。手元の操縦桿ではない
    // （HUD と同じ理由。`publish_hud` を参照）。
    let throttle = playback.as_ref().map_or_else(
        || controls.throttle.value(),
        |playback| playback.last_controls.throttle(),
    );

    // ヒステリシス。**同じ閾値で入り切りすると境界で鳴り続ける。**
    let fraction = simulation.0.stall_fraction();
    if *warning {
        if fraction < STALL_WARNING_RELEASE {
            *warning = false;
        }
    } else if fraction >= STALL_WARNING_FRACTION {
        *warning = true;
    }

    let crashed = simulation.0.crashed();
    *sound = flightsim_audio::AircraftSound {
        throttle,
        airspeed: simulation.0.airspeed(),
        // 壊れた機体は失速しない。**止まっているのに警報が鳴り続けない。**
        stall_warning: *warning && !crashed,
        // 一時停止と墜落では黙る。動いていないのに音がするのは変。
        muted: paused.is_paused() || crashed,
    };
}

/// 墜落を UI へ渡し、一度だけログに出す。
///
/// `flightsim-ui` は `flightsim-sim` に依存できない（依存は一方向）ので、
/// 原因の文言を作るのは app の仕事。
fn publish_crash(
    simulation: Res<FlightSimulation>,
    mut notice: ResMut<flightsim_ui::CrashNotice>,
    mut reported: Local<bool>,
) {
    match simulation.0.crash() {
        Some(crash) => {
            if !*reported {
                *reported = true;
                let headline = crash.cause.headline();
                // **何が起きたかを数字で残す。** 「墜落した」だけでは
                // 次に何を直せばいいか分からない。
                error!(
                    "{headline} (at {:.5}, {:.5} after {:.0} s)",
                    crash.position.latitude_degrees(),
                    crash.position.longitude_degrees(),
                    crash.elapsed.get()
                );
                notice.set(headline);
            }
        }
        None => {
            if *reported {
                *reported = false;
                notice.clear();
            }
        }
    }
}

/// 再生状態を UI へ渡す。
fn publish_replay_status(
    playback: Option<Res<ReplayPlayback>>,
    mut status: ResMut<flightsim_ui::ReplayStatus>,
) {
    let next = playback.map_or_else(flightsim_ui::ReplayStatus::default, |playback| {
        flightsim_ui::ReplayStatus {
            active: true,
            paused: playback.player.is_paused(),
            speed: playback.player.speed(),
            elapsed: playback.elapsed,
            total: playback.total,
        }
    });
    if *status != next {
        *status = next;
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
    info!("difficulty: {}", startup.difficulty.name());
    if startup.replay.is_some() {
        // 記録の再生に「今すぐ離陸しろ」と指示しても意味がない。**指示どおりに
        // 操作しても何も起きないので、壊れているように見える。**
        commands.insert_resource(flightsim_ui::TutorialVisibility(false));
    }
    // 案内の既定は難易度が決める。**進入練習はこの後で上書きする**
    // （離陸の案内は進入練習では誤った指示になるため）。
    if !startup.difficulty.shows_tutorial() {
        commands.insert_resource(flightsim_ui::TutorialVisibility(false));
    }

    match &startup.tiles {
        Some(path) => info!("terrain: {}", path.display()),
        None => info!("terrain: none — the whole world is at sea level"),
    }
    info!(
        "start: {:.5}, {:.5}",
        startup.start.latitude_degrees(),
        startup.start.longitude_degrees()
    );
    match startup.runway_source {
        RunwaySource::Synthetic => info!("runway: synthetic"),
        RunwaySource::OpenStreetMap { way_id } => {
            info!("runway: OpenStreetMap way {way_id}")
        }
    }

    // --- シミュレーション ---

    // OSM `ele` の基準・品質は一定せず、使用中の地形とは揃わない（ADR-0008）。
    // 選択した滑走路の進入端で実際の地形数値を引き、描画・接地を局所的に揃える。
    // これは鉛直基準の変換ではない。Copernicus DEM の EGM2008 ⇒ WGS84 は #22。
    let airport_sampler = GroundSampler::default();
    let mut airport_probe = Terrain::new(
        make_source(&startup),
        8 * 1024 * 1024,
        startup.min_level..=startup.max_level,
    );
    let runway_elevation = airport_sampler
        .sample(&mut airport_probe, startup.runway.threshold)
        .elevation;
    let runway = startup.runway.with_elevation(runway_elevation);

    let terrain = Terrain::new(
        make_source(&startup),
        64 * 1024 * 1024,
        startup.min_level..=startup.max_level,
    );
    // 着陸練習。滑走路の手前・進入角に乗った状態から始める。
    //
    // **操縦入力も進入の形に合わせる。** 状態だけ空中に置いてスロットルを
    // 0 のままにすると、始まった瞬間から失速へ向かって突っ込む
    // （実測 -1685 ft/min）。練習にならない。
    if startup.approach.is_some() {
        let mut controls = PilotControls::default();
        controls.throttle.set_absolute(APPROACH_THROTTLE);
        controls.flaps.set_absolute(APPROACH_FLAPS);
        commands.insert_resource(controls);

        // チュートリアルは離陸から始まる流れを案内する。進入練習では
        // **「戻ってきて降下しろ」と誤った指示を出す**ので黙らせる。
        // `H` でいつでも戻せる。
        commands.insert_resource(flightsim_ui::TutorialVisibility(false));
    }

    let mut start_condition = StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    };
    let simulation = if let Some(miles) = startup.approach {
        let state = flightsim_sim::approach_state(
            &runway,
            flightsim_core::NauticalMiles(miles).to_meters(),
            Degrees(3.0).to_radians(),
            flightsim_core::MetersPerSecond(35.0),
        );
        start_condition = StartCondition::InFlight(state);
        Simulation::from_state(
            AircraftConfig::light_single(),
            state,
            terrain,
            GroundSampler::default(),
        )
    } else {
        match startup.drop_height {
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
                start_condition = StartCondition::InFlight(state);
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
        }
    };

    let mut simulation = simulation;
    simulation.set_wind(startup.wind);
    simulation.set_turbulence(startup.turbulence);

    commands.insert_resource(startup.view);

    // --- 空港面 ---

    // エプロンは誘導路より低い lift で先に置く。三角形の各頂点で DEM を引くため、
    // 大きな面も平らな板にはならず地形へ追従する。
    let airport_surface_material = materials.add(flightsim_render::default_terrain_material());
    let mut rendered_aprons = 0_usize;
    for apron in &startup.aprons {
        let mut surface_triangles = Vec::new();
        if surface_triangles
            .try_reserve_exact(apron.triangles().len())
            .is_err()
        {
            warn!(
                "apron: could not allocate OSM feature {}",
                apron.source_id()
            );
            continue;
        }
        for triangle in apron.triangles() {
            surface_triangles.push(triangle.map(|point| {
                let elevation = airport_sampler.sample(&mut airport_probe, point).elevation;
                Geodetic::new(point.latitude, point.longitude, elevation)
            }));
        }
        let (mesh, origin) =
            match flightsim_render::apron::apron_mesh(&surface_triangles, apron.surface()) {
                Ok(result) => result,
                Err(error) => {
                    warn!(
                        "apron: skipped invalid OSM feature {} ({error})",
                        apron.source_id()
                    );
                    continue;
                }
            };
        commands.spawn((
            flightsim_render::terrain_mesh_bundle(
                meshes.add(mesh),
                airport_surface_material.clone(),
                origin,
            ),
            Name::new(format!("apron OSM feature {}", apron.source_id())),
        ));
        rendered_aprons += 1;
    }

    // 明示 OSM 灯火を先に登録する。同種かつ 0.25 m 以内の fallback は後から除かれ、
    // 明示データが常に優先される。上限は raw 入力ではなく dedup 後の unique 数に掛ける。
    let ground_light_cap = flightsim_render::taxiway_lights::MAX_GROUND_LIGHTS;
    let has_ground_light_sources = !startup.ground_lights.is_empty()
        || startup
            .taxiways
            .iter()
            .any(|taxiway| taxiway.lighting() != flightsim_world::TaxiwayLighting::None);
    let mut ground_light_accumulator = if has_ground_light_sources {
        match GroundLightAccumulator::try_new(ground_light_cap) {
            Ok(accumulator) => Some(accumulator),
            Err(error) => {
                warn!("airport lights: could not allocate ground-light layout ({error})");
                None
            }
        }
    } else {
        None
    };
    let mut duplicate_ground_lights = 0_usize;
    let mut dropped_ground_lights = 0_usize;
    let mut rendered_explicit_lights = 0_usize;
    let mut rendered_fallback_lights = 0_usize;
    let mut ground_light_allocation_failed =
        has_ground_light_sources && ground_light_accumulator.is_none();
    if let Some(accumulator) = ground_light_accumulator.as_mut() {
        for light in &startup.ground_lights {
            let elevation = airport_sampler
                .sample(&mut airport_probe, light.position())
                .elevation;
            let point = Geodetic::new(
                light.position().latitude,
                light.position().longitude,
                elevation,
            );
            match accumulator.insert(point, light.kind()) {
                GroundLightInsert::Inserted => rendered_explicit_lights += 1,
                GroundLightInsert::Duplicate => duplicate_ground_lights += 1,
                GroundLightInsert::AtCapacity => dropped_ground_lights += 1,
                GroundLightInsert::AllocationFailed => {
                    warn!("airport lights: could not extend ground-light spatial index");
                    ground_light_allocation_failed = true;
                    break;
                }
            }
        }
    }

    // 誘導路は各 OSM node で DEM を引く。滑走路標高を全 way へ固定すると、長い
    // 誘導路の端が斜面へ埋まる。中心線 way ごとに 1 mesh へまとめるため、entity 数は
    // node 数ではなく way 数に抑えられる。
    let mut rendered_taxiways = 0_usize;
    for taxiway in &startup.taxiways {
        let point_count = taxiway.points().len();
        if !valid_taxiway_surface_point_count(point_count) {
            warn!(
                "taxiway: skipped invalid OpenStreetMap way {} ({point_count} points)",
                taxiway.source_way_id
            );
            continue;
        }
        let mut surface_points = Vec::new();
        if surface_points.try_reserve_exact(point_count).is_err() {
            warn!(
                "taxiway: could not allocate OpenStreetMap way {}",
                taxiway.source_way_id
            );
            continue;
        }
        for &point in taxiway.points() {
            let elevation = airport_sampler.sample(&mut airport_probe, point).elevation;
            surface_points.push(Geodetic::new(point.latitude, point.longitude, elevation));
        }
        let Some((mesh, origin)) =
            flightsim_render::taxiway::taxiway_mesh(&surface_points, taxiway.width)
        else {
            warn!(
                "taxiway: skipped invalid OpenStreetMap way {}",
                taxiway.source_way_id
            );
            continue;
        };
        commands.spawn((
            flightsim_render::terrain_mesh_bundle(
                meshes.add(mesh),
                airport_surface_material.clone(),
                origin,
            ),
            Name::new(format!("taxiway OSM way {}", taxiway.source_way_id)),
        ));
        rendered_taxiways += 1;

        if ground_light_allocation_failed || ground_light_accumulator.is_none() {
            continue;
        }
        match flightsim_render::taxiway_lights::procedural_taxiway_light_layout(
            &surface_points,
            taxiway.width,
            taxiway.lighting(),
        ) {
            Ok(layout) => {
                let Some(accumulator) = ground_light_accumulator.as_mut() else {
                    continue;
                };
                for (point, kind) in layout {
                    let elevation = airport_sampler.sample(&mut airport_probe, point).elevation;
                    let surface_point = Geodetic::new(point.latitude, point.longitude, elevation);
                    match accumulator.insert(surface_point, kind) {
                        GroundLightInsert::Inserted => rendered_fallback_lights += 1,
                        GroundLightInsert::Duplicate => duplicate_ground_lights += 1,
                        GroundLightInsert::AtCapacity => dropped_ground_lights += 1,
                        GroundLightInsert::AllocationFailed => {
                            warn!(
                                "taxiway lights: could not extend fallback index at OSM way {}",
                                taxiway.source_way_id
                            );
                            ground_light_allocation_failed = true;
                            break;
                        }
                    }
                }
            }
            Err(error) => warn!(
                "taxiway lights: skipped fallback for OSM way {} ({error})",
                taxiway.source_way_id
            ),
        }
    }

    // 待機位置の路面標示と物理標識。標識は中心線上へ立てず、滑走路へ向かう方位の
    // 右側へ逃がし、正面を接近機側へ向ける。
    let mut rendered_markings = 0_usize;
    let mut rendered_signs = 0_usize;
    for holding in &startup.holding_positions {
        let centre_elevation = airport_sampler
            .sample(&mut airport_probe, holding.position())
            .elevation;
        let centre = Geodetic::new(
            holding.position().latitude,
            holding.position().longitude,
            centre_elevation,
        );
        match flightsim_render::holding_position::holding_position_mesh(
            centre,
            holding.heading(),
            holding.width(),
            holding.runway_side(),
        ) {
            Ok(Some((mesh, origin))) => {
                commands.spawn((
                    flightsim_render::terrain_mesh_bundle(
                        meshes.add(mesh),
                        airport_surface_material.clone(),
                        origin,
                    ),
                    Name::new(format!(
                        "holding marking OSM feature {}",
                        holding.source_id()
                    )),
                ));
                rendered_markings += 1;
            }
            Ok(None) => {}
            Err(error) => warn!(
                "holding marking: skipped OSM feature {} ({error})",
                holding.source_id()
            ),
        }

        let Some(holding_ref) = holding.reference() else {
            continue;
        };
        let Some(taxiway_ref) = holding.related_taxiway().and_then(|source_way_id| {
            startup
                .taxiways
                .iter()
                .find(|taxiway| taxiway.source_way_id == source_way_id)
                .and_then(AirportTaxiway::reference)
        }) else {
            continue;
        };
        let Some(approach_heading) =
            holding_approach_heading(holding.heading(), holding.runway_side())
        else {
            continue;
        };
        if !taxiway_ref.is_ascii()
            || !holding_ref.is_ascii()
            || taxiway_ref.len() > flightsim_render::taxiway_sign::MAX_SIGN_REF_CHARS
            || holding_ref.len() > flightsim_render::taxiway_sign::MAX_SIGN_REF_CHARS
        {
            warn!(
                "holding sign: unsupported ref on OSM feature {}",
                holding.source_id()
            );
            continue;
        }
        let sign_centre = point_right_of_heading(
            holding.position(),
            approach_heading,
            Meters(holding.width().get() * 0.5 + 2.0),
        );
        let sign_elevation = airport_sampler
            .sample(&mut airport_probe, sign_centre)
            .elevation;
        let sign_position =
            Geodetic::new(sign_centre.latitude, sign_centre.longitude, sign_elevation);
        let sign_facing = Radians(approach_heading.get() + core::f64::consts::PI).wrap_positive();
        let taxiway_ref = taxiway_ref.to_ascii_uppercase();
        let holding_ref = holding_ref.to_ascii_uppercase();
        match flightsim_render::taxiway_sign::holding_position_sign_mesh(
            sign_position,
            sign_facing,
            &taxiway_ref,
            &holding_ref,
        ) {
            Ok(Some((mesh, origin))) => {
                commands.spawn((
                    flightsim_render::terrain_mesh_bundle(
                        meshes.add(mesh),
                        airport_surface_material.clone(),
                        origin,
                    ),
                    Name::new(format!("holding sign OSM feature {}", holding.source_id())),
                ));
                rendered_signs += 1;
            }
            Ok(None) => warn!(
                "holding sign: unsupported ref on OSM feature {}",
                holding.source_id()
            ),
            Err(error) => warn!(
                "holding sign: skipped OSM feature {} ({error})",
                holding.source_id()
            ),
        }
    }

    // 明示灯と各 way の fallback を統合済みの配列だけ、色別の少数 mesh に束ねる。
    if duplicate_ground_lights != 0 {
        info!("airport lights: removed {duplicate_ground_lights} duplicate points");
    }
    if dropped_ground_lights != 0 {
        warn!(
            "airport lights: dropped {dropped_ground_lights} points beyond the deduplicated active-airport limit"
        );
    }
    info!(
        "airport lights: {rendered_explicit_lights} explicit + \
         {rendered_fallback_lights} fallback points"
    );
    debug_assert_eq!(
        ground_light_accumulator
            .as_ref()
            .map_or(0, GroundLightAccumulator::len),
        rendered_explicit_lights + rendered_fallback_lights
    );
    let airport_ground_lights =
        ground_light_accumulator.map_or_else(Vec::new, GroundLightAccumulator::into_lights);
    let mut rendered_light_groups = 0_usize;
    match flightsim_render::taxiway_lights::ground_light_meshes(&airport_ground_lights) {
        Ok(Some((groups, origin))) => {
            for group in groups {
                commands.spawn((
                    flightsim_render::terrain_mesh_bundle(
                        meshes.add(group.mesh),
                        materials.add(group.material),
                        origin,
                    ),
                    group.marker,
                    Name::new(format!("airport ground lights {rendered_light_groups}")),
                ));
                rendered_light_groups += 1;
            }
        }
        Ok(None) => {}
        Err(error) => warn!("airport lights: skipped ground-light layout ({error})"),
    }
    if matches!(startup.runway_source, RunwaySource::OpenStreetMap { .. }) {
        info!(
            "airport ground: {rendered_aprons} aprons, {rendered_taxiways} taxiways, \
             {rendered_markings} holding markings, {rendered_signs} signs, \
             {} ground lights in {rendered_light_groups} groups",
            airport_ground_lights.len()
        );
    }

    // 見た目も進入・評価と同じ滑走路、同じ DEM 標高へ置く。
    let visual_threshold = runway.threshold;
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
    // 滑走路灯。**夜に降りるには滑走路の側が光る必要がある。**
    // 太陽高度に応じて `update_airport_lights` が明るさを動かす。
    let (light_groups, light_origin) = flightsim_render::runway_lights::runway_light_meshes(
        visual_threshold,
        runway.heading,
        runway.length,
        runway.width,
    );
    for group in light_groups {
        commands.spawn((
            flightsim_render::terrain_mesh_bundle(
                meshes.add(group.mesh),
                materials.add(group.material),
                light_origin,
            ),
            group.marker,
            Name::new("runway lights"),
        ));
    }

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
    commands.insert_resource(start_condition);

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
///
/// 再生中は**操縦入力を見ない**。記録されたフレームをそのまま流す。
/// 操縦を混ぜると軌跡が記録から外れ、再生が別の飛行になる。
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy のシステム引数。分けると1フレームの進行が2箇所に散る"
)]
fn advance_simulation(
    time: Res<Time>,
    controls: Res<PilotControls>,
    paused: Res<flightsim_ui::Paused>,
    mut simulation: ResMut<FlightSimulation>,
    mut camera_position: ResMut<CameraWorldPosition>,
    mut recorder: ResMut<FlightRecorder>,
    playback: Option<ResMut<ReplayPlayback>>,
    mut aircraft: Query<(&mut WorldPosition, &mut WorldOrientation), With<Aircraft>>,
) {
    if paused.is_paused() {
        // **物理も記録も進めない。** 止めている間に記録が伸びると、
        // 再生したときに何もしていない時間が入る。
        return;
    }
    let frame_time = Seconds(f64::from(time.delta_secs()));
    let diverged = match playback {
        Some(mut playback) => {
            playback.player.accumulate(frame_time);
            let mut diverged = false;
            // 予算のぶんだけ記録を流す。速度を上げれば 1 描画フレームで
            // 複数フレーム進む。
            while let Some(frame) = playback.player.next_due() {
                let report = simulation.0.advance(frame.frame_time, frame.controls);
                playback.elapsed = Seconds(playback.elapsed.get() + frame.frame_time.get());
                playback.last_controls = frame.controls;
                if report.diverged {
                    diverged = true;
                    break;
                }
            }
            diverged
        }
        None => {
            let input = controls.to_control_inputs();
            // **進める前に記録する。** 後だと、そのフレームの入力に対応する
            // 状態が 1 フレームずれる。
            recorder
                .0
                .record(frame_time, input, Some(simulation.0.state()));
            simulation.0.advance(frame_time, input).diverged
        }
    };
    if diverged {
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
    playback: Option<Res<ReplayPlayback>>,
    mode: Res<ViewMode>,
    sun: Res<SunDirection>,
    mut hud: ResMut<HudState>,
) {
    // 再生中に手元の操縦桿を映すと、機体が加速しているのにスロットル 0% と
    // 出る。**表示は今飛んでいる機体のものでなければ意味がない。**
    let shown = playback.map_or_else(
        || (controls.throttle.value(), controls.flaps.value()),
        |playback| {
            (
                playback.last_controls.throttle(),
                playback.last_controls.flaps(),
            )
        },
    );
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
        throttle: shown.0,
        flaps: shown.1,
        // 脚の長さぶん余裕を見る。重心の対地高度なので接地時でも 1 m 前後ある。
        on_ground: agl.get() < flightsim_sim::gear_height(simulation.0.config()).get() + 0.3,
        terrain_available: ground.from_terrain,
        view_mode: mode.name(),
        wind_from: simulation.0.wind().from,
        wind_speed: simulation.0.wind().speed,
        // 計器の照明に使う。ui は render に依存できないので app が渡す。
        sun_elevation: sun.elevation,
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

    fn parse(args: &[&str]) -> (Startup, Vec<String>) {
        let (startup, diagnostics) =
            parse_arguments_from(args.iter().map(|argument| (*argument).to_owned()));
        (startup, diagnostics.0)
    }

    // --- 空港 DB と開始位置の CLI ---

    #[test]
    fn airport_database_and_explicit_start_are_recorded_separately() {
        let (startup, notes) = parse(&[
            "--airports",
            "data/tokyo.fsairports",
            "--start",
            "35.55,139.78",
            "--heading",
            "335",
        ]);

        assert_eq!(
            startup.airports.as_deref(),
            Some(std::path::Path::new("data/tokyo.fsairports"))
        );
        assert!(startup.start_was_explicit);
        assert!(startup.heading_was_explicit);
        assert!((startup.start.latitude_degrees() - 35.55).abs() < 1.0e-12);
        assert!((startup.start.longitude_degrees() - 139.78).abs() < 1.0e-12);
        assert!((startup.heading.to_degrees().get() - 335.0).abs() < 1.0e-12);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn airport_database_does_not_make_the_default_start_explicit() {
        let (startup, notes) = parse(&["--airports", "data/tokyo.fsairports"]);
        assert!(startup.airports.is_some());
        assert!(!startup.start_was_explicit);
        assert!(!startup.heading_was_explicit);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn optional_approach_value_does_not_consume_the_following_airport_option() {
        let (startup, notes) = parse(&["--approach", "--airports", "data/tokyo.fsairports"]);

        assert_eq!(startup.approach, Some(1.0));
        assert_eq!(
            startup.airports.as_deref(),
            Some(std::path::Path::new("data/tokyo.fsairports"))
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn malformed_approach_distance_is_reported_instead_of_enabling_one_mile() {
        for value in ["far", "0", "-1", "NaN", "inf"] {
            let (startup, notes) = parse(&["--approach", value]);

            assert_eq!(startup.approach, None, "{value}");
            assert_eq!(notes.len(), 1, "{value}: {notes:?}");
            assert!(notes[0].contains(value), "{value}: {notes:?}");
        }
    }

    #[test]
    fn a_missing_required_value_does_not_consume_the_following_option() {
        let (startup, notes) = parse(&["--heading", "--airports", "data/tokyo.fsairports"]);

        assert!(!startup.heading_was_explicit);
        assert_eq!(
            startup.airports.as_deref(),
            Some(std::path::Path::new("data/tokyo.fsairports"))
        );
        assert!(notes.iter().any(|note| note.contains("--heading needs")));
    }

    #[test]
    fn malformed_or_out_of_range_start_is_rejected_atomically() {
        for value in [
            "35,nope,139",
            "35",
            "35,139,8",
            "91,139",
            "35,181",
            "NaN,139",
            "35,inf",
        ] {
            let (startup, notes) = parse(&["--start", value]);
            assert!(!startup.start_was_explicit, "{value}");
            assert_eq!(
                startup.start,
                Runway::synthetic().takeoff_start(),
                "{value}"
            );
            assert_eq!(notes.len(), 1, "{value}: {notes:?}");
        }
    }

    #[test]
    fn missing_airport_path_and_non_finite_heading_are_reported() {
        let (startup, notes) = parse(&["--airports", "--heading", "NaN"]);
        assert!(startup.airports.is_none());
        assert!(!startup.heading_was_explicit);
        assert!(
            notes
                .iter()
                .any(|note| note.contains("needs a .fsairports"))
        );
        assert!(notes.iter().any(|note| note.contains("finite degrees")));

        let (startup, notes) = parse(&["--airports"]);
        assert!(startup.airports.is_none());
        assert!(
            notes
                .iter()
                .any(|note| note.contains("needs a .fsairports"))
        );

        let (startup, notes) = parse(&["--heading", "inf"]);
        assert!(!startup.heading_was_explicit);
        assert!(notes.iter().any(|note| note.contains("finite degrees")));
    }

    fn airport_database_for_app_tests() -> (AirportDatabase, flightsim_world::AirportRunway) {
        let near = flightsim_world::AirportRunway::from_endpoints(
            200,
            Geodetic::from_degrees(35.55, 139.78, 0.0),
            Geodetic::from_degrees(35.56, 139.78, 0.0),
            Meters(45.0),
        )
        .expect("valid Tokyo runway");
        let far = flightsim_world::AirportRunway::from_endpoints(
            100,
            Geodetic::from_degrees(0.0, 0.0, 0.0),
            Geodetic::from_degrees(0.01, 0.0, 0.0),
            Meters(30.0),
        )
        .expect("valid distant runway");
        let expected = near;
        let database = AirportDatabase::new(vec![far, near]).expect("valid airport database");
        (database, expected)
    }

    #[test]
    fn database_only_starts_on_the_nearest_runway_and_uses_its_heading() {
        let (database, expected) = airport_database_for_app_tests();
        let mut startup = Startup::default();

        assert_eq!(apply_nearest_airport(&mut startup, database), Some(200));
        assert_eq!(startup.runway, expected.runway);
        assert_eq!(startup.start, expected.runway.takeoff_start());
        assert_eq!(startup.heading, expected.runway.heading);
        assert_eq!(
            startup.runway_source,
            RunwaySource::OpenStreetMap { way_id: 200 }
        );
        assert!(startup.taxiways.is_empty());
    }

    #[test]
    fn only_taxiways_near_the_active_runway_are_kept() {
        let (_, selected) = airport_database_for_app_tests();
        let near = AirportTaxiway::from_points(
            300,
            vec![
                Geodetic::from_degrees(35.551, 139.779, 0.0),
                Geodetic::from_degrees(35.552, 139.780, 0.0),
            ],
            Meters(20.0),
        )
        .expect("valid nearby taxiway");
        let far = AirportTaxiway::from_points(
            400,
            vec![
                Geodetic::from_degrees(36.0, 140.0, 0.0),
                Geodetic::from_degrees(36.001, 140.0, 0.0),
            ],
            Meters(20.0),
        )
        .expect("valid distant taxiway");
        let database = AirportDatabase::with_taxiways(vec![selected], vec![far, near])
            .expect("valid airport database");
        let mut startup = Startup::default();

        assert_eq!(apply_nearest_airport(&mut startup, database), Some(200));
        assert_eq!(startup.taxiways.len(), 1);
        assert_eq!(startup.taxiways[0].source_way_id, 300);
    }

    #[test]
    fn taxiway_selection_includes_the_radius_boundary() {
        let (_, selected) = airport_database_for_app_tests();
        let centre = selected.runway.center();
        let taxiway = AirportTaxiway::from_points(
            300,
            vec![
                centre.offset_by(Meters(15_000.0), Meters::ZERO),
                centre.offset_by(Meters(15_010.0), Meters::ZERO),
            ],
            Meters(20.0),
        )
        .expect("valid boundary taxiway");
        let exact_radius = centre.to_ecef().distance_to(taxiway.points()[0].to_ecef());

        assert!(taxiway_is_near_runway(
            &taxiway,
            selected.runway,
            exact_radius
        ));
        assert!(!taxiway_is_near_runway(
            &taxiway,
            selected.runway,
            Meters(exact_radius.get() - 0.01)
        ));
    }

    #[test]
    fn a_long_taxiway_crossing_the_airport_is_selected_even_if_both_ends_are_outside() {
        let (_, selected) = airport_database_for_app_tests();
        let centre = selected.runway.center();
        let taxiway = AirportTaxiway::from_points(
            300,
            vec![
                centre.offset_by(Meters(-20_000.0), Meters::ZERO),
                centre.offset_by(Meters(20_000.0), Meters::ZERO),
            ],
            Meters(20.0),
        )
        .expect("valid crossing taxiway");

        assert!(taxiway.points().iter().all(|point| {
            selected
                .runway
                .center()
                .to_ecef()
                .distance_to(point.to_ecef())
                .get()
                > ACTIVE_AIRPORT_RADIUS.get()
        }));
        assert!(taxiway_is_near_runway(
            &taxiway,
            selected.runway,
            ACTIVE_AIRPORT_RADIUS
        ));
    }

    #[test]
    fn an_apron_covering_the_airport_is_selected_even_if_every_vertex_is_outside() {
        let (_, selected) = airport_database_for_app_tests();
        let centre = selected.runway.center();
        let triangle = [
            centre.offset_by(Meters(-16_000.0), Meters(-16_000.0)),
            centre.offset_by(Meters(-16_000.0), Meters(16_000.0)),
            centre.offset_by(Meters(16_000.0), Meters::ZERO),
        ];
        let apron = AirportApron::new(
            flightsim_world::AirportSourceKind::Way,
            500,
            flightsim_world::AirportSurface::Concrete,
            vec![triangle],
        )
        .expect("valid crossing apron");

        assert!(triangle.iter().all(|point| {
            centre.to_ecef().distance_to(point.to_ecef()).get() > ACTIVE_AIRPORT_RADIUS.get()
        }));
        assert!(apron_is_near_runway(
            &apron,
            selected.runway,
            ACTIVE_AIRPORT_RADIUS
        ));
    }

    #[test]
    fn ground_features_are_filtered_with_the_selected_airport() {
        let (_, selected) = airport_database_for_app_tests();
        let centre = selected.runway.center();
        let near_apron = AirportApron::new(
            flightsim_world::AirportSourceKind::Way,
            500,
            flightsim_world::AirportSurface::Asphalt,
            vec![[
                centre,
                centre.offset_by(Meters(10.0), Meters::ZERO),
                centre.offset_by(Meters::ZERO, Meters(10.0)),
            ]],
        )
        .expect("valid nearby apron");
        let far_origin = Geodetic::from_degrees(36.0, 140.0, 0.0);
        let far_apron = AirportApron::new(
            flightsim_world::AirportSourceKind::Way,
            501,
            flightsim_world::AirportSurface::Asphalt,
            vec![[
                far_origin,
                far_origin.offset_by(Meters(10.0), Meters::ZERO),
                far_origin.offset_by(Meters::ZERO, Meters(10.0)),
            ]],
        )
        .expect("valid distant apron");
        let near_holding = AirportHoldingPosition::new(
            flightsim_world::AirportSourceKind::Node,
            600,
            centre,
            flightsim_world::HoldingPositionType::Runway,
            Radians::ZERO,
            Meters(20.0),
            Some("A".to_owned()),
            None,
            flightsim_world::RunwaySide::Forward,
        )
        .expect("valid nearby holding position");
        let far_holding = AirportHoldingPosition::new(
            flightsim_world::AirportSourceKind::Node,
            601,
            far_origin,
            flightsim_world::HoldingPositionType::Runway,
            Radians::ZERO,
            Meters(20.0),
            None,
            None,
            flightsim_world::RunwaySide::Forward,
        )
        .expect("valid distant holding position");
        let near_light = AirportGroundLight::new(
            flightsim_world::AirportSourceKind::Node,
            700,
            centre,
            flightsim_world::GroundLightKind::RunwayGuard,
        )
        .expect("valid nearby light");
        let far_light = AirportGroundLight::new(
            flightsim_world::AirportSourceKind::Node,
            701,
            far_origin,
            flightsim_world::GroundLightKind::TaxiwayEdge,
        )
        .expect("valid distant light");
        let database = AirportDatabase::with_ground_features(
            vec![selected],
            Vec::new(),
            vec![far_apron, near_apron],
            vec![far_holding, near_holding],
            vec![far_light, near_light],
        )
        .expect("valid ground-feature database");
        let mut startup = Startup::default();

        assert_eq!(apply_nearest_airport(&mut startup, database), Some(200));
        assert_eq!(startup.aprons.len(), 1);
        assert_eq!(startup.aprons[0].source_id(), 500);
        assert_eq!(startup.holding_positions.len(), 1);
        assert_eq!(startup.holding_positions[0].source_id(), 600);
        assert_eq!(startup.ground_lights.len(), 1);
        assert_eq!(startup.ground_lights[0].source_id(), 700);
    }

    #[test]
    fn ground_light_accumulator_deduplicates_same_kind_by_ecef_distance() {
        let origin = Geodetic::from_degrees(35.55, 139.78, 6.0);
        let within = origin.offset_by(Meters(0.1), Meters::ZERO);
        let outside = origin.offset_by(Meters(0.3), Meters::ZERO);
        let mut accumulator = GroundLightAccumulator::try_new(4).expect("small allocation");

        assert_eq!(
            accumulator.insert(origin, GroundLightKind::TaxiwayEdge),
            GroundLightInsert::Inserted
        );
        assert_eq!(
            accumulator.insert(within, GroundLightKind::TaxiwayEdge),
            GroundLightInsert::Duplicate
        );
        assert_eq!(
            accumulator.insert(origin, GroundLightKind::TaxiwayCenterline),
            GroundLightInsert::Inserted
        );
        assert_eq!(
            accumulator.insert(outside, GroundLightKind::TaxiwayEdge),
            GroundLightInsert::Inserted
        );
        assert_eq!(accumulator.len(), 3);
        assert_eq!(
            accumulator.into_lights()[0],
            (origin, GroundLightKind::TaxiwayEdge),
            "先に登録した明示灯相当の点を残す"
        );
    }

    #[test]
    fn duplicate_ground_light_does_not_consume_unique_capacity() {
        let origin = Geodetic::from_degrees(35.55, 139.78, 6.0);
        let within = origin.offset_by(Meters::ZERO, Meters(0.1));
        let mut accumulator = GroundLightAccumulator::try_new(2).expect("small allocation");

        assert_eq!(
            accumulator.insert(origin, GroundLightKind::TaxiwayEdge),
            GroundLightInsert::Inserted
        );
        assert_eq!(
            accumulator.insert(within, GroundLightKind::TaxiwayEdge),
            GroundLightInsert::Duplicate
        );
        assert_eq!(
            accumulator.insert(origin, GroundLightKind::TaxiwayCenterline),
            GroundLightInsert::Inserted
        );
        assert_eq!(
            accumulator.insert(origin, GroundLightKind::RunwayGuard),
            GroundLightInsert::AtCapacity
        );
        assert_eq!(accumulator.len(), 2);
    }

    #[test]
    fn taxiway_surface_point_limit_is_checked_before_dem_expansion() {
        assert!(!valid_taxiway_surface_point_count(0));
        assert!(!valid_taxiway_surface_point_count(1));
        assert!(valid_taxiway_surface_point_count(2));
        assert!(valid_taxiway_surface_point_count(
            MAX_TAXIWAY_SURFACE_POINTS
        ));
        assert!(!valid_taxiway_surface_point_count(
            MAX_TAXIWAY_SURFACE_POINTS + 1
        ));
    }

    #[test]
    fn holding_sign_approach_heading_follows_the_runway_side() {
        let heading = Degrees(10.0).to_radians();
        let forward = holding_approach_heading(heading, RunwaySide::Forward)
            .expect("forward side has a known approach");
        let backward = holding_approach_heading(heading, RunwaySide::Backward)
            .expect("backward side has a known approach");

        assert!(
            forward
                .shortest_difference_to(Degrees(10.0).to_radians())
                .get()
                .abs()
                < 1.0e-12
        );
        assert!(
            backward
                .shortest_difference_to(Degrees(190.0).to_radians())
                .get()
                .abs()
                < 1.0e-12
        );
        assert!(holding_approach_heading(heading, RunwaySide::Unknown).is_none());
    }

    #[test]
    fn explicit_start_and_heading_survive_nearest_runway_selection() {
        let (database, expected) = airport_database_for_app_tests();
        let (mut startup, notes) = parse(&["--start", "35.555,139.781", "--heading", "123"]);
        assert!(notes.is_empty(), "{notes:?}");
        let explicit_start = startup.start;
        let explicit_heading = startup.heading;

        assert_eq!(apply_nearest_airport(&mut startup, database), Some(200));
        assert_eq!(startup.runway, expected.runway);
        assert_eq!(startup.start, explicit_start);
        assert_eq!(startup.heading, explicit_heading);
    }

    #[test]
    fn explicit_start_is_the_query_for_nearest_runway_selection() {
        let (database, _) = airport_database_for_app_tests();
        let (mut startup, notes) = parse(&["--start", "0.005,0"]);
        assert!(notes.is_empty(), "{notes:?}");

        assert_eq!(apply_nearest_airport(&mut startup, database), Some(100));
        assert_eq!(
            startup.runway_source,
            RunwaySource::OpenStreetMap { way_id: 100 }
        );
    }

    #[test]
    fn empty_database_keeps_the_synthetic_runway() {
        let database = AirportDatabase::new(Vec::new()).expect("empty databases are valid");
        let mut startup = Startup::default();
        let original = startup.clone();

        assert_eq!(apply_nearest_airport(&mut startup, database), None);
        assert_eq!(startup.runway, original.runway);
        assert_eq!(startup.start, original.start);
        assert_eq!(startup.heading, original.heading);
        assert_eq!(startup.runway_source, RunwaySource::Synthetic);
    }

    // --- 雲の CLI ---

    #[test]
    fn clouds_default_to_clear_without_changing_the_existing_scene() {
        let (startup, notes) = parse(&[]);
        assert_eq!(startup.clouds.cover.to_bits(), 0.0_f32.to_bits());
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn cloud_cover_base_top_and_visibility_can_be_set_in_any_order() {
        let (startup, notes) = parse(&[
            "--cloud-top",
            "1700",
            "--cloud-visibility",
            "250",
            "--cloud-cover",
            "0.65",
            "--cloud-base",
            "900",
        ]);

        assert_eq!(startup.clouds.cover.to_bits(), 0.65_f32.to_bits());
        assert_eq!(startup.clouds.base, Meters(900.0));
        assert_eq!(startup.clouds.top, Meters(1_700.0));
        assert_eq!(startup.clouds.visibility, Meters(250.0));
        assert_eq!(startup.clouds.seed, 1);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn both_cloud_cover_boundaries_are_accepted() {
        for cover in ["0", "1"] {
            let (startup, notes) = parse(&["--cloud-cover", cover]);
            assert_eq!(
                startup.clouds.cover.to_bits(),
                cover.parse::<f32>().unwrap().to_bits()
            );
            assert!(notes.is_empty(), "{cover}: {notes:?}");
        }
    }

    #[test]
    fn invalid_cloud_layers_fall_back_to_clear_and_explain_why() {
        for args in [
            &["--cloud-cover", "1.01"][..],
            &["--cloud-cover", "NaN"][..],
            &["--cloud-base", "1700", "--cloud-top", "900"][..],
            &["--cloud-base", "-1"][..],
            &["--cloud-visibility", "0"][..],
        ] {
            let (startup, notes) = parse(args);
            assert_eq!(
                startup.clouds.cover.to_bits(),
                CloudLayer::default().cover.to_bits(),
                "{args:?}"
            );
            assert!(
                notes
                    .iter()
                    .any(|note| note.contains("invalid cloud layer")),
                "{args:?}: {notes:?}"
            );
        }
    }

    #[test]
    fn malformed_cloud_arguments_make_the_whole_layer_clear() {
        for args in [
            &["--cloud-cover", "0.5", "--cloud-base", "nope"][..],
            &["--cloud-cover", "0.5", "--cloud-visibility"][..],
        ] {
            let (startup, notes) = parse(args);
            assert_eq!(
                startup.clouds.cover.to_bits(),
                CloudLayer::default().cover.to_bits(),
                "{args:?}"
            );
            assert!(
                notes
                    .iter()
                    .any(|note| note.contains("using the clear default")),
                "{args:?}: {notes:?}"
            );
        }
    }

    // --- リプレイ ---

    #[test]
    fn the_replay_flag_takes_a_path() {
        let (startup, notes) = parse(&["--replay", "flight-001.fsreplay"]);
        assert_eq!(startup.replay, Some(PathBuf::from("flight-001.fsreplay")));
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_replay_flag_without_a_path_is_reported() {
        // 値を食べずに次の option を飲み込むと、そちらが黙って消える。
        let (startup, notes) = parse(&["--replay", "--difficulty", "beginner"]);
        assert!(startup.replay.is_none());
        assert_eq!(startup.difficulty, Difficulty::Beginner);
        assert!(
            notes.iter().any(|note| note.contains("--replay")),
            "{notes:?}"
        );
    }

    #[test]
    fn without_the_flag_nothing_is_replayed() {
        assert!(Startup::default().replay.is_none());
    }

    #[test]
    fn the_recorded_conditions_describe_this_flight() {
        // 条件が抜けると、同じ入力を流しても別の飛行になる。
        let mut startup = Startup {
            heading: Degrees(210.0).to_radians(),
            time_rate: 4.0,
            ..Startup::default()
        };
        startup.start = Geodetic::from_degrees(35.55, 139.78, 12.0);
        startup.wind = flightsim_sim::Wind {
            from: Degrees(270.0).to_radians(),
            speed: flightsim_core::Knots(10.0).to_meters_per_second(),
        };
        startup.turbulence = flightsim_fdm::Turbulence::moderate(3);

        let clock = flightsim_render::TimeOfDay::default();
        let conditions = recording_conditions(&startup, &clock);

        assert_eq!(conditions.start, startup.start);
        assert_eq!(conditions.heading, startup.heading);
        assert_eq!(conditions.wind, startup.wind);
        assert_eq!(conditions.turbulence, startup.turbulence);
        // 暦上の一点をそのまま持つ。**丸めると日付をまたぐ瞬間がずれる。**
        assert!(
            (conditions.start_epoch - clock.utc.get()).abs() < f64::EPSILON,
            "the recorded epoch must be the clock's epoch"
        );
        assert!(
            conditions.aircraft_fingerprint != 0,
            "the aircraft must be identified, otherwise any aircraft would replay it"
        );
        // 同じ機体で記録した飛行は、同じ機体で再生できること。
        flightsim_sim::Recorder::new(conditions)
            .finish()
            .check_reproducible_with(&AircraftConfig::light_single())
            .expect("the recording must be reproducible with the aircraft it names");
    }

    // --- 難易度 ---

    fn startup_at(difficulty: Difficulty) -> Startup {
        Startup {
            difficulty,
            ..Startup::default()
        }
    }

    #[test]
    fn difficulty_names_are_parsed_including_common_aliases() {
        assert_eq!(Difficulty::parse("beginner"), Some(Difficulty::Beginner));
        assert_eq!(Difficulty::parse("easy"), Some(Difficulty::Beginner));
        assert_eq!(Difficulty::parse("normal"), Some(Difficulty::Normal));
        assert_eq!(Difficulty::parse("realistic"), Some(Difficulty::Realistic));
        assert_eq!(Difficulty::parse("hard"), Some(Difficulty::Realistic));
        // 大文字と余分な空白も受ける。
        assert_eq!(Difficulty::parse("  NORMAL "), Some(Difficulty::Normal));
        assert_eq!(Difficulty::parse("insane"), None);
    }

    #[test]
    fn the_difficulty_ladder_gets_harder_in_one_direction() {
        // 段が入れ替わっていると、上げたのに楽になる。
        let calm = Difficulty::Beginner.default_turbulence().intensity.get();
        let light = Difficulty::Normal.default_turbulence().intensity.get();
        let moderate = Difficulty::Realistic.default_turbulence().intensity.get();
        assert!(
            calm < light && light < moderate,
            "turbulence should increase with difficulty: {calm} / {light} / {moderate}"
        );

        let heading = Degrees(50.0).to_radians();
        assert!(
            Difficulty::Beginner.default_wind(heading).speed.get() < 0.01,
            "beginners should start in calm air"
        );
        assert!(
            Difficulty::Realistic.default_wind(heading).speed.get() > 5.0,
            "the realistic preset should actually blow"
        );
    }

    #[test]
    fn the_realistic_wind_is_neither_head_on_nor_straight_across() {
        // **真横だと着陸できず、真正面だと横風の練習にならない。**
        let heading = Degrees(50.0).to_radians();
        let wind = Difficulty::Realistic.default_wind(heading);
        let mut offset = (wind.from.get() - heading.get()).to_degrees() % 360.0;
        if offset > 180.0 {
            offset -= 360.0;
        }
        assert!(
            (20.0..=70.0).contains(&offset.abs()),
            "the crosswind component should be meaningful but landable, got {offset} deg"
        );
    }

    #[test]
    fn only_the_realistic_preset_hides_the_guidance() {
        assert!(Difficulty::Beginner.shows_tutorial());
        assert!(Difficulty::Normal.shows_tutorial());
        assert!(!Difficulty::Realistic.shows_tutorial());
    }

    #[test]
    fn difficulty_fills_in_the_defaults() {
        let mut startup = startup_at(Difficulty::Realistic);
        apply_difficulty(&mut startup);
        assert!(
            startup.wind.speed.get() > 5.0,
            "the realistic preset should set a wind"
        );
        assert!(startup.turbulence.intensity.get() > 0.0);
    }

    #[test]
    fn an_explicit_wind_survives_the_difficulty_preset() {
        // **利用者の明示指定を打ち消さない。** ここが逆になると
        // `--wind` が黙って無視され、原因を掴めない。
        let mut startup = startup_at(Difficulty::Realistic);
        startup.wind = flightsim_sim::Wind {
            from: Degrees(180.0).to_radians(),
            speed: flightsim_core::Knots(3.0).to_meters_per_second(),
        };
        startup.wind_was_given = true;
        apply_difficulty(&mut startup);

        assert!(
            (startup.wind.from.to_degrees().get() - 180.0).abs() < 1e-6,
            "the explicit wind bearing was overwritten"
        );
        assert!(
            (startup.wind.speed.to_knots().get() - 3.0).abs() < 1e-6,
            "the explicit wind speed was overwritten"
        );
    }

    #[test]
    fn an_explicit_calm_survives_the_hardest_preset() {
        // `--turbulence calm` は「静かにしてくれ」という明示の意思。
        // 難易度が上書きすると、指定が効かない理由が分からない。
        let mut startup = startup_at(Difficulty::Realistic);
        startup.turbulence = flightsim_fdm::Turbulence::CALM;
        startup.turbulence_was_given = true;
        apply_difficulty(&mut startup);
        assert!(
            startup.turbulence.intensity.get() < 1e-9,
            "an explicit calm was overwritten by the difficulty preset"
        );
    }

    #[test]
    fn the_default_difficulty_is_playable_without_arguments() {
        // 引数なしで起動したときの姿。**いきなり横風は出さない。**
        let startup = Startup::default();
        assert_eq!(startup.difficulty, Difficulty::Normal);
        assert!(
            startup.difficulty.shows_tutorial(),
            "a first-time player needs the guidance"
        );
        assert!(
            startup.difficulty.default_wind(startup.heading).speed.get() < 0.01,
            "the default should not start with a crosswind"
        );
    }

    #[test]
    fn difficulty_names_are_ascii() {
        // ログと画面に出る。既定フォントに字形の無い記号を混ぜない。
        for level in [
            Difficulty::Beginner,
            Difficulty::Normal,
            Difficulty::Realistic,
        ] {
            assert!(level.name().is_ascii(), "{}", level.name());
        }
    }

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
