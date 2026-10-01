//! Fixed-bound artificial horizon material.
//!
//! Bevy 0.18.1 clips UI quads by moving their vertices as if they were axis
//! aligned. Rotating oversized children therefore leaks and distorts the
//! horizon (see `examples/attitude_clip_repro.rs`). This material rotates the
//! sky/ground boundary inside a stationary dial-sized quad instead. There is
//! no out-of-bounds geometry for the clipper to repair. A separate analytic
//! circular alpha mask gives the instrument a true round aperture.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use flightsim_core::Radians;

use crate::instruments::{DIAL_SIZE, horizon_placement};

const ATTITUDE_SHADER: Handle<Shader> = uuid_handle!("f0da5ba8-6c4b-447d-a968-d351fbd94018");

/// The ground-facing normal and signed offset in dial-width coordinates.
/// Screen x points right and screen y points down. A positive right bank puts
/// ground on the right and sky on the left at 90 degrees of bank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HorizonPlane {
    pub normal: [f32; 2],
    pub offset: f32,
}

impl HorizonPlane {
    #[must_use]
    pub fn from_attitude(pitch: Radians, roll: Radians) -> Self {
        let placement = horizon_placement(pitch, roll);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "bounded display angle 0..2pi"
        )]
        let angle = placement.roll.0.wrap_positive().get() as f32;
        let (sin, cos) = angle.sin_cos();
        Self {
            normal: [-sin, cos],
            offset: placement.offset / DIAL_SIZE,
        }
    }

    /// Positive means ground, negative means sky. The instrument corners are
    /// (-0.5,-0.5) and (0.5,0.5); pitch moves along the rotated normal.
    #[must_use]
    pub fn ground_distance(self, point: [f32; 2]) -> f32 {
        self.normal[0] * point[0] + self.normal[1] * point[1] - self.offset
    }
}

/// Embedded UI material; no file in the user's asset directory is required.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct AttitudeMaterial {
    #[uniform(0)]
    plane: Vec4,
    #[uniform(1)]
    sky: LinearRgba,
    #[uniform(2)]
    ground: LinearRgba,
}

impl Default for AttitudeMaterial {
    fn default() -> Self {
        Self::from_attitude(Radians::ZERO, Radians::ZERO)
    }
}

impl AttitudeMaterial {
    #[must_use]
    pub fn from_attitude(pitch: Radians, roll: Radians) -> Self {
        let plane = HorizonPlane::from_attitude(pitch, roll);
        Self {
            plane: Vec4::new(plane.normal[0], plane.normal[1], plane.offset, 0.0),
            sky: Color::srgb(0.20, 0.42, 0.70).to_linear(),
            ground: Color::srgb(0.35, 0.26, 0.16).to_linear(),
        }
    }

    pub fn set_attitude(&mut self, pitch: Radians, roll: Radians) {
        let plane = HorizonPlane::from_attitude(pitch, roll);
        self.plane = Vec4::new(plane.normal[0], plane.normal[1], plane.offset, 0.0);
    }
}

impl UiMaterial for AttitudeMaterial {
    fn fragment_shader() -> ShaderRef {
        ATTITUDE_SHADER.into()
    }
}

#[derive(Debug, Default)]
pub struct AttitudeIndicatorPlugin;

impl Plugin for AttitudeIndicatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(UiMaterialPlugin::<AttitudeMaterial>::default());
        load_internal_asset!(app, ATTITUDE_SHADER, "attitude.wgsl", Shader::from_wgsl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;

    #[test]
    fn level_flight_places_sky_above_and_ground_below() {
        let plane = HorizonPlane::from_attitude(Radians::ZERO, Radians::ZERO);
        assert!(plane.ground_distance([0.0, -0.25]) < 0.0);
        assert!(plane.ground_distance([0.0, 0.25]) > 0.0);
    }

    #[test]
    fn right_bank_puts_ground_on_the_right_and_left_bank_reverses_it() {
        let right = HorizonPlane::from_attitude(Radians::ZERO, Degrees(90.0).to_radians());
        let left = HorizonPlane::from_attitude(Radians::ZERO, Degrees(-90.0).to_radians());
        assert!(right.ground_distance([0.25, 0.0]) > 0.0);
        assert!(right.ground_distance([-0.25, 0.0]) < 0.0);
        assert!(left.ground_distance([0.25, 0.0]) < 0.0);
        assert!(left.ground_distance([-0.25, 0.0]) > 0.0);
    }

    #[test]
    fn inverted_flight_puts_ground_above_the_aircraft() {
        let plane = HorizonPlane::from_attitude(Radians::ZERO, Degrees(180.0).to_radians());
        assert!(plane.ground_distance([0.0, -0.25]) > 0.0);
        assert!(plane.ground_distance([0.0, 0.25]) < 0.0);
    }

    #[test]
    fn pitch_displacement_rotates_with_bank_instead_of_screen_y() {
        let plane =
            HorizonPlane::from_attitude(Degrees(30.0).to_radians(), Degrees(90.0).to_radians());
        assert!((plane.offset - 0.18).abs() < 1e-5);
        assert!(plane.ground_distance([0.10, 0.0]) < 0.0);
        assert!(plane.ground_distance([0.25, 0.0]) > 0.0);
        assert!(
            (plane.ground_distance([0.10, -0.25]) - plane.ground_distance([0.10, 0.25])).abs()
                < 1e-5
        );
    }

    #[test]
    fn extreme_and_broken_attitudes_produce_finite_normalized_planes() {
        for pitch in [-90.0, -60.0, 0.0, 60.0, 90.0, f64::NAN, f64::INFINITY] {
            for roll in [
                -180.0,
                -135.0,
                -90.0,
                -45.0,
                0.0,
                45.0,
                90.0,
                135.0,
                180.0,
                f64::NAN,
            ] {
                let plane = HorizonPlane::from_attitude(
                    Degrees(pitch).to_radians(),
                    Degrees(roll).to_radians(),
                );
                assert!(plane.normal.iter().all(|value| value.is_finite()));
                assert!(plane.offset.is_finite());
                assert!((plane.normal[0].hypot(plane.normal[1]) - 1.0).abs() < 1e-5);
            }
        }
    }
}
