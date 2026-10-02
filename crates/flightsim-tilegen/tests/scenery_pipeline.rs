#[path = "support/pbf.rs"]
mod pbf;
use flightsim_tilegen::{SceneryBakeOptions, bake_scenery_bytes, generate_scenery_database};
use flightsim_world::{BuildingHeightSource, RoadClass, SceneryDatabase};
use std::fs;

const STRINGS: &[&str] = &[
    "",
    "highway",
    "residential",
    "building",
    "yes",
    "height",
    "12",
    "landuse",
    "grass",
    "width",
    "6",
    "building:levels",
    "2",
    "type",
    "multipolygon",
    "outer",
    "inner",
    "bridge",
    "area",
    "tunnel",
    "no",
    "min_height",
    "2 m",
    "NaN",
    "-9",
    "30 ft",
    "natural",
    "water",
];
fn options() -> SceneryBakeOptions {
    SceneryBakeOptions {
        source_name: "synthetic PBF codec test, not geographic data".into(),
        source_url: "https://example.test/synthetic.osm.pbf".into(),
    }
}
fn encode(group: &[u8]) -> Vec<u8> {
    let table: Vec<_> = STRINGS
        .iter()
        .flat_map(|s| pbf::bytes(1, s.as_bytes()))
        .collect();
    pbf::pbf(&[pbf::bytes(1, &table), pbf::bytes(2, group)].concat())
}
fn nodes() -> Vec<u8> {
    // WGS84 1e-7 degrees, matching OSM's default100nm granularity.
    [
        (1, 470_000_000, 90_000_000),
        (2, 470_001_000, 90_000_000),
        (3, 470_000_000, 90_001_000),
        (4, 470_001_000, 90_001_000),
        (5, 470_001_000, 90_002_000),
        (6, 470_000_000, 90_002_000),
    ]
    .iter()
    .flat_map(|&(id, lat, lon)| pbf::bytes(1, &pbf::node(id, lat, lon, &[])))
    .collect()
}
fn good_road() -> Vec<u8> {
    pbf::bytes(3, &pbf::way(10, &[1, 1], &[(1, 2), (9, 10)]))
}
fn building(tags: &[(u64, u64)]) -> Vec<u8> {
    pbf::bytes(3, &pbf::way(20, &[3, 1, 1, 1, -3], tags))
}
fn base() -> Vec<u8> {
    [nodes(), good_road()].concat()
}
fn fixture() -> Vec<u8> {
    encode(
        &[
            base(),
            building(&[(3, 4), (5, 6)]),
            pbf::bytes(3, &pbf::way(30, &[3, 1, 1, 1, -3], &[(7, 8)])),
        ]
        .concat(),
    )
}

