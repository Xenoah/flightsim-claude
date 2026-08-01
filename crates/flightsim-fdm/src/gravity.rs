//! WGS84 正規重力。
//!
//! 定数の重力加速度（9.81 m/s²）で済ませることもできるが、実際の重力は
//! 赤道と極で約 0.5% 違う。長距離巡航ではトリムの差として現れるため、
//! Somigliana の式で緯度依存を入れてある。計算量は無視できる。
//!
//! 「正規重力」は定義上、楕円体面に垂直に働く。したがって方向はローカル系の「上」の逆。

use flightsim_core::geodetic::wgs84;
use flightsim_core::{Geodetic, LocalFrame};
use glam::DVec3;

/// 赤道における正規重力 `m/s²`。WGS84 の定義値。
pub const EQUATORIAL_GRAVITY: f64 = 9.780_325_335_9;

/// Somigliana の式の定数 k = (b·γ_p)/(a·γ_e) - 1。
pub const SOMIGLIANA_CONSTANT: f64 = 0.001_931_852_652_458;

/// 遠心力と重力の比 m = ω²a²b / GM。高度補正で使う。
pub const GRAVITY_RATIO_M: f64 = 0.003_449_786_506_84;

/// 楕円体面における正規重力の大きさ `m/s²`（Somigliana の式）。
#[must_use]
pub fn surface_gravity(latitude_rad: f64) -> f64 {
    let sin_lat_sq = latitude_rad.sin().powi(2);
    EQUATORIAL_GRAVITY * (1.0 + SOMIGLIANA_CONSTANT * sin_lat_sq)
        / (1.0 - wgs84::ECCENTRICITY_SQ * sin_lat_sq).sqrt()
}

/// 高度を含めた正規重力の大きさ `m/s²`。
///
/// WGS84 の標準的な高度補正（h の 2 次まで）を用いる。
/// 巡航高度（約 11 km）で海面比 0.34% の減少になる。
#[must_use]
pub fn magnitude(position: Geodetic) -> f64 {
    let latitude = position.latitude.get();
    let h = position.altitude.get();
    let a = wgs84::SEMI_MAJOR_AXIS;
    let f = wgs84::FLATTENING;
    let sin_lat_sq = latitude.sin().powi(2);

    let correction = 1.0 - (2.0 / a) * (1.0 + f + GRAVITY_RATIO_M - 2.0 * f * sin_lat_sq) * h
        + (3.0 / (a * a)) * h * h;

    // 極端な高度で補正項が符号反転しないよう下限を設ける。
    // 重力が負になると機体が上に落ちる。
    surface_gravity(latitude) * correction.max(0.0)
}

/// ECEF 系での重力加速度ベクトル `m/s²`。
///
/// 楕円体法線の下向きに [`magnitude`] の大きさで働く。
#[must_use]
pub fn acceleration_ecef(position: Geodetic, frame: &LocalFrame) -> DVec3 {
    -frame.up_ecef() * magnitude(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Meters;

    /// 相対誤差での比較。リテラル同士の比較でも型が曖昧にならないよう注釈を付けている。
    macro_rules! assert_relative {
        ($actual:expr, $expected:expr, $relative_tolerance:expr) => {{
            let a: f64 = $actual;
            let e: f64 = $expected;
            let t: f64 = $relative_tolerance;
            assert!(
                (a - e).abs() <= e.abs() * t,
                "expected {a} ≈ {e} (within {}%), difference was {}",
                t * 100.0,
                (a - e).abs()
            );
        }};
    }

    #[test]
    fn surface_gravity_matches_published_values() {
        // WGS84 の公表値との照合。実装から導いた値ではない。
        assert_relative!(surface_gravity(0.0), 9.780_325_335_9, 1e-12);
        assert_relative!(
            surface_gravity(core::f64::consts::FRAC_PI_2),
            9.832_184_937_8,
            1e-9
        );
        // 緯度 45°: 9.806 199 m/s²
        assert_relative!(surface_gravity(45.0_f64.to_radians()), 9.806_199, 1e-6);
    }

    #[test]
    fn gravity_is_stronger_at_the_poles() {
        // 地球が扁平なので極は地心に近く、重力が強い。差は約 0.5%。
        let equator = surface_gravity(0.0);
        let pole = surface_gravity(core::f64::consts::FRAC_PI_2);
        assert!(pole > equator);
        assert_relative!((pole - equator) / equator, 0.0053, 0.02);
    }

    #[test]
    fn gravity_decreases_with_altitude() {
        let sea_level = magnitude(Geodetic::from_degrees(45.0, 0.0, 0.0));
        let cruise = magnitude(Geodetic::from_degrees(45.0, 0.0, 11_000.0));

        assert!(cruise < sea_level);
        // 11 km で約 0.34% の減少。
        assert_relative!((sea_level - cruise) / sea_level, 0.0034, 0.05);
    }

    #[test]
    fn gravity_points_straight_down_in_the_local_frame() {
        for latitude in [-80.0, -45.0, 0.0, 23.5, 60.0] {
            let position = Geodetic::from_degrees(latitude, 137.0, 2_000.0);
            let frame = LocalFrame::new(position);
            let g = acceleration_ecef(position, &frame);

            // ローカル NED 系で見ると、北成分・東成分がゼロで下成分のみが正。
            let ned = frame.ecef_to_ned_vector(g);
            assert!(
                ned.north().abs() < 1e-9,
                "gravity has a north component: {}",
                ned.north()
            );
            assert!(
                ned.east().abs() < 1e-9,
                "gravity has an east component: {}",
                ned.east()
            );
            assert!(ned.down() > 0.0, "gravity must point down (NED Z positive)");
            assert_relative!(ned.down(), magnitude(position), 1e-12);
        }
    }

    #[test]
    fn magnitude_stays_in_a_physically_sane_range() {
        for latitude in (-90..=90).step_by(5) {
            for altitude in [0.0, 1_000.0, 12_000.0, 30_000.0] {
                let g = magnitude(Geodetic::from_degrees(f64::from(latitude), 0.0, altitude));
                assert!(
                    (9.5..9.9).contains(&g),
                    "gravity {g} m/s² at lat {latitude} alt {altitude} is outside the plausible range"
                );
            }
        }
    }

    #[test]
    fn extreme_altitude_does_not_produce_negative_gravity() {
        // 発散した機体が上向きに加速されると、以後の挙動が完全に無意味になる。
        for altitude in [1e6, 1e7, 1e9] {
            let g = magnitude(Geodetic::new(
                flightsim_core::Radians(0.5),
                flightsim_core::Radians(0.0),
                Meters(altitude),
            ));
            assert!(
                g >= 0.0,
                "gravity went negative ({g}) at altitude {altitude}"
            );
            assert!(g.is_finite());
        }
    }
}
