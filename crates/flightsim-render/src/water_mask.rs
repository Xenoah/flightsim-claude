//! Upper-only geographic water lookup; the original terrain mesh is never edited.
//!
//! The cube is a radial texture projection, not a second geodetic conversion.
//! Every texel is classified by the validated atlas through core's ECEF API.
//! RG8 channels mean ocean and inland water, never inferred from RGB or height.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
use flightsim_core::{Ecef, geodetic::wgs84};
use flightsim_world::global::{GlobalTerrain, GlobalTerrainSample};

/// 512² × six RG8 faces = exactly 3 MiB, shared by High and Ultra.
/// Resolution near a face centre is about 25 km; the atlas itself is ~20 km.
pub const WATER_MASK_SIDE: u32 = 512;
pub const WATER_MASK_BYTES: usize = 512 * 512 * 6 * 2;
/// Bound CPU work during first activation; never regenerate on each frame.
pub const WATER_MASK_TEXELS_PER_UPDATE: usize = 8_192;

/// Quantized geographic fractions. Inland and ocean remain independently tagged.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped mask channels quantize into RG8"
)]
pub fn geographic_water_channels(sample: GlobalTerrainSample) -> [u8; 2] {
    let fraction = if sample.land_fraction.is_finite() {
        (1.0 - sample.land_fraction).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let channel = (fraction * 255.0).round() as u8;
    if sample.is_inland_water {
        [0, channel]
    } else {
        [channel, 0]
    }
}

/// Incremental, cancelable generation. The atlas is a shared immutable Arc.
#[derive(Debug)]
pub struct WaterMaskBuilder {
    atlas: GlobalTerrain,
    bytes: Vec<u8>,
    next: usize,
}

impl WaterMaskBuilder {
    #[must_use]
    pub fn new(atlas: GlobalTerrain) -> Self {
        Self {
            atlas,
            bytes: vec![0; WATER_MASK_BYTES],
            next: 0,
        }
    }

    #[must_use]
    pub const fn completed_texels(&self) -> usize {
        self.next
    }

    /// Spend at most the fixed per-update budget. True only when all faces exist.
    pub fn advance(&mut self) -> bool {
        let end = (self.next + WATER_MASK_TEXELS_PER_UPDATE).min(WATER_MASK_BYTES / 2);
        for index in self.next..end {
            let direction = texel_direction(index);
            let position = Ecef(direction * wgs84::MEAN_RADIUS).to_geodetic();
            if let Some(sample) = self.atlas.sample(position) {
                self.bytes[index * 2..index * 2 + 2]
                    .copy_from_slice(&geographic_water_channels(sample));
            }
        }
        self.next = end;
        self.next == WATER_MASK_BYTES / 2
    }

    /// Incomplete/cancelled masks never become visible images.
    #[must_use]
    pub fn into_image(self) -> Option<Image> {
        if self.next != WATER_MASK_BYTES / 2 {
            return None;
        }
        let mut image = Image::new(
            Extent3d {
                width: WATER_MASK_SIDE,
                height: WATER_MASK_SIDE,
                depth_or_array_layers: 6,
            },
            TextureDimension::D2,
            self.bytes,
            TextureFormat::Rg8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        });
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Nearest,
            ..default()
        });
        Some(image)
    }
}

// WebGPU cube faces, +X/-X/+Y/-Y/+Z/-Z. Linear cube filtering crosses faces;
// there is no longitude wrap or pole singularity and no independently tiled mask.
fn face_direction(face: usize, u: f64, v: f64) -> glam::DVec3 {
    match face {
        0 => glam::DVec3::new(1.0, -v, -u),
        1 => glam::DVec3::new(-1.0, -v, u),
        2 => glam::DVec3::new(u, 1.0, v),
        3 => glam::DVec3::new(u, -1.0, -v),
        4 => glam::DVec3::new(u, -v, 1.0),
        _ => glam::DVec3::new(-u, -v, -1.0),
    }
    .normalize()
}

#[allow(clippy::cast_precision_loss, reason = "indices are at most 512")]
fn texel_direction(index: usize) -> glam::DVec3 {
    let side = WATER_MASK_SIDE as usize;
    let face = index / (side * side);
    let x = index % side;
    let y = (index / side) % side;
    let u = 2.0 * (x as f64 + 0.5) / f64::from(WATER_MASK_SIDE) - 1.0;
    let v = 2.0 * (y as f64 + 0.5) / f64::from(WATER_MASK_SIDE) - 1.0;
    face_direction(face, u, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Geodetic, Meters};

    #[test]
    fn geographic_classes_preserve_negative_land_and_lake_heights() {
        let atlas = GlobalTerrain::bundled().unwrap();
        let dry = atlas
            .sample(Geodetic::from_degrees(
                36.123_046_875,
                -116.806_640_625,
                0.0,
            ))
            .unwrap();
        assert!(dry.is_land && dry.elevation_msl < Meters::ZERO);
        let [sea, lake] = geographic_water_channels(dry);
        assert!(sea < 128 && lake == 0);
        for (lat, lon) in [
            (31.728_515_625, 35.595_703_125),
            (53.341_666_666_7, 108.175),
            (47.7, -87.5),
        ] {
            let sample = atlas.sample(Geodetic::from_degrees(lat, lon, 0.0)).unwrap();
            let before = sample.surface_height;
            let [sea, lake] = geographic_water_channels(sample);
            assert_eq!(sea, 0);
            assert!(lake >= 128);
            assert_eq!(sample.surface_height, before);
        }
        assert_eq!(
            geographic_water_channels(
                atlas
                    .sample(Geodetic::from_degrees(0.0, -140.0, 0.0))
                    .unwrap()
            ),
            [255, 0]
        );
    }

    #[test]
    fn cube_shared_edges_and_corners_have_identical_directions() {
        for i in -16..=16 {
            let p = f64::from(i) / 16.0;
            assert!((face_direction(0, -1.0, p) - face_direction(4, 1.0, p)).length() < 1e-14);
            assert!((face_direction(0, 1.0, p) - face_direction(5, -1.0, p)).length() < 1e-14);
            assert!((face_direction(2, p, 1.0) - face_direction(4, p, -1.0)).length() < 1e-14);
        }
        for face in 0..6 {
            for u in [-1.0, 1.0] {
                for v in [-1.0, 1.0] {
                    let a = face_direction(face, u, v);
                    let count = (0..6)
                        .flat_map(|f| {
                            [-1.0, 1.0].into_iter().flat_map(move |x| {
                                [-1.0, 1.0]
                                    .into_iter()
                                    .map(move |y| face_direction(f, x, y))
                            })
                        })
                        .filter(|b| (*b - a).length() < 1e-14)
                        .count();
                    assert_eq!(count, 3);
                }
            }
        }
    }

    #[test]
    fn mask_budget_is_incremental_and_incomplete_upload_is_rejected() {
        let mut builder = WaterMaskBuilder::new(GlobalTerrain::bundled().unwrap());
        assert!(!builder.advance());
        assert_eq!(builder.completed_texels(), WATER_MASK_TEXELS_PER_UPDATE);
        assert!(builder.into_image().is_none());
        assert_eq!(WATER_MASK_BYTES, 3 * 1024 * 1024);
    }
}
