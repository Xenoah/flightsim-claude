//! 物理ステップのベンチマーク。
//!
//! # 何を知りたいか
//!
//! 描画フレームの予算のうち、FDM が何を持っていくか。
//! `FIXED_DT = 1/120 s` なので 60 Hz 描画なら 1 フレームで 2 ステップ、
//! 144 Hz なら 1〜2 ステップ回る（ADR-0004）。
//!
//! **接地中はサブステップに分割される**ので、空中より確実に重い。
//! そこを測らずに「軽い」と言わないこと。

use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Attitude, Geodetic, Meters, Ned, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState,
};
use std::hint::black_box;

fn airborne_state(altitude: f64, speed: f64) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.553, 139.781, altitude),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(speed, 0.0, 0.0),
    )
}

fn benchmarks(criterion: &mut Criterion) {
    let config = AircraftConfig::light_single();
    let cruise_controls = ControlInputs::new(0.0, 0.05, 0.0, 0.6, 0.0);

    let mut group = criterion.benchmark_group("fdm_step");

    // 空中・定常。もっとも軽い経路。
    group.bench_function("cruise", |bencher| {
        let environment = Environment::still_air();
        bencher.iter_batched_ref(
            || FlightDynamics::new(config.clone(), airborne_state(1_500.0, 50.0)),
            |dynamics| {
                dynamics.step(
                    black_box(RECOMMENDED_FIXED_DT),
                    black_box(cruise_controls),
                    black_box(&environment),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // 旋回中。角速度が大きいとサブステップが増える。
    group.bench_function("turning", |bencher| {
        let environment = Environment::still_air();
        let mut state = airborne_state(1_500.0, 50.0);
        state.angular_velocity = glam::DVec3::new(0.6, 0.2, 0.15);
        let controls = ControlInputs::new(0.7, 0.2, 0.1, 0.7, 0.0);
        bencher.iter_batched_ref(
            || FlightDynamics::new(config.clone(), state),
            |dynamics| {
                dynamics.step(
                    black_box(RECOMMENDED_FIXED_DT),
                    black_box(controls),
                    black_box(&environment),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // 接地中。脚の剛性でサブステップが増える最悪ケース。
    group.bench_function("on_ground", |bencher| {
        let ground = Geodetic::from_degrees(35.553, 139.781, 0.0);
        let environment = Environment::still_air().with_ground_plane(
            ground,
            Meters::ZERO,
            flightsim_fdm::GroundSlope::LEVEL,
        );
        let parked = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.553, 139.781, 1.0),
            Attitude::default(),
            Ned::new(20.0, 0.0, 0.0),
        );
        bencher.iter_batched_ref(
            || FlightDynamics::new(config.clone(), parked),
            |dynamics| {
                dynamics.step(
                    black_box(RECOMMENDED_FIXED_DT),
                    black_box(ControlInputs::new(0.0, 0.0, 0.0, 1.0, 0.0)),
                    black_box(&environment),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();

    // 1 秒ぶんの飛行。フレーム単位ではなく体感的な規模で見る。
    criterion.bench_function("fdm_one_second_of_flight", |bencher| {
        let environment = Environment::still_air();
        bencher.iter_batched_ref(
            || FlightDynamics::new(config.clone(), airborne_state(1_500.0, 50.0)),
            |dynamics| {
                for _ in 0..120 {
                    dynamics.step(
                        black_box(Seconds(1.0 / 120.0)),
                        black_box(cruise_controls),
                        black_box(&environment),
                    );
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
