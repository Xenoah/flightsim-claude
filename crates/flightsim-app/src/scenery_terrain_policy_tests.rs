//! Independent policy checks; no GPU, filesystem fixtures or worker tasks.

use super::*;
use flightsim_world::scenery::{RoadClass, SceneryRoad, ScenerySource};
use flightsim_world::{DemTile, HeightGrid, TileId};

fn anchor() -> Geodetic {
    Geodetic::from_degrees(47.127, 9.529, 0.0)
}

fn point(north: f64, east: f64) -> Geodetic {
    let point = LocalFrame::new(anchor())
        .ned_to_ecef_position(Ned::new(north, east, 0.0))
        .to_geodetic();
    Geodetic::new(point.latitude, point.longitude, Meters::ZERO)
}

fn road(id: i64, north: f64, width: f64) -> SceneryRoad {
    SceneryRoad {
        source_id: id,
        class: RoadClass::Primary,
        width: Meters(width),
        width_inferred: true,
        points: vec![point(north, -100.0), point(north, 100.0)],
    }
}

fn database(roads: Vec<SceneryRoad>) -> SceneryDatabase {
    SceneryDatabase::new(
        ScenerySource {
            kind: ScenerySourceKind::Synthetic,
            name: "regional terrain policy test".into(),
            url: "urn:flightsim:test:regional-terrain-policy".into(),
            input_fingerprint: 1,
        },
        roads,
        vec![],
        vec![],
    )
    .unwrap()
}

fn policy_runtime(with_pack: bool) -> SceneryRuntime {
    let mut runtime = SceneryRuntime::new(&Startup::default()).unwrap();
    if with_pack {
        runtime.database = Some(Arc::new(database(vec![road(1, 0.0, 10.0)])));
    }
    runtime
}

fn selector(max_level: u8) -> LodSelector {
    // Suppress ordinary SSE refinement so the regional floor is independently
    // observable rather than accidentally covered by an already-fine cut.
    LodSelector::new(1.0e12, 1080.0, Radians(1.0), max_level, Meters(20_000.0))
}

fn dem(id: TileId, width: u32, height: u32) -> DemTile {
    DemTile::new(id.bounds(), HeightGrid::flat(width, height, Meters(643.0)))
}

#[test]
fn no_pack_and_remote_pack_restore_exact_baseline_selection() {
    let base = selector(13);
    let configured = base.with_near_detail(TERRAIN_DETAIL_RADIUS, 13);
    for position in [anchor(), Geodetic::from_degrees(90.0, 180.0, 1000.0)] {
        assert_eq!(
            policy_runtime(false)
                .terrain_selector(configured, position)
                .select(position.to_ecef()),
            base.select(position.to_ecef())
        );
    }
    let remote = Geodetic::from_degrees(-30.0, -170.0, 5000.0);
    assert_eq!(
        policy_runtime(true)
            .terrain_selector(configured, remote)
            .select(remote.to_ecef()),
        base.select(remote.to_ecef())
    );
}

#[test]
fn loaded_pack_floor_is_agl_independent_and_respects_maximum_level() {
    let runtime = policy_runtime(true);
    for altitude in [0.0, 1000.0, 20_000.0] {
        let camera = Geodetic::new(anchor().latitude, anchor().longitude, Meters(altitude));
        for maximum in [11, 13] {
            let selected = runtime
                .terrain_selector(selector(maximum), camera)
                .select(camera.to_ecef());
            assert!(!selected.truncated);
            assert!(
                selected
                    .tiles
                    .contains(&TileId::containing(maximum, camera))
            );
            assert!(selected.tiles.iter().all(|tile| tile.level <= maximum));
        }
    }
}

