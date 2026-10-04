//! Deterministic, render-only cloud morphology with calibrated plan-view coverage.
//!
//! NOAA monthly total cloud fraction supplies the requested area fraction, not
//! cloud shapes, cloud types, a sounding, or a forecast. The distribution below
//! is an explicitly procedural realization of that fraction. Its base/top shape
//! and fixed drift are artistic parameters and never feed the flight model.
//!
//! A radial texture projection of core ECEF coordinates avoids a longitude seam
//! and polar singularity. It is constant along Earth-centred radial columns.
//! It is not a conversion to geodetic coordinates. GPU positions use the same
//! projection; their f32 precision has a small, tested density tolerance rather
//! than a promise of CPU/GPU bit identity.
//!
//! The fixed inverse CDF was calibrated on the same quantized, trilinearly
//! interpolated 64³ RG8 texture that the GPU samples: 2,097,152 uniform 3D
//! samples, using eight seeds excluded from the validation grids. The quantile
//! controls the area with positive horizontal density (and this field’s alpha >=0.5),
//! not average alpha or perceived sky coverage. Local finite patches fluctuate;
//! the target is the large-area/ensemble projected fraction. Vertical shaping
//! has positive thickness wherever the horizontal support is positive.

use flightsim_core::{Ecef, Geodetic, Meters, Seconds, geodetic::wgs84};

/// Base noise-cell size; visual morphology, not a meteorological measurement.
pub const CLOUD_FIELD_CELL_SIZE: Meters = Meters(8_000.0);
/// Lattice period in dimensionless cells, shared with `cloud_field.wgsl`.
pub const CLOUD_FIELD_PERIOD: f32 = 16.0;
/// Bounded 3D lookup, RG8: red morphology and green independent erosion detail.
pub const CLOUD_FIELD_TEXTURE_SIZE: u32 = 64;
/// Exactly 512 KiB per seed, shared by every high-quality cloud draw.
pub const CLOUD_FIELD_TEXTURE_BYTES: usize = 64 * 64 * 64 * 2;
/// Exact common repeat interval for the fixed integer ECEF drift velocities.
pub const CLOUD_FIELD_DRIFT_PERIOD: Seconds = Seconds(128_000.0);
/// Shared shader source. The renderer installs its `flightsim::cloud_field` import.
pub const CLOUD_FIELD_SHADER: &str = include_str!("cloud_field.wgsl");
const RADIAL_SCALE: f64 = wgs84::MEAN_RADIUS / CLOUD_FIELD_CELL_SIZE.0;
const DRIFT: [f64; 3] = [8.0, 2.0, 3.0];

/// Stateless field; no textures, heaps, mutable random generator or wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudField {
    seed: u32,
}

