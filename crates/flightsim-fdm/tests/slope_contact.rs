//! 傾斜した接地平面へ静止状態で置かれた機体が破綻しないこと。
//!
//! # 何の再現なのか
//!
//! 実地形の山腹へ spawn した機体が裏返る不具合（HUD で BNK -175.5°・AGL -4 ft・GND）。
//! `flightsim-sim` の `parked_state` は **接地平面の勾配を見ずに**
//! 「基準点の標高 + 脚の高さ」へ水平姿勢で機体を置く。傾斜地ではこれで上り側の車輪が
//! 最初から地面へめり込み、脚のばねに実体のない弾性エネルギーが仕込まれる。
//! 15° 斜面・前脚の場合で 0.43 m のめり込み、蓄積エネルギー 20 kJ に相当し、
//! 機体を前脚まわりに一回転させるのに必要な位置エネルギー 2.9 kJ を大きく超えていた。
//!
//! テストは FDM だけで閉じている（地形も乱数も壁時計も使わない）。接地平面は
//! 基準点を固定した無限斜面として与える。

use flightsim_core::{Attitude, Geodetic, LocalFrame, Meters, Ned, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, GroundSlope, RECOMMENDED_FIXED_DT,
    RigidBodyState,
};

/// 山腹の再現に使う地点。標高は合成地形の山腹に合わせた。
const SPAWN_LATITUDE: f64 = 35.55;
const SPAWN_LONGITUDE: f64 = 139.33;
const SPAWN_ELEVATION: f64 = 1_400.0;

/// 斜面に置いた機体で許す姿勢の偏位。
const MAXIMUM_ATTITUDE_EXCURSION_DEGREES: f64 = 30.0;

/// 脚を伸ばし切った状態での車輪の高さ。
fn gear_height(config: &AircraftConfig) -> f64 {
    config
        .landing_gear
        .legs()
        .iter()
        .map(|leg| leg.contact_point().as_vec().z)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// `flightsim-sim::flight::parked_state` と同じ置き方。
///
/// **勾配を考慮していない。** 実装をここに写しているのは、統合層の初期化が
/// 変わっても FDM 側の頑健性を検査し続けるため。統合層が正しく置くようになっても、
/// このテストは「めり込んだ初期状態を渡されても破綻しないこと」を保証し続ける。
fn parked_like_the_integration_layer(
    config: &AircraftConfig,
    heading_degrees: f64,
) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(
            SPAWN_LATITUDE,
            SPAWN_LONGITUDE,
            SPAWN_ELEVATION + gear_height(config),
        ),
        Attitude::from_degrees(0.0, 0.0, heading_degrees),
        Ned::default(),
    )
}

/// 北・東方向の勾配を持つ無限斜面。基準点は spawn 地点の直下。
fn sloped_ground(slope_degrees: f64, slope_bearing_degrees: f64) -> Environment {
    let tangent = slope_degrees.to_radians().tan();
    let bearing = slope_bearing_degrees.to_radians();
    Environment::still_air().with_ground_plane(
        Geodetic::from_degrees(SPAWN_LATITUDE, SPAWN_LONGITUDE, 0.0),
        Meters(SPAWN_ELEVATION),
        GroundSlope::new(tangent * bearing.cos(), tangent * bearing.sin()),
    )
}

#[derive(Debug)]
struct Excursion {
    maximum_bank_degrees: f64,
    maximum_pitch_degrees: f64,
    /// 局所鉛直と機体の上向き軸のなす角。オイラー角と違い巻き戻りが無いので、
    /// 「裏返ったか」はこちらで見るのが確実（ピッチが 90° を超えるとロールが
    /// ±180° へ飛ぶため、ロール角だけでは裏返りと表現の切り替わりを区別できない）。
    maximum_tilt_degrees: f64,
    /// 接地平面からの重心高さの最大値。跳ね上げの検出に使う。
    maximum_height_above_plane: f64,
    final_bank_degrees: f64,
    final_pitch_degrees: f64,
}

