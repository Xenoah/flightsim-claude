//! Render the actual fixed-bound attitude material at extreme pitch/bank angles.
//!
//! `cargo run -p flightsim-ui --example attitude_matrix -- /tmp/attitude-fixed.png`
//! Add `--negative-bank` after the path for mirrored bank angles. Use
//! `check_attitude_pixels.py` to verify full coverage and no out-of-dial pixels.

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy systems take Res and Query by value"
)]

use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use flightsim_core::Degrees;
use flightsim_ui::attitude::{AttitudeIndicatorPlugin, AttitudeMaterial};

const BANKS: [f32; 6] = [0.0, 30.0, 45.0, 90.0, 135.0, 180.0];
const PITCH_DEGREES: [f32; 3] = [-90.0, 0.0, 90.0];

#[derive(Resource, Debug)]
struct CaptureTarget(Handle<Image>);

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_plugins(AttitudeIndicatorPlugin)
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<AttitudeMaterial>>,
) {
    let mut image = Image::new_target_texture(1024, 560, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(image);
    commands.spawn((
        Camera2d,
        RenderTarget::Image(target.clone().into()),
        IsDefaultUiCamera,
    ));
    commands.insert_resource(CaptureTarget(target));
    let negative_bank = std::env::args().any(|argument| argument == "--negative-bank");
    for (row, pitch) in PITCH_DEGREES.into_iter().enumerate() {
        for (column, bank) in BANKS.into_iter().enumerate() {
            let bank = if negative_bank { -bank } else { bank };
            #[allow(clippy::cast_precision_loss, reason = "six columns and three rows")]
            let (left, top) = (48.0 + 160.0 * column as f32, 80.0 + 160.0 * row as f32);
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(left),
                    top: px(top - 28.0),
                    ..default()
                },
                Text::new(format!("R {bank:+.0}  P {pitch:+.0}")),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
            ));
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(top),
                        width: px(64.0),
                        height: px(64.0),
                        border_radius: BorderRadius::MAX,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.06, 0.07, 0.08)),
                ))
                .with_children(|dial| {
                    dial.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: percent(100.0),
                            height: percent(100.0),
                            left: px(0.0),
                            top: px(0.0),
                            ..default()
                        },
                        MaterialNode(materials.add(AttitudeMaterial::from_attitude(
                            Degrees(f64::from(pitch)).to_radians(),
                            Degrees(f64::from(bank)).to_radians(),
                        ))),
                    ));
                });
        }
    }
}

fn capture(
    time: Res<Time>,
    mut commands: Commands,
    mut captured: Local<bool>,
    target: Res<CaptureTarget>,
    mut exit: MessageWriter<AppExit>,
) {
    if !*captured && time.elapsed_secs() >= 3.0 {
        let path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "attitude-fixed.png".into());
        commands
            .spawn(Screenshot::image(target.0.clone()))
            .observe(save_to_disk(path))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
        *captured = true;
    }
    if time.elapsed_secs() >= 60.0 {
        exit.write(AppExit::error());
    }
}