#[test]
fn relocation_and_runtime_reset_cannot_retain_or_erase_regional_policy() {
    let mut runtime = policy_runtime(true);
    let base = selector(13);
    let close = anchor();
    let far = point(100_000.0, 0.0);
    let initial = runtime
        .terrain_selector(base, close)
        .select(close.to_ecef());
    // Reuse a previously decorated selector deliberately: leaving coverage must
    // remove its floor, and returning must recover it from immutable pack facts.
    let decorated = runtime.terrain_selector(base, close);
    assert_eq!(
        runtime
            .terrain_selector(decorated, far)
            .select(far.to_ecef()),
        base.select(far.to_ecef())
    );
    runtime.selected_at = Some(far);
    runtime.reset_revision = 7;
    let retained_database = runtime.database.as_ref().unwrap().clone();
    assert!(reset_owned_assets(&mut runtime, None).is_empty());
    assert!(Arc::ptr_eq(
        runtime.database.as_ref().unwrap(),
        &retained_database
    ));
    assert_eq!(
        runtime
            .terrain_selector(base, close)
            .select(close.to_ecef()),
        initial
    );
}

#[test]
fn regional_mesh_density_requires_both_axes_level_and_immutable_coverage() {
    let runtime = policy_runtime(true);
    for level in [11, 12, 13] {
        let id = TileId::containing(level, anchor());
        for (width, height) in [(33, 33), (64, 65), (65, 64), (65, 65), (129, 65)] {
            let source = dem(id, width, height);
            let before = source.clone();
            let options = runtime.terrain_mesh_options(id, &source);
            let expected = if level >= 12 && width >= 65 && height >= 65 {
                65
            } else {
                33
            };
            assert_eq!(options.resolution, expected);
            assert_eq!(
                options.skirt_depth,
                flightsim_render::mesh_options_for(level).skirt_depth
            );
            assert_eq!(
                source, before,
                "mesh policy must not modify physical samples"
            );
            assert_eq!(
                policy_runtime(false)
                    .terrain_mesh_options(id, &source)
                    .resolution,
                33
            );
        }
    }
    let outside = TileId::containing(13, Geodetic::from_degrees(-30.0, -170.0, 0.0));
    assert_eq!(
        runtime
            .terrain_mesh_options(outside, &dem(outside, 65, 65))
            .resolution,
        33
    );
}

#[test]
fn mesh_density_is_stable_across_observation_relocation_and_reset() {
    let mut runtime = policy_runtime(true);
    let id = TileId::containing(13, anchor());
    let source = dem(id, 65, 65);
    let before = runtime.terrain_mesh_options(id, &source);
    runtime.selected_at = Some(point(100_000.0, 0.0));
    let _ = runtime.terrain_selector(selector(13), runtime.selected_at.unwrap());
    assert!(reset_owned_assets(&mut runtime, None).is_empty());
    let after = runtime.terrain_mesh_options(id, &source);
    assert_eq!(before.resolution, after.resolution);
    assert_eq!(before.skirt_depth, after.skirt_depth);
}

#[test]
fn tree_neighbor_query_includes_wide_road_beyond_tree_candidate_radius() {
    let db = database(vec![road(1, 1840.0, 100.0)]);
    let observer = anchor();
    // The centreline is outside the tree radius, but its maximum validated
    // half-width and clearance overlap an eligible tree candidate.
    assert!(
        db.query_near(observer, Meters(1800.0), MAX_SELECTED)
            .is_empty()
    );
    let neighbors = db.query_near(observer, TREE_EXCLUSION_RADIUS, MAX_SELECTED);
    assert_eq!(neighbors.len(), 1);
    let candidate = point(1790.0, 0.0);
    assert!(candidate.great_circle_distance(observer).get() < 1800.0);
    let startup = Startup::default();
    assert!(!Exclusions::new(&[], &startup).contains(candidate));
    assert!(Exclusions::new(&neighbors, &startup).contains(candidate));
}

#[test]
fn narrower_tree_query_avoids_unrelated_outer_view_truncation() {
    let mut roads = vec![road(1, 1840.0, 100.0)];
    roads.extend((2..=4097).map(|id| road(id, 3000.0, 10.0)));
    let db = database(roads);
    assert_eq!(
        db.query_near(anchor(), VIEW_RADIUS, MAX_SELECTED).len(),
        MAX_SELECTED
    );
    let neighbors = db.query_near(anchor(), TREE_EXCLUSION_RADIUS, MAX_SELECTED);
    assert_eq!(neighbors.len(), 1);
    let exclusions = Exclusions::new(&neighbors, &Startup::default());
    assert!(!exclusions.suppress_all);
    assert!(exclusions.contains(point(1790.0, 0.0)));
}