/// 斜面へ置いた機体を `seconds` 秒進め、姿勢の最大偏位を返す。
///
/// # Panics
///
/// 状態が非有限になった時点で落とす。NaN は全状態へ伝播するため、
/// 最後まで回してから判定すると原因がわからなくなる。
fn settle_on_slope(
    slope_degrees: f64,
    slope_bearing_degrees: f64,
    heading_degrees: f64,
    controls: ControlInputs,
    seconds: f64,
) -> Excursion {
    let config = AircraftConfig::light_single();
    let initial = parked_like_the_integration_layer(&config, heading_degrees);
    let environment = sloped_ground(slope_degrees, slope_bearing_degrees);
    let reference = environment
        .ground_reference()
        .expect("the sloped ground plane must carry a reference point");
    let plane_frame = LocalFrame::new(reference);
    let mut fdm = FlightDynamics::new(config, initial);

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "テストの反復回数。120 Hz × 数十秒で 10 000 未満"
    )]
    let steps = (seconds / RECOMMENDED_FIXED_DT.get()).round() as u32;
    let mut maximum_bank_degrees: f64 = 0.0;
    let mut maximum_pitch_degrees: f64 = 0.0;
    let mut maximum_tilt_degrees: f64 = 0.0;
    let mut maximum_height_above_plane = f64::NEG_INFINITY;

    for step in 0..steps {
        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);
        let state = fdm.state();
        assert!(
            state.is_finite(),
            "state became non-finite at step {step} on a {slope_degrees}° slope"
        );

        let attitude = state.attitude();
        maximum_bank_degrees = maximum_bank_degrees.max(attitude.roll.get().to_degrees().abs());
        maximum_pitch_degrees = maximum_pitch_degrees.max(attitude.pitch.get().to_degrees().abs());

        // 機体 Z 軸（下向き）と局所 Down のなす角。機体軸は正規直交なので
        // 変換後も単位ベクトルであり、Down 成分がそのまま余弦になる。
        let body_down = state
            .local_frame()
            .ecef_to_ned_vector(state.orientation * glam::DVec3::Z);
        maximum_tilt_degrees =
            maximum_tilt_degrees.max(body_down.down().clamp(-1.0, 1.0).acos().to_degrees());

        let offset = plane_frame.ecef_to_ned_position(state.position);
        let ground_here = environment.ground_elevation.get()
            + environment.ground_slope().north() * offset.north()
            + environment.ground_slope().east() * offset.east();
        maximum_height_above_plane =
            maximum_height_above_plane.max(state.altitude().get() - ground_here);
    }

    let attitude = fdm.state().attitude();
    Excursion {
        maximum_bank_degrees,
        maximum_pitch_degrees,
        maximum_tilt_degrees,
        maximum_height_above_plane,
        final_bank_degrees: attitude.roll.get().to_degrees(),
        final_pitch_degrees: attitude.pitch.get().to_degrees(),
    }
}

/// 姿勢が許容範囲に収まっていること。
fn assert_upright(excursion: &Excursion, what: &str) {
    assert!(
        excursion.maximum_bank_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: banked to {:.1}° (finished at {:.1}°)",
        excursion.maximum_bank_degrees,
        excursion.final_bank_degrees
    );
    assert!(
        excursion.maximum_pitch_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: pitched to {:.1}° (finished at {:.1}°)",
        excursion.maximum_pitch_degrees,
        excursion.final_pitch_degrees
    );
    assert!(
        excursion.maximum_tilt_degrees < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
        "{what}: tilted {:.1}° from the local vertical",
        excursion.maximum_tilt_degrees
    );
}

/// 駐機ブレーキ。
///
/// 斜面に置いた機体はブレーキが無ければ転がり落ちる（転がり抵抗 0.015 に対し
/// 15° 斜面は 0.27）。これは物理として正しい挙動なので、姿勢の判定を 10 秒行うには
/// 実機と同じくブレーキを掛けておく。制動摩擦 0.715 は 25° 斜面（0.466）まで保持できる。
fn parking_brake() -> ControlInputs {
    ControlInputs::neutral().with_brakes(1.0)
}

#[test]
fn spawning_across_a_fifteen_degree_slope_does_not_flip_the_aircraft() {
    // 斜面は真東へ上る。機体は北を向いているので、勾配はまるごとロール軸に乗る。
    // 上り側の主脚が 1.3 m × tan15° = 0.35 m めり込む。
    let excursion = settle_on_slope(15.0, 90.0, 0.0, parking_brake(), 10.0);
    assert_upright(&excursion, "across a 15° slope");
}

#[test]
fn spawning_along_a_fifteen_degree_slope_does_not_flip_the_aircraft() {
    // 斜面は真北へ上る。勾配はピッチ軸に乗り、前脚だけが 1.6 m × tan15° = 0.43 m
    // めり込む。**実測で機体が背面へ一回転したのはこの向き。**
    let excursion = settle_on_slope(15.0, 0.0, 0.0, parking_brake(), 10.0);
    assert_upright(&excursion, "along a 15° slope");
}

#[test]
fn spawning_on_a_fifteen_degree_slope_never_flips_whatever_the_aspect() {
    for bearing in [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
        let excursion = settle_on_slope(15.0, bearing, 0.0, parking_brake(), 10.0);
        assert_upright(&excursion, &format!("15° slope rising toward {bearing}°"));
    }
}

#[test]
fn spawning_on_a_twenty_five_degree_slope_does_not_flip_the_aircraft() {
    // 25° はもう着陸できる面ではない。それでも裏返らないこと。
    for bearing in [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
        let excursion = settle_on_slope(25.0, bearing, 0.0, parking_brake(), 10.0);
        assert_upright(&excursion, &format!("25° slope rising toward {bearing}°"));
    }
}

#[test]
fn a_slope_spawn_never_launches_the_aircraft_off_the_ground() {
    // 裏返らなくても、めり込みが跳ね上げに変わっていれば不合格。
    // 車輪の高さ 1.0 m ぶんは常に浮いており、斜面に沿って傾くと重心はさらに
    // わずかに上がる。跳ね上げが起きればこの余裕を大きく超える。
    let limit = gear_height(&AircraftConfig::light_single()) + 0.25;

    for bearing in [0.0, 90.0, 180.0, 270.0] {
        for slope in [15.0, 25.0] {
            let excursion = settle_on_slope(slope, bearing, 0.0, parking_brake(), 10.0);
            assert!(
                excursion.maximum_height_above_plane < limit,
                "a {slope}° slope rising toward {bearing}° launched the aircraft to \
                 {:.2} m above the plane (limit {limit:.2} m)",
                excursion.maximum_height_above_plane
            );
        }
    }
}

