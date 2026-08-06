//! 座標変換と floating origin のベンチマーク。
//!
//! # 何を知りたいか
//!
//! [ADR-0002](../../../docs/adr/0002-coordinate-system.md) が名指しで要求している項目。
//!
//! > floating origin の打ち直し時、`f32` 側の全オブジェクト位置を一括更新する必要がある。
//! > この処理はフレームスパイクになり得るため、`render` 担当はここをベンチ対象に含めること。
//!
//! 打ち直し 1 回のコストと、1 オブジェクトあたりの変換コストを分けて測る。
//! 掛け算すれば「N 個のオブジェクトを持つシーンでスパイクがどれだけ出るか」が出る。
//!
//! 測地変換も測る。ECEF → 測地は Bowring 法の反復なので、片道より高い。

use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Ecef, FloatingOrigin, Geodetic};
use std::hint::black_box;

/// 実在の空港くらいの位置に散らした点群。
fn scene(count: usize, around: Ecef) -> Vec<Ecef> {
    (0..count)
        .map(|index| {
            // 決定論的に散らす。乱数を使うと実行ごとに結果が動く。
            #[allow(
                clippy::cast_precision_loss,
                reason = "ベンチ用の点群生成。index は高々 10 万"
            )]
            let t = index as f64;
            Ecef::new(
                around.as_vec().x + (t * 0.7).sin() * 3_000.0,
                around.as_vec().y + (t * 1.3).cos() * 3_000.0,
                around.as_vec().z + (t * 0.11).sin() * 500.0,
            )
        })
        .collect()
}

fn benchmarks(criterion: &mut Criterion) {
    let anchor = Geodetic::from_degrees(35.553, 139.781, 100.0).to_ecef();

    // --- 1 点あたりの変換 ---

    let mut group = criterion.benchmark_group("floating_origin");
    group.bench_function("to_render_single", |bencher| {
        let origin = FloatingOrigin::new(anchor);
        let point = Ecef::new(
            anchor.as_vec().x + 1_234.0,
            anchor.as_vec().y - 567.0,
            anchor.as_vec().z + 89.0,
        );
        bencher.iter(|| black_box(origin.to_render(black_box(point))));
    });

    // --- 打ち直しに伴う一括更新 ---
    //
    // ADR-0002 が要求しているフレームスパイクの評価。
    // シーン規模ごとに測って、許容できる上限を把握する。
    for count in [1_000_usize, 10_000, 100_000] {
        let points = scene(count, anchor);
        group.bench_function(format!("rebase_and_update_{count}_objects"), |bencher| {
            bencher.iter_batched_ref(
                || FloatingOrigin::new(anchor),
                |origin| {
                    // カメラが閾値を越えて移動し、打ち直しが走る想定。
                    let moved = Ecef::new(
                        anchor.as_vec().x + 5_000.0,
                        anchor.as_vec().y,
                        anchor.as_vec().z,
                    );
                    black_box(origin.rebase_if_needed(black_box(moved)));
                    // 打ち直したら全オブジェクトの f32 位置を作り直す。
                    for &point in &points {
                        black_box(origin.to_render(point));
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();

    // --- 測地変換 ---

    let mut group = criterion.benchmark_group("geodetic");
    let position = Geodetic::from_degrees(35.553, 139.781, 3_000.0);
    let ecef = position.to_ecef();

    group.bench_function("geodetic_to_ecef", |bencher| {
        bencher.iter(|| black_box(black_box(position).to_ecef()));
    });
    // Bowring 法 + 固定点反復。片道より高いのが普通。
    group.bench_function("ecef_to_geodetic", |bencher| {
        bencher.iter(|| black_box(black_box(ecef).to_geodetic()));
    });
    group.bench_function("local_frame_construction", |bencher| {
        bencher.iter(|| black_box(flightsim_core::LocalFrame::new(black_box(position))));
    });
    group.finish();
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
