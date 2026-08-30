//! # flightsim-render
//!
//! 描画層。**Bevy に依存する最初のクレート。**
//!
//! ## 責務の境界
//!
//! - 地形メッシュの**データ**は `flightsim-world` が作る。ここは GPU に載せるだけ
//! - 地形と FDM の**結線**は `flightsim-sim` が持つ。ここでは再実装しない（ADR-0006）
//! - 世界座標の正は `f64` ECEF。`Transform` はそこから毎フレーム導出する派生値
//!   （[ADR-0002](https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0002-coordinate-system.md)）
//!
//! ## 描画フレームの流れ
//!
//! ```text
//!   1. カメラ位置から RenderFrame を打ち直す（必要なら）
//!   2. WorldPosition / WorldOrientation → Transform を更新
//!   3. 時刻を進め、太陽の位置と光量を決める
//!   4. 雲面と雲中視程を更新
//!   5. LOD を選ぶ
//!   6. 予算内でタイルを読み、メッシュを作って spawn
//!   7. 選ばれなくなったタイルを despawn
//! ```
//!
//! この手順は `flightsim-sim` の `tests/render_rehearsal.rs` が Bevy 抜きで
//! 検証している。挙動が食い違ったらそちらを先に疑うこと。
//!
//! ## 座標系
//!
//! 描画は [`RenderFrame`] のローカル接平面（X = 東、Y = 上、Z = 南）を使う。
//! ECEF 相対ではない。Bevy の大気散乱が `world_position.y` を海抜高度として
//! 読むため（ADR-0007）。
//!
//! ## 時刻と太陽
//!
//! [`TimeOfDay`] が唯一の入力で、[`SunDirection`] はそこからの派生値
//! （[`daylight`] と [`sun`]）。**壁時計時間は見ない。**
//!
//! 上位（`app`）がやることは 2 つだけ。
//!
//! 1. 太陽の光源を [`sun_light_bundle`] で spawn する
//!    （[`SunLight`] の印が付いていない光源は時刻に追随しない）
//! 2. 開始時刻を決めたければ [`TimeOfDay`] を `insert_resource` する。
//!    既定でも [`TimeOfDay::default`] が入る
//!
//! 絵を目で確かめるには `cargo run -p flightsim-render --example sun_clock`。

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, Meters, RenderFrame};
use flightsim_world::lod::distance_to_bounds;
use flightsim_world::{
    DemTile, LodSelector, MeshOptions, TerrainMesh, TileCache, TileId, TileSource,
};
use std::collections::HashMap;

pub mod aircraft;
pub mod daylight;
pub mod model;
pub mod runway;
pub mod runway_lights;
pub mod sun;
pub mod terrain;
pub mod weather;

pub use aircraft::{AircraftPart, placeholder_extents, placeholder_parts};
pub use daylight::{
    SunIlluminancePolicy, SunLight, SunLighting, TimeOfDay, TimeRate, direct_normal_illuminance,
};
pub use model::{ModelAxis, ModelFit, ModelFitError, extents_in_model_space};
pub use sun::{JulianDate, SolarPosition, UtcDateTime, solar_position};
pub use terrain::{TerrainRenderConfig, TerrainTiles};
pub use weather::{CloudDeckSurface, CloudDistanceFog, CloudLayer, CloudLayerError};

/// 世界座標での位置。**これが正であり、`Transform` は派生値。**
///
/// `f32` の `Transform` を位置の正にすると、地表で約 76cm に量子化して
/// 機体が振動する（ADR-0002）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct WorldPosition(pub Ecef);

/// 世界座標系での姿勢（機体軸 → ECEF）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct WorldOrientation(pub glam::DQuat);

/// 描画の基準となるローカル接平面。
#[derive(Resource, Debug, Clone, Copy)]
pub struct RenderOrigin(pub RenderFrame);

impl RenderOrigin {
    #[must_use]
    pub fn new(camera: Geodetic) -> Self {
        Self(RenderFrame::new(camera))
    }
}

/// 描画側から見たカメラの世界位置。`flightsim-input` が毎フレーム更新する。
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraWorldPosition(pub Geodetic);

impl Default for CameraWorldPosition {
    fn default() -> Self {
        Self(Geodetic::from_degrees(0.0, 0.0, 0.0))
    }
}

