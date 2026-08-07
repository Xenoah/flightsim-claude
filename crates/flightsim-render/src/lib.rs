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
//!   3. LOD を選ぶ
//!   4. 予算内でタイルを読み、メッシュを作って spawn
//!   5. 選ばれなくなったタイルを despawn
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
pub mod model;
pub mod terrain;

pub use aircraft::{AircraftPart, placeholder_extents, placeholder_parts};
pub use model::{ModelAxis, ModelFit, ModelFitError};
pub use terrain::{TerrainRenderConfig, TerrainTiles};

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
            azimuth: flightsim_core::Degrees(200.0).to_radians(),
            elevation: flightsim_core::Degrees(45.0).to_radians(),
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
            .init_resource::<TerrainTiles>()
            .init_resource::<TerrainRenderConfig>()
            .configure_sets(
                Update,
                (RenderSet::Rebase, RenderSet::Transforms, RenderSet::Terrain).chain(),
            )
            .add_systems(
                Update,
                (
                    rebase_render_origin.in_set(RenderSet::Rebase),
                    apply_world_positions.in_set(RenderSet::Transforms),
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
        base_color: Color::srgb(0.36, 0.42, 0.30),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
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
