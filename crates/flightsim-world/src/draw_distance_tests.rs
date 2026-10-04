use super::*;
use crate::{DemTile, HeightGrid, MemoryTileSource, Terrain, TileId};
use flightsim_core::{Degrees, Geodetic};
use std::collections::BTreeSet;

fn selector() -> LodSelector {
    LodSelector::new(
        16.0,
        1080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    )
}

fn assert_complete_cut(selection: &crate::LodSelection, budget: usize) {
    assert!(selection.tiles.len() <= budget);
    let area: f64 = selection
        .tiles
        .iter()
        .map(|id| id.bounds().width().get() * id.bounds().height().get())
        .sum();
    assert!((area - core::f64::consts::TAU * core::f64::consts::PI).abs() < 1e-9);
    let ids: BTreeSet<_> = selection.tiles.iter().copied().collect();
    for id in &selection.tiles {
        assert!(id.level <= 13);
        let mut ancestor = id.parent();
        while let Some(parent) = ancestor {
            assert!(!ids.contains(&parent), "overlapping leaves");
            ancestor = parent.parent();
        }
    }
}

#[test]
fn presets_roundtrip_validate_and_standard_keeps_existing_constants() {
    for preset in DrawDistancePreset::ALL {
        assert_eq!(preset.to_string().parse(), Ok(preset));
        let p = preset.policy();
        assert_eq!(
            DrawDistancePolicy::new(
                p.refinement_radius(),
                p.scenery_radius(),
                p.terrain_detail_radius(),
                p.camera_far(),
                p.screen_space_error()
            ),
            Ok(p)
        );
    }
    for value in ["", "medium", "SHORT", "100", "unlimited"] {
        assert!(value.parse::<DrawDistancePreset>().is_err());
    }
    let standard = DrawDistancePolicy::default();
    assert_eq!(standard.refinement_radius(), None);
    assert_eq!(standard.scenery_radius(), Meters(4500.0));
    assert_eq!(standard.terrain_detail_radius(), Meters(5500.0));
    assert_eq!(standard.camera_far(), Meters(400_000.0));
    assert_eq!(standard.screen_space_error().to_bits(), 16.0_f64.to_bits());
    assert_eq!(
        DrawDistancePreset::Short.next().next().next(),
        DrawDistancePreset::Short
    );
}

#[test]
fn invalid_numbers_and_inconsistent_regions_are_rejected() {
    let valid = DrawDistancePolicy::default();
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1e20] {
        assert!(
            DrawDistancePolicy::new(
                Some(Meters(invalid)),
                valid.scenery_radius(),
                valid.terrain_detail_radius(),
                valid.camera_far(),
                16.0
            )
            .is_err()
        );
        assert!(
            DrawDistancePolicy::new(
                None,
                Meters(invalid),
                valid.terrain_detail_radius(),
                valid.camera_far(),
                16.0
            )
            .is_err()
        );
        assert!(
            DrawDistancePolicy::new(
                None,
                valid.scenery_radius(),
                Meters(invalid),
                valid.camera_far(),
                16.0
            )
            .is_err()
        );
        assert!(
            DrawDistancePolicy::new(
                None,
                valid.scenery_radius(),
                valid.terrain_detail_radius(),
                Meters(invalid),
                16.0
            )
            .is_err()
        );
        assert!(
            DrawDistancePolicy::new(
                None,
                valid.scenery_radius(),
                valid.terrain_detail_radius(),
                valid.camera_far(),
                invalid
            )
            .is_err()
        );
    }
    assert!(
        DrawDistancePolicy::new(None, Meters(4500.0), Meters(5499.0), MAX_CAMERA_FAR, 16.0)
            .is_err()
    );
    assert!(
        DrawDistancePolicy::new(
            Some(Meters(10000.0)),
            Meters(9000.0),
            Meters(11000.0),
            MAX_CAMERA_FAR,
            16.0
        )
        .is_err()
    );
    assert!(
        DrawDistancePolicy::new(
            Some(Meters(200000.0)),
            Meters(9000.0),
            Meters(11000.0),
            MIN_CAMERA_FAR,
            16.0
        )
        .is_err()
    );
    assert!(
        DrawDistancePolicy::new(
            Some(MIN_REFINEMENT_RADIUS),
            MIN_SCENERY_RADIUS,
            Meters(2000.0),
            MIN_CAMERA_FAR,
            MAX_SCREEN_SPACE_ERROR
        )
        .is_ok()
    );
    assert!(
        DrawDistancePolicy::new(
            Some(MAX_REFINEMENT_RADIUS),
            MAX_SCENERY_RADIUS,
            MAX_TERRAIN_DETAIL_RADIUS,
            MAX_CAMERA_FAR,
            MIN_SCREEN_SPACE_ERROR
        )
        .is_ok()
    );
}

#[test]
fn standard_matches_original_selection_exactly_and_removes_old_caps() {
    for (lat, lon) in [
        (47.127, 9.529),
        (0.0, 179.999),
        (90.0, 0.0),
        (-90.0, -180.0),
    ] {
        for altitude in [2.0, 3002.0, 12000.0, 200000.0] {
            let camera = Geodetic::from_degrees(lat, lon, altitude).to_ecef();
            let expected = selector().select_with_surface(camera, Meters(3000.0));
            assert_eq!(
                DrawDistancePolicy::default()
                    .apply_to_selector(selector())
                    .select_with_surface(camera, Meters(3000.0)),
                expected
            );
            let short = DrawDistancePreset::Short
                .policy()
                .apply_to_selector(selector())
                .with_near_detail(Meters(3250.0), 13);
            assert_eq!(
                DrawDistancePolicy::default()
                    .apply_to_selector(short)
                    .select_with_surface(camera, Meters(3000.0)),
                expected
            );
        }
    }
}

