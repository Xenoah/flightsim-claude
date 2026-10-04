//! Register airport depiction without changing the physical ground sampler.
use super::*;
use flightsim_core::Ecef;
use flightsim_render::terrain_drape::TerrainOverlay;

#[derive(Component, Debug)]
pub(super) struct AirportGeometry;

#[derive(Debug, Clone, Copy)]
pub(super) enum Support {
    Surface,
    Fixtures(usize),
}

/// Legacy scenes with no rendered terrain keep their authored flat depiction.
/// With terrain, airport geometry starts hidden and is published only with its
/// matching displayed surface. Compound vertical sign boards are not registered.
#[allow(
    clippy::too_many_arguments,
    reason = "startup registration needs the caller-owned mesh, material, source and assets"
)]
pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tiles: &mut TerrainTiles,
    mesh: Mesh,
    material: Handle<StandardMaterial>,
    origin: Ecef,
    name: String,
    rendered_terrain: bool,
    support: Support,
    elevation: impl FnMut(Geodetic) -> Meters,
) -> Option<Entity> {
    let source = if rendered_terrain {
        let result = match support {
            Support::Surface => TerrainOverlay::surface(&mesh, origin, elevation),
            Support::Fixtures(vertices) => {
                TerrainOverlay::fixtures(&mesh, origin, vertices, elevation)
            }
        };
        match result {
            Ok(source) => Some(source),
            Err(error) => {
                warn!("airport overlay {name} rejected: {error}");
                return None;
            }
        }
    } else {
        None
    };
    let handle = meshes.add(mesh);
    let entity = commands
        .spawn((
            flightsim_render::terrain_mesh_bundle(handle.clone(), material, origin),
            Name::new(name.clone()),
            AirportGeometry,
        ))
        .id();
    if let Some(source) = source {
        commands.entity(entity).insert(Visibility::Hidden);
        if let Err(error) = tiles.register_overlay(entity, handle.clone(), source) {
            warn!("airport overlay {name} exceeds scene admission: {error}");
            commands.entity(entity).despawn();
            meshes.remove(&handle);
            return None;
        }
    }
    Some(entity)
}
