//! Independent actual-triangle regressions for mixed-LOD render bridges.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use std::collections::{BTreeMap, BTreeSet};

use flightsim_core::Meters;
use flightsim_world::{
    DemTile, HeightGrid, MeshOptions, TerrainBoundary, TerrainEdge, TerrainMesh, TerrainSeam,
    TerrainSeamPlanner, TileId, build_mesh, global::GlobalTerrain, plan_seams,
};
use glam::DVec3;

fn point(mesh: &TerrainMesh, index: usize) -> DVec3 {
    mesh.origin.as_vec() + DVec3::from_array(mesh.positions[index].map(f64::from))
}

fn flat(id: TileId, elevation: f64, resolution: u32) -> TerrainMesh {
    build_mesh(
        id,
        &DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(elevation))),
        &MeshOptions {
            resolution,
            ..MeshOptions::default()
        },
    )
}

fn prepare(
    meshes: &[(TileId, TerrainMesh)],
) -> (BTreeMap<TileId, TerrainBoundary>, Vec<TerrainSeam>) {
    let boundaries: BTreeMap<_, _> = meshes
        .iter()
        .map(|(tile, mesh)| (*tile, TerrainBoundary::from_mesh(*tile, mesh)))
        .collect();
    let visible = boundaries.keys().copied().collect();
    let seams = plan_seams(&boundaries, &visible);
    // Every actual-triangle fixture also checks exact descriptor/order equality
    // against the independent synchronous planner with a tiny work budget.
    let incremental = incremental_plan(&boundaries, &visible, 7);
    assert_eq!(incremental, seams);
    (boundaries, seams)
}

fn incremental_plan(
    boundaries: &BTreeMap<TileId, TerrainBoundary>,
    visible: &BTreeSet<TileId>,
    budget: usize,
) -> Vec<TerrainSeam> {
    let mut planner = TerrainSeamPlanner::default();
    let mut seams = Vec::new();
    let mut total = 0;
    let polar_vertices: usize = boundaries.values().map(TerrainBoundary::vertex_count).sum();
    // At most 45N non-polar units plus all possible polar vertices and a fixed
    // number of phase transitions, including scratch cleanup.
    let work_bound = 45 * visible.len() + polar_vertices + 16;
    while !planner.is_finished() {
        let worked = planner.advance(boundaries, visible, budget, |seam| seams.push(seam));
        assert!(worked > 0 && worked <= budget);
        total += worked;
        assert!(total <= work_bound, "planner did not make bounded progress");
        let usage = planner.resource_usage();
        assert!(usage.indexed_edges <= 4 * visible.len());
        assert!(usage.adjacent_pairs <= 4 * visible.len());
        assert!(usage.corners <= 8 * visible.len() + 2);
        assert!(usage.polar_edges <= 2 * visible.len());
        assert!(usage.assigned_caps <= 2 * visible.len());
    }
    assert_eq!(planner.resource_usage(), Default::default());
    assert_eq!(
        planner.advance(boundaries, visible, budget, |_| panic!(
            "finished planner emitted"
        )),
        0
    );
    seams
}

#[test]
fn incremental_planning_handles_empty_missing_and_arbitrary_lod_source_changes() {
    let mut planner = TerrainSeamPlanner::default();
    assert_eq!(
        planner.advance(&BTreeMap::new(), &BTreeSet::new(), 0, |_| unreachable!()),
        0
    );
    assert!(!planner.is_finished());
    assert!(incremental_plan(&BTreeMap::new(), &BTreeSet::new(), 1).is_empty());

    let mut visible: BTreeSet<_> = TileId::roots().into_iter().collect();
    // Deterministic, deliberately unbalanced refinements reaching L24. This
    // samples dateline, poles and ordinary junctions without assuming 2:1 LOD.
    let mut tip = TileId::roots()[0];
    for level in 0..24 {
        visible.remove(&tip);
        let children = tip.children().unwrap();
        visible.extend(children);
        tip = children[if level % 3 == 0 { 1 } else { 0 }];
    }
    let mut boundaries: BTreeMap<_, _> = visible
        .iter()
        .enumerate()
        .map(|(index, &tile)| {
            let source = flat(
                tile,
                if index % 2 == 0 { -500.0 } else { 1_700.0 },
                if index % 3 == 0 { 18 } else { 31 },
            );
            (tile, TerrainBoundary::from_mesh(tile, &source))
        })
        .collect();
    for revision in 0..4 {
        let expected = plan_seams(&boundaries, &visible);
        for budget in [1, 13, 1_024, usize::MAX] {
            assert_eq!(incremental_plan(&boundaries, &visible, budget), expected);
        }
        let tile = *visible.iter().nth(revision * 13).unwrap();
        if revision % 2 == 0 {
            let source = flat(tile, -3_000.0, 17);
            boundaries.insert(tile, TerrainBoundary::from_mesh(tile, &source));
        } else {
            boundaries.remove(&tile); // Missing sources are omitted identically.
        }
    }
}

