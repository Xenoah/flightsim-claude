//! ADR-0004 が定める FDM の不変条件を検査する統合テスト。
//!
//! ユニットテストが個々の式の正しさを見るのに対し、ここでは
//! **積分ループ全体を長時間回したときに壊れないこと** を見る。
//!
//! FDM は Bevy に依存せず決定論的なので、10 分の飛行を数秒でヘッドレス実行できる。
//! これは ADR-0001 の技術選定で意図的に確保した性質であり、ここで使い切る。

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "ステップ数の算出。秒数も刻み幅もテスト内の既知の正の定数であり、\
              切り捨ても符号も問題にならない"
)]

use flightsim_core::{Attitude, Degrees, Geodetic, LocalFrame, Ned, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState, aircraft::MassProperties, gravity,
};
use glam::DVec3;

/// 高度 2 000 m を北へ 55 m/s（約 107 kt）で巡航する状態。
fn cruising_state(altitude: f64, speed: f64) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.0, 139.0, altitude),
        Attitude::default(),
        Ned::new(speed, 0.0, 0.0),
    )
}

fn cruising() -> FlightDynamics {
    FlightDynamics::new(
        AircraftConfig::light_single(),
        cruising_state(2_000.0, 55.0),
    )
}

/// 一定の操縦入力で `seconds` 秒ぶん飛ばす。
fn fly(fdm: &mut FlightDynamics, controls: ControlInputs, environment: &Environment, seconds: f64) {
    let steps = (seconds / RECOMMENDED_FIXED_DT.get()).round() as usize;
    for _ in 0..steps {
        fdm.step(RECOMMENDED_FIXED_DT, controls, environment);
    }
}

// ---------------------------------------------------------------------------
// 不変条件 1: 決定論
// ---------------------------------------------------------------------------

#[test]
fn identical_inputs_produce_bit_identical_trajectories() {
    // ADR-0004 の中核。これが崩れるとリプレイもネットワーク同期も回帰テストも
    // 全て成立しなくなる。
    let run = || {
        let mut fdm = cruising();
        let environment = Environment::still_air();
        let mut samples = Vec::new();

        for i in 0..2_400 {
            // 時間変化する入力列。定常入力より条件が厳しい。
            let phase = f64::from(i) * 0.01;
            let controls = ControlInputs::new(
                0.3 * phase.sin(),
                0.2 * phase.cos(),
                0.1 * (phase * 0.5).sin(),
                0.7,
                0.0,
            );
            fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);

            if i % 100 == 0 {
                samples.push(*fdm.state());
            }
        }
        samples
    };

    // ビット単位で完全一致すること。「ほぼ同じ」では不十分。
    assert_eq!(run(), run(), "the FDM is not deterministic");
}

#[test]
fn simulation_does_not_depend_on_how_many_times_step_is_called_per_second() {
    // 同じ状態から同じ入力で 1 ステップ進めた結果は、何度やっても同じ。
    // グローバル状態や内部カウンタへの依存を検出する。
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral()
        .with_throttle(0.6)
        .with_elevator(0.1);

    let mut reference = cruising();
    reference.step(RECOMMENDED_FIXED_DT, controls, &environment);
    let expected = *reference.state();

    for _ in 0..10 {
        let mut fdm = cruising();
        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);
        assert_eq!(*fdm.state(), expected);
    }
}

// ---------------------------------------------------------------------------
// 不変条件 2: 積分器の健全性（エネルギー保存）
// ---------------------------------------------------------------------------

/// 空気力を完全に除いた機体。重力だけが働く弾道運動になる。
fn ballistic_config() -> AircraftConfig {
    let mut config = AircraftConfig::light_single();
    config.mass_properties = MassProperties::new(config.mass_properties.mass(), 1.0, 1.0, 1.0, 0.0);
    config.aero = flightsim_fdm::AeroCoefficients {
        lift_zero: 0.0,
        lift_alpha: 0.0,
        lift_flaps: 0.0,
        stall_angle: flightsim_core::Radians(1.0),
        stall_blend_rate: 0.0,
        drag_min: 0.0,
        oswald_efficiency: 1.0,
        drag_flaps: 0.0,
        side_beta: 0.0,
        side_rudder: 0.0,
        roll_beta: 0.0,
        roll_rate_p: 0.0,
        roll_rate_r: 0.0,
        roll_aileron: 0.0,
        roll_rudder: 0.0,
        pitch_zero: 0.0,
        pitch_alpha: 0.0,
        pitch_rate_q: 0.0,
        pitch_elevator: 0.0,
        pitch_flaps: 0.0,
        yaw_beta: 0.0,
        yaw_rate_p: 0.0,
        yaw_rate_r: 0.0,
        yaw_aileron: 0.0,
        yaw_rudder: 0.0,
    };
    config
}

