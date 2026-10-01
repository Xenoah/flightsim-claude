//! Explicit real-data regression. The external DEM is not distributed with the
//! source; run with FLIGHTSIM_REAL_DEM_TILES pointing at the normalized Haneda
//! fixture documented in docs/qa/terrain-parent-fallback-2026-10-01.md.
use flightsim_core::{Degrees, Geodetic, Meters};
use flightsim_render::{TerrainSelectionState, update_terrain_selection};
use flightsim_world::{
    DiskTileSource, LodSelector, Runway, Terrain, TileCache, TileId, TileSource,
};
use std::collections::BTreeMap;

fn runway_points(source: &DiskTileSource) -> Vec<Geodetic> {
    let mut sampler = Terrain::new(source, 16 * 1024 * 1024, 8..=13);
    let runway = Runway::synthetic();
    let runway = runway.with_elevation(sampler.elevation_at(runway.threshold).unwrap());
    let (s, c) = runway.heading.get().sin_cos();
    (0..=500)
        .flat_map(|along| {
            (0..=18).map(move |across| {
                let along = f64::from(along) * 5.0;
                let across = -22.5 + f64::from(across) * 2.5;
                runway.threshold.offset_by(
                    Meters(along * c - across * s),
                    Meters(along * s + across * c),
                )
            })
        })
        .collect()
}

fn coverage(ids: &[TileId], points: &[Geodetic]) -> (usize, BTreeMap<u8, usize>) {
    let mut missing = 0;
    let mut levels = BTreeMap::new();
    for &point in points {
        let mut covering = ids
            .iter()
            .filter(|id| TileId::containing(id.level, point) == **id);
        if let Some(id) = covering.next() {
            assert!(
                covering.next().is_none(),
                "overlapping terrain surfaces at {point:?}"
            );
            *levels.entry(id.level).or_insert(0) += 1;
        } else {
            missing += 1;
        }
    }
    (missing, levels)
}

#[test]
#[ignore = "requires the documented external normalized Copernicus Haneda tiles"]
fn haneda_ground_and_approach_have_no_missing_runway_footprint() {
    let source = DiskTileSource::new(
        std::env::var_os("FLIGHTSIM_REAL_DEM_TILES").expect("set FLIGHTSIM_REAL_DEM_TILES"),
    );
    let points = runway_points(&source);
    assert_eq!(points.len(), 9_519);
    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    );
    for (name, lat, lon, alt, expected_missing) in [
        ("night_ground", 35.55, 139.78, 40.0, 6_774),
        ("night_approach", 35.5331, 139.7533, 173.0, 2_474),
        ("drop_start", 35.55, 139.78, 3_022.0, 0),
        ("drop_end", 35.5501, 139.7802, 2_885.0, 0),
    ] {
        let camera = Geodetic::from_degrees(lat, lon, alt).to_ecef();
        let desired = selector.select(camera);
        assert!(!desired.truncated);
        let old_live: Vec<_> = desired
            .tiles
            .iter()
            .copied()
            .filter(|id| source.load(*id).unwrap().is_some())
            .collect();
        let (old_missing, old_levels) = coverage(&old_live, &points);
        assert_eq!(
            old_missing, expected_missing,
            "wrong external fixture for {name}"
        );
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(512 * 1024 * 1024);
        let mut attempts = 0;
        let mut complete_at = None;
        for frame in 1..=240 {
            let update = update_terrain_selection(
                &selector,
                &source,
                &mut cache,
                &mut state,
                camera,
                8,
                &mut |_, _| {},
            );
            assert!(update.load_attempts <= 8);
            assert!(update.prepared.len() <= 8);
            attempts += update.load_attempts;
            let (missing, _) = coverage(&state.ids().collect::<Vec<_>>(), &points);
            if missing == 0 {
                complete_at.get_or_insert(frame);
            }
            if complete_at.is_some() {
                assert_eq!(missing, 0, "coverage regressed at frame {frame}");
            }
        }
        let ids: Vec<_> = state.ids().collect();
        let (new_missing, new_levels) = coverage(&ids, &points);
        assert_eq!(new_missing, 0, "{name}: missing fallback terrain");
        println!(
            "{name}: selected={}, old_live={}, new_live={}, old_missing={old_missing}/9519, new_missing={new_missing}/9519, old_levels={old_levels:?}, new_levels={new_levels:?}, complete_at_frame={}, reads_over_240_frames={attempts}, live={ids:?}",
            desired.tiles.len(),
            old_live.len(),
            state.len(),
            complete_at.unwrap()
        );
    }
}
