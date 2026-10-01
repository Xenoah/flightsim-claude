//! 地形タイルを ECS の実体として出し入れする。
//!
//! 選択・予算管理そのものは [`crate::update_terrain_selection`] にあり、
//! Bevy に依存しない形でテストされている。ここはその結果を
//! `Commands` に反映するだけ。

use bevy::prelude::*;
use flightsim_core::{Meters, RenderFrame};
use flightsim_world::{TileId, build_mesh};
use std::collections::HashMap;

/// 地形描画の調整値。
#[derive(Resource, Debug, Clone, Copy)]
pub struct TerrainRenderConfig {
    /// 1 フレームの読込試行数とメッシュ準備数それぞれの上限（欠落・失敗も含む）。
    ///
    /// **ここを無制限にすると、高速で飛んだ瞬間にスタッターになる。**
    /// タイル復号の実測は 65×65 で 10.5 µs（`cargo bench -p flightsim-world`）。
    /// 8 枚なら 84 µs、60 Hz フレーム予算の 0.5%。
    pub load_budget_per_frame: usize,
    /// 許容する screen-space error `px`。小さいほど細かく分割される。
    pub screen_space_error: f64,
    /// 探索する最も細かいタイルレベル。
    pub max_level: u8,
    /// level 0 タイルの幾何誤差。LOD 判定の基準になる。
    pub root_geometric_error: Meters,
    /// タイルキャッシュの容量。
    pub cache_bytes: usize,
}

impl Default for TerrainRenderConfig {
    fn default() -> Self {
        Self {
            load_budget_per_frame: 8,
            screen_space_error: 16.0,
            max_level: 13,
            root_geometric_error: Meters(20_000.0),
            cache_bytes: 512 * 1024 * 1024,
        }
    }
}

/// 現在 ECS に存在する地形タイル。
#[derive(Resource, Debug, Default)]
pub struct TerrainTiles {
    entities: HashMap<TileId, (Entity, Handle<Mesh>)>,
}

impl TerrainTiles {
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    #[must_use]
    pub fn contains(&self, id: TileId) -> bool {
        self.entities.contains_key(&id)
    }

    pub fn insert(&mut self, id: TileId, entity: Entity, mesh: Handle<Mesh>) {
        self.entities.insert(id, (entity, mesh));
    }

    #[must_use]
    pub fn entity(&self, id: TileId) -> Option<Entity> {
        self.entities.get(&id).map(|(entity, _)| *entity)
    }

    pub fn remove(&mut self, id: TileId) -> Option<(Entity, Handle<Mesh>)> {
        self.entities.remove(&id)
    }

    pub fn ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.entities.keys().copied()
    }
}

/// 地形タイルであることを示す印。デバッグ表示と一括削除に使う。
#[derive(Component, Debug, Clone, Copy)]
pub struct TerrainTile(pub TileId);

/// タイル 1 枚ぶんの実体を非表示で準備する。
///
/// メッシュ生成は `flightsim-world::build_mesh`（純 Rust、テスト済み）。
/// ここは GPU 資産への登録と spawn だけ。
pub fn spawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
) -> (Entity, Handle<Mesh>) {
    let source = build_mesh(id, dem, &crate::mesh_options_for(id.level));
    let handle = meshes.add(crate::to_bevy_mesh(&source));

    // Terrain runs after the regular Transforms set. Give a tile its correct
    // initial frame immediately, even if it becomes visible in this update.
    let transform = Transform {
        translation: frame.to_render(source.origin),
        rotation: frame.rotation_to_render(glam::DQuat::IDENTITY),
        ..default()
    };
    let entity = commands
        .spawn((
            crate::terrain_mesh_bundle(handle.clone(), material, source.origin),
            TerrainTile(id),
            Name::new(format!("terrain {}/{}/{}", id.level, id.x, id.y)),
        ))
        .insert((transform, Visibility::Hidden))
        .id();
    (entity, handle)
}

/// 描画対象から外れたタイルを片付ける。
///
/// **メッシュのハンドルも落とすこと。** 実体だけ消してハンドルを持ち続けると、
/// GPU メモリが解放されずに飛べば飛ぶほど増えていく。
pub fn despawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tiles: &mut TerrainTiles,
    id: TileId,
) {
    let Some((entity, mesh)) = tiles.remove(id) else {
        return;
    };
    // Keep the handle with the entity: a prepared mesh may become obsolete in
    // the same Commands batch, before a Query can see the new entity.
    meshes.remove(&mesh);
    commands.entity(entity).despawn();
}

/// Apply a complete visibility cut and release obsolete prepared meshes.
/// Run once after registering this update's prepared entities in `TerrainTiles`.
pub fn apply_terrain_update(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tiles: &mut TerrainTiles,
    update: crate::TerrainUpdate,
) {
    for id in update.hidden {
        if let Some(entity) = tiles.entity(id) {
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }
    for id in update.spawned {
        if let Some(entity) = tiles.entity(id) {
            commands.entity(entity).insert(Visibility::Inherited);
        }
    }
    for id in update.despawned {
        despawn_tile(commands, meshes, tiles, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_budget_matches_the_measured_decode_cost() {
        // 実測 10.5 µs/枚（65×65）。8 枚で 84 µs、60 Hz フレームの 0.5%。
        // ここを大きくするなら、先に cargo bench で測ること。
        let config = TerrainRenderConfig::default();
        let frame_budget_us = 1_000_000.0 / 60.0;
        #[allow(
            clippy::cast_precision_loss,
            reason = "予算枚数は高々数十。f64 で正確に表せる"
        )]
        let decode_us = config.load_budget_per_frame as f64 * 10.5;
        assert!(
            decode_us / frame_budget_us < 0.05,
            "the load budget would spend {:.1}% of a 60 Hz frame on decoding",
            decode_us / frame_budget_us * 100.0
        );
    }

    #[test]
    fn tiles_are_tracked_and_untracked() {
        let mut tiles = TerrainTiles::default();
        assert!(tiles.is_empty());

        let id = TileId::new(10, 1, 1);
        tiles.insert(
            id,
            Entity::from_raw_u32(1).expect("valid entity id"),
            Handle::default(),
        );
        assert!(tiles.contains(id));
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles.ids().collect::<Vec<_>>(), vec![id]);

        assert!(tiles.remove(id).is_some());
        assert!(tiles.is_empty());
        assert!(
            tiles.remove(id).is_none(),
            "removing twice should be a no-op"
        );
    }
}