impl CloudField {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let bytes = seed.to_le_bytes();
        let low = u32::from_le_bytes(bytes[..4].try_into().expect("four bytes"));
        let high = u32::from_le_bytes(bytes[4..].try_into().expect("four bytes"));
        Self {
            seed: mix(low ^ high.rotate_left(13)),
        }
    }

    /// Folded seed uploaded unchanged to the shader.
    #[must_use]
    pub const fn seed(self) -> u32 {
        self.seed
    }

    /// Origin-independent ECEF texture coordinates; all geodesy stays in core.
    #[must_use]
    pub fn coordinates(position: Geodetic, elapsed: Seconds) -> Option<[f32; 3]> {
        if !position.latitude.is_finite()
            || !position.longitude.is_finite()
            || !position.altitude.is_finite()
            || position.latitude.get().abs() > std::f64::consts::FRAC_PI_2
        {
            return None;
        }
        Self::coordinates_ecef(position.to_ecef(), elapsed)
    }

    /// Project a core ECEF point directly, avoiding geodetic round trips.
    ///
    /// Reduce in f64 before conversion. All samples of a radial column have the
    /// same coordinates. The zero vector and non-finite values are rejected.
    #[must_use]
    pub fn coordinates_ecef(position: Ecef, elapsed: Seconds) -> Option<[f32; 3]> {
        let p = position.as_vec();
        if !p.is_finite() || p.abs().max_element() <= f64::MIN_POSITIVE {
            return None;
        }
        // Scaling first also keeps very large finite test inputs away from an
        // overflowing squared length. This is a texture direction, not a world position.
        let direction = (p / p.abs().max_element()).normalize();
        let drift = drift_f64(elapsed)?;
        let q = direction.to_array();
        Some(std::array::from_fn(|i| {
            bounded_f32(q[i] * RADIAL_SCALE + drift[i])
        }))
    }

    /// Bounded additive phase for WGSL; deliberately not a physical wind.
    #[must_use]
    pub fn drift(elapsed: Seconds) -> Option<[f32; 3]> {
        drift_f64(elapsed).map(|d| d.map(bounded_f32))
    }

    /// Nonuniform, continuous trilinearly sampled precomputed value-fBm. Use [`cloud_cover_threshold`] for coverage.
    #[must_use]
    pub fn noise(self, coordinates: [f32; 3]) -> f32 {
        if coordinates.iter().any(|v| !v.is_finite()) {
            return 0.0;
        }
        let q = coordinates.map(|v| v.rem_euclid(CLOUD_FIELD_PERIOD) * 4.0);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "texture coordinate is bounded to 0..64"
        )]
        let i = q.map(|v| v.floor() as u32);
        let f = q.map(|v| v - v.floor());
        let v = |x, y, z| f32::from(self.texel([i[0] + x, i[1] + y, i[2] + z])[0]) / 255.0;
        let z0 = lerp(
            lerp(v(0, 0, 0), v(1, 0, 0), f[0]),
            lerp(v(0, 1, 0), v(1, 1, 0), f[0]),
            f[1],
        );
        let z1 = lerp(
            lerp(v(0, 0, 1), v(1, 0, 1), f[0]),
            lerp(v(0, 1, 1), v(1, 1, 1), f[0]),
            f[1],
        );
        lerp(z0, z1, f[2])
    }

    /// Generate a single, bounded upload only when the visual seed changes.
    /// x is fastest, then y then z; each texel is `[morphology, erosion]`.
    /// Hardware sampling must use repeat addressing, linear filtering and mip0.
    #[must_use]
    pub fn texture_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CLOUD_FIELD_TEXTURE_BYTES);
        for z in 0..CLOUD_FIELD_TEXTURE_SIZE {
            for y in 0..CLOUD_FIELD_TEXTURE_SIZE {
                for x in 0..CLOUD_FIELD_TEXTURE_SIZE {
                    bytes.extend_from_slice(&self.texel([x, y, z]));
                }
            }
        }
        bytes
    }

    fn texel(self, index: [u32; 3]) -> [u8; 2] {
        let i = index.map(|v| v & (CLOUD_FIELD_TEXTURE_SIZE - 1));
        #[allow(
            clippy::cast_precision_loss,
            reason = "wrapped texture indices are at most63"
        )]
        let p = i.map(|v| v as f32 * 0.25);
        let noise = 0.625 * value_noise(p, self.seed)
            + 0.25 * value_noise(p.map(|v| v * 2.0), self.seed ^ 0xa511_e9b3)
            + 0.125 * value_noise(p.map(|v| v * 4.0), self.seed ^ 0x63d8_3595);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "unit morphology is quantized into an R8 texture channel"
        )]
        let red = (noise.clamp(0.0, 1.0) * 255.0).round() as u8;
        let detail = mix(i[0].wrapping_mul(0x9e37_79b9)
            ^ i[1].wrapping_mul(0x85eb_ca6b)
            ^ i[2].wrapping_mul(0xc2b2_ae35)
            ^ self.seed
            ^ 0x6a09_e667);
        [red, detail.to_le_bytes()[3]]
    }

    /// Alpha mask for a texture of this upper-quality field. The 0.5 contour is calibrated coverage.
    #[must_use]
    pub fn sample_ecef(self, position: Ecef, elapsed: Seconds, cover: f32) -> f32 {
        Self::coordinates_ecef(position, elapsed).map_or(0.0, |p| {
            cloud_footprint_mask(self.noise(p), cloud_cover_threshold(cover))
        })
    }

    /// Same mask through the core Geodetic entry point.
    #[must_use]
    pub fn sample(self, position: Geodetic, elapsed: Seconds, cover: f32) -> f32 {
        Self::coordinates(position, elapsed).map_or(0.0, |p| {
            cloud_footprint_mask(self.noise(p), cloud_cover_threshold(cover))
        })
    }

    /// Model density inside an explicitly configured base/top layer.
    #[must_use]
    pub fn density(
        self,
        position: Geodetic,
        elapsed: Seconds,
        cover: f32,
        base: Meters,
        top: Meters,
    ) -> f32 {
        if !base.is_finite() || !top.is_finite() || top <= base {
            return 0.0;
        }
        let Some(p) = Self::coordinates(position, elapsed) else {
            return 0.0;
        };
        let h = (position.altitude.get() - base.get()) / (top.get() - base.get());
        if !h.is_finite() || !(0.0..=1.0).contains(&h) {
            return 0.0;
        }
        let noise = self.noise(p);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "normalized layer height is bounded to 0..=1"
        )]
        let h = h as f32;
        cloud_horizontal_density(noise, cloud_cover_threshold(cover))
            * cloud_vertical_profile(h, noise)
    }
}

