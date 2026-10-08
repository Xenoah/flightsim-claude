//! 地形アクセスのベンチマーク。
//!
//! # 何を知りたいか
//!
//! - **標高クエリ**: `GroundSampler` が 1 物理ステップあたり 5 回呼ぶ。
//!   120 Hz なら毎秒 600 回。ここが重いと物理が予算を食い潰す
//! - **タイルの復号**: ストリーミングの 1 フレーム予算を決める根拠になる。
//!   1 枚あたりの時間 × 予算枚数がフレーム時間に乗る
//! - **LOD 選択**: 毎フレーム 1 回。カメラが動くたびに走る

use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Degrees, Geodetic, Meters};
use flightsim_world::dem::io::{read_tile, write_tile};
use flightsim_world::{
    ClimateDate, DemTile, GlobalClimate, HeightGrid, LodSelector, MemoryTileSource, Terrain, TileId,
};
use std::hint::black_box;

/// 起伏のある格子。平坦だと分岐予測が効きすぎて実態から外れる。
fn hilly(size: u32) -> HeightGrid {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "ベンチ用の合成標高。f32 の精度で十分"
    )]
    let samples: Vec<f32> = (0..size)
        .flat_map(|row| {
            (0..size).map(move |column| {
                let x = f64::from(column) / f64::from(size - 1);
                let y = f64::from(row) / f64::from(size - 1);
                (800.0 + 400.0 * (x * 7.0).sin() * (y * 5.0).cos()) as f32
            })
        })
        .collect();
    HeightGrid::new(size, size, samples)
}

fn benchmarks(criterion: &mut Criterion) {
    // --- 標高クエリ ---

    let id = TileId::new(12, 3_600, 1_500);
    let mut source = MemoryTileSource::new();
    source.insert(id, DemTile::new(id.bounds(), hilly(65)));

    let mut group = criterion.benchmark_group("terrain_lookup");

    // キャッシュに乗っている場合。通常の飛行中はこれが支配的。
    group.bench_function("cached_hit", |bencher| {
        let mut terrain = Terrain::new(&source, 32 * 1024 * 1024, 12..=12);
        let probe = id.center();
        // 先に暖める。
        let _ = terrain.elevation_at(probe);
        bencher.iter(|| black_box(terrain.elevation_at(black_box(probe))));
    });

    // タイルが無い場合。海上を飛ぶときはこれが毎回走る。
    group.bench_function("known_miss", |bencher| {
        let mut terrain = Terrain::new(&source, 32 * 1024 * 1024, 8..=12);
        let probe = Geodetic::from_degrees(0.0, -150.0, 0.0);
        let _ = terrain.elevation_at(probe);
        bencher.iter(|| black_box(terrain.elevation_at(black_box(probe))));
    });

    // 接地平面 1 回ぶん（中心 + 北南東西の 4 探査）。
    group.bench_function("ground_plane_five_probes", |bencher| {
        let mut terrain = Terrain::new(&source, 32 * 1024 * 1024, 12..=12);
        let centre = id.center();
        let offset = id.bounds().width().get() * 1e-4;
        let probes = [
            centre,
            Geodetic::new(
                flightsim_core::Radians(centre.latitude.get() + offset),
                centre.longitude,
                Meters::ZERO,
            ),
            Geodetic::new(
                flightsim_core::Radians(centre.latitude.get() - offset),
                centre.longitude,
                Meters::ZERO,
            ),
            Geodetic::new(
                centre.latitude,
                flightsim_core::Radians(centre.longitude.get() + offset),
                Meters::ZERO,
            ),
            Geodetic::new(
                centre.latitude,
                flightsim_core::Radians(centre.longitude.get() - offset),
                Meters::ZERO,
            ),
        ];
        for probe in probes {
            let _ = terrain.elevation_at(probe);
        }
        bencher.iter(|| {
            for probe in probes {
                black_box(terrain.elevation_at(black_box(probe)));
            }
        });
    });

    group.finish();

    // --- タイルの符号化・復号 ---

    let mut group = criterion.benchmark_group("tile_codec");
    for size in [33_u32, 65, 129] {
        let grid = hilly(size);
        let mut encoded = Vec::new();
        write_tile(&mut encoded, id, &grid).expect("the synthetic grid encodes");

        group.bench_function(format!("decode_{size}x{size}"), |bencher| {
            bencher.iter(|| black_box(read_tile(&mut black_box(encoded.as_slice()))).is_ok());
        });
        group.bench_function(format!("encode_{size}x{size}"), |bencher| {
            bencher.iter(|| {
                let mut out = Vec::with_capacity(encoded.len());
                write_tile(&mut out, black_box(id), black_box(&grid)).expect("encodes");
                black_box(out.len())
            });
        });
    }
    group.finish();

    // --- LOD 選択 ---

    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        12,
        Meters(20_000.0),
    );
    let mut group = criterion.benchmark_group("lod_select");
    for (label, altitude) in [
        ("low_500m", 500.0),
        ("cruise_3km", 3_000.0),
        ("high_10km", 10_000.0),
    ] {
        let camera = Geodetic::from_degrees(35.553, 139.781, altitude).to_ecef();
        group.bench_function(label, |bencher| {
            bencher.iter(|| black_box(selector.select(black_box(camera))).tiles.len());
        });
    }
    let coverage = flightsim_world::PrimaryCoverage::from_tiles([
        TileId::new(10, 1077, 244),
        TileId::new(10, 1078, 244),
    ])
    .unwrap();
    let camera = Geodetic::from_degrees(47.068, 9.501, 3522.675).to_ecef();
    for (label, hint) in [
        ("balzers_3km_sse", None),
        ("balzers_3km_primary_coverage", Some(&coverage)),
    ] {
        group.bench_function(label, |bencher| {
            bencher.iter(|| {
                black_box(selector.select_with_coverage(black_box(camera), Meters(522.675), hint))
                    .tiles
                    .len()
            });
        });
    }
    group.finish();
}

