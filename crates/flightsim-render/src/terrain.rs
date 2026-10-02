//! 地形タイルを ECS の実体として出し入れする。
//!
//! 選択・予算管理そのものは [`crate::update_terrain_selection`] にあり、
//! Bevy に依存しない形でテストされている。ここはその結果を
//! `Commands` に反映するだけ。

use bevy::{pbr::Material, prelude::*};
use flightsim_core::{Geodetic, Meters, Radians, RenderFrame};
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
    pub(crate) stitching: crate::terrain_stitching::TerrainStitching,
    pub(crate) overlays: crate::terrain_overlays::TerrainOverlays,
}

impl TerrainTiles {
    /// Register an immutable airport footprint for exact, atomic terrain draping.
    /// The entity and handle must already belong to this scene and be unique.
    ///
    /// # Errors
    /// Registration exceeds the scene bound or duplicates an existing target.
    pub fn register_overlay(
        &mut self,
        entity: Entity,
        mesh: Handle<Mesh>,
        source: crate::terrain_drape::TerrainOverlay,
    ) -> Result<(), crate::terrain_drape::DrapeError> {
        self.overlays.register(entity, mesh, source)
    }

    /// Stop managing one overlay before its owner despawns/removes its assets.
    /// Any in-flight result is cancelled; the next cut uses the updated registry.
    pub fn unregister_overlay(&mut self, entity: Entity) -> bool {
        self.overlays.unregister(entity)
    }

    /// Atomically admit a complete optional scenery cohort while preserving
    /// essential airport reservations. New entities must start hidden. Keep
    /// returned retired assets until the returned revision has committed.
    ///
    /// # Errors
    /// Invalid/duplicate targets, >24 optional meshes, >40,000 optional source
    /// triangles, or aggregate scene limits. Rejection changes nothing.
    pub fn replace_optional_overlays(
        &mut self,
        additions: Vec<crate::TerrainOverlayRegistration>,
    ) -> Result<crate::TerrainOverlaySwap, crate::terrain_drape::DrapeError> {
        self.overlays.replace_optional(additions)
    }

    /// Cancel optional scenery preparation and return both staged and retained
    /// old scenery assets for the owner to remove on relocation/reset. Essential
    /// airport registrations remain intact. No cancelled result can commit.
    pub fn clear_optional_overlays(&mut self) -> Vec<(Entity, Handle<Mesh>)> {
        self.overlays.clear_optional()
    }

    #[must_use]
    pub fn overlay_usage(&self) -> crate::TerrainOverlayUsage {
        self.overlays.usage()
    }

    pub(crate) fn mesh_handle(&self, id: TileId) -> Option<&Handle<Mesh>> {
        self.entities.get(&id).map(|(_, mesh)| mesh)
    }

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
        self.stitching.remove_boundary(id);
        self.entities.remove(&id)
    }

    /// Register a bounded hidden preparation. A replaced visible mesh stays
    /// alive until its replacement's bridges and visibility cut commit.
    pub fn insert_prepared(&mut self, prepared: PreparedTerrainTile) {
        if let Some(previous) = self
            .entities
            .insert(prepared.id, (prepared.entity, prepared.mesh))
        {
            self.stitching.retire(previous);
        }
        self.stitching.record_surface(
            prepared.entity,
            prepared.vertices,
            prepared.indices,
            prepared.origin,
            prepared.surface_vertices,
        );
        self.stitching
            .insert_boundary(prepared.id, prepared.boundary);
    }

    /// Selection must pause while bridges for a new cut are prepared. Advancing
    /// that transaction uses the same per-frame mesh budget as surface tiles.
    #[must_use]
    pub fn is_stitching(&self) -> bool {
        self.stitching.is_pending()
    }

    /// IDs actually displayed by the stitched path. During preparation these
    /// intentionally lag the selector's ready cut.
    pub fn displayed_ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.stitching.displayed_ids()
    }

    /// Logical mesh and compact-boundary storage owned by the stitched path.
    /// Includes both old and staged assets during an atomic transition.
    #[must_use]
    pub fn resource_usage(&self) -> crate::terrain_stitching::TerrainResourceUsage {
        self.stitching.resource_usage()
    }

    /// Remove every asset owned by this terrain scene, including old same-ID
    /// meshes and pending/visible bridges. The caller despawns returned entities
    /// and removes returned mesh handles; useful for an explicit new flight.
    pub fn drain_all(&mut self) -> Vec<(Entity, Handle<Mesh>)> {
        self.overlays.reset();
        let mut assets: Vec<_> = self.entities.drain().map(|(_, asset)| asset).collect();
        assets.extend(self.stitching.drain_all());
        assets
    }

    pub fn ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.entities.keys().copied()
    }
}

