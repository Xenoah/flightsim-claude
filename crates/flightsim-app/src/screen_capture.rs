//! Actual-scene screenshots, including a windowless software-rendering path.
//!
//! This changes only the camera's output surface. Physics, assets, atmosphere,
//! terrain and UI use the same plugins and systems as the interactive app.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::io::Write;
use std::path::Path;

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
    if startup.windows_readback_diagnostic {
        commands.insert_resource(crate::windows_readback_diagnostic::ProbeRequest);
        eprintln!("FS_READBACK_PROBE event=armed");
    }
}

fn save_capture(captured: On<ScreenshotCaptured>, startup: Res<Startup>) {
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    let result = save_capture_image(captured.image.clone(), path);
    match &result {
        Ok(()) => info!("Screenshot saved to {}", path.display()),
        Err(error) => error!("Cannot save screenshot: {error}"),
    }
    finish_batch_capture(startup.exit_after_screenshot, &result);
}

/// Encoding, final buffered writes and filesystem synchronization must all
/// succeed before a capture is acknowledged. ImageBuffer::save drops its own
/// BufWriter, which cannot report an error from the final flush.
fn save_capture_image(image: Image, path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err("screenshot path must end in .png".into());
    }
    let image = image
        .try_into_dynamic()
        .map_err(|error| error.to_string())?;
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    let format = bevy::image::ImageFormat::Png
        .as_image_crate_format()
        .expect("PNG is enabled");
    // HDR alpha represents brightness, not transparency, matching Bevy's saver.
    image
        .to_rgb8()
        .write_to(&mut writer, format)
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|error| error.to_string())?;
    drop(writer); // Close the file before publishing the completion marker.
    Ok(())
}

/// This is an explicit one-shot batch option, never the interactive quit path.
/// On the Windows software-D3D12 runner, AppExit stopped frames after saving the
/// image but teardown did not terminate the process within 180 seconds. Avoid
/// waiting on renderer/window destructors after all requested output is closed.
fn finish_batch_capture(requested: bool, result: &Result<(), String>) {
    if !requested {
        return;
    }
    let status = i32::from(result.is_err());
    eprintln!("Batch capture complete: status {status}");
    let stdout_ok = std::io::stdout().flush().is_ok();
    let stderr_ok = std::io::stderr().flush().is_ok();
    std::process::exit(if stdout_ok && stderr_ok { status } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    #[test]
    fn diagnostic_arms_once_at_the_ordinary_thirtieth_frame_request() {
        for enabled in [false, true] {
            let mut app = App::new();
            app.insert_resource(Time::<()>::default())
                .insert_resource(Startup {
                    screenshot: Some("unused-proof.png".into()),
                    screenshot_delay: 0.0,
                    windows_readback_diagnostic: enabled,
                    ..default()
                })
                .add_systems(Update, capture_screenshot);
            for _ in 0..29 {
                app.update();
            }
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                0
            );
            assert!(
                !app.world()
                    .contains_resource::<crate::windows_readback_diagnostic::ProbeRequest>()
            );
            app.update();
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                1
            );
            assert_eq!(
                app.world()
                    .contains_resource::<crate::windows_readback_diagnostic::ProbeRequest>(),
                enabled
            );
            app.update();
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                1
            );
        }
    }

    fn image() -> Image {
        Image::new_fill(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[10, 20, 30, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn png_completion_and_write_errors_are_reported() {
        let directory =
            std::env::temp_dir().join(format!("flightsim-capture-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("complete.png");
        save_capture_image(image(), &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            &bytes[bytes.len() - 12..],
            &[0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130]
        );
        assert!(save_capture_image(image(), &directory.join("missing/failed.png")).is_err());
        assert!(save_capture_image(image(), &directory.join("wrong.jpg")).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn batch_exit_child() {
        let Ok(mode) = std::env::var("FLIGHTSIM_CAPTURE_EXIT_TEST") else {
            return;
        };
        print!("stdout proof before exit");
        eprintln!("stderr proof before exit");
        let result = if mode == "failure" {
            Err("injected write failure".into())
        } else {
            Ok(())
        };
        finish_batch_capture(mode != "interactive", &result);
        println!("interactive continued");
    }

    #[test]
    fn batch_status_flush_and_interactive_nontermination_are_regressed() {
        for (mode, code) in [("success", 0), ("failure", 1), ("interactive", 0)] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "screen_capture::tests::batch_exit_child",
                    "--nocapture",
                ])
                .env("FLIGHTSIM_CAPTURE_EXIT_TEST", mode)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(code), "{mode}");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stdout.contains("stdout proof before exit"));
            assert!(stderr.contains("stderr proof before exit"));
            assert_eq!(
                stdout.contains("interactive continued"),
                mode == "interactive"
            );
            assert_eq!(
                stderr.contains("Batch capture complete:"),
                mode != "interactive"
            );
        }
    }
}
