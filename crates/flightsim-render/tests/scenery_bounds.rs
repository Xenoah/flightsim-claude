//! Independent public-API CPU checks. No render device or GPU is created.
#[path = "support/scenery_fixtures.rs"]
mod fixtures;

use fixtures::{Fixture, anchor};
use flightsim_core::Meters;
use flightsim_render::scenery::{
    MAX_BATCH_FEATURES, MAX_BATCH_TREE_CANDIDATES, MAX_BATCH_TREES, MAX_BATCH_VERTICES,
    SceneryMeshOptions, scenery_mesh,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn dense_fixtures_respect_vertex_and_tree_caps_even_with_unbounded_requested_limits() {
    for fixture in [Fixture::buildings(), Fixture::roads(), Fixture::forest()] {
        let features = fixture.features();
        assert!(features.len() <= MAX_BATCH_FEATURES);
        let mut options = SceneryMeshOptions::near(anchor());
        options.max_vertices = usize::MAX;
        options.max_trees = usize::MAX;
        options.max_tree_candidates = usize::MAX;
        let batch = scenery_mesh(
            &features,
            anchor(),
            options,
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap();
        let s = batch.statistics;
        assert!(s.vertices > 0 && s.vertices <= MAX_BATCH_VERTICES);
        assert!(s.procedural_trees <= MAX_BATCH_TREES);
        assert!(s.tree_candidates <= MAX_BATCH_TREE_CANDIDATES);
        assert_eq!(s.vertices, s.triangles * 3);
        assert_eq!(
            batch.mesh.count_vertices()
                + batch
                    .ground
                    .as_ref()
                    .map_or(0, |ground| ground.mesh.count_vertices()),
            s.vertices
        );
        assert_eq!(
            s.roads + s.buildings + s.land_polygons + s.skipped_features,
            features.len()
        );
    }
}

#[test]
fn clipped_dense_source_never_exceeds_callers_vertex_budget() {
    let fixture = Fixture::buildings();
    let features = fixture.features();
    for cap in [3, 29, 30, 31, 63, 511, 1_024, 7_777] {
        let mut options = SceneryMeshOptions::near(anchor());
        options.max_vertices = cap;
        if let Some(batch) = scenery_mesh(
            &features,
            anchor(),
            options,
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        ) {
            assert!(batch.statistics.vertices <= cap);
            assert_eq!(batch.statistics.vertices % 3, 0);
            assert!(batch.statistics.skipped_features > 0);
            assert_eq!(
                batch.statistics.buildings + batch.statistics.skipped_features,
                features.len()
            );
        }
    }
}

#[test]
fn rejected_tree_candidates_count_towards_the_hard_work_budget() {
    let fixture = Fixture::rejected_forest();
    let rejected = AtomicUsize::new(0);
    let mut options = SceneryMeshOptions::near(anchor());
    options.vegetation_distance = Meters(10_000.0);
    let batch = scenery_mesh(
        &fixture.features(),
        anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|_| {
            rejected.fetch_add(1, Ordering::Relaxed);
            true
        },
    )
    .unwrap();
    assert_eq!(batch.statistics.procedural_trees, 0);
    assert_eq!(batch.statistics.tree_candidates, MAX_BATCH_TREE_CANDIDATES);
    let callbacks = rejected.load(Ordering::Relaxed);
    assert!(callbacks > 0 && callbacks <= batch.statistics.tree_candidates);
    // Out-of-ring lattice cells are still attempted and charged, even though
    // only in-ring candidates reach the exclusion callback.
    assert!(batch.statistics.land_polygons > 0);
}

#[test]
fn cancellation_during_rejected_trees_stops_on_the_next_candidate() {
    let fixture = Fixture::rejected_forest();
    let cancelled = AtomicBool::new(false);
    let checked = AtomicUsize::new(0);
    let mut options = SceneryMeshOptions::near(anchor());
    options.cancellation = Some(&cancelled);
    options.vegetation_distance = Meters(10_000.0);
    let result = scenery_mesh(
        &fixture.features(),
        anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|_| {
            checked.fetch_add(1, Ordering::Relaxed);
            cancelled.store(true, Ordering::Relaxed);
            true
        },
    );
    assert!(
        result.is_none(),
        "cancelled ground and solid geometry must not escape"
    );
    assert_eq!(checked.load(Ordering::Relaxed), 1);
}

#[test]
fn cancelled_batch_never_samples_elevation_or_exclusion() {
    let fixture = Fixture::buildings();
    let cancelled = AtomicBool::new(true);
    let mut options = SceneryMeshOptions::near(anchor());
    options.cancellation = Some(&cancelled);
    assert!(
        scenery_mesh(
            &fixture.features(),
            anchor(),
            options,
            &mut |_| panic!("cancelled elevation work"),
            &|_| panic!("cancelled exclusion work")
        )
        .is_none()
    );
}

#[test]
fn caller_candidate_and_tree_limits_include_zero() {
    let fixture = Fixture::forest();
    for (trees, candidates) in [(0, 512), (128, 0), (2, 7), (128, 7)] {
        let mut options = SceneryMeshOptions::near(anchor());
        options.max_trees = trees;
        options.max_tree_candidates = candidates;
        let batch = scenery_mesh(
            &fixture.features(),
            anchor(),
            options,
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        )
        .unwrap();
        assert!(batch.statistics.procedural_trees <= trees);
        assert!(batch.statistics.tree_candidates <= candidates);
        if trees == 0 || candidates == 0 {
            assert_eq!(batch.statistics.tree_candidates, 0);
        }
    }
}

#[test]
fn terrain_ground_and_solid_geometry_remain_separate_with_exact_total_counts() {
    let roads = Fixture::roads();
    let buildings = Fixture::buildings();
    for (fixture, is_ground) in [(&roads, true), (&buildings, false)] {
        let batch = scenery_mesh(
            &fixture.features(),
            anchor(),
            SceneryMeshOptions::near(anchor()),
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap();
        if is_ground {
            assert_eq!(batch.mesh.count_vertices(), 0);
            let ground = batch.ground.unwrap();
            assert_eq!(ground.mesh.count_vertices(), batch.statistics.vertices);
            assert_eq!(ground.lifts.len(), ground.mesh.count_vertices());
            assert!(
                ground
                    .lifts
                    .iter()
                    .all(|lift| lift.is_finite() && *lift > 0.0 && *lift < 1.0)
            );
        } else {
            assert!(batch.ground.is_none());
            assert_eq!(batch.mesh.count_vertices(), batch.statistics.vertices);
        }
    }
}

#[test]
fn caller_ground_cap_does_not_remove_solid_buildings_or_exceed_total_budget() {
    let mut fixture = Fixture::buildings();
    fixture.buildings.truncate(16);
    fixture.roads = Fixture::roads().roads;
    fixture.roads.truncate(16);
    for ground_cap in [0, 3, 7, 63, 511, 1024] {
        let mut options = SceneryMeshOptions::near(anchor());
        options.max_ground_vertices = ground_cap;
        let batch = scenery_mesh(
            &fixture.features(),
            anchor(),
            options,
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        )
        .unwrap();
        let ground_vertices = batch
            .ground
            .as_ref()
            .map_or(0, |ground| ground.mesh.count_vertices());
        assert!(ground_vertices <= ground_cap);
        assert_eq!(batch.statistics.buildings, 16);
        assert!(batch.mesh.count_vertices() > 0);
        assert_eq!(
            batch.mesh.count_vertices() + ground_vertices,
            batch.statistics.vertices
        );
        assert!(batch.statistics.vertices <= MAX_BATCH_VERTICES);
    }
}

#[test]
fn first_optional_overlay_swap_from_empty_registry_blocks_second_until_commit_or_clear() {
    use bevy::prelude::*;
    use flightsim_render::terrain_drape::{DrapeError, TerrainOverlay};
    use flightsim_render::{TerrainOverlayRegistration, TerrainTiles};
    let mut world = World::new();
    let mut meshes = Assets::<Mesh>::default();
    let mut tiles = TerrainTiles::default();
    let mut registration = || {
        let source = Fixture::roads();
        let batch = scenery_mesh(
            &source.features()[..1],
            anchor(),
            SceneryMeshOptions::near(anchor()),
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        )
        .unwrap();
        let ground = batch.ground.unwrap();
        let template =
            TerrainOverlay::surface(&ground.mesh, batch.origin, |_| Meters::ZERO).unwrap();
        let mesh = meshes.add(ground.mesh);
        let entity = world.spawn((Mesh3d(mesh.clone()), Visibility::Hidden)).id();
        TerrainOverlayRegistration {
            entity,
            mesh,
            source: template,
        }
    };
    let first = registration();
    let first_entity = first.entity;
    let swap = tiles.replace_optional_overlays(vec![first]).unwrap();
    assert!(
        swap.retired.is_empty(),
        "first admission has no old scene to use as a pending marker"
    );
    let before = tiles.overlay_usage();
    assert!(before.optional_swap_pending);
    assert_eq!(before.optional_registered, 1);
    let second = registration();
    assert!(matches!(
        tiles.replace_optional_overlays(vec![second]),
        Err(DrapeError::UpdatePending)
    ));
    assert_eq!(
        tiles.overlay_usage(),
        before,
        "rejection must not alter the pending cohort"
    );
    let cleared = tiles.clear_optional_overlays();
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0].0, first_entity);
    assert_eq!(tiles.overlay_usage().optional_registered, 0);
    assert!(!tiles.overlay_usage().optional_swap_pending);
    assert!(
        tiles
            .replace_optional_overlays(vec![registration()])
            .is_ok()
    );
}

fn convex_interiors_overlap(a: &[glam::DVec2], b: &[glam::DVec2]) -> bool {
    for polygon in [a, b] {
        for (p, q) in polygon
            .iter()
            .zip(polygon.iter().cycle().skip(1))
            .take(polygon.len())
        {
            let edge = q - p;
            let axis = glam::DVec2::new(-edge.y, edge.x).normalize();
            let interval = |vertices: &[glam::DVec2]| {
                vertices
                    .iter()
                    .map(|v| v.dot(axis))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(v), hi.max(v))
                    })
            };
            let (a_min, a_max) = interval(a);
            let (b_min, b_max) = interval(b);
            if a_max <= b_min + 0.001 || b_max <= a_min + 0.001 {
                return false;
            }
        }
    }
    true
}