#[test]
fn each_distance_preserves_complete_globe_and_existing_leaf_maximum() {
    for preset in DrawDistancePreset::ALL {
        for budget in [2, 128, crate::lod::DEFAULT_MAX_TILES] {
            for (lat, lon) in [(47.127, 9.529), (0.0, 180.0), (90.0, 0.0), (-90.0, -180.0)] {
                let camera = Geodetic::from_degrees(lat, lon, 3002.0).to_ecef();
                let selection = preset
                    .policy()
                    .apply_to_selector(selector().with_max_tiles(budget))
                    .with_near_detail(preset.policy().terrain_detail_radius(), 13)
                    .select_with_surface(camera, Meters(3000.0));
                assert_complete_cut(&selection, budget);
            }
        }
    }
}

#[test]
fn short_changes_real_terrain_selection_and_cap_is_not_camera_clipping() {
    let camera = Geodetic::from_degrees(47.127, 9.529, 2.0);
    assert!(
        camera
            .great_circle_distance(TileId::containing(COARSE_TERRAIN_LEVEL, camera).center())
            .get()
            > 10000.0,
        "fixture must distinguish footprint intersection from center distance"
    );
    let short = DrawDistancePreset::Short
        .policy()
        .apply_to_selector(selector())
        .select(camera.to_ecef());
    let standard = selector().select(camera.to_ecef());
    let long = DrawDistancePreset::Long
        .policy()
        .apply_to_selector(selector())
        .select(camera.to_ecef());
    assert!(short.tiles.len() < standard.tiles.len());
    assert!(long.tiles.len() >= standard.tiles.len());
    assert!(
        short.tiles.contains(&TileId::containing(13, camera)),
        "near ground detail is retained"
    );
    // Isolate the geographic cap from SSE: a deliberately strict selector
    // would refine worldwide without it. Every split beyond the coarse shell
    // must intersect the radius; children may extend over its boundary.
    let constrained = selector()
        .with_screen_space_error(0.001)
        .with_refinement_radius(Meters(10000.0), COARSE_TERRAIN_LEVEL)
        .with_max_tiles(100000)
        .select(camera.to_ecef());
    let surface = Geodetic::new(camera.latitude, camera.longitude, Meters::ZERO);
    for leaf in constrained.tiles {
        if let Some(parent) = leaf.parent().filter(|p| p.level >= COARSE_TERRAIN_LEVEL) {
            assert!(
                crate::lod::conservative_distance_to_bounds(surface.to_ecef(), parent.bounds())
                    .get()
                    <= 10000.0
            );
        }
    }
}

#[test]
fn changing_render_policy_cannot_change_physical_dem_level_or_samples() {
    let position = Geodetic::from_degrees(47.127, 9.529, 3000.0);
    let mut source = MemoryTileSource::new();
    for (level, height) in [(6, 100.0), (13, 643.125)] {
        let id = TileId::containing(level, position);
        source.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(3, 3, Meters(height))),
        );
    }
    let mut terrain = Terrain::new(source, 1024 * 1024, 0..=13);
    let expected = Some(Meters(643.125));
    assert_eq!(terrain.elevation_at(position), expected);
    for preset in [
        DrawDistancePreset::Long,
        DrawDistancePreset::Short,
        DrawDistancePreset::Standard,
    ] {
        let _ = preset
            .policy()
            .apply_to_selector(selector())
            .select(position.to_ecef());
        assert_eq!(terrain.elevation_at(position), expected);
    }
}

#[test]
fn short_cap_refines_off_meridian_north_and_south_polar_footprints() {
    for (latitude, row) in [(89.95, 0), (-89.95, 63)] {
        let camera = Geodetic::from_degrees(latitude, 0.0, 0.0);
        let tile = TileId::new(6, 127, row);
        let legacy = crate::lod::distance_to_bounds(camera.to_ecef(), camera, tile.bounds());
        let lower = crate::lod::conservative_distance_to_bounds(camera.to_ecef(), tile.bounds());
        let pole = Geodetic::from_degrees(90.0_f64.copysign(latitude), 180.0, 0.0);
        let included_distance = camera.to_ecef().distance_to(pole.to_ecef());
        assert!(
            legacy.get() > 11000.0,
            "fixture reproduces the old rejection"
        );
        assert!((5580.0..5590.0).contains(&included_distance.get()));
        assert!(lower.get() <= included_distance.get());
        assert!(lower.get() < 10000.0);
        let selector = DrawDistancePreset::Short
            .policy()
            .apply_to_selector(LodSelector::new(
                24.0,
                1080.0,
                Degrees(60.0).to_radians(),
                7,
                Meters(20000.0),
            ));
        assert!(selector.should_refine(tile.level, legacy));
        let selection = selector.select(camera.to_ecef());
        assert!(!selection.truncated);
        assert!(!selection.tiles.contains(&tile));
        assert!(
            tile.children()
                .unwrap()
                .iter()
                .all(|child| selection.tiles.contains(child))
        );
        assert_complete_cut(&selection, crate::lod::DEFAULT_MAX_TILES);
        eprintln!(
            "latitude {latitude}: old clamp {:.3} m, conservative bound {:.3} m, included pole {:.3} m",
            legacy.get(),
            lower.get(),
            included_distance.get()
        );
    }
}