#[test]
fn ballistic_motion_conserves_energy() {
    // 無風・無推力・無空気力なら、運動エネルギー + 位置エネルギーは保存する。
    // オイラー法だとここで明確にエネルギーが増減する。RK4 の健全性検査。
    let position = Geodetic::from_degrees(0.0, 0.0, 5_000.0);
    let frame = LocalFrame::new(position);

    // 真上に 50 m/s で打ち上げる。高度変化は約 130 m に収まるので、
    // その範囲で重力加速度はほぼ一定とみなせる（変化率 4e-5）。
    let state =
        RigidBodyState::from_geodetic(position, Attitude::default(), Ned::new(0.0, 0.0, -50.0));
    let mut fdm = FlightDynamics::new(ballistic_config(), state);

    let g = gravity::magnitude(position);
    let specific_energy = |s: &RigidBodyState| {
        let speed_sq = s.velocity.length_squared();
        0.5 * speed_sq + g * s.altitude().get()
    };

    let initial = specific_energy(fdm.state());

    for _ in 0..1_200 {
        fdm.step(
            RECOMMENDED_FIXED_DT,
            ControlInputs::neutral(),
            &Environment::still_air(),
        );

        let error = (specific_energy(fdm.state()) - initial).abs() / initial;
        assert!(
            error < 1e-4,
            "specific energy drifted by {:.3e} (relative); the integrator is leaking energy",
            error
        );
    }

    // 10 秒後には打ち上げた高さから戻ってきている（放物運動）。
    let _ = frame;
    assert!(fdm.state().is_finite());
}

#[test]
fn ballistic_motion_matches_the_analytic_solution() {
    // 一定重力での鉛直投射: h(t) = h0 + v0·t - ½g·t²
    let position = Geodetic::from_degrees(0.0, 0.0, 5_000.0);
    let launch_speed = 50.0;
    let state = RigidBodyState::from_geodetic(
        position,
        Attitude::default(),
        Ned::new(0.0, 0.0, -launch_speed),
    );
    let mut fdm = FlightDynamics::new(ballistic_config(), state);

    let g = gravity::magnitude(position);
    let duration = 5.0;
    fly(
        &mut fdm,
        ControlInputs::neutral(),
        &Environment::still_air(),
        duration,
    );

    let expected = 5_000.0 + launch_speed * duration - 0.5 * g * duration * duration;
    let actual = fdm.state().altitude().get();

    // 重力の高度変化ぶんのずれが残るため 0.1 m の許容。
    assert!(
        (actual - expected).abs() < 0.1,
        "ballistic altitude was {actual:.3} m, analytic solution gives {expected:.3} m"
    );
}

// ---------------------------------------------------------------------------
// 不変条件 3: 長時間の数値安定性
// ---------------------------------------------------------------------------

#[test]
fn ten_minutes_of_flight_does_not_diverge() {
    // ADR-0004 の不変条件。トリムを取っていないので長周期振動（フゴイド）は起きるが、
    // それは物理的に正しい挙動であり発散ではない。
    // ここで見るのは「値が爆発しないこと」と「NaN が出ないこと」。
    let mut fdm = cruising();
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral().with_throttle(0.65);

    let steps = (600.0 / RECOMMENDED_FIXED_DT.get()).round() as usize;
    for step in 0..steps {
        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);

        let state = fdm.state();
        assert!(
            state.is_finite(),
            "state went non-finite after {:.1} s",
            f64::from(u32::try_from(step).unwrap()) * RECOMMENDED_FIXED_DT.get()
        );

        let altitude = state.altitude().get();
        assert!(
            (-1_000.0..30_000.0).contains(&altitude),
            "altitude reached {altitude:.0} m after {:.1} s — the simulation diverged",
            f64::from(u32::try_from(step).unwrap()) * RECOMMENDED_FIXED_DT.get()
        );

        let speed = state.velocity.length();
        assert!(
            speed < 500.0,
            "speed reached {speed:.0} m/s after {:.1} s — the simulation diverged",
            f64::from(u32::try_from(step).unwrap()) * RECOMMENDED_FIXED_DT.get()
        );
    }
}