/// Quantile threshold for modeled projected fraction, monotone in cover.
/// Invalid cover fails clear. Endpoint sentinels make clear/overcast exact.
#[must_use]
pub fn cloud_cover_threshold(cover: f32) -> f32 {
    if !cover.is_finite() || cover <= 0.0 {
        return 1.1;
    }
    if cover >= 1.0 {
        return -0.1;
    }
    let x = (1.0 - cover) * 128.0;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite table index is bounded to 0..128"
    )]
    let i = (x.floor() as usize).min(127);
    #[allow(clippy::cast_precision_loss, reason = "table index is at most 127")]
    let fraction = x - i as f32;
    lerp(QUANTILES[i], QUANTILES[i + 1], fraction)
}

/// PBR alpha >= 0.5 has the same support boundary as positive volume density.
#[must_use]
pub fn cloud_footprint_mask(noise: f32, threshold: f32) -> f32 {
    smoothstep(threshold - 0.015, threshold + 0.015, noise)
}

/// Positive only within the calibrated footprint, with a soft interior edge.
#[must_use]
pub fn cloud_horizontal_density(noise: f32, threshold: f32) -> f32 {
    smoothstep(threshold, threshold + 0.06, noise)
}

/// Procedural flattened underside and variable rounded top, within base/top.
/// No cloud type or thermodynamic state is inferred by this shaping function.
#[must_use]
pub fn cloud_vertical_profile(height: f32, noise: f32) -> f32 {
    if !height.is_finite() || !noise.is_finite() || !(0.0..=1.0).contains(&height) {
        return 0.0;
    }
    let n = noise.clamp(0.0, 1.0);
    let bottom = 0.04 + 0.06 * (1.0 - n);
    let top = 0.58 + 0.38 * n;
    smoothstep(bottom, bottom + 0.12, height) * (1.0 - smoothstep(top - 0.22, top, height))
}

/// Convenience upload entry point; format and lifetime match [`CloudField::texture_bytes`].
#[must_use]
pub fn cloud_noise_texture(seed: u64) -> Vec<u8> {
    CloudField::new(seed).texture_bytes()
}