/// 太陽の向き（ローカル NED 基準の方位と仰角）。
///
/// **時刻から導かれる派生値。** [`daylight::update_sun_direction`] が
/// [`TimeOfDay`] とカメラ位置から毎フレーム計算して上書きする。
/// ここを直接書き換えても次のフレームで戻る。時刻のほうを動かすこと。
#[derive(Resource, Debug, Clone, Copy)]
pub struct SunDirection {
    /// 真方位。
    pub azimuth: flightsim_core::Radians,
    /// 仰角。水平が 0、天頂が π/2。
    pub elevation: flightsim_core::Radians,
}

impl Default for SunDirection {
    fn default() -> Self {
        Self {
            // 南南西・仰角 45°。地形の陰影が出やすい向き。
            // 最初のフレームで時刻から計算した値に置き換わる。
            azimuth: flightsim_core::Degrees(200.0).to_radians(),
            elevation: flightsim_core::Degrees(45.0).to_radians(),
        }
    }
}

impl From<SolarPosition> for SunDirection {
    fn from(position: SolarPosition) -> Self {
        Self {
            azimuth: position.azimuth,
            elevation: position.elevation,
        }
    }
}

/// 描画システムの実行順。
///
/// **順序を固定しないと 1 フレーム遅れが出る。** 打ち直しの直後に
/// `Transform` を作り直さないと、その 1 フレームだけ world が飛んで見える。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderSet {
    /// アンカーの打ち直し。
    Rebase,
    /// 世界座標 → `Transform`。
    Transforms,
    /// 時刻の進行と太陽の位置・光量。
    Sun,
    /// 雲面と雲中視程。
    Weather,
    /// 地形の LOD 選択・ストリーミング・spawn。
    Terrain,
}

/// 描画層のプラグイン。
#[derive(Debug, Default)]
pub struct FlightsimRenderPlugin;

impl Plugin for FlightsimRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraWorldPosition>()
            .init_resource::<SunDirection>()
            .init_resource::<TimeOfDay>()
            .init_resource::<SunLighting>()
            .init_resource::<CloudLayer>()
            .init_resource::<weather::CloudVisuals>()
            // `LightPlugin` も同じことをする。**どちらが先でも同じ値**になるよう
            // `init_resource` で入れること（`insert_resource` だと上書きし合う）。
            .init_resource::<bevy::light::GlobalAmbientLight>()
            .init_resource::<TerrainTiles>()
            .init_resource::<TerrainRenderConfig>()
            // **背景は黒でなければならない。**
            //
            // 大気散乱は「散乱光 + 背景 × 透過率」を書く。bevy の既定の背景は
            // sRGB (43, 44, 47) の暗い灰色で、これが透過率ごしに空へ滲む。
            // 実測すると、太陽をどこへ動かしても、照度を 0 にしてさえ、
            // 空の平均画素が (40.3, 35.3, 27.9) から動かなかった。
            // **夜が来ない原因がこれだった。** 散乱の計算ではなく背景の色。
            //
            // `init_resource` では駄目。`CameraPlugin` が既定値を先に入れるので、
            // ここは上書きする必要がある。
            .insert_resource(ClearColor(Color::BLACK))
            .configure_sets(
                Update,
                (
                    RenderSet::Rebase,
                    RenderSet::Transforms,
                    RenderSet::Sun,
                    RenderSet::Weather,
                    RenderSet::Terrain,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    rebase_render_origin.in_set(RenderSet::Rebase),
                    apply_world_positions.in_set(RenderSet::Transforms),
                    (
                        daylight::advance_time_of_day,
                        daylight::update_sun_direction,
                        daylight::apply_sun_light,
                    )
                        .chain()
                        .in_set(RenderSet::Sun),
                    (
                        weather::sync_cloud_visuals,
                        weather::update_cloud_visuals,
                        weather::update_cloud_distance_fog,
                    )
                        .chain()
                        .in_set(RenderSet::Weather),
                ),
            );
    }
}

/// カメラが閾値を越えたらアンカーを打ち直す。
///
/// 打ち直すと全オブジェクトの描画座標が変わるので、この直後に
/// [`apply_world_positions`] が走る必要がある（[`RenderSet`] で順序を固定）。
pub fn rebase_render_origin(mut origin: ResMut<RenderOrigin>, camera: Res<CameraWorldPosition>) {
    // 実際に打ち直したときだけ変更を通知する。毎フレーム ResMut を触ると
    // Changed<> による下流の再計算が無駄に走る。
    if origin.bypass_change_detection().0.needs_rebase(camera.0) {
        origin.0.rebase_if_needed(camera.0);
    }
}

