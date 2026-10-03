//! Geometry checks use emitted f32 triangles, not the cap triangulation recipe.
#[path = "../tests/support/scenery_fixtures.rs"]
mod fixtures;

use super::*;
use bevy::mesh::VertexAttributeValues;
use glam::DVec3;
use std::collections::BTreeMap;

fn positions(mesh: &Mesh) -> &[[f32; 3]] {
    let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        panic!("tree positions");
    };
    p
}

fn vector(p: [f32; 3]) -> DVec3 {
    Vec3::from_array(p).as_dvec3()
}

// Moller-Trumbore with back-face culling, independent of stored normals.
fn front_hit(triangle: &[DVec3], origin: DVec3, direction: DVec3) -> bool {
    let e1 = triangle[1] - triangle[0];
    let e2 = triangle[2] - triangle[0];
    let cross = direction.cross(e2);
    let determinant = e1.dot(cross);
    if determinant <= 1.0e-8 {
        return false;
    }
    let delta = origin - triangle[0];
    let u = delta.dot(cross) / determinant;
    let q = delta.cross(e1);
    let v = direction.dot(q) / determinant;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / determinant > 0.0
}

#[test]
fn canopy_bases_are_finite_closed_downward_faces_visible_from_below() {
    for point in [
        Geodetic::from_degrees(0.0, 0.0, 0.0),
        Geodetic::from_degrees(47.1741125794, 9.5924242341, 1088.19),
        Geodetic::from_degrees(-40.0, 179.999, 400.0),
        Geodetic::from_degrees(89.999, -70.0, 100.0),
    ] {
        let frame = LocalFrame::new(point);
        let down = frame.ned_to_ecef_vector(Ned::new(0.0, 0.0, 1.0));
        for height in [7.0, 11.0, 16.0] {
            let mut mesh = Builder::new(point.to_ecef(), 96, None);
            assert!(tree(&mut mesh, point, Meters(height), 17));
            assert_eq!(mesh.len(), 96);
            assert!(mesh.positions.iter().flatten().all(|p| p.is_finite()));
            assert!(mesh.normals.iter().flatten().all(|p| p.is_finite()));
            assert!(mesh.colors.iter().flatten().all(|p| p.is_finite()));
            for (crown, radius) in [0.24, 0.18].into_iter().enumerate() {
                let side_start = 6 + crown * 3;
                let mut nodes: Vec<_> = (0..6)
                    .map(|side| vector(mesh.positions[side * 12 + side_start + 1]))
                    .collect();
                nodes.push(vector(mesh.positions[side_start + 2]));
                let caps: Vec<_> = mesh.positions[72 + crown * 12..84 + crown * 12]
                    .iter()
                    .copied()
                    .map(vector)
                    .collect();
                let sides: Vec<_> = (0..6)
                    .flat_map(|side| {
                        mesh.positions[side * 12 + side_start..side * 12 + side_start + 3]
                            .iter()
                            .copied()
                            .map(vector)
                    })
                    .collect();
                let mut edges = BTreeMap::new();
                for triangle in sides.chunks_exact(3).chain(caps.chunks_exact(3)) {
                    let ids: Vec<_> = triangle
                        .iter()
                        .map(|p| {
                            nodes
                                .iter()
                                .position(|node| node.distance(*p) < 0.00001)
                                .expect("caps share the emitted side rims; no new rim or hole")
                        })
                        .collect();
                    for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
                        assert_ne!(a, b);
                        *edges.entry((a, b)).or_insert(0) += 1;
                    }
                }
                assert_eq!(edges.len(), 30, "closed seven-vertex, ten-face cone");
                for (&(a, b), &count) in &edges {
                    assert_eq!(count, 1);
                    assert_eq!(
                        edges.get(&(b, a)),
                        Some(&1),
                        "every edge has an opposite mate"
                    );
                }
                let mut area = 0.0;
                for (index, triangle) in caps.chunks_exact(3).enumerate() {
                    let cross = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
                    area += cross.length() * 0.5;
                    assert!(cross.normalize().dot(down) > 0.99999);
                    for vertex in 0..3 {
                        let offset = 72 + crown * 12 + index * 3 + vertex;
                        assert!(vector(mesh.normals[offset]).dot(down) > 0.99999);
                        assert_eq!(
                            mesh.colors[offset].map(f32::to_bits),
                            mesh.colors[side_start].map(f32::to_bits)
                        );
                    }
                    // The triangle interior is off the shared fan edges.
                    let interior = (triangle[0] + triangle[1] + triangle[2]) / 3.0;
                    assert!(front_hit(triangle, interior + down, -down));
                    assert!(!front_hit(triangle, interior - down, down));
                }
                let expected_area = 3.0 * 3.0_f64.sqrt() * (height * radius).powi(2) / 2.0;
                assert!((area - expected_area).abs() < 0.0001);
                // Six off-centre rays exercise the whole hexagon, independently
                // of the fan's choice of diagonal, away from its shared edges.
                let center = nodes[..6].iter().copied().sum::<DVec3>() / 6.0;
                for angle in [0.17_f64, 1.21, 2.38, 3.49, 4.61, 5.77] {
                    let sample = center
                        + frame.ned_to_ecef_vector(Ned::new(
                            angle.cos() * height * radius * 0.55,
                            angle.sin() * height * radius * 0.55,
                            0.0,
                        ));
                    assert_eq!(
                        caps.chunks_exact(3)
                            .filter(|t| front_hit(t, sample + down, -down))
                            .count(),
                        1
                    );
                    assert!(
                        !sides
                            .chunks_exact(3)
                            .any(|t| front_hit(t, sample + down, -down)),
                        "the original open shell is invisible from underneath"
                    );
                }
            }
        }
    }
}