#[test]
fn actual_pbf_decode_produces_deterministic_roads_footprints_and_landcover() {
    let source = fixture();
    let (db, report) = bake_scenery_bytes(&source, &options()).unwrap();
    assert_eq!(
        (
            report.roads_written,
            report.buildings_written,
            report.landcover_written
        ),
        (1, 1, 1)
    );
    assert_eq!(db.roads()[0].class, RoadClass::Residential);
    assert!(!db.roads()[0].width_inferred);
    assert_eq!(
        db.buildings()[0].height_source,
        BuildingHeightSource::OsmHeight
    );
    assert_eq!(db.buildings()[0].footprint.len(), 4);
    assert_eq!(db.landcover()[0].triangles.len(), 2);
    assert!((db.buildings()[0].height.get() - 12.0).abs() < 1e-12);
    assert!(
        db.roads()
            .iter()
            .flat_map(|r| &r.points)
            .all(|p| p.altitude.get().abs() < f64::EPSILON)
    );
    let mut first = Vec::new();
    db.write(&mut first).unwrap();
    let (db2, report2) = bake_scenery_bytes(&source, &options()).unwrap();
    let mut second = Vec::new();
    db2.write(&mut second).unwrap();
    assert_eq!(first, second);
    assert_eq!(report, report2);
    assert_eq!(
        SceneryDatabase::read(first.as_slice())
            .unwrap()
            .feature_count(),
        3
    );
}
#[test]
fn building_height_provenance_never_claims_defaults_are_measured() {
    for (tags, expected, height) in [
        (vec![(3, 4), (11, 12)], BuildingHeightSource::OsmLevels, 6.0),
        (
            vec![(3, 4), (5, 23), (11, 24)],
            BuildingHeightSource::Default,
            9.0,
        ),
        (
            vec![(3, 4), (5, 25)],
            BuildingHeightSource::OsmHeight,
            9.144,
        ),
    ] {
        let (db, _) =
            bake_scenery_bytes(&encode(&[base(), building(&tags)].concat()), &options()).unwrap();
        assert_eq!(db.buildings()[0].height_source, expected);
        assert!((db.buildings()[0].height.get() - height).abs() < 1e-9);
    }
}
#[test]
fn multipolygon_members_are_omitted_even_for_old_style_untagged_relations() {
    // Relation carries only type=multipolygon; the outer member carries building.
    let relation = [
        pbf::integer(1, 40),
        pbf::packed(2, &[13]),
        pbf::packed(3, &[14]),
        pbf::packed(8, &[15]),
        pbf::deltas(9, &[20]),
        pbf::packed(10, &[1]),
    ]
    .concat();
    let (db, report) = bake_scenery_bytes(
        &encode(&[base(), building(&[(3, 4)]), pbf::bytes(4, &relation)].concat()),
        &options(),
    )
    .unwrap();
    assert!(db.buildings().is_empty());
    assert_eq!(report.skipped_multipolygon_members, 1);
    assert_eq!(report.skipped_multipolygon_relations, 1);
}
#[test]
fn bridges_tunnels_areas_raised_buildings_and_oversized_ways_are_counted() {
    let extra = [
        pbf::bytes(3, &pbf::way(11, &[1, 1], &[(1, 2), (17, 4)])),
        pbf::bytes(3, &pbf::way(12, &[1, 1], &[(1, 2), (19, 4)])),
        pbf::bytes(3, &pbf::way(13, &[1, 1], &[(1, 2), (18, 4)])),
        building(&[(3, 4), (21, 22)]),
        pbf::bytes(3, &pbf::way(14, &vec![1; 513], &[(1, 2)])),
    ]
    .concat();
    let (db, r) = bake_scenery_bytes(&encode(&[base(), extra].concat()), &options()).unwrap();
    assert_eq!(db.feature_count(), 1);
    assert_eq!(r.skipped_non_ground_roads, 2);
    assert_eq!(r.skipped_area_roads, 1);
    assert_eq!(r.skipped_raised_buildings, 1);
    assert_eq!(r.skipped_oversized_ways, 1);
}
#[test]
fn missing_nodes_open_rings_self_intersections_and_bad_coordinates_are_counted() {
    let extra = [
        pbf::bytes(3, &pbf::way(11, &[1, 998], &[(1, 2)])),
        pbf::bytes(3, &pbf::way(12, &[3, 1, 1], &[(3, 4)])),
        pbf::bytes(3, &pbf::way(13, &[3, 2, -1, 2, -3], &[(3, 4)])),
        pbf::bytes(1, &pbf::node(90, 1_000_000_000, 90_000_000, &[])),
        pbf::bytes(3, &pbf::way(14, &[1, 89], &[(1, 2)])),
    ]
    .concat();
    let (db, r) = bake_scenery_bytes(&encode(&[base(), extra].concat()), &options()).unwrap();
    assert_eq!(db.feature_count(), 1);
    assert_eq!(r.skipped_missing_nodes, 1);
    assert_eq!(r.skipped_open_polygons, 1);
    assert_eq!(r.skipped_bad_geometry, 1);
    assert_eq!(r.skipped_invalid_coordinates, 1);
}
#[test]
fn malformed_input_and_duplicate_ids_fail_without_replacing_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pbf");
    let output = dir.path().join("scene.fsscenery");
    fs::write(&input, fixture()).unwrap();
    generate_scenery_database(&input, &output, &options()).unwrap();
    let before = fs::read(&output).unwrap();
    for bytes in [
        vec![],
        b"download failed".to_vec(),
        encode(&[base(), good_road()].concat()),
    ] {
        fs::write(&input, bytes).unwrap();
        assert!(generate_scenery_database(&input, &output, &options()).is_err());
        assert_eq!(fs::read(&output).unwrap(), before);
    }
    assert!(generate_scenery_database(&output, &output, &options()).is_err());
}
#[test]
fn invalid_source_metadata_does_not_write_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pbf");
    let output = dir.path().join("scene.fsscenery");
    fs::write(&input, fixture()).unwrap();
    let mut invalid = options();
    invalid.source_name = "bad\nmetadata".into();
    assert!(generate_scenery_database(&input, &output, &invalid).is_err());
    assert!(!output.exists());
}
#[test]
fn source_hash_is_of_exact_input_bytes() {
    let bytes = fixture();
    let (db, r) = bake_scenery_bytes(&bytes, &options()).unwrap();
    let expected = flightsim_world::scenery::io::fingerprint(&bytes);
    assert_eq!(r.source_fingerprint, expected);
    assert_eq!(db.source().input_fingerprint, expected);
}