/// 地形タイルであることを示す印。デバッグ表示と一括削除に使う。
#[derive(Component, Debug, Clone, Copy)]
pub struct TerrainTile(pub TileId);

/// Hidden surface preparation plus its compact post-f32 boundary data.
#[derive(Debug)]
pub struct PreparedTerrainTile {
    pub id: TileId,
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub(crate) boundary: flightsim_world::seams::TerrainBoundary,
    vertices: usize,
    indices: usize,
    origin: flightsim_core::Ecef,
    surface_vertices: usize,
}

/// Build a surface and retain its actual rendered edge geometry for stitching.
/// The optional palette has the same contract as [`spawn_tile_colored`].
#[allow(
    clippy::too_many_arguments,
    reason = "surface preparation receives its rendering context and optional palette"
)]
pub fn prepare_tile<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> PreparedTerrainTile {
    prepare_tile_with_options(
        commands,
        meshes,
        material,
        frame,
        id,
        dem,
        &crate::mesh_options_for(id.level),
        color,
    )
}

/// Opt-in bounded tessellation for a regional terrain surface. The existing
/// [`prepare_tile`] wrapper retains its 33-point policy. Source samples, DEM
/// cache identity and physical terrain remain untouched; boundaries and overlay
/// copies retain the exact generated grid for the atomic terrain transaction.
///
/// # Panics
/// Resolution outside 2..=65. Callers must keep a tile's chosen options stable
/// for its source generation, rather than changing them with camera cadence.
#[allow(
    clippy::too_many_arguments,
    reason = "explicit bounded mesh options plus the existing preparation context"
)]
pub fn prepare_tile_with_options<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
    options: &flightsim_world::MeshOptions,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> PreparedTerrainTile {
    assert!(
        (2..=65).contains(&options.resolution),
        "runtime terrain resolution must be in 2..=65"
    );
    let source = build_mesh(id, dem, options);
    let boundary = flightsim_world::seams::TerrainBoundary::from_mesh(id, &source);
    let (entity, mesh) = spawn_source(commands, meshes, material, frame, id, &source, color);
    PreparedTerrainTile {
        id,
        entity,
        mesh,
        boundary,
        vertices: source.positions.len(),
        indices: source.indices.len(),
        origin: source.origin,
        surface_vertices: source.surface_vertex_count,
    }
}

/// タイル 1 枚ぶんの実体を非表示で準備する。
///
/// メッシュ生成は `flightsim-world::build_mesh`（純 Rust、テスト済み）。
/// ここは GPU 資産への登録と spawn だけ。
pub fn spawn_tile<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
) -> (Entity, Handle<Mesh>) {
    spawn_tile_impl(commands, meshes, material, frame, id, dem, None)
}

/// Prepare a terrain mesh whose surface uses an app-supplied geographic palette.
/// The callback returns linear RGBA, not sRGB. Its position is the actual DEM
/// surface vertex; source data and physics stay outside the renderer.
pub fn spawn_tile_colored<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
    color: &dyn Fn(Geodetic, Radians) -> [f32; 4],
) -> (Entity, Handle<Mesh>) {
    spawn_tile_impl(commands, meshes, material, frame, id, dem, Some(color))
}

fn spawn_tile_impl<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    dem: &flightsim_world::DemTile,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> (Entity, Handle<Mesh>) {
    let source = build_mesh(id, dem, &crate::mesh_options_for(id.level));
    spawn_source(commands, meshes, material, frame, id, &source, color)
}

fn spawn_source<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<M>,
    frame: &RenderFrame,
    id: TileId,
    source: &flightsim_world::TerrainMesh,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> (Entity, Handle<Mesh>) {
    let mut mesh = crate::to_bevy_mesh(source);
    if let Some(color) = color {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            terrain_vertex_colors(id, source, color),
        );
    }
    let handle = meshes.add(mesh);

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

