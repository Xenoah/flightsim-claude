//! 太陽位置の**独立**検算。
//!
//! # なぜ二重に検査するのか
//!
//! `sun.rs` の中にも既知値との突き合わせがある。ここはそれとは別に、
//! **統合担当が外から確かめた記録**として残す。実装と同じ人が書いた
//! テストだけだと、前提の取り違えが両方に同じ形で入る。
//!
//! 使う真値はすべて外部由来（天文暦・幾何学の定義）で、
//! 実装の出力から逆算した値は一つも使っていない。

use flightsim_core::{Degrees, Geodetic};
use flightsim_render::sun::{JulianDate, UtcDateTime, solar_position};

/// 東京（羽田）。
fn tokyo() -> Geodetic {
    Geodetic::from_degrees(35.55, 139.78, 0.0)
}

/// 与えた UTC 時刻の太陽位置。
fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8, at: Geodetic) -> (f64, f64) {
    let julian = UtcDateTime::new(year, month, day, hour, minute, 0.0).to_julian_date();
    let position = solar_position(julian, at);
    (
        position.azimuth.to_degrees().get(),
        position.elevation.to_degrees().get(),
    )
}

#[test]
fn the_solstice_noon_elevation_matches_spherical_geometry() {
    // 幾何学の定義: 南中高度 = 90° − |緯度 − 太陽赤緯|。
    // 夏至の赤緯は黄道傾斜角 +23.44°、冬至は −23.44°。
    // 東京（35.55°N）なら夏至 77.89°、冬至 30.99°（±0.2° を許容。
    // 均時差により真南中が正午からずれるため、1 分刻みで最大を探す）。
    let peak_elevation = |month: u8, day: u8| {
        let mut best = f64::NEG_INFINITY;
        // 東京の正午は UTC 03:00 前後。前後 1 時間を 1 分刻みで。
        for minute in 0..120_u8 {
            let (_, elevation) = at(2026, month, day, 2, minute, tokyo());
            best = best.max(elevation);
        }
        best
    };

    let summer = peak_elevation(6, 21);
    let winter = peak_elevation(12, 21);

    let expected_summer = 90.0 - (35.55 - 23.44_f64).abs();
    let expected_winter = 90.0 - (35.55 - (-23.44_f64)).abs();

    assert!(
        (summer - expected_summer).abs() < 0.3,
        "summer solstice noon elevation should be about {expected_summer:.2}, got {summer:.3}"
    );
    assert!(
        (winter - expected_winter).abs() < 0.3,
        "winter solstice noon elevation should be about {expected_winter:.2}, got {winter:.3}"
    );
    // 夏至と冬至の差は黄道傾斜角の 2 倍。
    assert!(
        ((summer - winter) - 2.0 * 23.44).abs() < 0.5,
        "the seasonal swing should be twice the obliquity, got {:.2}",
        summer - winter
    );
}

#[test]
fn the_sun_is_south_at_local_noon_in_the_northern_hemisphere() {
    // 北半球の中緯度では、南中の方位は 180°（真南）。
    let mut best = (0.0, f64::NEG_INFINITY);
    for minute in 0..120_u8 {
        let (azimuth, elevation) = at(2026, 6, 21, 2, minute, tokyo());
        if elevation > best.1 {
            best = (azimuth, elevation);
        }
    }
    assert!(
        (best.0 - 180.0).abs() < 1.0,
        "at local noon the sun should bear about 180 deg, got {:.2}",
        best.0
    );
}

#[test]
fn the_equinox_sun_rises_due_east_everywhere() {
    // 分点の日の出は、緯度によらず真東（方位 90°）。
    // 球面天文の基本性質で、外部の真値として使える。
    for (name, latitude) in [("Tokyo", 35.55), ("equator", 0.0), ("Sydney", -33.87)] {
        let place = Geodetic::from_degrees(latitude, 139.78, 0.0);
        // 地平線を横切る瞬間を 1 分刻みで探す（東京の日の出は UTC 20〜22 時台）。
        let mut crossing = None;
        // 6 時間ぶんを 1 分刻み。u8 の範囲に収まるよう時と分に分けて数える。
        for hour in 18_u8..24 {
            for minute in 0_u8..60 {
                let (azimuth, elevation) = at(2026, 3, 20, hour, minute, place);
                if elevation > 0.0 {
                    crossing = Some(azimuth);
                    break;
                }
            }
            if crossing.is_some() {
                break;
            }
        }
        let azimuth = crossing.unwrap_or_else(|| panic!("{name}: the sun never rose"));
        assert!(
            (azimuth - 90.0).abs() < 2.0,
            "{name}: the equinox sunrise should be due east, got {azimuth:.2}"
        );
    }
}