#[test]
fn cancelling_across_planning_stages_leaves_no_state_in_a_fresh_plan() {
    let meshes: Vec<_> = TileId::roots()
        .into_iter()
        .flat_map(|tile| tile.children().unwrap())
        .map(|tile| (tile, flat(tile, 123.0, 33)))
        .collect();
    let (boundaries, expected) = prepare(&meshes);
    let visible = boundaries.keys().copied().collect();
    for cancel_after in [0, 1, 8, 20, 50, 100, 200, 400, 800] {
        let mut cancelled = TerrainSeamPlanner::default();
        let mut emitted = Vec::new();
        cancelled.advance(&boundaries, &visible, cancel_after, |seam| {
            emitted.push(seam)
        });
        drop(cancelled);
        drop(emitted);
        assert_eq!(incremental_plan(&boundaries, &visible, 1), expected);
    }
}

// Moller-Trumbore, independent of the seam builder, accepting either winding.
fn segment_hits(origin: DVec3, delta: DVec3, a: DVec3, b: DVec3, c: DVec3) -> bool {
    let ab = b - a;
    let ac = c - a;
    let h = delta.cross(ac);
    let determinant = ab.dot(h);
    if determinant.abs() < 1.0e-12 {
        return false;
    }
    let inverse = determinant.recip();
    let relative = origin - a;
    let u = relative.dot(h) * inverse;
    let q = relative.cross(ab);
    let v = delta.dot(q) * inverse;
    let t = ac.dot(q) * inverse;
    (-1.0e-7..=1.0 + 1.0e-7).contains(&u)
        && v >= -1.0e-7
        && u + v <= 1.0 + 1.0e-7
        && (-1.0e-7..=1.0 + 1.0e-7).contains(&t)
}

fn hits(mesh: &TerrainMesh, target: DVec3, normal: DVec3, half_length: f64) -> bool {
    mesh.indices.chunks_exact(3).any(|triangle| {
        segment_hits(
            target - normal * half_length,
            normal * (2.0 * half_length),
            point(mesh, triangle[0] as usize),
            point(mesh, triangle[1] as usize),
            point(mesh, triangle[2] as usize),
        )
    })
}