/// Reconstruct geographic colour positions with closed polar endpoints. Binary
/// arithmetic can put a south-edge vertex one ulp below -PI/2 even though the
/// tile and UV are valid; that must not become an invalid global data query.
fn terrain_vertex_colors(
    id: TileId,
    source: &flightsim_world::TerrainMesh,
    color: &dyn Fn(Geodetic, Radians) -> [f32; 4],
) -> Vec<[f32; 4]> {
    let bounds = id.bounds();
    source
        .uvs
        .iter()
        .zip(&source.elevations)
        .zip(&source.slopes)
        .map(|((&[u, v], &height), &slope)| {
            let latitude = (bounds.north.get() - f64::from(v) * bounds.height().get())
                .clamp(-core::f64::consts::FRAC_PI_2, core::f64::consts::FRAC_PI_2);
            color(
                Geodetic::new(
                    Radians(latitude),
                    Radians(bounds.west.get() + f64::from(u) * bounds.width().get()),
                    Meters(f64::from(height)),
                ),
                Radians(f64::from(slope)),
            )
        })
        .collect()
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

    #[test]
    fn colored_mesh_queries_never_cross_a_closed_polar_endpoint() {
        for level in [0, 6, 10, 13] {
            for row in [0, (1_u32 << level) - 1] {
                let id = TileId::new(level, 0, row);
                let dem = flightsim_world::DemTile::new(
                    id.bounds(),
                    flightsim_world::HeightGrid::flat(33, 33, Meters(-30.0)),
                );
                let mesh = build_mesh(id, &dem, &crate::mesh_options_for(level));
                let colors = terrain_vertex_colors(id, &mesh, &|position, _| {
                    assert!(
                        (-core::f64::consts::FRAC_PI_2..=core::f64::consts::FRAC_PI_2)
                            .contains(&position.latitude.get())
                    );
                    assert!(position.longitude.is_finite() && position.altitude.is_finite());
                    [0.1, 0.2, 0.3, 1.0]
                });
                assert_eq!(colors.len(), mesh.positions.len());
            }
        }
    }
    #[test]
    fn opt_in_mesh_options_preserve_the_legacy_wrapper_and_bound_regional_grids() {
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let id = TileId::containing(13, Geodetic::from_degrees(47.127, 9.529, 0.0));
        let dem = flightsim_world::DemTile::new(
            id.bounds(),
            flightsim_world::HeightGrid::flat(65, 65, Meters(643.0)),
        );
        let original = dem.clone();
        let frame = RenderFrame::new(id.center());
        let old = prepare_tile(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &frame,
            id,
            &dem,
            None,
        );
        let explicit = prepare_tile_with_options(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &frame,
            id,
            &dem,
            &crate::mesh_options_for(13),
            None,
        );
        assert_eq!(
            meshes
                .get(&old.mesh)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION),
            meshes
                .get(&explicit.mesh)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
        );
        let regional = prepare_tile_with_options(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            Handle::<crate::terrain_detail::TerrainMaterial>::default(),
            &frame,
            id,
            &dem,
            &flightsim_world::MeshOptions {
                resolution: 65,
                skirt_depth: None,
            },
            None,
        );
        assert_eq!(old.surface_vertices, 33 * 33);
        assert_eq!(regional.surface_vertices, 65 * 65);
        assert!(regional.boundary.vertex_count() > old.boundary.vertex_count());
        assert_eq!(
            dem, original,
            "render options cannot modify physical DEM samples"
        );
        queue.apply(&mut world);
        assert_eq!(
            world.get::<Visibility>(regional.entity),
            Some(&Visibility::Hidden)
        );
        assert!(
            world
                .get::<MeshMaterial3d<crate::terrain_detail::TerrainMaterial>>(regional.entity)
                .is_some()
        );
    }

    #[test]
    #[should_panic(expected = "runtime terrain resolution must be in 2..=65")]
    fn opt_in_terrain_mesh_rejects_an_unbounded_runtime_resolution() {
        let world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let id = TileId::new(0, 0, 0);
        let dem = flightsim_world::DemTile::new(
            id.bounds(),
            flightsim_world::HeightGrid::flat(2, 2, Meters::ZERO),
        );
        prepare_tile_with_options(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &RenderFrame::new(id.center()),
            id,
            &dem,
            &flightsim_world::MeshOptions {
                resolution: 66,
                skirt_depth: None,
            },
            None,
        );
    }
}
