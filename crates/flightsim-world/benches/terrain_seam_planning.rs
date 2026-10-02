//! Full-cost comparison and per-call latency probe for bounded seam planning.
//! Compile with --no-run, then run only in a coordinated quiet window.
//! FLIGHTSIM_PLANNING_PROBE=1 additionally times every call in 20 complete plans;
//! fixture/DEM/surface construction is excluded, but planner construction, target
//! clone/validation, descriptor queueing and destruction are separately reported.

use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Degrees, Geodetic, Meters};
use flightsim_world::global::GlobalTerrain;
use flightsim_world::{
    LodSelector, MeshOptions, TerrainBoundary, TerrainSeam, TerrainSeamKey, TerrainSeamPlanner,
    TileId, build_mesh, plan_seams,
};
use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

type PlanOutput = (
    BTreeMap<TerrainSeamKey, TerrainSeam>,
    BTreeSet<TerrainSeamKey>,
);
const BUDGET: usize = 1_024;

fn bounded(boundaries: &BTreeMap<TileId, TerrainBoundary>, ids: &BTreeSet<TileId>) -> PlanOutput {
    let mut planner = TerrainSeamPlanner::default();
    let mut desired = BTreeMap::new();
    let mut queued = BTreeSet::new();
    while !planner.is_finished() {
        planner.advance(boundaries, ids, BUDGET, |seam| {
            queued.insert(seam.key());
            desired.insert(seam.key(), seam);
        });
    }
    (desired, queued)
}

fn probe(label: &str, boundaries: &BTreeMap<TileId, TerrainBoundary>, ids: &BTreeSet<TileId>) {
    let mut calls = Vec::new();
    let mut assembly = Vec::new();
    let mut construction = Vec::new();
    let mut destruction = Vec::new();
    let mut total_work = 0;
    let mut frames = 0;
    for _ in 0..20 {
        let start = Instant::now();
        // Same-cut source replacement path: owning target clone and source
        // validation are explicit, not hidden in a batched untimed setup.
        let target = ids.clone();
        assert!(target.iter().all(|id| boundaries.contains_key(id)));
        assembly.push(start.elapsed());
        let start = Instant::now();
        let mut planner = TerrainSeamPlanner::default();
        construction.push(start.elapsed());
        let mut desired = BTreeMap::new();
        let mut queued = BTreeSet::new();
        frames = 0;
        total_work = 0;
        while !planner.is_finished() {
            let start = Instant::now();
            total_work += planner.advance(boundaries, &target, BUDGET, |seam| {
                queued.insert(seam.key());
                desired.insert(seam.key(), seam);
            });
            calls.push(start.elapsed());
            frames += 1;
        }
        assert_eq!(planner.resource_usage(), Default::default());
        let start = Instant::now();
        drop((planner, desired, queued, target));
        destruction.push(start.elapsed());
    }
    fn report(label: &str, category: &str, values: &mut [Duration]) {
        values.sort_unstable();
        println!(
            "probe={label} category={category} samples={} p50_us={:.3} p95_us={:.3} max_us={:.3}",
            values.len(),
            values[values.len() / 2].as_secs_f64() * 1e6,
            values[(values.len() - 1) * 95 / 100].as_secs_f64() * 1e6,
            values.last().unwrap().as_secs_f64() * 1e6
        );
    }
    println!(
        "probe={label} tiles={} budget={BUDGET} frames={frames} total_work={total_work}",
        ids.len()
    );
    report(label, "same_cut_target_clone_validate", &mut assembly);
    report(label, "planner_constructor", &mut construction);
    report(label, "advance_including_queueing", &mut calls);
    report(
        label,
        "output_target_and_completed_planner_drop",
        &mut destruction,
    );
}

fn benchmarks(criterion: &mut Criterion) {
    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    );
    for (label, latitude, longitude, altitude) in [
        ("pacific", -5.976_562_5, -133.5, 12_000.0),
        ("dateline", 0.0, 179.99, 12_000.0),
        ("north_pole", 89.999, 30.0, 1_000.0),
        ("south_pole", -89.999, 30.0, 1_000.0),
        ("uniform_l6_8192", 0.0, 0.0, 0.0),
    ] {
        let ids: BTreeSet<_> = if label == "uniform_l6_8192" {
            (0..TileId::rows(6))
                .flat_map(|y| (0..TileId::columns(6)).map(move |x| TileId::new(6, x, y)))
                .collect()
        } else {
            selector
                .select_with_surface(
                    Geodetic::from_degrees(latitude, longitude, altitude).to_ecef(),
                    Meters::ZERO,
                )
                .tiles
                .into_iter()
                .collect()
        };
        let atlas = GlobalTerrain::bundled().unwrap();
        let boundaries: BTreeMap<_, _> = ids
            .iter()
            .map(|&id| {
                let source = build_mesh(id, &atlas.tile(id).unwrap(), &MeshOptions::default());
                (id, TerrainBoundary::from_mesh(id, &source))
            })
            .collect();
        let reference = plan_seams(&boundaries, &ids);
        let result = bounded(&boundaries, &ids);
        assert_eq!(reference, result.0.values().cloned().collect::<Vec<_>>());
        drop((reference, result));
        if std::env::var_os("FLIGHTSIM_PLANNING_PROBE").is_some() {
            probe(label, &boundaries, &ids);
        }
        let mut group = criterion.benchmark_group(format!("terrain_seam_planning/{label}"));
        group.bench_function("reference_full_including_queue_and_drop", |b| {
            b.iter(|| {
                let descriptors = plan_seams(black_box(&boundaries), black_box(&ids));
                let mut desired = BTreeMap::new();
                let mut queued = BTreeSet::new();
                for seam in descriptors {
                    queued.insert(seam.key());
                    desired.insert(seam.key(), seam);
                }
                drop(black_box((desired, queued)));
            })
        });
        group.bench_function("incremental_full_including_queue_and_drop", |b| {
            b.iter(|| {
                drop(black_box(bounded(black_box(&boundaries), black_box(&ids))));
            })
        });
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(1));
    targets = benchmarks
}
criterion_main!(benches);
