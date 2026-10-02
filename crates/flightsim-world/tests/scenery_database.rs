use flightsim_core::{Geodetic, Meters};
use flightsim_world::scenery::io::{HEADER_LEN, fingerprint};
use flightsim_world::{
    BuildingHeightSource, LandCoverClass, RoadClass, SceneryBuilding, SceneryDatabase,
    SceneryFeatureRef, SceneryLandCover, SceneryRoad, ScenerySource, ScenerySourceKind,
};

fn source() -> ScenerySource {
    ScenerySource {
        kind: ScenerySourceKind::Synthetic,
        name: "Deliberately synthetic tests".into(),
        url: "urn:test:scenery".into(),
        input_fingerprint: 123,
    }
}
fn polygon() -> Vec<Geodetic> {
    vec![
        Geodetic::from_degrees(47.0, 9.0, 0.0),
        Geodetic::from_degrees(47.001, 9.0, 0.0),
        Geodetic::from_degrees(47.001, 9.001, 0.0),
        Geodetic::from_degrees(47.0, 9.001, 0.0),
    ]
}
fn road(id: i64, lon: f64) -> SceneryRoad {
    SceneryRoad {
        source_id: id,
        class: RoadClass::Residential,
        width: Meters(5.0),
        width_inferred: true,
        points: vec![
            Geodetic::from_degrees(47.0, lon, 0.0),
            Geodetic::from_degrees(47.001, lon, 0.0),
        ],
    }
}
fn database() -> SceneryDatabase {
    SceneryDatabase::new(
        source(),
        vec![road(9, 9.1), road(4, 9.0)],
        vec![SceneryBuilding {
            source_id: 20,
            height: Meters(7.0),
            height_source: BuildingHeightSource::Default,
            footprint: polygon(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }],
        vec![SceneryLandCover {
            source_id: 30,
            class: LandCoverClass::Grass,
            boundary: polygon(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }],
    )
    .unwrap()
}
fn bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    database().write(&mut bytes).unwrap();
    bytes
}
fn metadata_len(bytes: &[u8]) -> usize {
    u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize
}
fn rehash(bytes: &mut [u8]) {
    let sum = fingerprint(&bytes[HEADER_LEN..]);
    bytes[40..48].copy_from_slice(&sum.to_le_bytes());
}

#[test]
fn roundtrip_is_byte_exact_and_source_order_is_canonical() {
    let bytes = bytes();
    let db = SceneryDatabase::read(bytes.as_slice()).unwrap();
    assert_eq!(db.roads()[0].source_id, 4);
    assert_eq!(db.roads()[1].source_id, 9);
    assert_eq!(
        db.buildings()[0].height_source,
        BuildingHeightSource::Default
    );
    assert_eq!(db.source(), &source());
    let mut again = Vec::new();
    db.write(&mut again).unwrap();
    assert_eq!(bytes, again);
}
#[test]
fn header_and_payload_corruption_never_decode() {
    for position in [0, 4, 6, 8, 12, 24, 28, 32, 36, 40] {
        let mut b = bytes();
        b[position] ^= 0xff;
        assert!(
            SceneryDatabase::read(b.as_slice()).is_err(),
            "position {position}"
        );
    }
    let original = bytes();
    for len in [0, 1, 4, 47, 48, original.len() - 1] {
        assert!(SceneryDatabase::read(&original[..len]).is_err());
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(SceneryDatabase::read(trailing.as_slice()).is_err());
    let mut corrupt = original;
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(SceneryDatabase::read(corrupt.as_slice()).is_err());
}
#[test]
fn valid_checksum_does_not_bypass_semantic_validation() {
    let original = bytes();
    let road = HEADER_LEN + metadata_len(&original);
    for offset in [road + 8, road + 9, road + 10, road + 24] {
        let mut b = original.clone();
        b[offset] = 255;
        rehash(&mut b);
        assert!(
            SceneryDatabase::read(b.as_slice()).is_err(),
            "offset {offset}"
        );
    }
    let mut b = original.clone();
    b[road + 12..road + 20].copy_from_slice(&f64::NAN.to_le_bytes());
    rehash(&mut b);
    assert!(SceneryDatabase::read(b.as_slice()).is_err());
    let mut b = original.clone();
    b[road + 28..road + 36].copy_from_slice(&f64::INFINITY.to_le_bytes());
    rehash(&mut b);
    assert!(SceneryDatabase::read(b.as_slice()).is_err());
    let mut b = original;
    b[road + 20..road + 24].copy_from_slice(&u32::MAX.to_le_bytes());
    rehash(&mut b);
    assert!(SceneryDatabase::read(b.as_slice()).is_err());
}
#[test]
fn geometry_rejects_nonfinite_nonzero_altitude_and_duplicate_ids() {
    let mut bad = road(1, 9.0);
    bad.points[0].altitude = Meters(1.0);
    assert!(SceneryDatabase::new(source(), vec![bad], vec![], vec![]).is_err());
    let mut bad = road(1, 9.0);
    bad.points[0].latitude.0 = f64::NAN;
    assert!(SceneryDatabase::new(source(), vec![bad], vec![], vec![]).is_err());
    assert!(
        SceneryDatabase::new(source(), vec![road(1, 9.0), road(1, 9.1)], vec![], vec![]).is_err()
    );
    assert!(
        SceneryDatabase::new(source(), vec![road(1, 9.0), road(2, 120.0)], vec![], vec![]).is_err()
    );
    let mut bad = road(1, 9.0);
    bad.points[1] = bad.points[0];
    assert!(SceneryDatabase::new(source(), vec![bad], vec![], vec![]).is_err());
}
#[test]
fn query_is_nearest_first_capped_and_altitude_independent() {
    let db =
        SceneryDatabase::new(source(), vec![road(1, 9.1), road(9, 9.0)], vec![], vec![]).unwrap();
    let near = Geodetic::from_degrees(47.0, 9.0, 10000.0);
    let selected = db.query_near(near, Meters(10000.0), 1);
    assert_eq!(selected.len(), 1);
    assert!(matches!(selected[0],SceneryFeatureRef::Road(r) if r.source_id==9));
    assert!(db.query_near(near, Meters(f64::NAN), 1).is_empty());
    assert!(db.query_near(near, Meters(-1.0), 1).is_empty());
    assert!(db.query_near(near, Meters(10000.0), 0).is_empty());
}
#[test]
fn longitude_seam_and_poles_remain_finite() {
    let mut seam = road(1, 179.999);
    seam.points[1] = Geodetic::from_degrees(47.001, -179.999, 0.0);
    let db = SceneryDatabase::new(source(), vec![seam], vec![], vec![]).unwrap();
    assert_eq!(
        db.query_near(Geodetic::from_degrees(47.0, -180.0, 0.0), Meters(1000.0), 1)
            .len(),
        1
    );
    let mut polar = road(1, 0.0);
    polar.points = vec![
        Geodetic::from_degrees(90.0, 0.0, 0.0),
        Geodetic::from_degrees(89.999, 120.0, 0.0),
    ];
    let db = SceneryDatabase::new(source(), vec![polar], vec![], vec![]).unwrap();
    assert!(db.bounds().radius.is_finite());
    assert_eq!(
        db.query_near(Geodetic::from_degrees(90.0, -180.0, 0.0), Meters(1000.0), 1)
            .len(),
        1
    );
}

fn test_building(
    footprint: Vec<Geodetic>,
    triangles: Vec<[u32; 3]>,
) -> Result<SceneryDatabase, flightsim_world::SceneryError> {
    SceneryDatabase::new(
        source(),
        vec![],
        vec![SceneryBuilding {
            source_id: 1,
            height: Meters(9.0),
            height_source: BuildingHeightSource::Default,
            footprint,
            triangles,
        }],
        vec![],
    )
}
#[test]
fn polygon_topology_rejects_duplicate_bow_tie_and_outside_coverage() {
    assert!(test_building(polygon(), vec![[0, 1, 2], [0, 1, 2]]).is_err());
    let mut crossed = polygon();
    crossed.swap(1, 2);
    assert!(test_building(crossed, vec![[0, 1, 2], [0, 2, 3]]).is_err());
    let origin = Geodetic::from_degrees(47.0, 9.0, 0.0);
    let concave: Vec<_> = [
        (0.0, 0.0),
        (0.0, 20.0),
        (10.0, 8.0),
        (20.0, 20.0),
        (20.0, 0.0),
    ]
    .into_iter()
    .map(|(n, e)| origin.offset_by(Meters(n), Meters(e)))
    .collect();
    // Convex fan incorrectly bridges the notch; inconsistent/overlapping coverage.
    assert!(test_building(concave.clone(), vec![[0, 1, 4], [1, 2, 3], [1, 3, 4]]).is_err());
    assert!(test_building(concave, vec![[0, 1, 2], [0, 2, 4], [2, 3, 4]]).is_ok());
    assert!(test_building(polygon(), vec![[0, 1, 2], [3, 2, 0]]).is_err());
}
#[test]
fn consistent_reversed_winding_is_accepted() {
    assert!(test_building(polygon(), vec![[2, 1, 0], [3, 2, 0]]).is_ok());
    let mut reverse = polygon();
    reverse.reverse();
    assert!(test_building(reverse, vec![[0, 1, 2], [0, 2, 3]]).is_ok());
}
#[test]
fn conservative_spheres_do_not_outrank_near_geometry() {
    let center = Geodetic::from_degrees(47.0, 9.0, 0.0);
    let far = SceneryRoad {
        source_id: 1,
        class: RoadClass::Primary,
        width: Meters(8.0),
        width_inferred: true,
        points: vec![
            center.offset_by(Meters(0.0), Meters(-1000.0)),
            center.offset_by(Meters(1000.0), Meters(-1000.0)),
            center.offset_by(Meters(1000.0), Meters(1000.0)),
        ],
    };
    let close = SceneryRoad {
        source_id: 2,
        class: RoadClass::Residential,
        width: Meters(5.0),
        width_inferred: true,
        points: vec![
            center.offset_by(Meters(-10.0), Meters(10.0)),
            center.offset_by(Meters(10.0), Meters(10.0)),
        ],
    };
    let db = SceneryDatabase::new(source(), vec![far, close], vec![], vec![]).unwrap();
    assert!(
        matches!(db.query_near(center,Meters(2000.0),1)[0],SceneryFeatureRef::Road(r) if r.source_id==2)
    );
    assert_eq!(db.query_near(center, Meters(50.0), 10).len(), 1);
}

#[test]
fn aggregate_validation_budget_rejects_before_quadratic_work() {
    let origin = Geodetic::from_degrees(47.0, 9.0, 0.0);
    let ring: Vec<_> = (0u32..512)
        .map(|i| {
            let a = f64::from(i) * std::f64::consts::TAU / 512.0;
            origin.offset_by(Meters(100.0 * a.sin()), Meters(100.0 * a.cos()))
        })
        .collect();
    let triangles: Vec<_> = (1u32..511).map(|i| [0, i, i + 1]).collect();
    let buildings = (1..=64)
        .map(|id| SceneryBuilding {
            source_id: id,
            height: Meters(9.0),
            height_source: BuildingHeightSource::Default,
            footprint: ring.clone(),
            triangles: triangles.clone(),
        })
        .collect();
    let error = SceneryDatabase::new(source(), vec![], buildings, vec![]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("aggregate polygon validation work")
    );
}
#[test]
fn result_cap_cannot_starve_near_building_behind_misleading_road_bounds() {
    let center = Geodetic::from_degrees(47.0, 9.0, 0.0);
    let far_points = vec![
        center.offset_by(Meters(0.0), Meters(-1000.0)),
        center.offset_by(Meters(1000.0), Meters(-1000.0)),
        center.offset_by(Meters(1000.0), Meters(1000.0)),
    ];
    let roads = (1..=4100)
        .map(|id| SceneryRoad {
            source_id: id,
            class: RoadClass::Primary,
            width: Meters(8.0),
            width_inferred: true,
            points: far_points.clone(),
        })
        .collect();
    let db = SceneryDatabase::new(
        source(),
        roads,
        vec![SceneryBuilding {
            source_id: 9000,
            height: Meters(9.0),
            height_source: BuildingHeightSource::Default,
            footprint: polygon(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }],
        vec![],
    )
    .unwrap();
    let query = db.query_near(center, Meters(2000.0), usize::MAX);
    assert_eq!(query.len(), 4096);
    assert!(matches!(query[0], SceneryFeatureRef::Building(_)));
}