#[test]
fn quaternion_norm_stays_within_tolerance_over_a_long_flight() {
    // ADR-0004 の不変条件: ノルムの誤差が 1e-9 を超えない。
    // 正規化を怠ると姿勢がじわじわ歪み、水平飛行しているのに機体が傾いて見える。
    let mut fdm = cruising();
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral()
        .with_throttle(0.7)
        .with_aileron(0.15)
        .with_rudder(0.05);

    fly(&mut fdm, controls, &environment, 300.0);

    let norm = fdm.state().orientation.length();
    assert!(
        (norm - 1.0).abs() < 1e-9,
        "quaternion norm drifted to {norm} after five minutes of manoeuvring"
    );
}

#[test]
fn halving_the_timestep_converges_to_the_same_trajectory() {
    // RK4 は 4 次精度なので、刻みを半分にしても結果はほとんど変わらないはず。
    // 大きく変わるなら、刻みが粗すぎるか積分器に欠陥がある。
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral()
        .with_throttle(0.7)
        .with_elevator(0.05);
    let duration = 30.0;

    let run_at = |dt: f64| {
        let mut fdm = cruising();
        let steps = (duration / dt).round() as usize;
        for _ in 0..steps {
            fdm.step(Seconds(dt), controls, &environment);
        }
        *fdm.state()
    };

    let coarse = run_at(RECOMMENDED_FIXED_DT.get());
    let fine = run_at(RECOMMENDED_FIXED_DT.get() / 2.0);

    let separation = coarse.position.distance_to(fine.position).get();
    assert!(
        separation < 1.0,
        "halving the timestep moved the aircraft {separation:.3} m after {duration} s; \
         the integrator has not converged"
    );
}

// ---------------------------------------------------------------------------
// 飛行特性
// ---------------------------------------------------------------------------

#[test]
fn full_back_elevator_leads_to_a_stall_without_numerical_breakdown() {
    // 操縦桿を引き切ると迎角が失速角を超える。ここで NaN が出たり
    // 揚力が増え続けたりしないことを確認する。
    let mut fdm = cruising();
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral()
        .with_throttle(0.3)
        .with_elevator(1.0);

    let mut peak_angle_of_attack: f64 = 0.0;

    let steps = (20.0 / RECOMMENDED_FIXED_DT.get()).round() as usize;
    for _ in 0..steps {
        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);
        assert!(
            fdm.state().is_finite(),
            "the stall produced a non-finite state"
        );

        let angles = flightsim_fdm::aero_angles(fdm.state().body_velocity());
        peak_angle_of_attack = peak_angle_of_attack.max(angles.angle_of_attack.get());
    }

    let stall_angle = fdm.config().aero.stall_angle.get();
    assert!(
        peak_angle_of_attack > stall_angle,
        "pulling full back never reached the stall angle ({:.1}° vs {:.1}°)",
        peak_angle_of_attack.to_degrees(),
        stall_angle.to_degrees()
    );
}

#[test]
fn aileron_input_rolls_the_aircraft_and_changes_heading() {
    // ロール → 揚力ベクトルが傾く → 旋回。基本的な飛行の因果関係。
    let mut fdm = cruising();
    let environment = Environment::still_air();
    let initial_heading = fdm.state().attitude().yaw;

    // 横操舵は 0.8 秒だけ。押し続けると何回転もしてしまう（実機と同じ）。
    fly(
        &mut fdm,
        ControlInputs::neutral()
            .with_throttle(0.7)
            .with_aileron(0.4),
        &environment,
        0.8,
    );

    let bank = fdm.state().attitude().roll.to_degrees().get();
    assert!(
        (5.0..60.0).contains(&bank),
        "0.8 s of right aileron produced {bank:.1}° of bank, expected a moderate right bank"
    );

    // 舵を中立に戻したまま旋回を継続させる。
    fly(
        &mut fdm,
        ControlInputs::neutral().with_throttle(0.7),
        &environment,
        8.0,
    );

    // 方位の比較は最短角差で行う（359° → 1° の折り返しがあるため）。
    let heading_change = initial_heading.shortest_difference_to(fdm.state().attitude().yaw);
    assert!(
        heading_change.get() > 0.0,
        "a right bank should turn the aircraft right, but heading changed by {:.1}°",
        heading_change.to_degrees().get()
    );
}

