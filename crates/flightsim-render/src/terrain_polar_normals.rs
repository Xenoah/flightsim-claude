//! Source-aware visual normal filtering for global fallback polar caps.
//!
//! FSGT stores metre-quantized orthometric heights on a geographic lattice.
//! Longitude spacing contracts near a pole, and radial cap interpolation keeps
//! the resulting angular slopes visible even as mesh tessellation increases.
//! A fixed 505 m physical half-stencil reduces that shading sensitivity while
//! retaining all geometry, slopes, palette inputs, source/physics and replay data.
//!
//! <=0.505 m packing error gives <=0.001 uncertainty per central-difference
//! gradient component in a local height graph; this does not bound normal-angle
//! error or source accuracy. Filtering also suppresses retained source gradients.
//! The inner 75% of each canonical source cap uses filtered normals; the outer
//! quarter blends to the original normal. Outside-cap vertex attributes stay
//! unchanged, though coarse facets can interpolate pole shading farther out.
//!
//! Only selector-identified fallback data may use the app's already validated
//! global atlas. Work is four atlas samples per affected surface vertex, without
//! IO, caches, frame state or source searches. Filter before boundary extraction
//! so seam bridges inherit the same normal attributes without changing geometry.

use flightsim_core::{Geodetic, LocalFrame, Meters, Ned, Radians};
use flightsim_world::{TerrainMesh, TileId, global::GlobalTerrain};
use glam::DVec3;

use crate::TerrainMeshProvenance;

const HALF_STENCIL: Meters = Meters(505.0);

#[derive(Debug, Clone, Copy)]
pub struct GlobalTerrainShading<'a> {
    pub atlas: &'a GlobalTerrain,
    /// Read-path identity supplied by the selector, never inferred from heights.
    pub provenance: TerrainMeshProvenance,
}

impl GlobalTerrainShading<'_> {
    /// Apply only normal attributes to a regular grid from world::build_mesh.
    /// Regional/primary preparations return before sampling anything.
    ///
    /// # Panics
    /// The input is not a regular surface grid with its optional standard skirt.
    pub fn apply(self, id: TileId, mesh: &mut TerrainMesh) {
        if self.provenance != TerrainMeshProvenance::Fallback {
            return;
        }
        let (_, north, step) = self.atlas.grid_geometry();
        let south = north.get() - step.get() * f64::from(self.atlas.dimensions().1 - 1);
        let bounds = id.bounds();
        if bounds.north.get() < north.get() && bounds.south.get() > south {
            return;
        }
        let resolution = mesh.surface_vertex_count.isqrt();
        assert!(resolution >= 2 && resolution * resolution == mesh.surface_vertex_count);
        let last = resolution - 1;
        let steps = f64::from(u32::try_from(last).expect("terrain resolution fits u32"));
        for row in 0..resolution {
            for column in 0..resolution {
                let index = row * resolution + column;
                let v = f64::from(u32::try_from(row).expect("row fits u32")) / steps;
                let u = f64::from(u32::try_from(column).expect("column fits u32")) / steps;
                let at_north = row == 0 && id.y == 0;
                let at_south = row == last && id.y == TileId::rows(id.level) - 1;
                let latitude = if at_north {
                    core::f64::consts::FRAC_PI_2
                } else if at_south {
                    -core::f64::consts::FRAC_PI_2
                } else {
                    bounds.north.get() - v * bounds.height().get()
                };
                let longitude = if at_north || at_south {
                    0.0
                } else {
                    bounds.west.get() + u * bounds.width().get()
                };
                let point = Geodetic::new(
                    Radians(latitude),
                    Radians(longitude),
                    Meters(f64::from(mesh.elevations[index])),
                );
                let weight = cap_weight(self.atlas, point);
                if weight <= 0.0 {
                    continue;
                }
                let filtered = filtered_normal(self.atlas, point);
                let previous = DVec3::from_array(mesh.normals[index].map(f64::from));
                let normal = previous.lerp(filtered, weight).normalize();
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "bounded unit normal GPU attribute"
                )]
                {
                    mesh.normals[index] = normal.to_array().map(|component| component as f32);
                }
            }
        }
        // Match world::mesh::append_skirt's exact perimeter order. No skirt
        // position is sampled or raised; it retains its surface shading copy.
        let surface = mesh.surface_vertex_count;
        if mesh.positions.len() == surface {
            return;
        }
        assert_eq!(mesh.positions.len(), surface + 4 * last);
        let ring = (0..last)
            .chain((0..last).map(|r| r * resolution + last))
            .chain((0..last).map(|c| last * resolution + last - c))
            .chain((0..last).map(|r| (last - r) * resolution));
        for (offset, top) in ring.enumerate() {
            mesh.normals[surface + offset] = mesh.normals[top];
        }
    }
}

