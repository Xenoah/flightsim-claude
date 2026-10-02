//! Characterize the shared-surface requirement, not a claim of solved occlusion.
//!
//! The physical bilinear sampler must not be silently changed to follow render
//! LOD. A 10 m airport drape alone cannot guarantee overlap-free terrain. See
//! docs/qa/airport-mesh-clearance-2026-10-02.md for the independent CPU evidence
//! and the clipping/atomic-cut acceptance contract.

use flightsim_core::{Geodetic, LocalFrame, Meters, Radians};
use flightsim_world::{DemTile, HeightGrid, MeshOptions, TerrainMesh, TileId, build_mesh};
use glam::DVec3;

/// Intersect an ellipsoid-normal line with actual stored triangle vertices.
/// This test helper is not a production terrain or FDM API.
fn triangle_height(mesh: &TerrainMesh, indices: &[u32], point: Geodetic) -> Option<f64> {
    let vertex = |index: u32| {
        let p = mesh.positions[index as usize];
        mesh.origin.as_vec() + DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    };
    let a = vertex(indices[0]);
    let ab = vertex(indices[1]) - a;
    let ac = vertex(indices[2]) - a;
    let normal = ab.cross(ac);
    let origin = point.to_ecef().as_vec();
    let up = LocalFrame::new(point).up_ecef();
    let denominator = normal.dot(up);
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let height = normal.dot(a - origin) / denominator;
    let ap = origin + up * height - a;
    let d00 = ab.dot(ab);
    let d01 = ab.dot(ac);
    let d11 = ac.dot(ac);
    let d20 = ap.dot(ab);
    let d21 = ap.dot(ac);
    let determinant = d00 * d11 - d01 * d01;
    let v = (d11 * d20 - d01 * d21) / determinant;
    let w = (d00 * d21 - d01 * d20) / determinant;
    (v >= -1e-8 && w >= -1e-8 && v + w <= 1.0 + 1e-8).then_some(height)
}

#[test]
fn default_mesh_triangle_can_stand_above_the_unchanged_bilinear_ground() {
    let id = TileId::containing(13, Geodetic::from_degrees(35.55, 139.78, 0.0));
    let bounds = id.bounds();
    let mut samples = vec![0.0; 33 * 33];
    // One adversarial saddle: NW=SE=0, NE=SW=100 m. These are synthetic
    // heights, not a claim about the actual terrain near Haneda.
    samples[16 * 33 + 17] = 100.0;
    samples[17 * 33 + 16] = 100.0;
    let dem = DemTile::new(bounds, HeightGrid::new(33, 33, samples));
    let point = Geodetic::new(
        Radians(bounds.north.get() - bounds.height().get() * 16.5 / 32.0),
        Radians(bounds.west.get() + bounds.width().get() * 16.5 / 32.0),
        Meters::ZERO,
    );
    let physical_height = dem.elevation_at(point).get();
    assert!((physical_height - 50.0).abs() < 1e-6);
    let mesh = build_mesh(
        id,
        &dem,
        &MeshOptions {
            resolution: 33,
            skirt_depth: Some(Meters::ZERO),
        },
    );
    let first_index = (16 * 32 + 16) * 6;
    let rendered_height = mesh.indices[first_index..first_index + 6]
        .chunks_exact(3)
        .filter_map(|triangle| triangle_height(&mesh, triangle, point))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        rendered_height.is_finite(),
        "the probe must intersect a surface triangle"
    );
    // Planar answer is exactly 50 m above the physical 50 m center. WGS84
    // curvature and the production f32-relative vertices change it slightly.
    assert!((rendered_height - physical_height - 50.0).abs() < 0.05);
    assert!(
        rendered_height > physical_height + 0.47,
        "even the light top is covered"
    );
    assert!((dem.elevation_at(point).get() - physical_height).abs() < f64::EPSILON);
}