#[test]
fn flat_and_bundled_proven_aperture_is_covered_by_actual_bridge_triangles() {
    let atlas = GlobalTerrain::bundled().unwrap();
    for (coarse, fine, fine_row) in [
        (TileId::new(3, 1, 4), TileId::new(4, 4, 8), 17),
        (TileId::new(4, 3, 8), TileId::new(5, 8, 16), 17),
        (TileId::new(3, 1, 4), TileId::new(7, 32, 68), 8),
    ] {
        for bundled in [false, true] {
            let make = |id| {
                if bundled {
                    build_mesh(id, &atlas.tile(id).unwrap(), &MeshOptions::default())
                } else {
                    flat(id, 0.0, 33)
                }
            };
            let meshes = [(coarse, make(coarse)), (fine, make(fine))];
            let coarse_mesh = &meshes[0].1;
            let fine_mesh = &meshes[1].1;
            let coarse_top =
                (point(coarse_mesh, 8 * 33 + 32) + point(coarse_mesh, 9 * 33 + 32)) * 0.5;
            let fine_bottom = point(fine_mesh, 1089 + 96 + (32 - fine_row));
            let normal = point(coarse_mesh, 8 * 33 + 32)
                .cross(point(coarse_mesh, 9 * 33 + 32))
                .normalize();
            let target = (coarse_top + fine_bottom) * 0.5;
            let up = point(fine_mesh, fine_row * 33).normalize();
            assert!(
                (fine_bottom - coarse_top).dot(up) > if coarse.level == 3 { 90.0 } else { 5.0 }
            );
            assert!(!hits(coarse_mesh, target, normal, 100.0));
            assert!(!hits(fine_mesh, target, normal, 100.0));
            let (boundaries, seams) = prepare(&meshes);
            assert_eq!(seams.len(), 1);
            let bridge = seams[0].build_mesh(&boundaries);
            assert!(hits(&bridge, target, normal, 100.0), "bundled={bundled}");
            assert!(
                hits(&bridge, target, -normal, 100.0),
                "reverse ray bundled={bundled}"
            );
        }
    }
}

fn edge_point(mesh: &TerrainMesh, resolution: usize, east: bool, fraction: f64) -> DVec3 {
    let step = (fraction * (resolution - 1) as f64).clamp(0.0, (resolution - 1) as f64);
    let index = (step.floor() as usize).min(resolution - 2);
    let column = if east { resolution - 1 } else { 0 };
    point(mesh, index * resolution + column).lerp(
        point(mesh, (index + 1) * resolution + column),
        step - index as f64,
    )
}

#[test]
fn arbitrary_lod_and_resolution_differences_cover_the_entire_shared_interval() {
    // Difference 24 is deliberately much larger than any ordinary camera cut.
    // Relatively prime edge resolutions also exercise non-dyadic knot merging.
    for coarse_level in 0..=13 {
        for (coarse_height, fine_height) in [(0.0, 0.0), (-700.0, 3500.0), (2500.0, -300.0)] {
            let coarse = TileId::new(coarse_level, 0, TileId::rows(coarse_level) / 2);
            let fine_level = 24;
            let scale = 1_u32 << (fine_level - coarse_level);
            let fine = TileId::new(fine_level, scale, coarse.y * scale + scale / 3);
            let meshes = [
                (coarse, flat(coarse, coarse_height, 18)),
                (fine, flat(fine, fine_height, 31)),
            ];
            let (boundaries, seams) = prepare(&meshes);
            assert_eq!(seams.len(), 1);
            let seam = &seams[0];
            let bridge = seam.build_mesh(&boundaries);
            let (vertices, indices) = seam.mesh_size_bound(&boundaries);
            assert!(bridge.positions.len() <= vertices && bridge.indices.len() <= indices);
            let coarse_offset = f64::from(fine.y - coarse.y * scale) / f64::from(scale);
            for step in 1..60 {
                let t = f64::from(step) / 60.0;
                let a = edge_point(&meshes[0].1, 18, true, coarse_offset + t / f64::from(scale));
                let b = edge_point(&meshes[1].1, 31, false, t);
                let along = edge_point(&meshes[1].1, 31, false, (t + 0.001).min(1.0)) - b;
                let normal = along.cross(b - a).normalize_or_zero();
                if normal.length_squared() < 0.5 || a.distance(b) < 0.01 {
                    continue;
                }
                for across in [0.2, 0.5, 0.8] {
                    let target = a.lerp(b, across);
                    assert!(
                        hits(&bridge, target, normal, 2.0),
                        "LOD {coarse_level}/24, heights {coarse_height}/{fine_height}, t={t}, across={across}"
                    );
                }
            }
        }
    }
}

