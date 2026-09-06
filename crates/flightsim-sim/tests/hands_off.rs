//! 手を離したときに滑空するか、落ちるかの検査。
//!
//! # なぜ要るのか
//!
//! **昇降舵を離すと、機体は「舵中立で釣り合う速度」へ向かう。** この機体では
//! それが約 107 kt で、離陸直後の 75 kt からは大きく機首を下げて加速しようと
//! する。高度があれば長周期振動が減衰して落ち着くが、**離陸直後は最初の
//! 一振りで地面に届く**（実測: 60 m で手を離すと 8.8 秒後に接地）。
//!
//! 実機に必ずトリムが付いているのはこのため。ここではトリムの有無で
//! 高度損失がどう変わるかを固定する。
//!
//! # トリムそのものは `flightsim-input` が持つ
//!
//! FDM から見ればトリムは昇降舵の一部でしかないので、ここでは
//! **昇降舵に足した値**として渡して測る。

use flightsim_core::{Attitude, Degrees, Geodetic, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{CrashLimits, GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

/// 開始高度。ここから何 m 沈むかを測る。
const START_ALTITUDE: f64 = 1_000.0;

/// 指定した姿勢・速度から手を離して、最大の高度損失と落ち着いた速度を返す。
fn hands_off(trim: f64, throttle: f64, speed: f64, pitch_degrees: f64) -> (f64, f64) {
    let pitch = Degrees(pitch_degrees).to_radians();
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, START_ALTITUDE),
        Attitude::new(Radians::ZERO, pitch, Radians::ZERO),
        Ned::new(speed * pitch.get().cos(), 0.0, -speed * pitch.get().sin()),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    // 墜落で止めると、その後の回復が見えない。
    simulation.set_crash_limits(CrashLimits::NONE);

    let mut lowest = START_ALTITUDE;
    let mut settled = 0.0;
    for step in 0..(90 * 60) {
        simulation.advance(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral()
                .with_throttle(throttle)
                // **操縦桿は中立。トリムだけが舵を動かしている。**
                .with_elevator(trim),
        );
        lowest = lowest.min(simulation.state().altitude().get());
        if step > 60 * 60 {
            settled = simulation.airspeed().to_knots().get();
        }
    }
    (START_ALTITUDE - lowest, settled)
}

/// `flightsim-input` の既定トリム。**両方を直すのを忘れないための重複。**
const DEFAULT_TRIM: f64 = 0.09;

#[test]
fn without_trim_the_aircraft_dives_to_regain_speed() {
    // **これが「S を離すと墜落する」の正体。**
    // 舵中立の釣り合いが速いので、機首を下げて加速しようとする。
    let (lost, settled) = hands_off(0.0, 1.0, 45.0, 15.0);
    assert!(
        lost > 10.0,
        "without trim it should sink noticeably, only lost {lost} m"
    );
    assert!(
        settled > 95.0,
        "and it should settle fast, got {settled} kt"
    );
}

#[test]
fn the_default_trim_holds_the_climb_without_sinking() {
    // 通常の上昇姿勢（87 kt・機首上げ 15 度）から手を離したとき。
    // **実測で高度損失 0 m。** トリム無しでは 20 m 沈む。
    let (lost, settled) = hands_off(DEFAULT_TRIM, 1.0, 45.0, 15.0);
    assert!(
        lost < 2.0,
        "with trim the climb should continue, lost {lost} m"
    );
    assert!(
        (70.0..95.0).contains(&settled),
        "it should settle near the climb speed, got {settled} kt"
    );
}

#[test]
fn the_default_trim_gives_a_glide_with_the_power_off() {
    // **出力を切ったら降りる。それは滑空であって墜落ではない。**
    // 沈むこと自体は正しいので、確かめるのは「落ち着いた速度」の方。
    let (lost, settled) = hands_off(DEFAULT_TRIM, 0.0, 45.0, 0.0);
    assert!(lost > 50.0, "a power-off glide must lose height");
    assert!(
        (55.0..95.0).contains(&settled),
        "the glide should settle at a sane speed, got {settled} kt"
    );
}

#[test]
fn trim_sets_the_speed_it_settles_at() {
    // **トリムを引くほど遅い速度で釣り合う。** ここが逆だと、
    // トリムが速度計器と逆に動くことになる。
    let mut previous = f64::MAX;
    for trim in [0.0, 0.05, 0.09, 0.12] {
        let (_, settled) = hands_off(trim, 1.0, 45.0, 15.0);
        assert!(
            settled < previous,
            "trim {trim} settled at {settled} kt, not below the previous {previous}"
        );
        previous = settled;
    }
}

#[test]
fn trim_helps_even_from_a_badly_over_rotated_state() {
    // 45 kt・機首上げ 40 度——引き起こしすぎた状態。
    // **ここからは高度を失わずには戻れない**（速度が無いので）。
    // それでもトリムがあれば損失は大きく減る。
    // 実測: トリム無し 278 m、既定トリム 145 m。
    let (without, _) = hands_off(0.0, 1.0, 23.0, 40.0);
    let (with, _) = hands_off(DEFAULT_TRIM, 1.0, 23.0, 40.0);
    assert!(
        with < without * 0.75,
        "trim should cut the loss: {with} m against {without} m"
    );
    // **ただし「落ちない」とは言えない。** 60 m しか無ければ間に合わない。
    assert!(
        with > 50.0,
        "this test must keep documenting that recovery still costs height, got {with} m"
    );
}

#[test]
fn trim_does_not_stop_the_aircraft_taking_off() {
    // **既定トリムが強すぎると、滑走中に機首が上がって離陸を邪魔する。**
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.55, 139.78, 0.0),
        Radians::ZERO,
        Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    let mut lifted_off = None;
    for step in 0..(60 * 60) {
        let airspeed_kt = simulation.airspeed().to_knots().get();
        // 60 kt を超えたら軽く引く。人の操作を模す。
        let stick = if airspeed_kt > 60.0 { 0.2 } else { 0.0 };
        simulation.advance(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral()
                .with_throttle(1.0)
                .with_elevator(stick + DEFAULT_TRIM),
        );
        if simulation.agl().get() > 15.0 && lifted_off.is_none() {
            lifted_off = Some(f64::from(step) / 60.0);
        }
    }
    let lifted_off = lifted_off.expect("the aircraft must still get airborne with the trim on");
    assert!(
        (10.0..40.0).contains(&lifted_off),
        "lift-off at {lifted_off} s is outside the sane range"
    );
    assert!(
        !simulation.crashed(),
        "the take-off must not end in a crash"
    );
}
