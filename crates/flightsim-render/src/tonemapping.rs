//! Explicit alternate build policy, before render extraction.

use bevy::{core_pipeline::tonemapping::Tonemapping, prelude::*};

// SpawnScene and PostUpdate finish before Last. This also covers the ordinary
// app camera and imported GLB cameras without changing their producer code.
// Leave Camera2d/map policy and explicit linear/analytical methods unchanged.
pub(super) fn select_analytic_tonemapping(mut cameras: Query<&mut Tonemapping, With<Camera3d>>) {
    for mut tonemapping in &mut cameras {
        if matches!(
            *tonemapping,
            Tonemapping::AgX | Tonemapping::TonyMcMapface | Tonemapping::BlenderFilmic
        ) {
            *tonemapping = Tonemapping::Reinhard;
        }
    }
}