/// `f64` 世界座標から `Transform` を作る。
///
/// **逆向きにしないこと。** `Transform` を編集して世界座標へ書き戻す設計にすると、
/// `f32` の量子化が世界座標に混入する。
pub fn apply_world_positions(
    origin: Res<RenderOrigin>,
    mut query: Query<(&WorldPosition, Option<&WorldOrientation>, &mut Transform)>,
) {
    for (position, orientation, mut transform) in &mut query {
        transform.translation = origin.0.to_render(position.0);
        if let Some(orientation) = orientation {
            transform.rotation = origin.0.rotation_to_render(orientation.0);
        }
    }
}

/// `TerrainMesh`（純データ）を Bevy の `Mesh` へ変換する。
///
/// ここが `flightsim-world` と GPU の境界。**メッシュ生成のロジックは
/// 向こう側にあり、ここは詰め替えるだけ。**
#[must_use]
pub fn to_bevy_mesh(source: &TerrainMesh) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, source.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, source.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, source.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, terrain_colors(source))
    .with_inserted_indices(Indices::U32(source.indices.clone()))
}

/// 太陽の向きを描画座標のベクトルへ。
///
/// `DirectionalLight` は「光が進む向き」を見るので、太陽の位置ではなく
/// **太陽から地表へ向かうベクトル**を返す。符号を間違えると影が逆になる。
#[must_use]
pub fn sun_light_direction(sun: SunDirection) -> Vec3 {
    let (azimuth, elevation) = (sun.azimuth.get(), sun.elevation.get());
    // ローカル NED 基準で太陽の方向（地表 → 太陽）。
    let north = elevation.cos() * azimuth.cos();
    let east = elevation.cos() * azimuth.sin();
    let up = elevation.sin();

    #[allow(
        clippy::cast_possible_truncation,
        reason = "単位ベクトルの成分は [-1, 1]。f32 で十分"
    )]
    // 描画座標は X = 東、Y = 上、Z = 南。光の進む向きは太陽方向の逆。
    let to_sun = Vec3::new(east as f32, up as f32, -north as f32);
    -to_sun.normalize()
}

/// 太陽の平行光源を spawn するときの構成要素。
///
/// **`SunLight` の印を必ず付けること。** 付いていない `DirectionalLight` は
/// [`daylight::apply_sun_light`] が触らないので、時刻を進めても向きが変わらない。
///
/// 影を落とすのは 1 つだけにすること。平行光源ごとにカスケードシャドウマップを
/// 持つので、増やすと素直に重くなる。
#[must_use]
pub fn sun_light_bundle(lighting: &SunLighting, sun: SunDirection) -> impl Bundle {
    (
        SunLight,
        DirectionalLight {
            illuminance: lighting.illuminance(sun.elevation),
            shadows_enabled: true,
            ..default()
        },
        Transform::default().looking_to(sun_light_direction(sun), Vec3::Y),
        Name::new("sun"),
    )
}

/// LOD 選択とストリーミングの結果。
#[derive(Debug, Clone, Default)]
pub struct TerrainUpdate {
    /// 今フレーム新たに描画対象になったタイル。
    pub spawned: Vec<TileId>,
    /// 描画対象から外れたタイル。
    pub despawned: Vec<TileId>,
    /// 今フレーム読み込んだタイル数。
    pub loaded: usize,
}

