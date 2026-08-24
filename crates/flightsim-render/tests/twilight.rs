//! 薄明の環境光の検査。
//!
//! # なぜ要るのか
//!
//! **日没の瞬間に夜へ落ちると、薄暮が飛べない。** 実際にそうなっていた:
//! `daylight_fraction`（-6°..+6° を覆う）に `skylight_fraction`（地平線で 0）を
//! 掛けていたため、太陽高度 -0.18° で既に夜と同じ環境光になり、
//! 実 DEM の上で撮った薄暮の地形が画素値 3〜7/255 の黒だった。
//!
//! 市民薄明（0°〜-6°）は、外で本が読める明るさが続く時間帯。
//! ここに階調が無いと、夕方の着陸がただの暗転になる。

use flightsim_core::{Geodetic, Radians};
use flightsim_render::{SunLighting, TimeOfDay, UtcDateTime, solar_position};

fn tokyo() -> Geodetic {
    Geodetic::from_degrees(35.55, 139.40, 0.0)
}

/// 太陽高度から環境光の明るさを引く。
fn ambient_at(elevation_degrees: f64) -> f32 {
    SunLighting::default()
        .ambient(flightsim_core::Degrees(elevation_degrees).to_radians())
        .brightness
}

#[test]
fn civil_twilight_is_brighter_than_night() {
    // 地平線直下は、夜の下限より明確に明るいこと。
    let night = ambient_at(-20.0);
    let sunset = ambient_at(0.0);
    assert!(
        sunset > night * 1.3,
        "at sunset the ambient ({sunset:.0}) should clearly exceed night ({night:.0})"
    );
}

#[test]
fn the_light_fades_monotonically_through_twilight() {
    // 高度が下がるほど暗くなること。逆転や凹みがあると、
    // 「日が沈んだのに明るくなった」という絵になる。
    let mut previous = f32::INFINITY;
    let mut degrees = 20.0;
    while degrees >= -20.0 {
        let ambient = ambient_at(degrees);
        assert!(
            ambient <= previous + 1e-3,
            "the ambient rose from {previous:.1} to {ambient:.1} while the sun fell to {degrees}"
        );
        previous = ambient;
        degrees -= 0.5;
    }
}

#[test]
fn the_fade_has_no_cliff() {
    // 0.5 度あたりの変化が緩やかであること。段差があると
    // 日没の瞬間に画面が一段暗くなる。
    let mut degrees = 12.0;
    while degrees >= -12.0 {
        let here = ambient_at(degrees);
        let next = ambient_at(degrees - 0.5);
        let jump = (here - next).abs();
        assert!(
            jump < 250.0,
            "the ambient jumps by {jump:.0} between {degrees} and {:.1} deg",
            degrees - 0.5
        );
        degrees -= 0.5;
    }
}

#[test]
fn deep_night_settles_at_the_floor() {
    // 市民薄明が終われば夜の下限。いつまでも明るいのも困る。
    let floor = ambient_at(-30.0);
    for degrees in [-8.0, -12.0, -20.0, -30.0] {
        let ambient = ambient_at(degrees);
        assert!(
            (ambient - floor).abs() < 1.0,
            "past civil twilight the ambient should rest at the floor, got {ambient:.1} at {degrees}"
        );
    }
    assert!(floor > 0.0, "the night must not be pitch black");
}

#[test]
fn noon_keeps_full_daylight() {
    // 薄明を直したせいで昼が暗くなっていないこと。
    let noon = ambient_at(78.0);
    assert!(
        noon > 5_500.0,
        "high noon should stay at full daylight ambient, got {noon:.0}"
    );
}

#[test]
fn an_evening_landing_is_not_a_blackout() {
    // 実際の時刻で確かめる。東京・夏至の 19:30（地方平均太陽時）は
    // 日没直後で、まだ地形が読める明るさであるべき。
    let clock = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 19, 30, 0.0),
        tokyo().longitude,
    );
    let sun = solar_position(clock.utc, tokyo());
    let elevation = sun.elevation.to_degrees().get();
    assert!(
        (-6.0..0.0).contains(&elevation),
        "this check assumes civil twilight, got {elevation:.2} deg"
    );

    let ambient = SunLighting::default().ambient(sun.elevation).brightness;
    let night = ambient_at(-20.0);
    assert!(
        ambient > night * 1.1,
        "an evening landing should still have usable light: {ambient:.0} vs night {night:.0}"
    );
}

#[test]
fn a_broken_sun_angle_does_not_produce_a_broken_light() {
    for elevation in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let ambient = SunLighting::default().ambient(Radians(elevation));
        assert!(
            ambient.brightness.is_finite() && ambient.brightness >= 0.0,
            "a broken sun angle produced brightness {}",
            ambient.brightness
        );
    }
}
