//! Actual-scene screenshots, including a windowless software-rendering path.
//!
//! This changes only the camera's output surface. Physics, assets, atmosphere,
//! terrain and UI use the same plugins and systems as the interactive app.

use bevy::app::AppExit;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::{PendingModelFit, Startup};

#[derive(Resource, Debug)]
pub(super) struct OffscreenTarget(Handle<Image>);

pub(super) fn setup_offscreen_target(
    startup: Res<Startup>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    if !startup.headless_screenshot {
        return;
    }
    let image = images.add(Image::new_target_texture(
        1280,
        720,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    for entity in &cameras {
        commands
            .entity(entity)
            .insert((RenderTarget::Image(image.clone().into()), IsDefaultUiCamera));
    }
    commands.insert_resource(OffscreenTarget(image));
    info!("offscreen capture: actual scene and UI at 1280x720");
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy screenshot scheduling inputs"
)]
pub(super) fn capture_screenshot(
    time: Res<Time>,
    startup: Res<Startup>,
    target: Option<Res<OffscreenTarget>>,
    pending_models: Query<(), With<PendingModelFit>>,
    mut commands: Commands,
    mut elapsed: Local<f64>,
    mut frames: Local<u32>,
    mut done: Local<bool>,
) {
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    if *done {
        return;
    }
    *elapsed += f64::from(time.delta_secs());
    *frames = frames.saturating_add(1);
    // Do not capture an empty first frame or an unfitted model. Shader pipelines
    // and GPU readback are asynchronous even when the app has no window.
    if *elapsed < startup.screenshot_delay || *frames < 30 || !pending_models.is_empty() {
        return;
    }
    *done = true;
    let screenshot = target.map_or_else(Screenshot::primary_window, |target| {
        Screenshot::image(target.0.clone())
    });
    info!("capturing a screenshot to {}", path.display());
    commands.spawn(screenshot).observe(save_capture);
}

fn save_capture(
    captured: On<ScreenshotCaptured>,
    startup: Res<Startup>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    let result = captured
        .image
        .clone()
        .try_into_dynamic()
        .map_err(|error| error.to_string())
        // HDR alpha is brightness, not transparency, just as in Bevy's saver.
        .and_then(|image| {
            image
                .to_rgb8()
                .save(path)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(()) => {
            info!("Screenshot saved to {}", path.display());
            if startup.exit_after_screenshot {
                exit.write(AppExit::Success);
            }
        }
        Err(error) => {
            error!("Cannot save screenshot: {error}");
            if startup.exit_after_screenshot {
                exit.write(AppExit::error());
            }
        }
    }
}