fn cap_weight(atlas: &GlobalTerrain, point: Geodetic) -> f64 {
    let (_, north, step) = atlas.grid_geometry();
    let south = north.get() - step.get() * f64::from(atlas.dimensions().1 - 1);
    let (radius, extent) = if point.latitude.get() > north.get() {
        (
            core::f64::consts::FRAC_PI_2 - point.latitude.get(),
            core::f64::consts::FRAC_PI_2 - north.get(),
        )
    } else if point.latitude.get() < south {
        (
            point.latitude.get() + core::f64::consts::FRAC_PI_2,
            south + core::f64::consts::FRAC_PI_2,
        )
    } else {
        return 0.0;
    };
    // Shading transition: unchanged outside the canonical source cap, full
    // filtering in its inner 75%, smooth value/first derivative at the limits.
    let t = ((radius / extent - 0.75) / 0.25).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

fn filtered_normal(atlas: &GlobalTerrain, point: Geodetic) -> DVec3 {
    let frame = LocalFrame::new(point);
    let half = HALF_STENCIL.get();
    let sampled = [(half, 0.0), (-half, 0.0), (0.0, half), (0.0, -half)].map(|(north, east)| {
        let coordinate = frame
            .ned_to_ecef_position(Ned::new(north, east, 0.0))
            .to_geodetic();
        Geodetic::new(
            coordinate.latitude,
            coordinate.longitude,
            atlas
                .sample(coordinate)
                .expect("valid core-transformed coordinate")
                .surface_height,
        )
        .to_ecef()
        .as_vec()
    });
    (sampled[2] - sampled[3])
        .cross(sampled[0] - sampled[1])
        .normalize_or(frame.up_ecef())
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "bit-identical geometry and untouched attributes are required"
)]
mod tests {
    use super::*;
    use flightsim_core::RenderFrame;
    use flightsim_world::{MeshOptions, build_mesh};

    fn filtered(id: TileId, provenance: TerrainMeshProvenance) -> (TerrainMesh, TerrainMesh) {
        let atlas = GlobalTerrain::bundled().unwrap();
        let dem = atlas.tile(id).unwrap();
        let before = build_mesh(id, &dem, &MeshOptions::default());
        let mut after = build_mesh(id, &dem, &MeshOptions::default());
        GlobalTerrainShading {
            atlas: &atlas,
            provenance,
        }
        .apply(id, &mut after);
        assert_eq!(before.origin, after.origin);
        assert_eq!(before.positions, after.positions);
        assert_eq!(before.indices, after.indices);
        assert_eq!(before.elevations, after.elevations);
        assert_eq!(before.uvs, after.uvs);
        assert_eq!(before.slopes, after.slopes);
        assert_eq!(before.surface_vertex_count, after.surface_vertex_count);
        (before, after)
    }