/// Full public climate samples, including the annual regional classification.
/// Keep loading out of the timed loop; the real runtime shares this snapshot.
fn climate_benchmarks(criterion: &mut Criterion) {
    let climate = GlobalClimate::bundled().expect("bundled climate data validates");
    let date = ClimateDate::from_month(7).expect("valid month");
    let mut group = criterion.benchmark_group("climate_lookup");
    group.throughput(criterion::Throughput::Elements(1));
    for (label, point) in [
        ("midlatitude", Geodetic::from_degrees(35.55, 139.78, 1500.0)),
        ("dateline", Geodetic::from_degrees(-45.0, 179.999, 3000.0)),
        ("north_pole", Geodetic::from_degrees(90.0, 0.0, 500.0)),
    ] {
        group.bench_function(label, |bencher| {
            bencher.iter(|| black_box(climate.sample(black_box(point), black_box(date))));
        });
    }
    group.bench_function("bundled_snapshot_clone", |bencher| {
        bencher.iter(|| black_box(GlobalClimate::bundled().expect("already validated")));
    });
    group.finish();

    // Match the UI raster dimensions, without a dependency on its Bevy crate.
    // This measures only climate CPU sampling, not terrain, palette, upload,
    // UI frame time or the interactive app's total map latency.
    let probes: Vec<_> = (0..360)
        .flat_map(|row| {
            (0..720).map(move |col| {
                Geodetic::from_degrees(
                    90.0 - (f64::from(row) + 0.5) * 0.5,
                    (f64::from(col) + 0.5) * 0.5 - 180.0,
                    0.0,
                )
            })
        })
        .collect();
    let mut group = criterion.benchmark_group("climate_map");
    group.sample_size(10);
    group.throughput(criterion::Throughput::Elements(720 * 360));
    group.bench_function("full_sample_720x360", |bencher| {
        bencher.iter(|| {
            for &point in &probes {
                black_box(climate.sample(black_box(point), black_box(date)));
            }
        });
    });
    group.finish();
}

/// Compare render tessellation without changing the DEM or physical sampler.
/// An optional local .fsdem path supplies a reproducible regional-data fixture;
/// no fixture data or path is embedded in the repository or timed loop.
fn mesh_resolution_benchmarks(criterion: &mut Criterion) {
    let id = TileId::containing(13, Geodetic::from_degrees(47.139, 9.518, 0.0));
    let atlas = flightsim_world::global::GlobalTerrain::bundled().unwrap();
    let mut fixtures = vec![
        ("synthetic_65", id, DemTile::new(id.bounds(), hilly(65))),
        ("global_33", id, atlas.tile(id).unwrap()),
    ];
    if let Some(path) = std::env::var_os("FLIGHTSIM_BENCH_DEM") {
        let mut file = std::fs::File::open(path).expect("open explicitly supplied benchmark DEM");
        let stored = read_tile(&mut file).expect("validate explicitly supplied benchmark DEM");
        fixtures.push(("regional_fixture", stored.id, stored.tile));
    }
    let mut group = criterion.benchmark_group("mesh_resolution");
    group.sample_size(20);
    group.warm_up_time(std::time::Duration::from_secs(1));
    group.measurement_time(std::time::Duration::from_secs(2));
    for (label, id, dem) in fixtures {
        for resolution in [33, 65] {
            let options = flightsim_world::MeshOptions {
                resolution,
                skirt_depth: None,
            };
            group.bench_function(format!("{label}/{resolution}"), |bencher| {
                bencher.iter(|| {
                    black_box(flightsim_world::build_mesh(
                        black_box(id),
                        black_box(&dem),
                        black_box(&options),
                    ))
                });
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    benchmarks,
    climate_benchmarks,
    mesh_resolution_benchmarks
);
criterion_main!(benches);