#[test]
fn a_brakeless_spawn_on_a_slope_rolls_away_instead_of_tumbling() {
    // ブレーキ無しなら滑り落ちるのが正しい。ここで見るのは spawn の過渡（3 秒）で
    // 裏返らないこと。その先は転がり出した機体の話で、接地反力の問題ではない。
    //
    // **注意**: このまま 10 秒回すと、15° 斜面を後ろ向きに 16 m/s まで加速した機体が
    // 空力で前へ一回転する。接地反力ではなく逆流（迎角 ≒ 180°）の空力によるもので、
    // 実機でも吹き流された機体は裏返る。ここでは対象外とし、判定を 3 秒に切っている。
    for slope in [15.0, 25.0] {
        let excursion = settle_on_slope(slope, 0.0, 0.0, ControlInputs::neutral(), 3.0);
        assert_upright(&excursion, &format!("brakeless {slope}° slope"));
    }
}

#[test]
fn the_slope_spawn_is_deterministic() {
    let run = || {
        let config = AircraftConfig::light_single();
        let initial = parked_like_the_integration_layer(&config, 30.0);
        let environment = sloped_ground(15.0, 60.0);
        let mut fdm = FlightDynamics::new(config, initial);
        let mut samples = Vec::new();
        for step in 0..1_200 {
            fdm.step(Seconds(1.0 / 120.0), parking_brake(), &environment);
            if step % 60 == 0 {
                samples.push(*fdm.state());
            }
        }
        samples
    };

    assert_eq!(run(), run(), "the sloped-contact path is not deterministic");
}

#[test]
fn a_deeply_buried_spawn_climbs_out_instead_of_being_launched() {
    // 極端な入力。地面より 5 m 低く置かれても、跳ね上げず・裏返らず・NaN も出さない。
    // 抜け出す速度は脚の最大伸長速度で頭打ちになるので、深さによらず穏やか。
    let config = AircraftConfig::light_single();
    let recoil_limit = config.landing_gear.legs()[0].max_recoil_speed().get();
    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(
            SPAWN_LATITUDE,
            SPAWN_LONGITUDE,
            SPAWN_ELEVATION + gear_height(&config) - 5.0,
        ),
        Attitude::from_degrees(0.0, 0.0, 0.0),
        Ned::default(),
    );
    let environment = sloped_ground(15.0, 45.0);
    let mut fdm = FlightDynamics::new(config, initial);
    let mut maximum_climb_rate = f64::NEG_INFINITY;

    for step in 0..(30 * 120) {
        fdm.step(RECOMMENDED_FIXED_DT, parking_brake(), &environment);
        assert!(
            fdm.state().is_finite(),
            "state became non-finite at step {step}"
        );
        maximum_climb_rate = maximum_climb_rate.max(fdm.state().vertical_speed().get());
        let attitude = fdm.state().attitude();
        assert!(
            attitude.roll.get().to_degrees().abs() < MAXIMUM_ATTITUDE_EXCURSION_DEGREES
                && attitude.pitch.get().to_degrees().abs() < MAXIMUM_ATTITUDE_EXCURSION_DEGREES,
            "a buried spawn threw the aircraft to roll {:.1}° / pitch {:.1}° at step {step}",
            attitude.roll.get().to_degrees(),
            attitude.pitch.get().to_degrees()
        );
    }

    // 3 脚が同時に押し出すので重心の上昇率は 1 脚ぶんの上限をやや上回りうる。
    // 大事なのは「深さに比例して跳ね上がらない」こと。
    assert!(
        maximum_climb_rate < recoil_limit * 2.0,
        "climbing out of a 5 m burial reached {maximum_climb_rate:.2} m/s, \
         which is not bounded by the {recoil_limit:.2} m/s recoil limit"
    );
}

#[test]
fn a_level_ground_plane_still_settles_at_the_static_compression() {
    // 傾斜の修正が平地の性質を変えていないこと。1043 kg ÷ 360 000 N/m = 0.0284 m。
    let config = AircraftConfig::light_single();
    let gear = gear_height(&config);
    let expected = 0.028_4;
    let initial = RigidBodyState::from_geodetic(
        Geodetic::new(Radians::ZERO, Radians::ZERO, Meters(gear - expected)),
        Attitude::default(),
        Ned::default(),
    );
    let mut fdm = FlightDynamics::new(config, initial);

    for _ in 0..(10 * 120) {
        fdm.step(
            RECOMMENDED_FIXED_DT,
            parking_brake(),
            &Environment::still_air(),
        );
    }

    let compression = gear - fdm.state().altitude().get();
    assert!(
        (compression - expected).abs() < 1.0e-3,
        "static compression drifted to {compression:.5} m"
    );
}