/// 描画対象のタイル集合を 1 フレームぶん更新する。
///
/// Bevy に依存しない純粋な処理として切り出してある。**テストできるようにするため。**
/// ECS への反映は `terrain` モジュールが行う。
///
/// # Errors
///
/// タイルの読み込みに失敗した場合。読めなかったタイルは描画対象から外れるだけで、
/// 飛行そのものは続く。
pub fn update_terrain_selection<S: TileSource>(
    selector: &LodSelector,
    source: &S,
    cache: &mut TileCache,
    live: &mut HashMap<TileId, ()>,
    camera: Ecef,
    load_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile),
) -> TerrainUpdate {
    let selection = selector.select(camera);
    let camera_geodetic = camera.to_geodetic();

    let mut wanted: Vec<TileId> = selection.tiles;
    // 近いタイルから処理する。予算が尽きても手前が優先される。
    wanted.sort_by(|a, b| {
        let da = distance_to_bounds(camera, camera_geodetic, a.bounds()).get();
        let db = distance_to_bounds(camera, camera_geodetic, b.bounds()).get();
        da.total_cmp(&db)
    });

    let mut update = TerrainUpdate::default();

    for id in &wanted {
        if live.contains_key(id) {
            continue;
        }
        if update.loaded >= load_budget {
            // 予算を超えたぶんは次フレームへ回す。ここを無制限にすると
            // 高速で飛んだ瞬間にスタッターになる。
            break;
        }
        if !cache.contains(*id) {
            match source.load(*id) {
                Ok(Some(tile)) => cache.insert(*id, tile),
                // 焼かれていないタイルは存在しないのが正常（海上など）。
                Ok(None) => continue,
                Err(_) => continue,
            }
            update.loaded += 1;
        }
        if let Some(tile) = cache.get(*id) {
            mesh_sink(*id, tile);
            live.insert(*id, ());
            update.spawned.push(*id);
        }
    }

    let keep: std::collections::HashSet<TileId> = wanted.into_iter().collect();
    live.retain(|id, ()| {
        let retained = keep.contains(id);
        if !retained {
            update.despawned.push(*id);
        }
        retained
    });

    update
}

/// タイルのメッシュ生成設定を LOD レベルから決める。
///
/// 深いタイルほど狭いので、同じ頂点数でも実効解像度は上がる。
/// レベルによらず一定でよい。
#[must_use]
pub fn mesh_options_for(_level: u8) -> MeshOptions {
    MeshOptions {
        resolution: 33,
        skirt_depth: None,
    }
}

/// 地形マテリアルの既定値。
///
/// テクスチャはまだ無いので、標高で色を変えることもしていない。
/// M3 で地表画像を載せるまでの繋ぎ。
#[must_use]
pub fn default_terrain_material() -> StandardMaterial {
    StandardMaterial {
        // **白にすること。** 頂点色は base_color に乗算される。色を残すと
        // 全体がその色に染まり、高度による塗り分けが潰れる。
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    }
}

/// 標高と傾斜から地表の色を決める。**返すのは sRGB。**
///
/// # これはテクスチャではない
///
/// **衛星画像ではない。** 高度と傾斜だけで塗り分けた、地形の起伏を読むための
/// 色。実際の土地被覆（市街地・森林・農地）は反映されない。
/// 画像を貼るのは、データ源とその権利を決めてからの別の作業。
///
/// # 塗り分けの根拠
///
/// 単色だと起伏が読めない。陰影だけでは太陽の向きに依存し、
/// **逆光では地形が真っ平らに見える。**
///
/// - 傾斜が急なところは岩肌。**高度によらず**先に効かせる。崖に草を生やさない
/// - 低地は緑、上がるにつれて褐色、さらに上は岩と雪
/// - 雪線は緯度で変えていない。**合成地形でしか確かめていないため**、
///   実データを通すまで根拠のない精緻化はしない
///
/// # 色空間
///
/// ここで返すのは **sRGB**。人が数値を見て色を思い浮かべられる空間で書く。
/// GPU へ渡すのは線形なので、[`terrain_colors`] が変換する。
///
/// **一度これを取り違えた。** 同じ数値を線形として渡したところ、同じ地点・
/// 同じ光で画面の色が (0.328, 0.347, 0.229) から (0.509, 0.521, 0.374) へ
/// 明るく浅くなった。目で見ただけでは「そんなものか」で済んでしまい、
/// **画素を測って初めて分かった。**
#[must_use]
pub fn terrain_color(elevation_metres: f32, slope_radians: f32) -> [f32; 4] {
    // 高度による基本色。境目は線形に混ぜる。
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [0.42, 0.50, 0.32]),    // 海岸の低地。やや黄みの緑
        (300.0, [0.34, 0.45, 0.26]),  // 平野から丘陵の緑
        (900.0, [0.45, 0.42, 0.30]),  // 森林限界あたり。褐色へ
        (1800.0, [0.52, 0.48, 0.44]), // 岩
        (2600.0, [0.92, 0.93, 0.95]), // 雪
    ];

    let elevation = if elevation_metres.is_finite() {
        elevation_metres
    } else {
        0.0
    };

    let mut base = STOPS[STOPS.len() - 1].1;
    if elevation <= STOPS[0].0 {
        base = STOPS[0].1;
    } else {
        for pair in STOPS.windows(2) {
            let (low_height, low_color) = pair[0];
            let (high_height, high_color) = pair[1];
            if elevation < high_height {
                let span = high_height - low_height;
                // STOPS は昇順の定数なので span > 0。0 除算は起きない。
                let t = ((elevation - low_height) / span).clamp(0.0, 1.0);
                base = [
                    low_color[0] + (high_color[0] - low_color[0]) * t,
                    low_color[1] + (high_color[1] - low_color[1]) * t,
                    low_color[2] + (high_color[2] - low_color[2]) * t,
                ];
                break;
            }
        }
    }

    // 急斜面は岩肌。25° から効き始め、45° で完全に岩になる。
    const ROCK: [f32; 3] = [0.46, 0.43, 0.40];
    let slope = if slope_radians.is_finite() {
        slope_radians
    } else {
        0.0
    };
    let rockiness = ((slope.to_degrees() - 25.0) / 20.0).clamp(0.0, 1.0);

    [
        base[0] + (ROCK[0] - base[0]) * rockiness,
        base[1] + (ROCK[1] - base[1]) * rockiness,
        base[2] + (ROCK[2] - base[2]) * rockiness,
        1.0,
    ]
}

