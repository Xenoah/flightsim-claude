//! 傾斜地での初期配置の検査。
//!
//! `parked_state` が勾配を見ずに水平姿勢で置くと、上り側の車輪が最初から
//! 地面に入る（15° 斜面で前脚 0.43 m）。めり込みは脚のばねに偽のエネルギー
//! として蓄えられ、かつては機体を背面まで一回転させた。
//!
//! 検査は `parked_state` 内部の回転（sin/cos）ではなく、**返された状態の
//! quaternion 経由**で脚の位置を計算する。実装と同じ式を写すと、符号の
//! 取り違えを検出できない。

use flightsim_core::{Geodetic, LocalFrame, Meters, Radians};
use flightsim_fdm::{AircraftConfig, GroundSlope};
use flightsim_sim::parked_state;

/// 各脚のめり込み深さ（正 = 地面に入っている）を、状態の quaternion から測る。
fn worst_penetration(
    config: &AircraftConfig,
    state: &flightsim_fdm::RigidBodyState,
    reference: Geodetic,
    ground_elevation: Meters,
    slope: GroundSlope,
) -> f64 {
    let frame = LocalFrame::new(reference);
    let to_ned = frame.ned_to_ecef_rotation().inverse();
    let reference_ecef = reference.to_ecef();

    config
        .landing_gear
        .legs()
        .iter()
        .map(|leg| {
            // 脚の接地点を世界座標へ（実装とは別経路: 状態の quaternion を使う）。
            let world = state.position.as_vec() + state.orientation * leg.contact_point().as_vec();
            let ned = to_ned * (world - reference_ecef.as_vec());

            // その水平位置での接地平面の高さ。
            let plane = ground_elevation.get() + slope.north() * ned.x + slope.east() * ned.y;

            // 脚の高度（NED の down を高度差へ）。
            let leg_altitude = reference.altitude.get() - ned.z;
            plane - leg_altitude
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

#[test]
fn no_wheel_starts_below_a_sloped_ground_plane() {
    let config = AircraftConfig::light_single();
    let position = Geodetic::from_degrees(35.55, 139.33, 0.0);
    let slope_15 = (15.0_f64).to_radians().tan();

    // 8 方位 × 上り勾配の向き 4 通り。
    for heading_degrees in [0.0_f64, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
        for (north, east) in [
            (slope_15, 0.0),
            (-slope_15, 0.0),
            (0.0, slope_15),
            (slope_15, slope_15),
        ] {
            let slope = GroundSlope::new(north, east);
            let heading = Radians(heading_degrees.to_radians());
            let elevation = Meters(1400.0);
            let state = parked_state(&config, position, elevation, slope, heading);

            let penetration = worst_penetration(&config, &state, position, elevation, slope);
            assert!(
                penetration <= 0.001,
                "at heading {heading_degrees}° on slope ({north:.3}, {east:.3}) \
                 a wheel starts {penetration:.3} m inside the ground"
            );
            // 浮かせすぎも駄目。落下衝撃が入る。最も厳しい脚は接地している。
            assert!(
                penetration >= -0.05,
                "at heading {heading_degrees}° the aircraft floats {:.3} m above the ground",
                -penetration
            );
        }
    }
}

#[test]
fn level_ground_places_the_aircraft_exactly_as_before() {
    // 平地の挙動は従来と同じ「標高 + 脚の高さ」。回帰を防ぐ。
    let config = AircraftConfig::light_single();
    let position = Geodetic::from_degrees(35.548, 139.775, 0.0);
    let state = parked_state(
        &config,
        position,
        Meters(8.0),
        GroundSlope::LEVEL,
        Radians::ZERO,
    );

    let expected = 8.0 + flightsim_sim::gear_height(&config).get();
    assert!(
        (state.altitude().get() - expected).abs() < 1e-9,
        "on level ground the CG should sit at {expected} m, got {}",
        state.altitude()
    );
}
