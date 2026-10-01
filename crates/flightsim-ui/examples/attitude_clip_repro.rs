//! Reproduce Bevy 0.18.1 clipping of rotated UI children before choosing a fix.
//!
//! `cargo run -p flightsim-ui --example attitude_clip_repro -- /tmp/attitude-clip.png`
//! The 64px windows are at x = 48 + 160 * column, y = 80 + 160 * row.
//! Any blue or brown pixel outside a window is a clipping failure. This example
//! intentionally retains the old geometry, even after the production fix.

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

const BANKS: [f32; 6] = [0.0, 30.0, 45.0, 90.0, 135.0, 180.0];
const PITCH_OFFSETS: [f32; 3] = [-11.52, 0.0, 11.52];

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
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_target_texture(1024, 560, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(image);
    commands.spawn((
        Camera2d,
        RenderTarget::Image(target.clone().into()),
        IsDefaultUiCamera,
    ));
    commands.insert_resource(CaptureTarget(target));
    for (row, offset) in PITCH_OFFSETS.into_iter().enumerate() {
        for (column, bank) in BANKS.into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "six columns and three rows")]
            let (left, top) = (48.0 + 160.0 * column as f32, 80.0 + 160.0 * row as f32);
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(left),
                    top: px(top - 28.0),
                    ..default()
                },
                Text::new(format!("{bank:.0} / {offset:.2}")),
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
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.0, 1.0)),
                ))
                .with_children(|dial| {
                    dial.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: px(128.0),
                            height: px(128.0),
                            left: percent(50.0),
                            top: percent(50.0),
                            margin: UiRect {
                                left: px(-64.0),
                                top: px(-64.0),
                                ..default()
                            },
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        UiTransform {
                            rotation: Rot2::degrees(bank),
                            translation: Val2::px(0.0, offset),
                            ..default()
                        },
                    ))
                    .with_children(|horizon| {
                        for color in [Color::srgb(0.20, 0.42, 0.70), Color::srgb(0.35, 0.26, 0.16)]
                        {
                            horizon.spawn((
                                Node {
                                    width: percent(100.0),
                                    height: percent(50.0),
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                        }
                    });
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
            .unwrap_or_else(|| "attitude-clip.png".into());
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