fn drift_f64(elapsed: Seconds) -> Option<[f64; 3]> {
    if !elapsed.is_finite() {
        return None;
    }
    let t = elapsed.get().rem_euclid(CLOUD_FIELD_DRIFT_PERIOD.get());
    Some(DRIFT.map(|speed| {
        (-speed * t / CLOUD_FIELD_CELL_SIZE.get()).rem_euclid(f64::from(CLOUD_FIELD_PERIOD))
    }))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "only dimensionless phase modulo16 crosses f64/f32 boundary"
)]
fn bounded_f32(value: f64) -> f32 {
    // f64 values just below16 can round to16; both phases are equivalent.
    (value.rem_euclid(f64::from(CLOUD_FIELD_PERIOD)) as f32).rem_euclid(CLOUD_FIELD_PERIOD)
}

fn mix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn lattice(p: [u32; 3], seed: u32) -> f32 {
    let h = (p[0] & 15).wrapping_mul(0x9e37_79b9)
        ^ (p[1] & 15).wrapping_mul(0x85eb_ca6b)
        ^ (p[2] & 15).wrapping_mul(0xc2b2_ae35)
        ^ seed;
    #[allow(
        clippy::cast_precision_loss,
        reason = "24-bit integer exactly representable as f32"
    )]
    {
        (mix(h) >> 8) as f32 / 16_777_215.0
    }
}