    #[test]
    fn fixed_polar_policy_changes_only_normals() {
        let (before, after) = filtered(TileId::new(10, 5, 1023), TerrainMeshProvenance::Fallback);
        assert_ne!(before.normals, after.normals);
        for normal in &after.normals {
            let normal = DVec3::from_array(normal.map(f64::from));
            assert!(normal.is_finite() && (normal.length() - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn primary_and_nonpolar_preparations_are_unchanged() {
        for (id, provenance) in [
            (TileId::new(10, 5, 1023), TerrainMeshProvenance::Primary),
            (TileId::new(10, 5, 512), TerrainMeshProvenance::Fallback),
        ] {
            let (before, after) = filtered(id, provenance);
            assert_eq!(before.normals, after.normals);
            assert_eq!(before.slopes, after.slopes);
        }
    }

    #[test]
    fn filtered_shared_vertices_poles_and_rebased_frames_agree() {
        let atlas = GlobalTerrain::bundled().unwrap();
        for south in [false, true] {
            let row = if south { 1023 } else { 0 };
            let (_, west) = filtered(TileId::new(10, 5, row), TerrainMeshProvenance::Fallback);
            let (_, east) = filtered(TileId::new(10, 6, row), TerrainMeshProvenance::Fallback);
            // Inner half-cap belongs to the fully filtered portion; tile row
            // differences outside it intentionally preserve existing shading.
            for r in if south { 25..33 } else { 0..8 } {
                let a = DVec3::from_array(west.normals[r * 33 + 32].map(f64::from));
                let b = DVec3::from_array(east.normals[r * 33].map(f64::from));
                assert!((a - b).length() < 1e-7);
            }
            let p = Geodetic::from_degrees(if south { -90.0 } else { 90.0 }, 0.0, 350.0);
            let n = filtered_normal(&atlas, p);
            assert!(n.is_finite() && (n.length() - 1.0).abs() < 1e-12);
            let pole_row = if south { 32 } else { 0 };
            for c in 0..33 {
                assert_eq!(west.normals[pole_row * 33 + c], east.normals[pole_row * 33]);
            }
            // Only RenderFrame rotates ECEF normals; filtering has no origin state.
            for reference in [
                p,
                Geodetic::from_degrees(if south { -89.99 } else { 89.99 }, 80.0, 650.0),
            ] {
                let frame = RenderFrame::new(reference);
                let rotation = frame.rotation_to_render(glam::DQuat::IDENTITY).as_dquat();
                let restored = rotation.inverse() * (rotation * n);
                assert!((restored - n).length() < 1e-6);
            }
        }
    }

    #[test]
    fn every_polar_endpoint_is_longitude_and_lod_invariant() {
        for south in [false, true] {
            let mut reference = None;
            for level in [9, 10, 13] {
                for x in [
                    0,
                    1,
                    TileId::columns(level) / 4,
                    TileId::columns(level) / 2,
                    TileId::columns(level) - 1,
                ] {
                    let id = TileId::new(level, x, if south { TileId::rows(level) - 1 } else { 0 });
                    let (_, mesh) = filtered(id, TerrainMeshProvenance::Fallback);
                    let row = if south { 32 } else { 0 };
                    let expected = *reference.get_or_insert(mesh.normals[row * 33]);
                    for c in 0..33 {
                        assert_eq!(mesh.normals[row * 33 + c], expected);
                    }
                }
            }
        }
    }

    #[test]
    fn mixed_level_shading_and_bridge_geometry_remain_consistent() {
        use flightsim_world::seams::{TerrainBoundary, plan_seams};
        use std::collections::BTreeMap;
        let coarse_id = TileId::new(9, 5, 511);
        let fine_id = TileId::new(10, 12, 1023);
        let (coarse_before, coarse) = filtered(coarse_id, TerrainMeshProvenance::Fallback);
        let (fine_before, fine) = filtered(fine_id, TerrainMeshProvenance::Fallback);
        for r in 27..33 {
            let a = DVec3::from_array(coarse.normals[r * 33 + 32].map(f64::from));
            let b = DVec3::from_array(fine.normals[(2 * r - 32) * 33].map(f64::from));
            assert!((a - b).length() < 1e-7);
        }
        let before = BTreeMap::from([
            (
                coarse_id,
                TerrainBoundary::from_mesh(coarse_id, &coarse_before),
            ),
            (fine_id, TerrainBoundary::from_mesh(fine_id, &fine_before)),
        ]);
        let after = BTreeMap::from([
            (coarse_id, TerrainBoundary::from_mesh(coarse_id, &coarse)),
            (fine_id, TerrainBoundary::from_mesh(fine_id, &fine)),
        ]);
        let visible = before.keys().copied().collect();
        let old_plans = plan_seams(&before, &visible);
        let new_plans = plan_seams(&after, &visible);
        assert!(!old_plans.is_empty());
        assert_eq!(old_plans.len(), new_plans.len());
        for (old, new) in old_plans.iter().zip(&new_plans) {
            let a = old.build_mesh(&before);
            let b = new.build_mesh(&after);
            assert_eq!(a.origin, b.origin);
            assert_eq!(a.positions, b.positions);
            assert_eq!(a.indices, b.indices);
            assert_eq!(a.elevations, b.elevations);
            assert_eq!(a.uvs, b.uvs);
            assert_eq!(a.slopes, b.slopes);
        }
    }

    #[test]
    fn normal_transition_is_zero_at_source_cap_boundary() {
        let atlas = GlobalTerrain::bundled().unwrap();
        let (_, north, step) = atlas.grid_geometry();
        let south = north.get() - step.get() * f64::from(atlas.dimensions().1 - 1);
        for edge in [north.get(), south] {
            let p = Geodetic::new(Radians(edge), Radians::ZERO, Meters::ZERO);
            assert_eq!(cap_weight(&atlas, p), 0.0);
        }
    }
}
