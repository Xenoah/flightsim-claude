//! 地形タイルを ECS の実体として出し入れする。
//!
//! 選択・予算管理そのものは [`crate::update_terrain_selection`] にあり、
//! Bevy に依存しない形でテストされている。ここはその結果を
//! `Commands` に反映するだけ。

use bevy::prelude::*;
use flightsim_core::Meters;
use flightsim_world::{TileId, build_mesh};
use std::collections::HashMap;

/// 地形描画の調整値。
#[derive(Resource, Debug, Clone, Copy)]
pub struct TerrainRenderConfig {
    /// 1 フレームで新たに読み込むタイル数の上限。
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
    entities: HashMap<TileId, Entity>,
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

    pub fn insert(&mut self, id: TileId, entity: Entity) {
        self.entities.insert(id, entity);
    }

    pub fn remove(&mut self, id: TileId) -> Option<Entity> {
        self.entities.remove(&id)
    }

    pub fn ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.entities.keys().copied()
    }
}

/// 地形タイルであることを示す印。デバッグ表示と一括削除に使う。
#[derive(Component, Debug, Clone, Copy)]
pub struct TerrainTile(pub TileId);

/// タイル 1 枚ぶんの実体を作る。
///
/// メッシュ生成は `flightsim-world::build_mesh`（純 Rust、テスト済み）。
/// ここは GPU 資産への登録と spawn だけ。
pub fn spawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    id: TileId,
    dem: &flightsim_world::DemTile,
) -> Entity {
    let source = build_mesh(id, dem, &crate::mesh_options_for(id.level));
    let handle = meshes.add(crate::to_bevy_mesh(&source));

    commands
        .spawn((
            crate::terrain_mesh_bundle(handle, material, source.origin),
            TerrainTile(id),
            Name::new(format!("terrain {}/{}/{}", id.level, id.x, id.y)),
        ))
        .id()
}

/// 描画対象から外れたタイルを片付ける。
///
/// **メッシュのハンドルも落とすこと。** 実体だけ消してハンドルを持ち続けると、
/// GPU メモリが解放されずに飛べば飛ぶほど増えていく。
pub fn despawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_query: &Query<&Mesh3d, With<TerrainTile>>,
    tiles: &mut TerrainTiles,
    id: TileId,
) {
    let Some(entity) = tiles.remove(id) else {
        return;
    };
    if let Ok(mesh) = mesh_query.get(entity) {
        meshes.remove(&mesh.0);
    }
    commands.entity(entity).despawn();
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
        tiles.insert(id, Entity::from_raw_u32(1).expect("valid entity id"));
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
