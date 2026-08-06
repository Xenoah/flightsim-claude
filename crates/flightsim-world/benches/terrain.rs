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
use flightsim_world::{DemTile, HeightGrid, LodSelector, MemoryTileSource, Terrain, TileId};
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
    group.finish();
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
