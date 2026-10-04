//! Actual default polar cut, one mesh live at a time. CPU-only preparation;
//! includes atlas DEM generation and geometry so before/after differ only by
//! the source-aware normal stage. Excludes GPU, streaming, bridge builds,
//! uploads, frame cadence and native acceptance.
use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Degrees, Geodetic, Meters};
use flightsim_render::{TerrainMeshProvenance, terrain_polar_normals::GlobalTerrainShading};
use flightsim_world::{LodSelector, MeshOptions, build_mesh, global::GlobalTerrain};
use std::{collections::BTreeMap, hint::black_box, time::Duration};

fn polar_benchmarks(c: &mut Criterion) {
    let atlas = GlobalTerrain::bundled().unwrap();
    let pole = Geodetic::from_degrees(-90.0, 0.0, 0.0);
    let ground = atlas.sample(pole).unwrap().surface_height;
    let camera = Geodetic::new(pole.latitude, pole.longitude, Meters(ground.get() + 350.0));
    let cut = LodSelector::new(
        16.0,
        1080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    )
    .select_with_surface(camera.to_ecef(), ground);
    assert_eq!(cut.tiles.len(), 4094);
    assert!(cut.truncated);
    let mut levels = BTreeMap::new();
    for id in &cut.tiles {
        *levels.entry(id.level).or_insert(0_usize) += 1;
    }
    eprintln!(
        "actual south-pole 350m AGL cut: {} tiles, levels {levels:?}; immutable atlas reused, one mesh allocated at a time",
        cut.tiles.len()
    );
    let mut group = c.benchmark_group("polar_normals_actual_cut_cpu");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));
    for (name, filtered) in [("unfiltered", false), ("filtered_505m", true)] {
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut checksum = 0.0_f64;
                for &id in &cut.tiles {
                    let dem = atlas.tile(id).unwrap();
                    let mut mesh = build_mesh(id, &dem, &MeshOptions::default());
                    if filtered {
                        GlobalTerrainShading {
                            atlas: &atlas,
                            provenance: TerrainMeshProvenance::Fallback,
                        }
                        .apply(id, &mut mesh);
                    }
                    checksum += f64::from(black_box(mesh.normals[0][2]));
                    black_box(&mesh);
                }
                black_box(checksum)
            })
        });
    }
    group.finish();
}
fn water_mask_benchmarks(c: &mut Criterion) {
    use criterion::BatchSize;
    use flightsim_render::water::WaterMaskBuilder;
    use std::hint::black_box;
    use std::time::Duration;

    let atlas = GlobalTerrain::bundled().expect("validated bundled atlas");
    let mut group = c.benchmark_group("water_mask");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    group.bench_function("batch_8192", |b| {
        b.iter_batched_ref(
            || WaterMaskBuilder::new(atlas.clone()),
            |builder| black_box(builder.advance()),
            BatchSize::PerIteration,
        );
    });
    group.bench_function("complete_512_cube", |b| {
        b.iter(|| {
            let mut builder = WaterMaskBuilder::new(atlas.clone());
            while !builder.advance() {}
            black_box(builder.completed_texels())
        });
    });
    group.finish();
}

criterion_group!(benches, polar_benchmarks, water_mask_benchmarks);
criterion_main!(benches);
