//! Bounded scenery decode/query timings. Set FLIGHTSIM_SCENERY_BENCH_FILE to a
//! locally retained real fixture; no geography is downloaded or bundled here.
use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Geodetic, Meters};
use flightsim_world::{RoadClass, SceneryDatabase, SceneryRoad, ScenerySource, ScenerySourceKind};
use std::hint::black_box;

fn data() -> Vec<u8> {
    if let Some(path) = std::env::var_os("FLIGHTSIM_SCENERY_BENCH_FILE") {
        return std::fs::read(path).expect("read explicit local scenery benchmark fixture");
    }
    let center = Geodetic::from_degrees(47.165, 9.51, 0.0);
    let roads = (1u32..=1000)
        .map(|id| {
            let a = center.offset_by(
                Meters(f64::from(id % 32) * 100.0),
                Meters(f64::from(id / 32) * 100.0),
            );
            SceneryRoad {
                source_id: i64::from(id),
                class: RoadClass::Residential,
                width: Meters(5.5),
                width_inferred: true,
                points: vec![a, a.offset_by(Meters(50.0), Meters(50.0))],
            }
        })
        .collect();
    let db = SceneryDatabase::new(
        ScenerySource {
            kind: ScenerySourceKind::Synthetic,
            name: "Synthetic benchmark roads".into(),
            url: "urn:synthetic:benchmark".into(),
            input_fingerprint: 0,
        },
        roads,
        vec![],
        vec![],
    )
    .unwrap();
    let mut bytes = Vec::new();
    db.write(&mut bytes).unwrap();
    bytes
}
fn benches(c: &mut Criterion) {
    let bytes = data();
    let db = SceneryDatabase::read(bytes.as_slice())
        .expect("benchmark fixture passes current full validation");
    eprintln!(
        "scenery benchmark: {} features, {} bytes",
        db.feature_count(),
        bytes.len()
    );
    c.bench_function("scenery_decode_validate", |b| {
        b.iter(|| SceneryDatabase::read(black_box(bytes.as_slice())).unwrap())
    });
    c.bench_function("scenery_query_4km_cap4096", |b| {
        b.iter(|| {
            db.query_near(
                black_box(Geodetic::from_degrees(47.165, 9.51, 1000.0)),
                Meters(4000.0),
                4096,
            )
        })
    });
}
criterion_group!(scenery, benches);
criterion_main!(scenery);
