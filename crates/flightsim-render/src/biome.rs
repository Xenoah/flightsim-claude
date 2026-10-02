//! Geographic, seasonal terrain colours derived from coarse climate normals.
//!
//! These are deliberately procedural visual cues, not satellite imagery, a land
//! cover product, observed snow, or today's weather. The actual coastline/relief
//! comes from the world atlas; temperature and moisture control the palette.

use flightsim_core::{Kelvin, Meters, MetersPerSecond, Radians};

/// Sampled world/climate facts used to author one surface colour.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceAppearance {
    /// Independent geographic land fraction, including land below sea level.
    pub land_fraction: f64,
    pub elevation_msl: Meters,
    pub slope: Radians,
    pub temperature: Kelvin,
    /// Monthly climatological precipitation expressed as liquid water m/s.
    pub precipitation: MetersPerSecond,
    /// Model-derived seasonal snow indicator, not measured snow depth.
    pub snow_fraction: f64,
}

fn finite_fraction(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn blend(a: [f64; 3], b: [f64; 3], amount: f64) -> [f64; 3] {
    let t = finite_fraction(amount);
    std::array::from_fn(|index| a[index] + (b[index] - a[index]) * t)
}

/// Palette colour in sRGB, suitable for map pixels or conversion to linear GPU
/// vertex colour. Smooth blends avoid latitude bands or month-boundary stripes.
#[must_use]
pub fn surface_color(sample: SurfaceAppearance) -> [f32; 4] {
    let land = finite_fraction(sample.land_fraction);
    let snow = finite_fraction(sample.snow_fraction);
    let temperature = if sample.temperature.is_finite() {
        sample.temperature.get()
    } else {
        288.15
    };
    let precipitation = if sample.precipitation.is_finite() {
        sample.precipitation.get().max(0.0)
    } else {
        0.0
    };
    // 1 mm/day = 0.001 / 86400 m/s. These thresholds are a visual moisture
    // proxy, not a formal Koppen classification or a vegetation model.
    let moisture = (precipitation / (0.005 / 86_400.0)).clamp(0.0, 1.0).sqrt();
    let warm = (Kelvin(temperature).to_celsius() / 28.0).clamp(0.0, 1.0);
    let dry = blend([0.54, 0.49, 0.36], [0.74, 0.62, 0.40], warm);
    let wet = blend([0.29, 0.40, 0.32], [0.16, 0.34, 0.20], warm);
    let mut ground = blend(dry, wet, moisture);
    let slope = if sample.slope.is_finite() {
        sample.slope.get()
    } else {
        0.0
    };
    let rock = ((slope - 0.30) / 0.65).clamp(0.0, 1.0);
    ground = blend(ground, [0.48, 0.46, 0.43], rock * 0.88);
    ground = blend(ground, [0.90, 0.94, 0.97], snow * (1.0 - 0.45 * rock));
    // Surface water is blue regardless of orthometric/geoid height. Cold water
    // receives a restrained ice cue; it is not a sea-ice dataset.
    let ocean = blend([0.035, 0.20, 0.32], [0.65, 0.80, 0.84], snow * 0.45);
    let color = blend(ocean, ground, land);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "bounded colour channels in [0,1]"
    )]
    [color[0] as f32, color[1] as f32, color[2] as f32, 1.0]
}

/// Convert an authored sRGB colour to the linear colour space expected by
/// Bevy's vertex attributes. Keep alpha opaque.
#[must_use]
pub fn linear_surface_color(sample: SurfaceAppearance) -> [f32; 4] {
    let color = surface_color(sample);
    [
        crate::srgb_to_linear(color[0]),
        crate::srgb_to_linear(color[1]),
        crate::srgb_to_linear(color[2]),
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SurfaceAppearance {
        SurfaceAppearance {
            land_fraction: 1.0,
            elevation_msl: Meters(100.0),
            slope: Radians::ZERO,
            temperature: Kelvin(298.15),
            precipitation: MetersPerSecond(0.003 / 86_400.0),
            snow_fraction: 0.0,
        }
    }

    #[test]
    fn dry_warm_ground_is_sandy_and_wet_ground_is_green() {
        let dry = surface_color(SurfaceAppearance {
            precipitation: MetersPerSecond::ZERO,
            ..sample()
        });
        let wet = surface_color(SurfaceAppearance {
            precipitation: MetersPerSecond(0.010 / 86_400.0),
            ..sample()
        });
        assert!(dry[0] > dry[1] && dry[1] > dry[2]);
        assert!(wet[1] > wet[0] && wet[1] > wet[2]);
        assert!(dry[0] > wet[0] + 0.4);
    }

    #[test]
    fn below_sea_level_land_is_not_painted_as_ocean() {
        let low = surface_color(SurfaceAppearance {
            elevation_msl: Meters(-400.0),
            ..sample()
        });
        let water = surface_color(SurfaceAppearance {
            land_fraction: 0.0,
            elevation_msl: Meters(40.0),
            ..sample()
        });
        assert!(low[1] > low[2]);
        assert!(water[2] > water[1] && water[1] > water[0]);
    }

    #[test]
    fn seasonal_snow_is_visible_without_a_global_fixed_snowline() {
        let summer = surface_color(sample());
        let winter = surface_color(SurfaceAppearance {
            snow_fraction: 1.0,
            temperature: Kelvin(253.15),
            ..sample()
        });
        assert!(winter[0] > summer[0] + 0.4);
        assert!(winter[2] > 0.9);
    }

    #[test]
    fn invalid_visual_inputs_stay_finite_and_opaque() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1000.0, 1000.0] {
            let color = surface_color(SurfaceAppearance {
                land_fraction: value,
                elevation_msl: Meters(value),
                slope: Radians(value),
                temperature: Kelvin(value),
                precipitation: MetersPerSecond(value),
                snow_fraction: value,
            });
            assert!(
                color
                    .iter()
                    .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
            );
            assert!((color[3] - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn shore_and_snow_blends_are_continuous() {
        for index in 0..100 {
            let a = f64::from(index) / 100.0;
            let b = a + 1.0e-5;
            let first = surface_color(SurfaceAppearance {
                land_fraction: a,
                snow_fraction: a,
                ..sample()
            });
            let next = surface_color(SurfaceAppearance {
                land_fraction: b,
                snow_fraction: b,
                ..sample()
            });
            for channel in 0..3 {
                assert!((first[channel] - next[channel]).abs() < 0.0001);
            }
        }
    }
}