/// メッシュの全頂点ぶんの色。**線形 RGB**。
///
/// `Mesh::ATTRIBUTE_COLOR` は線形として扱われる。[`terrain_color`] は
/// sRGB を返すので、ここで変換する。**この変換を飛ばすと色が明るく浅くなる。**
///
/// 標高や傾斜が足りないメッシュでも落とさず、無い頂点は 0 として扱う。
/// **頂点数と色の数がずれると描画が壊れる**ので、必ず `positions` に合わせる。
#[must_use]
pub fn terrain_colors(source: &TerrainMesh) -> Vec<[f32; 4]> {
    (0..source.positions.len())
        .map(|index| {
            let elevation = source.elevations.get(index).copied().unwrap_or(0.0);
            let slope = source.slopes.get(index).copied().unwrap_or(0.0);
            let color = terrain_color(elevation, slope);
            [
                srgb_to_linear(color[0]),
                srgb_to_linear(color[1]),
                srgb_to_linear(color[2]),
                color[3],
            ]
        })
        .collect()
}

/// sRGB の 1 成分を線形へ。
///
/// 単純な 2.2 乗ではなく、暗部の直線部を含む正しい変換を使う。
#[must_use]
pub fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.040_449_936 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// メッシュを spawn するときの標準的な構成要素。
#[must_use]
pub fn terrain_mesh_bundle(
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    origin: Ecef,
) -> impl Bundle {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material),
        WorldPosition(origin),
        // **回転を必ず付けること。**
        //
        // メッシュの頂点はタイル中心を原点とする **ECEF 軸** の相対位置だが、
        // 描画フレームは ENU 軸（X = 東、Y = 上、Z = 南）。単位回転を渡すと
        // `rotation_to_render` が ECEF → 描画 の回転になり、両者が揃う。
        //
        // これを省くと、タイルが緯度経度に応じた角度だけ傾く。地面が斜めに
        // 突き刺さった絵になり、**しかも赤道・本初子午線の近くでは正しく見える**ので
        // 気付きにくい。実際にこの間違いをして、地平線が斜めになった。
        WorldOrientation(glam::DQuat::IDENTITY),
        Transform::default(),
    )
}

/// 機体軸からカメラ軸への回転。
///
/// 機体軸は **前・右・下**（X, Y, Z）。Bevy のカメラは **-Z を向き、+Y が上**。
/// 対応は次のとおり。
///
/// ```text
///   機体 +X（前） → カメラ -Z（視線方向）
///   機体 +Y（右） → カメラ +X（右）
///   機体 +Z（下） → カメラ -Y（下）
/// ```
///
/// # 向きに注意
///
/// これは **カメラ軸 → 機体軸** の回転。`camera.rotation = aircraft.rotation * R`
/// と合成したときに、カメラの視線（ローカル -Z）が機体の機首になる向き。
///
/// 逆向き（機体軸 → カメラ軸）を作ると**画面が 90° 転がる**。実際にこの間違いをして、
/// 地平線が画面の真ん中に縦に立った。しかも
/// **テストの側も逆向きの性質を検証していたため通ってしまった。**
/// ここの検査は「合成した結果どこを向くか」で書くこと。
#[must_use]
pub fn body_to_camera_rotation() -> Quat {
    // 列は「カメラ軸 X, Y, Z が機体軸でどこを向くか」。
    Quat::from_mat3(&Mat3::from_cols(
        Vec3::new(0.0, 1.0, 0.0),  // カメラ右 → 機体右 (+Y)
        Vec3::new(0.0, 0.0, -1.0), // カメラ上 → 機体上 (-Z)
        Vec3::new(-1.0, 0.0, 0.0), // カメラ後 → 機体後 (-X)
    ))
}