#[test]
fn full_aileron_roll_rate_is_physically_plausible() {
    // 回帰テスト。安定微係数は「舵角 1 rad あたり」で定義されているため、
    // 正規化入力 [-1, 1] にそのまま掛けると舵角 57.3° 相当の過大な効きになる。
    // この取り違えは毎秒 217° というロール率として現れた（実機は 60〜75°/s）。
    let mut fdm = cruising();
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral()
        .with_throttle(0.7)
        .with_aileron(1.0);

    // 定常ロール率に落ち着くまで待つ。
    fly(&mut fdm, controls, &environment, 1.5);

    let roll_rate = fdm.state().angular_velocity.x.to_degrees();
    assert!(
        (40.0..110.0).contains(&roll_rate),
        "full aileron produced a steady roll rate of {roll_rate:.0}°/s; \
         a light single should manage roughly 60–75°/s"
    );
}

#[test]
fn cutting_the_throttle_starts_a_descent() {
    let environment = Environment::still_air();

    let mut powered = cruising();
    let mut gliding = cruising();

    fly(
        &mut powered,
        ControlInputs::neutral().with_throttle(0.75),
        &environment,
        20.0,
    );
    fly(
        &mut gliding,
        ControlInputs::neutral().with_throttle(0.0),
        &environment,
        20.0,
    );

    assert!(
        gliding.state().altitude().get() < powered.state().altitude().get(),
        "the glider ({:.0} m) should be lower than the powered aircraft ({:.0} m)",
        gliding.state().altitude().get(),
        powered.state().altitude().get()
    );
    assert!(
        gliding.state().vertical_speed().get() < 0.0,
        "the glider should be descending"
    );
}

#[test]
fn a_steady_headwind_reduces_ground_speed_relative_to_airspeed() {
    // 風の符号の検査。ここを間違えると追い風で減速するという逆の挙動になる。
    let position = Geodetic::from_degrees(35.0, 139.0, 2_000.0);
    let environment = Environment::with_wind_ned(
        flightsim_fdm::Atmosphere::standard(),
        position,
        // 北向きに飛ぶ機体に対する向かい風は、空気が南へ動いている状態。
        Ned::new(-15.0, 0.0, 0.0),
    );

    let mut fdm = cruising();
    fly(
        &mut fdm,
        ControlInputs::neutral().with_throttle(0.7),
        &environment,
        5.0,
    );

    let ground_speed = fdm.state().ground_speed().get();
    let airspeed = flightsim_fdm::aero_angles(
        fdm.state().orientation.inverse() * (fdm.state().velocity - environment.wind_ecef),
    )
    .true_airspeed
    .get();

    assert!(
        airspeed > ground_speed + 10.0,
        "with a 15 m/s headwind, airspeed ({airspeed:.1}) should clearly exceed \
         ground speed ({ground_speed:.1})"
    );
}

