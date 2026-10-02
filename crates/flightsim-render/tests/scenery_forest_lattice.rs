//! Independent containment, stability and work-bound checks for forest detail.
#![allow(clippy::float_cmp)]

#[path = "support/scenery_fixtures.rs"]
mod fixtures;

use bevy::mesh::VertexAttributeValues;
use flightsim_core::{Geodetic, LocalFrame, Meters};
use flightsim_render::scenery::{
    MAX_BATCH_TREE_CANDIDATES, MAX_BATCH_TREES, SceneryMeshOptions, scenery_mesh,
};
use flightsim_world::scenery::{LandCoverClass, SceneryFeatureRef, SceneryLandCover};
use glam::DVec2;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn square(size: f64) -> SceneryLandCover {
    SceneryLandCover {
        source_id: 7351,
        class: LandCoverClass::Forest,
        boundary: [(-size, -size), (-size, size), (size, size), (size, -size)]
            .map(|(n, e)| fixtures::point(n, e))
            .to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

fn concave() -> SceneryLandCover {
    SceneryLandCover {
        source_id: 981,
        class: LandCoverClass::Forest,
        boundary: [
            (0.0, 0.0),
            (0.0, 140_000.0),
            (140_000.0, 140_000.0),
            (140_000.0, 95_000.0),
            (100_000.0, 95_000.0),
            (100_000.0, 45_000.0),
            (140_000.0, 45_000.0),
            (140_000.0, 0.0),
        ]
        .map(|(n, e)| fixtures::point(n, e))
        .to_vec(),
        triangles: vec![
            [0, 1, 4],
            [1, 2, 4],
            [2, 3, 4],
            [0, 4, 5],
            [0, 5, 7],
            [5, 6, 7],
        ],
    }
}

fn projected(frame: &LocalFrame, point: Geodetic) -> DVec2 {
    let p = frame.ecef_to_ned_position(
        Geodetic::new(point.latitude, point.longitude, Meters::ZERO).to_ecef(),
    );
    DVec2::new(p.north(), p.east())
}

// Independent signed-edge triangle membership, rather than the production
// polygon ray-crossing algorithm. Both use the source's documented local frame.
fn source_contains(land: &SceneryLandCover, point: Geodetic) -> bool {
    let frame = LocalFrame::new(land.boundary[0]);
    let p = projected(&frame, point);
    land.triangles.iter().any(|triangle| {
        let [a, b, c] = triangle.map(|i| projected(&frame, land.boundary[i as usize]));
        let signs = [
            (b - a).perp_dot(p - a),
            (c - b).perp_dot(p - b),
            (a - c).perp_dot(p - c),
        ];
        signs.iter().all(|&s| s >= -1.0e-5) || signs.iter().all(|&s| s <= 1.0e-5)
    })
}

fn candidates(
    land: &SceneryLandCover,
    observer: Geodetic,
) -> (Vec<Geodetic>, usize, Vec<[f32; 3]>) {
    let captured = RefCell::new(Vec::new());
    let mut options = SceneryMeshOptions::near(observer);
    options.max_ground_vertices = 0;
    let batch = scenery_mesh(
        &[SceneryFeatureRef::LandCover(land)],
        fixtures::anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|point| {
            captured.borrow_mut().push(point);
            false
        },
    )
    .expect("nearby forest should produce bounded solid trees even when ground is omitted");
    assert_eq!(batch.statistics.procedural_trees, captured.borrow().len());
    assert!(batch.statistics.tree_candidates <= MAX_BATCH_TREE_CANDIDATES);
    let VertexAttributeValues::Float32x3(positions) = batch
        .mesh
        .attribute(bevy::mesh::Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    else {
        panic!("3D tree positions")
    };
    (
        captured.into_inner(),
        batch.statistics.tree_candidates,
        positions.clone(),
    )
}

#[test]
fn large_forest_samples_near_observer_instead_of_remote_source_triangles() {
    let land = square(20_000.0);
    let observer = fixtures::anchor();
    let (points, attempts, _) = candidates(&land, observer);
    assert_eq!(points.len(), MAX_BATCH_TREES);
    assert_eq!(attempts, MAX_BATCH_TREES);
    for point in points {
        assert!(source_contains(&land, point));
        assert!(point.great_circle_distance(observer).get() < 800.0);
    }
}

#[test]
fn final_geodetic_candidates_stay_inside_large_concave_source_polygon() {
    let land = concave();
    for offset in [0.0, 10.0, 20.0, 40.0] {
        let observer = fixtures::point(125_000.0, 95_000.0 + offset);
        let (points, _, _) = candidates(&land, observer);
        assert!(points.len() >= 32);
        for point in points {
            assert!(
                source_contains(&land, point),
                "projected tree escaped original source ring: {point:?}"
            );
            assert!(point.great_circle_distance(observer).get() <= 1800.0);
        }
    }
}

#[test]
fn outside_observer_selects_actual_near_concave_boundary() {
    let land = concave();
    // In the empty notch, over a kilometre from the nearest forest. A spiral
    // around the bounding-box interior cannot reach it in512 attempted cells.
    let observer = fixtures::point(125_000.0, 93_700.0);
    assert!(!source_contains(&land, observer));
    let (points, _, _) = candidates(&land, observer);
    assert!(points.len() >= 16);
    for point in points {
        assert!(source_contains(&land, point));
        assert!(point.great_circle_distance(observer).get() <= 1800.0);
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "bounded local test coordinates divided by40 fit i64"
)]
fn cell_positions(
    land: &SceneryLandCover,
    points: &[Geodetic],
) -> BTreeMap<(i64, i64), (u64, u64)> {
    let frame = LocalFrame::new(land.boundary[0]);
    points
        .iter()
        .map(|&p| {
            let xy = projected(&frame, p);
            (
                ((xy.x / 40.0).floor() as i64, (xy.y / 40.0).floor() as i64),
                (p.latitude.get().to_bits(), p.longitude.get().to_bits()),
            )
        })
        .collect()
}

#[test]
fn shared_lattice_cells_are_identical_after_camera_motion_and_triangle_reordering() {
    let land = square(5000.0);
    let (first, _, first_mesh) = candidates(&land, fixtures::anchor());
    let (moved, _, _) = candidates(&land, fixtures::point(160.0, 0.0));
    let a = cell_positions(&land, &first);
    let b = cell_positions(&land, &moved);
    let mut shared = 0;
    for (cell, position) in &a {
        if let Some(other) = b.get(cell) {
            assert_eq!(position, other);
            shared += 1;
        }
    }
    assert!(
        shared >= 32,
        "nearby observations should retain many identical cells"
    );
    let mut reordered = land.clone();
    reordered.triangles.reverse();
    for t in &mut reordered.triangles {
        t.rotate_left(1);
    }
    let (again, _, again_mesh) = candidates(&reordered, fixtures::anchor());
    assert_eq!(cell_positions(&reordered, &again), a);
    assert_eq!(
        again_mesh, first_mesh,
        "triangle order must not change cell positions or appearance"
    );
}

#[test]
fn excluded_cells_debit_one_shared_batch_budget_across_large_forests() {
    let mut visible_ground = square(5.0);
    visible_ground.class = LandCoverClass::Grass;
    let mut forests = [square(20_000.0), square(20_000.0), square(20_000.0)];
    forests[1].source_id += 1;
    forests[2].source_id += 2;
    let features: Vec<_> = std::iter::once(&visible_ground)
        .chain(forests.iter())
        .map(SceneryFeatureRef::LandCover)
        .collect();
    for cap in [0, 1, 7, 512, usize::MAX] {
        let checked = AtomicUsize::new(0);
        let mut options = SceneryMeshOptions::near(fixtures::anchor());
        options.max_ground_vertices = 6;
        options.max_tree_candidates = cap;
        let batch = scenery_mesh(
            &features,
            fixtures::anchor(),
            options,
            &mut |_| Some(Meters::ZERO),
            &|_| {
                checked.fetch_add(1, Ordering::Relaxed);
                true
            },
        )
        .unwrap();
        assert_eq!(batch.statistics.procedural_trees, 0);
        assert_eq!(
            batch.statistics.tree_candidates,
            cap.min(MAX_BATCH_TREE_CANDIDATES)
        );
        assert_eq!(
            checked.load(Ordering::Relaxed),
            batch.statistics.tree_candidates
        );
    }
}

#[test]
fn cancellation_during_lattice_sampling_discards_all_geometry_after_one_callback() {
    let land = square(20_000.0);
    let cancelled = AtomicBool::new(false);
    let checked = AtomicUsize::new(0);
    let mut options = SceneryMeshOptions::near(fixtures::anchor());
    options.max_ground_vertices = 0;
    options.cancellation = Some(&cancelled);
    let result = scenery_mesh(
        &[SceneryFeatureRef::LandCover(&land)],
        fixtures::anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|_| {
            checked.fetch_add(1, Ordering::Relaxed);
            cancelled.store(true, Ordering::Relaxed);
            true
        },
    );
    assert!(result.is_none());
    assert_eq!(checked.load(Ordering::Relaxed), 1);
}