#[test]
fn crossing_optional_road_and_land_cannot_overpaint_runway_or_apron() {
    use bevy::mesh::VertexAttributeValues;
    use bevy::prelude::Mesh;
    use flightsim_core::{Ecef, LocalFrame};
    use flightsim_render::scenery_exclusions::GroundSceneryExclusions;
    use flightsim_world::scenery::{
        LandCoverClass, RoadClass, SceneryFeatureRef, SceneryLandCover, SceneryRoad,
    };
    let runway = [fixtures::point(0.0, 0.0), fixtures::point(200.0, 0.0)];
    let apron = [
        fixtures::point(50.0, 45.0),
        fixtures::point(150.0, 45.0),
        fixtures::point(100.0, 100.0),
    ];
    let mut mask = GroundSceneryExclusions::default();
    mask.add_segment(runway[0], runway[1], Meters(20.0));
    mask.add_polygon(&apron);
    let road = SceneryRoad {
        source_id: 80_001,
        class: RoadClass::Residential,
        width: Meters(6.0),
        width_inferred: false,
        points: vec![
            fixtures::point(100.0, -150.0),
            fixtures::point(100.0, 150.0),
        ],
    };
    let land = SceneryLandCover {
        source_id: 80_002,
        class: LandCoverClass::Grass,
        boundary: vec![
            fixtures::point(-100.0, -200.0),
            fixtures::point(-100.0, 200.0),
            fixtures::point(300.0, 200.0),
            fixtures::point(300.0, -200.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    };
    let before = (road.clone(), land.clone());
    let features = [
        SceneryFeatureRef::Road(&road),
        SceneryFeatureRef::LandCover(&land),
    ];
    let mut options = SceneryMeshOptions::near(anchor());
    options.vegetation = false;
    let unmasked = scenery_mesh(
        &features,
        anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|_| false,
    )
    .unwrap();
    options.airport_ground_exclusions = Some(&mask);
    let masked = scenery_mesh(
        &features,
        anchor(),
        options,
        &mut |_| Some(Meters::ZERO),
        &|_| false,
    )
    .unwrap();
    assert!(!mask.is_suppressed());
    assert_eq!(
        (road, land),
        before,
        "display precedence must never alter source geography"
    );
    assert!(
        masked.statistics.vertices > 0,
        "retain scenery away from the airport"
    );
    assert!(masked.statistics.vertices < unmasked.statistics.vertices);
    let local = LocalFrame::new(anchor());
    let polygons = [
        vec![
            glam::DVec2::new(0.0, -20.0),
            glam::DVec2::new(200.0, -20.0),
            glam::DVec2::new(200.0, 20.0),
            glam::DVec2::new(0.0, 20.0),
        ],
        vec![
            glam::DVec2::new(50.0, 45.0),
            glam::DVec2::new(150.0, 45.0),
            glam::DVec2::new(100.0, 100.0),
        ],
    ];
    let overlaps = |batch: &flightsim_render::scenery::SceneryMesh| {
        let ground = &batch.ground.as_ref().unwrap().mesh;
        let Some(VertexAttributeValues::Float32x3(vertices)) =
            ground.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        ground
            .indices()
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
            .chunks_exact(3)
            .filter(|indices| {
                let triangle: Vec<_> = indices
                    .iter()
                    .map(|&i| {
                        let p = local.ecef_to_ned_position(Ecef::from_vec(
                            batch.origin.as_vec() + glam::Vec3::from_array(vertices[i]).as_dvec3(),
                        ));
                        glam::DVec2::new(p.north(), p.east())
                    })
                    .collect();
                polygons
                    .iter()
                    .any(|airport| convex_interiors_overlap(&triangle, airport))
            })
            .count()
    };
    assert!(
        overlaps(&unmasked) > 0,
        "fixture must actually cross known pavement"
    );
    assert_eq!(
        overlaps(&masked),
        0,
        "road paint and land triangles must not overpaint airport surfaces"
    );
}

#[test]
fn malformed_or_exhausted_airport_masks_fail_closed_without_removing_solids() {
    use flightsim_render::scenery_exclusions::GroundSceneryExclusions;
    let mut fixture = Fixture::buildings();
    fixture.buildings.truncate(1);
    fixture.roads = Fixture::roads().roads;
    fixture.roads.truncate(1);
    for invalid in [true, false] {
        let mut mask = GroundSceneryExclusions::default();
        if invalid {
            mask.add_segment(anchor(), anchor(), Meters(f64::NAN));
        } else {
            for _ in 0..16_385 {
                mask.add_segment(anchor(), fixtures::point(1.0, 0.0), Meters(1.0));
            }
        }
        assert!(mask.is_suppressed());
        let mut options = SceneryMeshOptions::near(anchor());
        options.airport_ground_exclusions = Some(&mask);
        let batch = scenery_mesh(
            &fixture.features(),
            anchor(),
            options,
            &mut |_| Some(Meters::ZERO),
            &|_| false,
        )
        .unwrap();
        assert!(batch.ground.is_none());
        assert_eq!(batch.statistics.buildings, 1);
        assert!(batch.mesh.count_vertices() > 0);
    }
}

#[test]
fn long_airport_segments_exclude_surface_midpoints_despite_ecef_chord_curvature() {
    use flightsim_core::{Geodetic, LocalFrame, Ned};
    use flightsim_render::scenery_exclusions::GroundSceneryExclusions;
    for center in [
        anchor(),
        Geodetic::from_degrees(0.0, 179.99, 0.0),
        Geodetic::from_degrees(89.9, 70.0, 0.0),
    ] {
        let frame = LocalFrame::new(center);
        let point = |north, east| {
            let p = frame
                .ned_to_ecef_position(Ned::new(north, east, 0.0))
                .to_geodetic();
            Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
        };
        for length in [1_000.0, 10_000.0, 20_000.0, 30_000.0, 39_000.0] {
            let mut mask = GroundSceneryExclusions::default();
            mask.add_segment(
                point(-length / 2.0, 0.0),
                point(length / 2.0, 0.0),
                Meters(7.5),
            );
            assert!(!mask.is_suppressed());
            let middle =
                [point(-1.0, -1.0), point(1.0, -1.0), point(0.0, 1.0)].map(Geodetic::to_ecef);
            assert!(
                mask.intersects_triangle(middle, &mut 10_000),
                "{length}m segment at {center:?}: a straight ECEF chord is below the surface midpoint"
            );
            let away =
                [point(-1.0, 100.0), point(1.0, 100.0), point(0.0, 102.0)].map(Geodetic::to_ecef);
            assert!(
                !mask.intersects_triangle(away, &mut 10_000),
                "retain ground 100 m away from a 15 m-wide pavement"
            );
        }
    }
}