/// 描画に使う既定のカメラ設定。
///
/// `Atmosphere` は HDR カメラを要求する（`#[require(AtmosphereSettings, Hdr)]`）。
#[must_use]
pub const fn default_far_plane() -> Meters {
    // 地平線まで見える距離。巡航高度 3 km で地平線は約 195 km。
    Meters(400_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Degrees, Radians};
    use flightsim_world::dem::HeightGrid;
    use flightsim_world::{MemoryTileSource, build_mesh};

    // --- 地表の色 ---

    /// 明るさ。塗り分けの向きを見るのに使う。
    fn luminance(color: [f32; 4]) -> f32 {
        color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722
    }

    #[test]
    fn high_ground_is_brighter_than_low_ground() {
        // 雪と岩は緑の低地より明るい。逆転していると起伏が読めなくなる。
        let low = terrain_color(50.0, 0.0);
        let high = terrain_color(3000.0, 0.0);
        assert!(
            luminance(high) > luminance(low) + 0.3,
            "3000 m ({high:?}) should read much brighter than 50 m ({low:?})"
        );
    }

    #[test]
    fn low_ground_is_the_greenest() {
        // 低地が緑でないと、平野と岩場の区別が付かない。
        let low = terrain_color(50.0, 0.0);
        assert!(
            low[1] > low[0] && low[1] > low[2],
            "the lowlands should be green, got {low:?}"
        );
    }

    #[test]
    fn the_colour_changes_gradually_with_height() {
        // 段差があると等高線のような縞が出る。
        let mut previous = terrain_color(0.0, 0.0);
        let mut step = 10.0_f32;
        while step <= 4000.0 {
            let current = terrain_color(step, 0.0);
            let jump = (0..3)
                .map(|channel| (current[channel] - previous[channel]).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                jump < 0.03,
                "the colour jumps by {jump:.3} at {step} m: {previous:?} -> {current:?}"
            );
            previous = current;
            step += 10.0;
        }
    }

    #[test]
    fn a_cliff_is_rock_regardless_of_how_low_it_is() {
        // **高度だけで塗ると崖に草が生える。** 傾斜を先に効かせること。
        let meadow = terrain_color(100.0, 0.0);
        let cliff = terrain_color(100.0, 60.0_f32.to_radians());
        assert!(
            cliff[0] > meadow[0] && cliff[1] < meadow[1],
            "a 60° slope at 100 m should read as rock, got {cliff:?} against {meadow:?}"
        );
    }

    #[test]
    fn a_gentle_slope_is_left_alone() {
        // 緩斜面まで岩にすると、平野が一様に灰色になる。
        let flat = terrain_color(200.0, 0.0);
        let gentle = terrain_color(200.0, 10.0_f32.to_radians());
        for channel in 0..3 {
            assert!(
                (flat[channel] - gentle[channel]).abs() < 1e-6,
                "a 10° slope should not be rocky yet: {gentle:?}"
            );
        }
    }

    #[test]
    fn the_colours_handed_to_the_gpu_are_linear_not_srgb() {
        // **一度これを取り違えて、同じ地点の色が明るく浅くなった。**
        // 目視では気付けず、画素を測って初めて分かった。
        assert!(
            (srgb_to_linear(0.5) - 0.214).abs() < 0.002,
            "sRGB 0.5 should be about 0.214 in linear, got {}",
            srgb_to_linear(0.5)
        );
        // 端は動かない。ここがずれると黒と白が濁る。
        assert!((srgb_to_linear(0.0)).abs() < 1e-9);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // 暗部は直線部を通る。2.2 乗で近似すると暗くなりすぎる。
        assert!(srgb_to_linear(0.02) > 0.02_f32.powf(2.2));
    }

    #[test]
    fn the_conversion_is_actually_applied_to_the_mesh() {
        // 変換を書いても呼び忘れれば同じこと。**外から確かめる。**
        let id = TileId::new(10, 500, 300);
        let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(640.0)));
        let source = build_mesh(id, &dem, &MeshOptions::default());
        let colors = terrain_colors(&source);
        assert_eq!(colors.len(), source.positions.len());

        let authored = terrain_color(source.elevations[0], source.slopes[0]);
        assert!(
            (colors[0][0] - srgb_to_linear(authored[0])).abs() < 1e-6,
            "the mesh colour {:?} is not the linear form of {authored:?}",
            colors[0]
        );
        assert!(
            colors.iter().all(|color| (color[3] - 1.0).abs() < 1e-6),
            "the terrain must stay opaque"
        );
    }

    #[test]
    fn every_colour_is_opaque_and_inside_the_unit_range() {
        // 範囲外の値は環境によって黒や白に飛ぶ。
        for elevation in [-500.0, 0.0, 1234.0, 9000.0, f32::NAN, f32::INFINITY] {
            for slope in [0.0, 0.5, 1.5, f32::NAN] {
                let color = terrain_color(elevation, slope);
                assert!(
                    color.iter().all(|value| value.is_finite()),
                    "terrain_color({elevation}, {slope}) = {color:?}"
                );
                assert!(
                    (0.0..=1.0).contains(&color[0])
                        && (0.0..=1.0).contains(&color[1])
                        && (0.0..=1.0).contains(&color[2]),
                    "terrain_color({elevation}, {slope}) = {color:?} is out of range"
                );
                assert!((color[3] - 1.0).abs() < 1e-6, "the terrain must be opaque");
            }
        }
    }

    #[test]
    fn a_mesh_survives_the_trip_into_bevy() {
        let id = TileId::new(10, 500, 300);
        let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(200.0)));
        let source = build_mesh(id, &dem, &MeshOptions::default());
        let mesh = to_bevy_mesh(&source);

        assert_eq!(mesh.count_vertices(), source.positions.len());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        assert_eq!(
            mesh.indices().map(bevy::mesh::Indices::len),
            Some(source.indices.len())
        );
    }

    #[test]
    fn the_sun_light_points_away_from_the_sun() {
        // 符号を間違えると影が逆になり、地形の凹凸が反転して見える。
        let overhead = SunDirection {
            azimuth: Radians::ZERO,
            elevation: Degrees(90.0).to_radians(),
        };
        let direction = sun_light_direction(overhead);
        assert!(
            direction.dot(Vec3::NEG_Y) > 0.99,
            "an overhead sun should light downward, got {direction:?}"
        );

        let eastern = SunDirection {
            azimuth: Degrees(90.0).to_radians(),
            elevation: Radians::ZERO,
        };
        let direction = sun_light_direction(eastern);
        assert!(
            direction.dot(Vec3::NEG_X) > 0.99,
            "a sun in the east should light westward, got {direction:?}"
        );
    }

    #[test]
    fn the_sun_direction_is_always_a_unit_vector() {
        for elevation in [-89.0, 0.0, 30.0, 89.9] {
            for azimuth in [0.0, 90.0, 180.0, 359.0] {
                let direction = sun_light_direction(SunDirection {
                    azimuth: Degrees(azimuth).to_radians(),
                    elevation: Degrees(elevation).to_radians(),
                });
                assert!(
                    (direction.length() - 1.0).abs() < 1e-5,
                    "azimuth {azimuth} elevation {elevation} gave length {}",
                    direction.length()
                );
            }
        }
    }

    #[test]
    fn the_camera_looks_where_the_aircraft_points() {
        // 前方を向き、上が上であること。ここを間違えると地平線が縦になる。
        let rotation = body_to_camera_rotation();

        // 検査は「`aircraft.rotation * R` と合成したときにカメラがどこを向くか」で書く。
        // 逆向きの性質を検査すると、転置を渡しても通ってしまう（実際に踏んだ）。
        //
        // カメラの視線はローカル -Z。それが機体の機首（+X）を向くこと。
        let view = rotation * Vec3::NEG_Z;
        assert!(
            view.dot(Vec3::X) > 0.999,
            "the camera looks along {view:?} in body axes instead of the nose (+X)"
        );
        // カメラの上はローカル +Y。それが機体の上（-Z）を向くこと。
        let up = rotation * Vec3::Y;
        assert!(
            up.dot(Vec3::NEG_Z) > 0.999,
            "the camera's up maps to {up:?} instead of the aircraft's up (-Z); \
             the horizon will be vertical"
        );
        // カメラの右はローカル +X。それが機体の右（+Y）を向くこと。
        let right = rotation * Vec3::X;
        assert!(
            right.dot(Vec3::Y) > 0.999,
            "the camera's right maps to {right:?} instead of the aircraft's right (+Y)"
        );
    }

    #[test]
    fn the_camera_rotation_is_a_pure_rotation() {
        let rotation = body_to_camera_rotation();
        assert!((rotation.length() - 1.0).abs() < 1e-6);
        // 鏡像でないこと。行列式が -1 だと左右が反転する。
        let matrix = Mat3::from_quat(rotation);
        assert!(
            matrix.determinant() > 0.999,
            "the mapping is a reflection (determinant {})",
            matrix.determinant()
        );
    }

    #[test]
    fn the_streaming_budget_is_respected() {
        // ここを無制限にすると、実機で必ずスタッターになる。
        let selector = LodSelector::new(
            16.0,
            1_080.0,
            Degrees(60.0).to_radians(),
            10,
            Meters(20_000.0),
        );
        let camera = Geodetic::from_degrees(35.553, 139.781, 2_000.0).to_ecef();

        // 選ばれるタイルを全部持っている供給元を用意する。
        let mut source = MemoryTileSource::new();
        for id in selector.select(camera).tiles {
            source.insert(
                id,
                DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(100.0))),
            );
        }

        let mut cache = TileCache::new(256 * 1024 * 1024);
        let mut live = HashMap::new();
        let budget = 4;

        for _ in 0..3 {
            let mut meshed = 0_usize;
            let update = update_terrain_selection(
                &selector,
                &source,
                &mut cache,
                &mut live,
                camera,
                budget,
                &mut |_, _| meshed += 1,
            );
            assert!(
                update.loaded <= budget,
                "loaded {} tiles with a budget of {budget}",
                update.loaded
            );
            assert!(meshed <= budget);
        }
    }

    #[test]
    fn tiles_that_leave_the_selection_are_despawned() {
        let selector = LodSelector::new(
            16.0,
            1_080.0,
            Degrees(60.0).to_radians(),
            8,
            Meters(20_000.0),
        );
        let near = Geodetic::from_degrees(35.553, 139.781, 1_000.0).to_ecef();
        let far = Geodetic::from_degrees(-33.4, -70.7, 1_000.0).to_ecef();

        let mut source = MemoryTileSource::new();
        for camera in [near, far] {
            for id in selector.select(camera).tiles {
                source.insert(
                    id,
                    DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(0.0))),
                );
            }
        }

        let mut cache = TileCache::new(256 * 1024 * 1024);
        let mut live = HashMap::new();

        // 十分な予算で東京周辺を埋める。
        for _ in 0..40 {
            update_terrain_selection(
                &selector,
                &source,
                &mut cache,
                &mut live,
                near,
                64,
                &mut |_, _| {},
            );
        }
        assert!(!live.is_empty(), "nothing was spawned around Tokyo");

        // 地球の反対側へ飛ぶと、元のタイルは対象から外れる。
        let update = update_terrain_selection(
            &selector,
            &source,
            &mut cache,
            &mut live,
            far,
            64,
            &mut |_, _| {},
        );
        assert!(
            !update.despawned.is_empty(),
            "moving to the far side despawned nothing"
        );
    }

    #[test]
    fn a_missing_tile_does_not_stop_the_others() {
        // 焼かれていないタイル（海上）は普通にある。
        let selector = LodSelector::new(
            16.0,
            1_080.0,
            Degrees(60.0).to_radians(),
            8,
            Meters(20_000.0),
        );
        let camera = Geodetic::from_degrees(35.553, 139.781, 5_000.0).to_ecef();

        // 半分だけ用意する。
        let mut source = MemoryTileSource::new();
        for (index, id) in selector.select(camera).tiles.into_iter().enumerate() {
            if index % 2 == 0 {
                source.insert(
                    id,
                    DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(0.0))),
                );
            }
        }

        let mut cache = TileCache::new(64 * 1024 * 1024);
        let mut live = HashMap::new();
        let mut meshed = 0_usize;
        for _ in 0..20 {
            update_terrain_selection(
                &selector,
                &source,
                &mut cache,
                &mut live,
                camera,
                64,
                &mut |_, _| meshed += 1,
            );
        }
        assert!(meshed > 0, "no tile was meshed at all");
    }
}