#[test]
fn post_f32_boundary_is_retained_and_reencoding_has_an_explicit_bound() {
    for level in [0, 3, 7, 13, 24] {
        let a = TileId::new(level, 0, 0);
        let b = TileId::new(level, 1, 0);
        let meshes = [(a, flat(a, 400.0, 33)), (b, flat(b, -50.0, 33))];
        let (boundaries, seams) = prepare(&meshes);
        assert_eq!(boundaries[&a].vertex_count(), 132);
        assert!(boundaries[&a].memory_footprint() < 8_000);
        assert_eq!(
            boundaries[&a].edge(TerrainEdge::East)[9]
                .position
                .map(f32::to_bits),
            meshes[0].1.positions[9 * 33 + 32].map(f32::to_bits)
        );
        let seam = seams
            .iter()
            .find(|seam| seam.key().edge == TerrainEdge::East)
            .unwrap();
        let bridge = seam.build_mesh(&boundaries);
        let bound = TerrainSeam::encoding_error_bound(&bridge).get();
        let mut largest_error = 0.0_f64;
        for row in 0..33 {
            largest_error = largest_error
                .max(point(&bridge, row * 2).distance(point(&meshes[0].1, row * 33 + 32)));
            largest_error = largest_error
                .max(point(&bridge, row * 2 + 1).distance(point(&meshes[1].1, row * 33)));
            assert!(point(&bridge, row * 2).distance(point(&meshes[0].1, row * 33 + 32)) <= bound);
            assert!(point(&bridge, row * 2 + 1).distance(point(&meshes[1].1, row * 33)) <= bound);
        }
        eprintln!(
            "seam precision level={level} attachment_error_m={largest_error:.9} conservative_bound_m={bound:.9} boundary_bytes={}",
            boundaries[&a].memory_footprint()
        );
        assert!(
            bound < 2.0,
            "root-scale bound remains metre-scale, got {bound}"
        );
        if level >= 7 {
            assert!(bound < 0.03);
        }
        for triangle in bridge.indices.chunks_exact(6) {
            assert_eq!(
                [triangle[0], triangle[1], triangle[2]],
                [triangle[3], triangle[5], triangle[4]]
            );
        }
        assert!(
            bridge
                .positions
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(
            bridge
                .normals
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
    }
}

#[test]
fn dateline_and_prime_meridian_have_distinct_keys_and_polar_caps() {
    let roots = TileId::roots();
    let meshes = [
        (roots[0], flat(roots[0], 300.0, 17)),
        (roots[1], flat(roots[1], -100.0, 33)),
    ];
    let (boundaries, seams) = prepare(&meshes);
    assert_eq!(seams.len(), 2);
    assert_ne!(seams[0].key(), seams[1].key());
    assert_eq!(seams[0].tiles(), roots);
    let mut extra = 0;
    for seam in seams {
        let bridge = seam.build_mesh(&boundaries);
        let (max_vertices, max_indices) = seam.mesh_size_bound(&boundaries);
        assert!(bridge.positions.len() <= max_vertices && bridge.indices.len() <= max_indices);
        assert!(
            bridge
                .positions
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        extra += bridge.positions.len() - (33 * 2 + 2);
    }
    // Exactly one north and one south fan for each root, each with an anchor.
    assert_eq!(extra, 2 * (17 + 1) + 2 * (33 + 1));
}

#[test]
fn integer_sweep_matches_brute_force_for_an_unbalanced_dateline_cut() {
    let mut visible: BTreeSet<_> = TileId::roots().into_iter().collect();
    let mut cursor = TileId::roots()[0];
    for level in 1..=13 {
        visible.remove(&cursor);
        let children = cursor.children().unwrap();
        visible.extend(children);
        cursor = children[(level % 4) as usize];
    }
    let meshes: Vec<_> = visible.iter().map(|id| (*id, flat(*id, 0.0, 2))).collect();
    let (_, seams) = prepare(&meshes);
    let mut expected = 0;
    // Independent integer rectangles at level 24, deliberately quadratic in test.
    for &a in &visible {
        for &b in visible.range((std::ops::Bound::Excluded(a), std::ops::Bound::Unbounded)) {
            let sa = 1_u32 << (24 - a.level);
            let sb = 1_u32 << (24 - b.level);
            let ax = a.x * sa;
            let ay = a.y * sa;
            let bx = b.x * sb;
            let by = b.y * sb;
            let vertical_overlap = ay.max(by) < (ay + sa).min(by + sb);
            let horizontal_overlap = ax.max(bx) < (ax + sa).min(bx + sb);
            expected += usize::from(vertical_overlap && (ax + sa) % (2 << 24) == bx);
            expected += usize::from(vertical_overlap && (bx + sb) % (2 << 24) == ax);
            expected += usize::from(horizontal_overlap && ay + sa == by);
            expected += usize::from(horizontal_overlap && by + sb == ay);
        }
    }
    assert_eq!(seams.len(), expected);
    assert!(seams.len() <= 4 * visible.len());
    assert!(seams.windows(2).all(|pair| pair[0].key() < pair[1].key()));
}

#[test]
fn crossing_source_profiles_are_covered_on_both_sides() {
    let west = TileId::new(4, 11, 8);
    let east = TileId::new(4, 12, 8);
    let make = |id: TileId, sign: f32, resolution: u32| {
        let heights = (0..33)
            .flat_map(|row| {
                (0..33).map(move |_| (f64::from(row) / 32.0 * 4000.0 - 2000.0) as f32 * sign)
            })
            .collect();
        build_mesh(
            id,
            &DemTile::new(id.bounds(), HeightGrid::new(33, 33, heights)),
            &MeshOptions {
                resolution,
                ..MeshOptions::default()
            },
        )
    };
    let meshes = [(west, make(west, 1.0, 18)), (east, make(east, -1.0, 37))];
    let (boundaries, seams) = prepare(&meshes);
    let bridge = seams[0].build_mesh(&boundaries);
    for step in 0..100 {
        let t = (f64::from(step) + 0.5) / 100.0;
        let a = edge_point(&meshes[0].1, 18, true, t);
        let b = edge_point(&meshes[1].1, 37, false, t);
        let tangent = edge_point(&meshes[1].1, 37, false, t + 0.0001) - b;
        let normal = tangent.cross(b - a).normalize();
        for across in [0.1, 0.3, 0.7, 0.9] {
            let target = a.lerp(b, across);
            assert!(
                hits(&bridge, target, normal, 2.0),
                "crossing t={t} across={across}"
            );
            assert!(hits(&bridge, target, -normal, 2.0));
        }
    }
}

fn set_point(mesh: &mut TerrainMesh, index: usize, desired: DVec3) {
    mesh.positions[index] = (desired - mesh.origin.as_vec())
        .to_array()
        .map(|x| x as f32);
}

fn only_endpoint_caps(mut mesh: TerrainMesh) -> TerrainMesh {
    // Both callers use five aligned shared-edge knots, i.e. ten ribbon vertices.
    let first_anchor = 10;
    mesh.indices = mesh
        .indices
        .chunks_exact(3)
        .filter(|triangle| triangle.iter().any(|index| *index >= first_anchor))
        .flatten()
        .copied()
        .collect();
    mesh
}

fn assert_t_junction_caps(coarse_x: u32, fine_x: u32) {
    let coarse = TileId::new(3, coarse_x, 4);
    let north = TileId::new(4, fine_x, 8);
    let south = TileId::new(4, fine_x, 9);
    let mut meshes = [
        (coarse, flat(coarse, 0.0, 5)),
        (north, flat(north, 0.0, 5)),
        (south, flat(south, 0.0, 5)),
    ];
    let base = point(&meshes[0].1, 2 * 5 + 4);
    // Exaggerated independent source-frame endpoint errors make the otherwise
    // centimetre-scale corner aperture observable without geometry tolerances.
    set_point(
        &mut meshes[0].1,
        2 * 5 + 4,
        base + DVec3::new(0.0, 12.0, 0.0),
    );
    set_point(&mut meshes[1].1, 4 * 5, base + DVec3::new(0.0, -6.0, 10.0));
    set_point(&mut meshes[2].1, 0, base + DVec3::new(0.0, -6.0, -10.0));
    let endpoints = [
        point(&meshes[0].1, 14),
        point(&meshes[1].1, 20),
        point(&meshes[2].1, 0),
    ];
    let normal = (endpoints[1] - endpoints[0])
        .cross(endpoints[2] - endpoints[0])
        .normalize();
    let (boundaries, seams) = prepare(&meshes);
    assert_eq!(seams.len(), 3);
    let caps: Vec<_> = seams
        .iter()
        .map(|seam| only_endpoint_caps(seam.build_mesh(&boundaries)))
        .collect();
    for weights in [
        [0.2, 0.3, 0.5],
        [0.7, 0.1, 0.2],
        [0.1, 0.8, 0.1],
        [0.1, 0.1, 0.8],
    ] {
        let target =
            endpoints[0] * weights[0] + endpoints[1] * weights[1] + endpoints[2] * weights[2];
        assert!(
            caps.iter().any(|mesh| hits(mesh, target, normal, 1.0)),
            "T-junction coarse_x={coarse_x}, weights={weights:?}"
        );
    }
    let old = seams
        .iter()
        .find(|seam| seam.tiles() == [coarse, north])
        .unwrap();
    set_point(&mut meshes[2].1, 0, endpoints[2] + DVec3::Y);
    let (_, revised) = prepare(&meshes);
    let new = revised.iter().find(|seam| seam.key() == old.key()).unwrap();
    assert_ne!(
        old, new,
        "third-neighbour endpoint changes invalidate the common cap"
    );
}

#[test]
fn four_way_corner_caps_close_the_polygon_and_are_deterministic() {
    let ids = [
        TileId::new(4, 15, 7),
        TileId::new(4, 16, 7),
        TileId::new(4, 16, 8),
        TileId::new(4, 15, 8),
    ];
    let mut meshes: Vec<_> = ids.into_iter().map(|id| (id, flat(id, 0.0, 5))).collect();
    let corner_indices = [24, 20, 0, 4];
    let base = point(&meshes[0].1, 24);
    let offsets = [
        DVec3::new(0.0, -10.0, 10.0),
        DVec3::new(0.0, 10.0, 10.0),
        DVec3::new(0.0, 10.0, -10.0),
        DVec3::new(0.0, -10.0, -10.0),
    ];
    for index in 0..4 {
        set_point(
            &mut meshes[index].1,
            corner_indices[index],
            base + offsets[index],
        );
    }
    let (boundaries, seams) = prepare(&meshes);
    assert_eq!(seams.len(), 4);
    let caps: Vec<_> = seams
        .iter()
        .map(|seam| only_endpoint_caps(seam.build_mesh(&boundaries)))
        .collect();
    for y in [-7.0, -3.0, 3.0, 7.0] {
        for z in [-7.0, -3.0, 3.0, 7.0] {
            assert!(caps.iter().any(|mesh| hits(
                mesh,
                base + DVec3::new(0.0, y, z),
                DVec3::X,
                1.0
            )));
        }
    }
    meshes.reverse();
    let (_, reversed) = prepare(&meshes);
    assert_eq!(seams, reversed);
}

#[test]
fn t_junction_caps_close_the_triangle_between_three_post_f32_endpoints() {
    assert_t_junction_caps(7, 16);
    assert_t_junction_caps(15, 0); // identical topology through longitude +/-180
}

#[test]
fn exact_poles_close_all_retained_edge_samples_to_one_shared_anchor() {
    let ids = TileId::roots();
    let mut meshes = [
        (ids[0], flat(ids[0], 0.0, 5)),
        (ids[1], flat(ids[1], 0.0, 5)),
    ];
    let north = point(&meshes[0].1, 0);
    let south = point(&meshes[0].1, 20);
    let outlines = [
        [
            [-10.0, 0.0],
            [-10.0, -10.0],
            [0.0, -10.0],
            [10.0, -10.0],
            [10.0, 0.0],
        ],
        [
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [-10.0, 10.0],
            [-10.0, 0.0],
        ],
    ];
    // Amplify source-frame XY rounding to form an unmistakable polar aperture.
    // Intermediate edge vertices are essential: corner-only fans cover no area.
    for (tile, (_, mesh)) in meshes.iter_mut().enumerate() {
        for (base, row) in [(north, 0), (south, 4)] {
            for (column, xy) in outlines[tile].iter().enumerate() {
                set_point(mesh, row * 5 + column, base + DVec3::new(xy[0], xy[1], 0.0));
            }
        }
    }
    let (boundaries, seams) = prepare(&meshes);
    for pole in [north, south] {
        let polar_triangles: Vec<_> = seams
            .iter()
            .map(|seam| {
                let mut mesh = seam.build_mesh(&boundaries);
                mesh.indices = mesh
                    .indices
                    .chunks_exact(3)
                    .filter(|triangle| {
                        triangle
                            .iter()
                            .all(|index| (point(&mesh, *index as usize).z - pole.z).abs() < 1.0)
                    })
                    .flatten()
                    .copied()
                    .collect();
                mesh
            })
            .collect();
        for x in [-7.0, -3.0, 3.0, 7.0] {
            for y in [-7.0, -3.0, 3.0, 7.0] {
                assert!(
                    polar_triangles.iter().any(|mesh| hits(
                        mesh,
                        pole + DVec3::new(x, y, 0.0),
                        DVec3::Z,
                        1.0
                    )),
                    "polar fan misses x={x}, y={y}, z={}",
                    pole.z
                );
            }
        }
    }
}

#[test]
fn edge_midpoint_source_changes_invalidate_a_plan_even_with_unchanged_corners() {
    let west = TileId::new(8, 16, 80);
    let east = TileId::new(8, 17, 80);
    let mut meshes = [(west, flat(west, 0.0, 33)), (east, flat(east, 0.0, 33))];
    let (_, old) = prepare(&meshes);
    let index = 16 * 33 + 32;
    meshes[0].1.positions[index][0] += 100.0;
    meshes[0].1.elevations[index] = 100.0;
    let (_, new) = prepare(&meshes);
    assert_eq!(old[0].key(), new[0].key());
    assert_ne!(old[0], new[0]);
}

#[test]
fn maximum_resident_sized_cut_has_linear_neighbor_and_boundary_storage() {
    let level = 6;
    let meshes: Vec<_> = (0..TileId::rows(level))
        .flat_map(|y| {
            (0..TileId::columns(level)).map(move |x| {
                let tile = TileId::new(level, x, y);
                (tile, flat(tile, 0.0, 2))
            })
        })
        .collect();
    assert_eq!(meshes.len(), 8192);
    let (boundaries, seams) = prepare(&meshes);
    // Longitude wraps; latitude ends at the two poles.
    assert_eq!(
        seams.len(),
        2 * meshes.len() - TileId::columns(level) as usize
    );
    assert!(seams.len() <= 4 * meshes.len());
    assert_eq!(
        boundaries
            .values()
            .map(TerrainBoundary::vertex_count)
            .sum::<usize>(),
        8 * meshes.len()
    );
    for index in [0, seams.len() / 2, seams.len() - 1] {
        let mesh = seams[index].build_mesh(&boundaries);
        let (vertices, indices) = seams[index].mesh_size_bound(&boundaries);
        assert!(mesh.positions.len() <= vertices && mesh.indices.len() <= indices);
    }
}

#[test]
fn actual_default_polar_cuts_bound_work_and_match_reference_at_default_resolution() {
    use flightsim_core::{Degrees, Geodetic};
    use flightsim_world::LodSelector;
    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    );
    for latitude in [-89.999, 89.999] {
        let selection = selector.select_with_surface(
            Geodetic::from_degrees(latitude, 30.0, 1_000.0).to_ecef(),
            Meters::ZERO,
        );
        assert!(selection.truncated);
        assert_eq!(selection.tiles.len(), 4_094);
        let visible: BTreeSet<_> = selection.tiles.into_iter().collect();
        // Retain only boundaries, never all full surface meshes at once.
        let boundaries: BTreeMap<_, _> = visible
            .iter()
            .map(|&id| {
                let mesh = flat(id, if id.x % 2 == 0 { -30.0 } else { 1_500.0 }, 33);
                (id, TerrainBoundary::from_mesh(id, &mesh))
            })
            .collect();
        let expected = plan_seams(&boundaries, &visible);
        assert_eq!(expected.len(), 8_185);
        assert_eq!(incremental_plan(&boundaries, &visible, 1_024), expected);
    }
}