fn large_forest() -> SceneryLandCover {
    SceneryLandCover {
        source_id: 7351,
        class: LandCoverClass::Forest,
        boundary: [
            (-1000.0, -1000.0),
            (-1000.0, 1000.0),
            (1000.0, 1000.0),
            (1000.0, -1000.0),
        ]
        .map(|(n, e)| fixtures::point(n, e))
        .to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

#[test]
fn cap_capacity_boundaries_roll_back_the_entire_tree_and_preserve_previous_geometry() {
    let land = large_forest();
    let anchor = fixtures::anchor();
    let mut reference = Builder::new(anchor.to_ecef(), 192, None);
    let options = SceneryMeshOptions::near(anchor);
    let mut stats = SceneryMeshStatistics::default();
    plant_trees(
        &mut reference,
        &land,
        options,
        2,
        &mut stats,
        &mut |_| Some(Meters::ZERO),
        &|_| false,
    );
    assert_eq!(stats.procedural_trees, 2);
    for (limit, trees) in [(95, 0), (96, 1), (191, 1), (192, 2)] {
        let mut builder = Builder::new(anchor.to_ecef(), limit, None);
        let mut stats = SceneryMeshStatistics::default();
        plant_trees(
            &mut builder,
            &land,
            options,
            2,
            &mut stats,
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        );
        assert_eq!(stats.procedural_trees, trees);
        let count = trees * 96;
        assert_eq!(builder.positions, reference.positions[..count]);
        assert_eq!(builder.normals, reference.normals[..count]);
        assert_eq!(builder.colors, reference.colors[..count]);
        assert_eq!(builder.lifts, reference.lifts[..count]);
    }
}

#[test]
fn cancellation_on_second_tree_keeps_only_the_complete_first_tree_in_the_builder() {
    let land = large_forest();
    let anchor = fixtures::anchor();
    let cancelled = AtomicBool::new(false);
    let mut builder = Builder::new(anchor.to_ecef(), 192, Some(&cancelled));
    let mut stats = SceneryMeshStatistics::default();
    let mut calls = 0;
    plant_trees(
        &mut builder,
        &land,
        SceneryMeshOptions::near(anchor),
        2,
        &mut stats,
        &mut |_| {
            calls += 1;
            if calls == 2 {
                cancelled.store(true, Ordering::Relaxed);
            }
            Some(Meters::ZERO)
        },
        &|_| false,
    );
    assert_eq!(calls, 2);
    assert_eq!(stats.procedural_trees, 1);
    assert_eq!(builder.len(), 96);
    assert_eq!(builder.normals.len(), 96);
    assert_eq!(builder.colors.len(), 96);
    assert_eq!(builder.lifts.len(), 96);
}

#[test]
fn actual_128_tree_batch_is_deterministic_and_preserved_across_origin_rebase() {
    let land = large_forest();
    let before = land.clone();
    let anchor = fixtures::anchor();
    let mut options = SceneryMeshOptions::near(anchor);
    options.max_ground_vertices = 0;
    let make = |origin| {
        scenery_mesh(
            &[SceneryFeatureRef::LandCover(&land)],
            origin,
            options,
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap()
    };
    let a = make(anchor);
    let repeated = make(anchor);
    let moved = make(fixtures::point(4000.0, 1000.0));
    assert_eq!(land, before);
    assert_eq!(a.statistics.procedural_trees, 128);
    assert_eq!(a.statistics.tree_candidates, 128);
    assert_eq!(a.statistics.vertices, 12_288);
    assert_eq!(a.statistics.triangles, 4096);
    assert!(a.ground.is_none());
    assert_eq!(a.statistics, repeated.statistics);
    assert_eq!(a.statistics, moved.statistics);
    for attribute in [
        Mesh::ATTRIBUTE_POSITION,
        Mesh::ATTRIBUTE_NORMAL,
        Mesh::ATTRIBUTE_COLOR,
    ] {
        assert_eq!(
            a.mesh.attribute(attribute),
            repeated.mesh.attribute(attribute)
        );
    }
    for attribute in [Mesh::ATTRIBUTE_NORMAL, Mesh::ATTRIBUTE_COLOR] {
        assert_eq!(a.mesh.attribute(attribute), moved.mesh.attribute(attribute));
    }
    for (a_pos, b_pos) in positions(&a.mesh).iter().zip(positions(&moved.mesh)) {
        let world_a = a.origin.as_vec() + vector(*a_pos);
        let world_b = moved.origin.as_vec() + vector(*b_pos);
        assert!(world_a.distance(world_b) < 0.001);
    }
}
