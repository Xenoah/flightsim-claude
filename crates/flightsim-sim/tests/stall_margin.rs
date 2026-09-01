//! 失速警報の入力になる迎角の検査。
//!
//! **警報が鳴るのと実際に失速するのがずれると、警報が信用されなくなる。**
//! 音そのものは `flightsim-audio` の担当だが、いつ鳴らすかを決める値は
//! ここが出す。

use flightsim_core::{Attitude, Degrees, Geodetic, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{GroundSampler, Simulation, Wind};
use flightsim_world::{MemoryTileSource, Terrain};

/// ちょうど 0 が返ること。**「小さい値」ではなく 0 が契約。**
///
/// 迎角が定義できない状況では判定に使える値が無いので、`stall_fraction` は
/// 近似ではなく 0 を返す。厳密比較で構わない。
#[expect(
    clippy::float_cmp,
    reason = "0 を返すことが契約なので近似では検査にならない"
)]
fn assert_silent(fraction: f64, message: &str) {
    assert_eq!(fraction, 0.0, "{message}");
}

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

/// 水平飛行の状態を、指定した迎角で作る。
///
/// 機首を `pitch` だけ上げ、速度は水平に置く。相対風が水平なので
/// 迎角はそのまま `pitch` になる。
fn flying_at(pitch_degrees: f64, speed: f64) -> Simulation<MemoryTileSource> {
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1_000.0),
        Attitude::new(
            Radians::ZERO,
            Degrees(pitch_degrees).to_radians(),
            Radians::ZERO,
        ),
        Ned::new(speed, 0.0, 0.0),
    );
    Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    )
}

#[test]
fn level_flight_is_far_from_the_stall() {
    // 巡航中に警報が鳴っては、警報そのものが無視されるようになる。
    let simulation = flying_at(0.0, 50.0);
    let fraction = simulation.stall_fraction();
    assert!(
        fraction < 0.3,
        "level cruise should not be near the stall, got {fraction}"
    );
}

#[test]
fn pulling_the_nose_up_walks_towards_the_stall() {
    // **単調に増えること。** 途中で下がると、警報が点いたり消えたりする。
    let mut previous = -1.0;
    for pitch in [0.0, 4.0, 8.0, 12.0, 16.0] {
        let fraction = flying_at(pitch, 50.0).stall_fraction();
        assert!(
            fraction > previous,
            "{pitch} deg gave {fraction}, which is not above the previous {previous}"
        );
        previous = fraction;
    }
}

#[test]
fn the_stall_angle_itself_reads_as_one() {
    // 機体の失速角は 16 度（`AircraftConfig::light_single`）。
    // そこで 1.0 になるように正規化されていること。
    let fraction = flying_at(16.0, 50.0).stall_fraction();
    assert!(
        (fraction - 1.0).abs() < 0.02,
        "at the stall angle the fraction should be 1.0, got {fraction}"
    );
}

#[test]
fn beyond_the_stall_the_value_keeps_going_up() {
    // **1.0 で頭打ちにしない。** 呼び出し側が「もう失速している」と
    // 「近づいている」を区別できなくなる。
    let fraction = flying_at(24.0, 50.0).stall_fraction();
    assert!(fraction > 1.0, "got {fraction}");
}

#[test]
fn a_parked_aircraft_does_not_warn() {
    // **駐機中に警報が鳴ってはいけない。** 静止時の迎角は定義できず、
    // わずかな速度成分で跳ねる。
    let simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.55, 139.78, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );
    assert_silent(
        simulation.stall_fraction(),
        "a parked aircraft must not warn",
    );
}

#[test]
fn a_slow_taxi_does_not_warn_either() {
    // 滑走開始直後も同じ。速度が乗るまでは黙っていること。
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.55, 139.78, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );
    for _ in 0..60 {
        simulation.advance(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral().with_throttle(0.3),
        );
        assert_silent(
            simulation.stall_fraction(),
            "a slow taxi must not raise the warning",
        );
    }
}

#[test]
fn the_relative_wind_is_what_counts_not_the_ground_speed() {
    // **失速は迎角の現象で、速度の現象ではない。** だから警報も迎角で出す。
    //
    // 経路に沿った追い風は対気速度を落とすが、相対風の**向き**は変えないので
    // 迎角は変わらない。これは正しい挙動で、実機の失速警報（迎角ベーン）も
    // 同じように振る舞う。ここではその 2 つを分けて確かめる。
    let mut still = flying_at(6.0, 30.0);
    still.set_wind(Wind::CALM);

    let mut tailwind = flying_at(6.0, 30.0);
    tailwind.set_wind(Wind {
        // 機首は北を向いている。南から吹く風は追い風。
        from: Degrees(180.0).to_radians(),
        speed: flightsim_core::MetersPerSecond(15.0),
    });

    // 迎角は変わらない。
    assert!(
        (tailwind.stall_fraction() - still.stall_fraction()).abs() < 1e-6,
        "an along-track wind must not change the angle of attack"
    );

    // 対気速度は変わる。**ここが変わらないなら風を見ていない。**
    let calm_speed = still.aero_angles().true_airspeed.get();
    let tail_speed = tailwind.aero_angles().true_airspeed.get();
    assert!(
        (calm_speed - tail_speed).abs() > 10.0,
        "a 15 m/s tailwind must show up in the airspeed; calm {calm_speed}, tailwind {tail_speed}"
    );
    assert!(
        tail_speed < calm_speed,
        "a tailwind reduces airspeed, got {tail_speed} against {calm_speed}"
    );
}

#[test]
fn a_crosswind_changes_the_relative_airflow() {
    // 経路を横切る風は横滑り角を作る。**風がまったく効いていなければ、
    // ここも 0 のままになる。**
    let mut crosswind = flying_at(0.0, 30.0);
    crosswind.set_wind(Wind {
        from: Degrees(90.0).to_radians(),
        speed: flightsim_core::MetersPerSecond(10.0),
    });
    let sideslip = crosswind.aero_angles().sideslip.get().abs();
    assert!(
        sideslip > 0.1,
        "a 10 m/s crosswind should produce a noticeable sideslip, got {sideslip} rad"
    );
}

#[test]
fn a_diverged_state_does_not_produce_a_warning_value() {
    // NaN が警報の判定に流れると、鳴りっぱなしか鳴らないかのどちらかになる。
    let state = RigidBodyState {
        velocity: glam::DVec3::new(f64::NAN, 0.0, 0.0),
        ..*flying_at(0.0, 50.0).state()
    };
    let simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );
    let fraction = simulation.stall_fraction();
    assert!(fraction.is_finite(), "got {fraction}");
    assert_silent(fraction, "a diverged state must not warn");
}
