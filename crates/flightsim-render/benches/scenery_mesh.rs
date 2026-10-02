//! CPU mesh preparation only; excludes DEM I/O, GPU upload, rendering and frame timing.
#[path = "../tests/support/scenery_fixtures.rs"]
mod fixtures;

use criterion::{Criterion, criterion_group, criterion_main};
use fixtures::{Fixture, anchor};
use flightsim_core::Meters;
use flightsim_render::scenery::{SceneryMeshOptions, scenery_mesh};
use std::hint::black_box;
use std::time::Duration;

fn scenery_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("scenery_mesh_cpu");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));
    for (name, fixture, rejected, capped) in [
        ("64_dense_buildings", Fixture::buildings(), false, false),
        ("64_roads_600m", Fixture::roads(), false, false),
        ("16_forests_300m", Fixture::forest(), false, false),
        (
            "3_forests_rejected_candidates",
            Fixture::rejected_forest(),
            true,
            false,
        ),
        (
            "64_buildings_clipped_1024_vertices",
            Fixture::buildings(),
            false,
            true,
        ),
    ] {
        let features = fixture.features();
        let mut options = SceneryMeshOptions::near(anchor());
        if rejected {
            options.vegetation_distance = Meters(10_000.0);
        }
        if capped {
            options.max_vertices = 1_024;
        }
        let run = || {
            scenery_mesh(
                black_box(&features),
                anchor(),
                options,
                &mut |_| Some(Meters(450.0)),
                &|_| rejected,
            )
        };
        let sample = run().expect("nonempty benchmark fixture");
        eprintln!(
            "{name}: features={}, solid_vertices={}, ground_vertices={}, statistics={:?}",
            features.len(),
            sample.mesh.count_vertices(),
            sample
                .ground
                .as_ref()
                .map_or(0, |ground| ground.mesh.count_vertices()),
            sample.statistics
        );
        group.bench_function(name, |b| b.iter(|| black_box(run())));
    }
    group.finish();
}

criterion_group!(benches, scenery_benchmarks);
criterion_main!(benches);