#[test]
fn the_aircraft_flies_correctly_anywhere_on_the_globe() {
    // ECEF で状態を持つ設計の要点。極でも日付変更線でも同じ挙動になること。
    let environment = Environment::still_air();
    let controls = ControlInputs::neutral().with_throttle(0.7);

    let mut vertical_speeds = Vec::new();

    for (latitude, longitude) in [
        (0.0, 0.0),
        (35.0, 139.0),
        (-33.0, 151.0),
        (0.0, 179.99),  // 日付変更線
        (0.0, -179.99), // 日付変更線の反対側
        (89.0, 0.0),    // 北極近傍
        (-89.0, 45.0),  // 南極近傍
    ] {
        let mut fdm = FlightDynamics::new(
            AircraftConfig::light_single(),
            RigidBodyState::from_geodetic(
                Geodetic::from_degrees(latitude, longitude, 2_000.0),
                Attitude::from_degrees(0.0, 0.0, 45.0),
                Ned::new(
                    55.0 * Degrees(45.0).to_radians().cos(),
                    55.0 * Degrees(45.0).to_radians().sin(),
                    0.0,
                ),
            ),
        );

        fly(&mut fdm, controls, &environment, 30.0);

        assert!(
            fdm.state().is_finite(),
            "flight at ({latitude}, {longitude}) produced a non-finite state"
        );
        vertical_speeds.push(fdm.state().vertical_speed().get());
    }

    // 緯度による重力差はあるが、挙動の桁が変わることはない。
    let min = vertical_speeds
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let max = vertical_speeds
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max - min).abs() < 5.0,
        "vertical speed varied from {min:.2} to {max:.2} m/s across the globe; \
         the equations of motion should not depend on where you are"
    );
}

#[test]
fn extreme_control_inputs_never_produce_nan() {
    // 操縦桿を全方向に振り回しても数値が壊れないこと。
    // NaN は一度出ると全状態に伝播し、原因特定が極めて困難になる。
    let mut fdm = cruising();
    let environment = Environment::still_air();

    for i in 0..12_000 {
        let phase = f64::from(i) * 0.05;
        let controls = ControlInputs::new(
            phase.sin().signum(),
            (phase * 1.3).cos().signum(),
            (phase * 0.7).sin().signum(),
            if phase.sin() > 0.0 { 1.0 } else { 0.0 },
            if (phase * 0.1).sin() > 0.0 { 1.0 } else { 0.0 },
        );
        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);

        assert!(
            fdm.state().is_finite(),
            "violent control inputs produced a non-finite state at step {i}"
        );
    }
}

#[test]
fn zero_airspeed_start_does_not_break_the_simulation() {
    // 駐機状態からの開始。対気速度ゼロでの迎角計算がゼロ除算にならないこと。
    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.0, 139.0, 100.0),
        Attitude::default(),
        Ned::default(),
    );
    let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), state);

    fly(
        &mut fdm,
        ControlInputs::neutral().with_throttle(1.0),
        &Environment::still_air(),
        5.0,
    );

    assert!(
        fdm.state().is_finite(),
        "starting from rest broke the simulation"
    );
    // 推力で前進を始めているはず（接地判定はまだ無いので落下もする）。
    assert!(fdm.state().velocity.length() > 0.0);
}

#[test]
fn wind_moves_a_stationary_aircraft_in_the_wind_direction() {
    // 風の向きの検査。空気が北へ動いていれば、機体も北へ押される。
    let position = Geodetic::from_degrees(0.0, 0.0, 3_000.0);
    let environment = Environment::with_wind_ned(
        flightsim_fdm::Atmosphere::standard(),
        position,
        Ned::new(25.0, 0.0, 0.0), // 空気が北へ動いている
    );

    let state = RigidBodyState::from_geodetic(position, Attitude::default(), Ned::default());
    let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), state);

    fly(&mut fdm, ControlInputs::neutral(), &environment, 3.0);

    let velocity_ned = fdm.state().velocity_ned();
    assert!(
        velocity_ned.north() > 0.0,
        "a northward wind should push the aircraft north, but its north velocity is {:.2} m/s",
        velocity_ned.north()
    );
}

#[test]
fn gyroscopic_coupling_is_present() {
    // ω × (Iω) の項。これを落とすと、回転中の機体の挙動が明確におかしくなる。
    // 慣性モーメントが軸ごとに異なる機体をロール・ピッチ同時に回すと、
    // 入力していないヨー軸にも角加速度が現れる。
    let mut state = cruising_state(3_000.0, 60.0);
    state.angular_velocity = DVec3::new(2.0, 1.5, 0.0);

    let mut fdm = FlightDynamics::new(AircraftConfig::light_single(), state);
    fdm.step(
        RECOMMENDED_FIXED_DT,
        ControlInputs::neutral(),
        &Environment::still_air(),
    );

    let yaw_rate = fdm.state().angular_velocity.z;
    assert!(
        yaw_rate.abs() > 1e-6,
        "no yaw rate appeared from combined roll and pitch; \
         the ω × (Iω) coupling term is probably missing"
    );
}