fn value_noise(p: [f32; 3], seed: u32) -> f32 {
    let p = p.map(|v| v.rem_euclid(CLOUD_FIELD_PERIOD));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "wrapped finite lattice indices fit u32"
    )]
    let i = p.map(|v| v.floor() as u32);
    let f = p.map(|v| {
        let t = v - v.floor();
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    });
    let v = |x, y, z| lattice([i[0] + x, i[1] + y, i[2] + z], seed);
    let z0 = lerp(
        lerp(v(0, 0, 0), v(1, 0, 0), f[0]),
        lerp(v(0, 1, 0), v(1, 1, 0), f[0]),
        f[1],
    );
    let z1 = lerp(
        lerp(v(0, 0, 1), v(1, 0, 1), f[0]),
        lerp(v(0, 1, 1), v(1, 1, 1), f[0]),
        f[1],
    );
    lerp(z0, z1, f[2])
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn smoothstep(a: f32, b: f32, value: f32) -> f32 {
    if !a.is_finite() || !b.is_finite() || !value.is_finite() || b <= a {
        return 0.0;
    }
    let t = ((value - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Empirical inverse CDF at p=i/128. Training: NumPy PCG64 seed0x636c6f756473,
// 262144 uniform[0,256)^3 samples for each seed19,71,257,1999,65537,12582917,
// 0xabcdef09,0xffffffff. Hash, quintic generation and weights exactly as above.
// Fit after RG8 quantization and linear 64³ interpolation, not analytic fBm.
// Endpoints deliberately widened to mathematical[0,1]; clear/overcast use sentinels.
const QUANTILES: [f32; 129] = [
    0., 0.2184814, 0.24213736, 0.25774816, 0.26985505, 0.28008643, 0.28896758, 0.29677826,
    0.30397874, 0.3106204, 0.3168258, 0.3227608, 0.3283159, 0.3336395, 0.33869356, 0.3435641,
    0.34827992, 0.35278648, 0.35712975, 0.3614079, 0.36554036, 0.36961102, 0.37352815, 0.37739292,
    0.38114458, 0.38483658, 0.38849595, 0.39205205, 0.3955595, 0.39895105, 0.4023205, 0.4056422,
    0.4089042, 0.41214246, 0.41533262, 0.41849613, 0.42158562, 0.42466876, 0.42772058, 0.43071863,
    0.43370163, 0.43666404, 0.43964052, 0.44255263, 0.4454591, 0.44833443, 0.45120245, 0.45402136,
    0.45683974, 0.4596407, 0.46244907, 0.46521395, 0.46794975, 0.47071898, 0.47345954, 0.47618708,
    0.47890908, 0.48161787, 0.48435298, 0.48707557, 0.4897584, 0.4924507, 0.49514636, 0.49783924,
    0.5004948, 0.5032036, 0.5059332, 0.508629, 0.5113355, 0.51402956, 0.5166989, 0.51938796,
    0.52207184, 0.52478385, 0.52746075, 0.5301938, 0.5329467, 0.53571635, 0.5384625, 0.54120845,
    0.5439812, 0.5468062, 0.549596, 0.5524086, 0.55523294, 0.55810475, 0.5609674, 0.56386834,
    0.566808, 0.5697421, 0.5727224, 0.57572275, 0.5787749, 0.5819049, 0.5849817, 0.5881478,
    0.59137315, 0.5946037, 0.5978558, 0.60121995, 0.60459685, 0.608051, 0.61159974, 0.6151664,
    0.6187648, 0.62248105, 0.6262366, 0.63017124, 0.6342012, 0.6383093, 0.6425067, 0.64683104,
    0.6513456, 0.655996, 0.66077214, 0.66580254, 0.67108035, 0.67649657, 0.6821996, 0.68834734,
    0.69490075, 0.7020451, 0.70991737, 0.7187219, 0.7287161, 0.7407665, 0.7561534, 0.7788853, 1.,
];

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{LocalFrame, Ned, RenderFrame};
    use glam::{DVec3, Vec3};

    #[test]
    fn held_out_local_grids_recover_projected_cloud_fraction() {
        // Neither these seeds nor these deterministic spatial grids were used
        // for the CDF fit. Measure binary plan-view support, not average opacity.
        // A 380km square contains many individual cells; it is not a guarantee
        // that every small airport view matches the monthly fraction exactly.
        let locations = [
            (0.0, 0.0),
            (35.55, 139.78),
            (-21.1, -47.8),
            (61.2, 23.5),
            (89.9, 179.9),
            (-89.9, -179.9),
        ];
        let covers = [0.1, 0.3, 0.5, 0.7, 0.9];
        let thresholds = covers.map(cloud_cover_threshold);
        let mut aggregate = [0_u32; 5];
        let mut total = 0_u32;
        let mut worst = 0.0_f64;
        let mut ranges = [(1.0_f64, 0.0_f64); 5];
        for seed in [3, 0x1234_5678_9abc_def0, u64::MAX] {
            let field = CloudField::new(seed);
            for (lat, lon) in locations {
                let frame = LocalFrame::new(Geodetic::from_degrees(lat, lon, 0.0));
                // Four independent seasonal visual-clock phases. Climate month
                // changes cover externally; it never reseeds this field.
                for elapsed in [0.0, 91.0 * 86_400.0, 182.0 * 86_400.0, 273.0 * 86_400.0] {
                    let mut count = [0_u32; 5];
                    for y in 0..96_u32 {
                        for x in 0..96_u32 {
                            let north = (f64::from(y) + 0.371) * 3971.3 - 190_000.0;
                            let east = (f64::from(x) + 0.619) * 3971.3 - 190_000.0;
                            let world = frame.ned_to_ecef_position(Ned::new(north, east, 0.0));
                            let p = CloudField::coordinates_ecef(world, Seconds(elapsed)).unwrap();
                            let noise = field.noise(p);
                            for i in 0..5 {
                                count[i] += u32::from(noise > thresholds[i]);
                            }
                        }
                    }
                    for i in 0..5 {
                        let actual = f64::from(count[i]) / f64::from(96 * 96);
                        let error = (actual - f64::from(covers[i])).abs();
                        eprintln!(
                            "CLOUD_COVER,{seed},{lat},{lon},{elapsed},{},{actual:.9}",
                            covers[i]
                        );
                        worst = worst.max(error);
                        ranges[i].0 = ranges[i].0.min(actual);
                        ranges[i].1 = ranges[i].1.max(actual);
                        // Snapshot fluctuations are reported, not mistaken
                        // for error in the long-term monthly coverage target.
                        aggregate[i] += count[i];
                    }
                    total += 96 * 96;
                }
            }
        }
        let measured = aggregate.map(|n| f64::from(n) / f64::from(total));
        eprintln!(
            "held-out projected coverage {measured:?}; worst local error {worst:.6}, samples{total}; ranges{ranges:?}"
        );
        for i in 0..5 {
            assert!((measured[i] - f64::from(covers[i])).abs() < 0.015);
        }
    }

    #[test]
    fn long_time_ensembles_preserve_fraction_at_each_latitude() {
        let covers = [0.1, 0.3, 0.5, 0.7, 0.9];
        let thresholds = covers.map(cloud_cover_threshold);
        for (lat, lon) in [
            (0.0, 0.0),
            (35.55, 139.78),
            (-21.1, -47.8),
            (61.2, 23.5),
            (89.9, 179.9),
            (-89.9, -179.9),
        ] {
            let frame = LocalFrame::new(Geodetic::from_degrees(lat, lon, 0.0));
            let mut count = [0_u32; 5];
            let mut total = 0_u32;
            for seed in [3, 0x1234_5678_9abc_def0, u64::MAX] {
                let field = CloudField::new(seed);
                for phase in 0..17_u32 {
                    let elapsed =
                        Seconds((f64::from(phase) + 0.413) * CLOUD_FIELD_DRIFT_PERIOD.get() / 17.0);
                    for y in 0..64_u32 {
                        for x in 0..64_u32 {
                            let world = frame.ned_to_ecef_position(Ned::new(
                                (f64::from(y) + 0.273) * 5956.9 - 190000.0,
                                (f64::from(x) + 0.731) * 5956.9 - 190000.0,
                                0.0,
                            ));
                            let noise =
                                field.noise(CloudField::coordinates_ecef(world, elapsed).unwrap());
                            for i in 0..5 {
                                count[i] += u32::from(noise > thresholds[i]);
                            }
                            total += 1;
                        }
                    }
                }
            }
            for i in 0..5 {
                let measured = f64::from(count[i]) / f64::from(total);
                eprintln!(
                    "CLOUD_ENSEMBLE,{lat},{lon},{},{measured:.9},{total}",
                    covers[i]
                );
                assert!(
                    (measured - f64::from(covers[i])).abs() < 0.015,
                    "latitude{lat}, cover{}: mean{measured}",
                    covers[i]
                );
            }
        }
    }

    #[test]
    fn cpu_noise_matches_independent_trilinear_read_of_uploaded_rg8() {
        let field = CloudField::new(0x5678_1234_eeee_ffff);
        let data = field.texture_bytes();
        assert_eq!(data.len(), CLOUD_FIELD_TEXTURE_BYTES);
        assert!(data.chunks_exact(2).any(|rg| rg[0] != rg[1]));
        for p in [
            [0.0, 0.0, 0.0],
            [0.137, 9.971, 15.983],
            [-0.012, 16.1, 112.397],
            [1.25, 0.5, 9.75],
        ] {
            // Sum all eight weighted bytes independently of the nested CPU
            // interpolation. The half-texel WGSL offset places these at i/4.
            let t = p.map(|x: f32| x.rem_euclid(16.0) * 4.0);
            let mut expected = 0.0_f32;
            for dz in 0..2_u32 {
                for dy in 0..2_u32 {
                    for dx in 0..2_u32 {
                        let offset = [dx, dy, dz];
                        let mut weight = 1.0;
                        let mut index = [0_usize; 3];
                        for axis in 0..3 {
                            #[allow(
                                clippy::cast_possible_truncation,
                                reason = "wrapped texture index is bounded to0..64"
                            )]
                            let i = t[axis].floor() as usize;
                            let f = t[axis] - t[axis].floor();
                            weight *= if offset[axis] == 0 { 1.0 - f } else { f };
                            index[axis] = (i + offset[axis] as usize) % 64;
                        }
                        expected += weight
                            * f32::from(data[(index[0] + 64 * (index[1] + 64 * index[2])) * 2])
                            / 255.0;
                    }
                }
            }
            assert!((field.noise(p) - expected).abs() < 0.000001);
        }
    }

    #[test]
    fn poles_and_dateline_share_the_same_footprint() {
        for seed in [0, 7, u64::MAX] {
            let field = CloudField::new(seed);
            for latitude in [-89.999, -30.0, 0.0, 47.0, 89.999] {
                let a = field.sample(
                    Geodetic::from_degrees(latitude, 180.0, 2000.0),
                    Seconds(12345.0),
                    0.5,
                );
                let b = field.sample(
                    Geodetic::from_degrees(latitude, -180.0, 2000.0),
                    Seconds(12345.0),
                    0.5,
                );
                assert_eq!(a.to_bits(), b.to_bits());
            }
            for latitude in [-90.0, 90.0] {
                let reference = field.sample(
                    Geodetic::from_degrees(latitude, 0.0, 0.0),
                    Seconds(12345.0),
                    0.5,
                );
                for longitude in [-180.0, -97.0, 0.0, 42.0, 180.0] {
                    let value = field.sample(
                        Geodetic::from_degrees(latitude, longitude, 0.0),
                        Seconds(12345.0),
                        0.5,
                    );
                    assert_eq!(reference.to_bits(), value.to_bits());
                }
            }
        }
    }

    #[test]
    fn radial_columns_and_rebased_frames_preserve_density() {
        let field = CloudField::new(71234);
        for (lat, lon) in [(35.0, 139.0), (0.0, 179.99), (89.95, -120.0)] {
            let point = Geodetic::from_degrees(lat, lon, 1800.0);
            let world = point.to_ecef();
            let p = CloudField::coordinates_ecef(world, Seconds(17.0)).unwrap();
            for factor in [0.9, 1.0, 1.001, 1.1] {
                let q = CloudField::coordinates_ecef(Ecef(world.as_vec() * factor), Seconds(17.0))
                    .unwrap();
                for i in 0..3 {
                    assert!((p[i] - q[i]).abs() < 0.0001);
                }
            }
            let before = RenderFrame::new(point);
            let moved = before
                .to_world(Vec3::new(8000.0, 1800.0, 1000.0))
                .to_geodetic();
            let after = RenderFrame::new(moved);
            let a = field.sample_ecef(before.to_world(before.to_render(world)), Seconds(17.0), 0.5);
            let b = field.sample_ecef(after.to_world(after.to_render(world)), Seconds(17.0), 0.5);
            assert!(
                (a - b).abs() < 0.001,
                "rebase alpha difference {}",
                (a - b).abs()
            );
        }
    }

    #[test]
    fn field_and_drift_are_deterministic_continuous_and_periodic() {
        let field = CloudField::new(55);
        let world = Geodetic::from_degrees(45.0, -122.0, 1300.0).to_ecef();
        for t in [-100_000.0, 0.0, 86_399.99, 86_400.01, 1_000_000.0] {
            let a = field.sample_ecef(world, Seconds(t), 0.5);
            let b = field.sample_ecef(world, Seconds(t), 0.5);
            assert_eq!(a.to_bits(), b.to_bits());
            let repeated =
                field.sample_ecef(world, Seconds(t + CLOUD_FIELD_DRIFT_PERIOD.get()), 0.5);
            assert!((a - repeated).abs() < 0.0001);
            let nearby = field.sample_ecef(world, Seconds(t + 0.01), 0.5);
            assert!((a - nearby).abs() < 0.002);
        }
        for p in [[0.123, 79.3, 17.92], [255.999, 0.013, 190.7]] {
            let a = field.noise(p);
            let b = field.noise(p.map(|x| x + 256.0));
            assert!((a - b).abs() < 0.0001);
        }
        let before = field.noise([255.9999, 12.3, 45.6]);
        let after = field.noise([0.0001, 12.3, 45.6]);
        assert!((before - after).abs() < 0.001);
    }

    #[test]
    fn invalid_and_extreme_inputs_are_finite_and_fail_clear() {
        let field = CloudField::new(u64::MAX);
        let valid = Geodetic::from_degrees(20.0, 40.0, 1500.0);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(CloudField::coordinates(valid, Seconds(bad)).is_none());
            assert!(CloudField::coordinates_ecef(Ecef::new(bad, 1.0, 2.0), Seconds(0.0)).is_none());
        }
        assert!(CloudField::coordinates_ecef(Ecef::default(), Seconds(0.0)).is_none());
        for p in [
            DVec3::splat(f64::MAX),
            DVec3::splat(f64::MIN_POSITIVE * 2.0),
            valid.to_ecef().as_vec(),
        ] {
            let q = CloudField::coordinates_ecef(Ecef(p), Seconds(f64::MAX)).unwrap();
            assert!(q.iter().all(|v| v.is_finite() && (0.0..16.0).contains(v)));
            assert!(field.noise(q).is_finite());
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            assert!(field.sample(valid, Seconds(0.0), bad).abs() < f32::EPSILON);
        }
        for noise in [0.0, 0.1, 0.5, 0.9, 1.0] {
            assert!(
                cloud_horizontal_density(noise, cloud_cover_threshold(0.0)).abs() < f32::EPSILON
            );
            assert!(
                (cloud_horizontal_density(noise, cloud_cover_threshold(1.0)) - 1.0).abs()
                    < f32::EPSILON
            );
            assert!(cloud_vertical_profile(0.0, noise).abs() < f32::EPSILON);
            assert!(cloud_vertical_profile(1.0, noise).abs() < f32::EPSILON);
            assert!(cloud_vertical_profile(0.35, noise) > 0.0);
            let mut previous = 0.0;
            for cover in [0.0, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0] {
                let mask = cloud_footprint_mask(noise, cloud_cover_threshold(cover));
                assert!(mask >= previous);
                previous = mask;
            }
        }
        assert!(
            field
                .density(valid, Seconds(0.0), 0.5, Meters(f64::NAN), Meters(2000.0))
                .abs()
                < f32::EPSILON
        );
        assert!(
            field
                .density(valid, Seconds(0.0), 0.5, Meters(2000.0), Meters(1000.0))
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn shader_projection_has_bounded_cpu_phase_error() {
        let field = CloudField::new(31);
        let mut max_noise_error = 0.0_f32;
        // Emulate only the shader's f32 projection, keeping the shared integer
        // noise identical. This bounds coordinate quantization, not all GPU math.
        for lat in [-90.0, -70.0, -20.0, 0.0, 37.0, 80.0, 90.0] {
            for lon in [-180.0, -122.0, 0.0, 139.0, 180.0] {
                let world = Geodetic::from_degrees(lat, lon, 2000.0).to_ecef();
                for t in [0.0, 123456.0, 127_999.0] {
                    let cpu = CloudField::coordinates_ecef(world, Seconds(t)).unwrap();
                    let ecef_cells = (world.as_vec() / CLOUD_FIELD_CELL_SIZE.get()).as_vec3();
                    let drift = Vec3::from_array(CloudField::drift(Seconds(t)).unwrap());
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "emulating shader f32 normalized texture scale"
                    )]
                    let gpu = (ecef_cells.normalize() * RADIAL_SCALE as f32 + drift)
                        .to_array()
                        .map(|v| v.rem_euclid(16.0));
                    let error = (field.noise(cpu) - field.noise(gpu)).abs();
                    max_noise_error = max_noise_error.max(error);
                    assert!(error < 0.0005, "projection noise error {error}");
                }
            }
        }
        eprintln!("maximum f32 projection noise error {max_noise_error:.8}");
    }

    #[test]
    fn shared_shader_parses_and_validates_without_a_gpu() {
        let source = CLOUD_FIELD_SHADER
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let module = naga::front::wgsl::parse_str(&source).expect("shared cloud WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("shared cloud WGSL validates");
    }
}