#[test]
fn the_midnight_sun_never_sets_at_the_pole_in_midsummer() {
    // 夏至の北極では、太陽が 1 日中沈まない。
    let pole = Geodetic::from_degrees(89.9, 0.0, 0.0);
    for hour in 0..24_u8 {
        let (_, elevation) = at(2026, 6, 21, hour, 0, pole);
        assert!(
            elevation > 20.0,
            "the midnight sun should stay up at the pole, got {elevation:.2} at {hour:02}:00 UTC"
        );
    }
}

#[test]
fn the_polar_night_keeps_the_sun_down_at_midwinter() {
    let pole = Geodetic::from_degrees(89.9, 0.0, 0.0);
    for hour in 0..24_u8 {
        let (_, elevation) = at(2026, 12, 21, hour, 0, pole);
        assert!(
            elevation < 0.0,
            "the polar night should keep the sun down, got {elevation:.2} at {hour:02}:00 UTC"
        );
    }
}

#[test]
fn the_two_hemispheres_have_opposite_seasons() {
    // 同じ日に、北半球の夏は南半球の冬。緯度の符号を取り違えていたら
    // ここで露見する。
    let tokyo_summer = {
        let mut best = f64::NEG_INFINITY;
        for minute in 0..120_u8 {
            best = best.max(at(2026, 6, 21, 2, minute, tokyo()).1);
        }
        best
    };
    let sydney = Geodetic::from_degrees(-33.87, 151.21, 0.0);
    let sydney_winter = {
        let mut best = f64::NEG_INFINITY;
        // シドニーの正午は UTC 01:00 前後。
        for minute in 0..120_u8 {
            best = best.max(at(2026, 6, 21, 0, minute, sydney).1);
        }
        best
    };
    assert!(
        tokyo_summer > 70.0,
        "Tokyo should be in high summer, got {tokyo_summer:.1}"
    );
    assert!(
        sydney_winter < 40.0,
        "Sydney should be in winter on the same day, got {sydney_winter:.1}"
    );
}

#[test]
fn a_broken_clock_does_not_produce_nan() {
    // 非有限の時刻で NaN が出ると、光源の Transform が全部 NaN になり
    // 画面が消える。安全側（夜）へ倒れること。
    for julian in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let position = solar_position(JulianDate(julian), tokyo());
        assert!(
            position.azimuth.get().is_finite() && position.elevation.get().is_finite(),
            "a broken clock produced a non-finite solar position"
        );
        assert!(
            !position.is_above_horizon(),
            "a broken clock should fail safe to night"
        );
    }
}

#[test]
fn the_azimuth_stays_inside_one_full_turn() {
    // 表示にも計算にも使うので、範囲外の値を出さないこと。
    let julian = UtcDateTime::new(2026, 9, 23, 0, 5, 0.0).to_julian_date();
    for hours in 0..48 {
        let moment = julian.advanced_by(flightsim_core::Seconds(f64::from(hours) * 3600.0));
        for latitude in [-89.0, -45.0, 0.0, 45.0, 89.0] {
            let place = Geodetic::from_degrees(latitude, -179.0, 0.0);
            let azimuth = solar_position(moment, place).azimuth.get();
            assert!(
                (0.0..std::f64::consts::TAU).contains(&azimuth),
                "azimuth {azimuth} is outside [0, 2pi) at latitude {latitude}"
            );
        }
    }
}

#[test]
fn moving_east_makes_the_sun_rise_earlier() {
    // 経度の符号の取り違えを捕まえる。東へ 15 度 = 1 時間早い。
    let elevation_at = |longitude: f64| {
        at(
            2026,
            3,
            20,
            21,
            0,
            Geodetic::from_degrees(0.0, longitude, 0.0),
        )
        .1
    };
    // 分点・UTC 21:00 に、東経 135 度は既に昼、東経 90 度はまだ夜明け前。
    let east = elevation_at(135.0);
    let further_west = elevation_at(90.0);
    assert!(
        east > further_west,
        "the eastern site should be further into its day: {east:.1} vs {further_west:.1}"
    );
    let _ = Degrees(0.0);
}
