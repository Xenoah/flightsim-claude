//! 時刻を変えて空の色と陰の向きを目で確かめるための最小の場面。
//!
//! **描画は自動テストが極めて難しい。** 数値の検査だけでは
//! 「テストは通るが絵は壊れている」を捕まえられないので、
//! 変更したら必ずここで撮って見ること。
//!
//! ```bash
//! # 東京の地方平均太陽時で 12 時
//! cargo run -p flightsim-render --example sun_clock -- --hour 12 --out noon.png
//! # 日の入り前後・夜
//! cargo run -p flightsim-render --example sun_clock -- --hour 18.5 --out dusk.png
//! cargo run -p flightsim-render --example sun_clock -- --hour 23 --out night.png
//! # 大気散乱を切って、こちらで減衰を掛ける経路を見る
//! cargo run -p flightsim-render --example sun_clock -- --hour 8 --no-atmosphere
//! ```
//!
//! `--out` を付けなければウィンドウが開いたままになる。`--rate` で時間加速を
//! 変えられるので、`--rate 3600` にすると 24 秒で 1 日が回る。

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム"
)]

use bevy::camera::Exposure;
use bevy::pbr::{Atmosphere, ScatteringMedium};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use flightsim_core::Geodetic;
use flightsim_render::daylight::{SunIlluminancePolicy, SunLighting, TimeOfDay, TimeRate};
use flightsim_render::sun::{UtcDateTime, solar_position};
use flightsim_render::{
    CameraWorldPosition, FlightsimRenderPlugin, RenderOrigin, SunDirection, sun_light_bundle,
};
use std::path::PathBuf;

/// 場面の設定。
#[derive(Resource, Debug, Clone)]
struct Scene {
    observer: Geodetic,
    /// 地方平均太陽時（時）。
    hour: f64,
    date: (i32, u8, u8),
    rate: TimeRate,
    atmosphere: bool,
    screenshot: Option<PathBuf>,
    delay: f64,
    /// 大気圏外の照度を上書きする。空が光源に比例するかを確かめるための穴。
    illuminance: Option<f32>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            // 東京。海抜 0 m の平地に立って地平線を見る。
            observer: Geodetic::from_degrees(35.6895, 139.6917, 0.0),
            hour: 12.0,
            date: (2026, 6, 21),
            rate: TimeRate::PAUSED,
            atmosphere: true,
            screenshot: None,
            delay: 2.0,
            illuminance: None,
        }
    }
}

fn main() {
    let scene = parse_arguments();
    let clock = TimeOfDay {
        utc: local_time(&scene),
        rate: scene.rate,
    };
    let lighting = SunLighting {
        policy: if scene.atmosphere {
            SunIlluminancePolicy::AboveAtmosphere
        } else {
            SunIlluminancePolicy::Attenuated
        },
        raw_illuminance: scene
            .illuminance
            .unwrap_or(SunLighting::default().raw_illuminance),
        ..SunLighting::default()
    };

    let position = solar_position(clock.utc, scene.observer);
    let utc = clock.utc_date_time();
    println!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC  azimuth {:.1}°  elevation {:.1}°  illuminance {:.0} lx  ambient {:.0} cd/m²",
        utc.year,
        utc.month,
        utc.day,
        utc.hour,
        utc.minute,
        position.azimuth.to_degrees().get(),
        position.elevation.to_degrees().get(),
        lighting.illuminance(position.elevation),
        lighting.ambient(position.elevation).brightness,
    );

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(FlightsimRenderPlugin)
        .insert_resource(RenderOrigin::new(scene.observer))
        .insert_resource(CameraWorldPosition(scene.observer))
        .insert_resource(clock)
        .insert_resource(lighting)
        .insert_resource(scene)
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}

fn setup(
    scene: Res<Scene>,
    lighting: Res<SunLighting>,
    sun: Res<SunDirection>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    // 地面。海抜 0 m の平面。**大気散乱は y を海抜高度として読む。**
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40_000.0, 40_000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.35, 0.24),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::default(),
        Name::new("ground"),
    ));

    // 光の向きが読めるように、影を落とす物を並べる。
    let pillar = meshes.add(Cuboid::new(2.0, 12.0, 2.0));
    let sphere = meshes.add(Sphere::new(3.0).mesh().uv(32, 18));
    let white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.75, 0.72),
        perceptual_roughness: 0.6,
        ..default()
    });
    for index in [-2.0_f32, -1.0, 0.0, 1.0, 2.0] {
        commands.spawn((
            Mesh3d(pillar.clone()),
            MeshMaterial3d(white.clone()),
            Transform::from_xyz(index * 14.0, 6.0, -30.0),
        ));
    }
    commands.spawn((
        Mesh3d(sphere),
        MeshMaterial3d(white),
        Transform::from_xyz(0.0, 3.0, -12.0),
    ));

    #[allow(
        clippy::cast_possible_truncation,
        reason = "遠クリップ面は 40 万 m。f32 で十分"
    )]
    let far = flightsim_render::default_far_plane().get() as f32;

    // カメラ。**露出は光量と組で決めること**（HANDOFF の地雷）。
    let mut camera = commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far,
            ..default()
        }),
        Exposure::SUNLIGHT,
        // 人の目の高さから地平線を見る。北を向く（描画座標は X = 東、Y = 上、Z = 南）。
        Transform::from_xyz(0.0, 1.7, 20.0).looking_at(Vec3::new(0.0, 6.0, -30.0), Vec3::Y),
        Name::new("camera"),
    ));
    if scene.atmosphere {
        camera.insert(Atmosphere::earthlike(
            media.add(ScatteringMedium::earthlike(64, 64)),
        ));
    }

    commands.spawn(sun_light_bundle(&lighting, *sun));

    // 大気散乱を切ると空が真っ黒になるので、比較用に地平線の色だけ入れておく。
    if !scene.atmosphere {
        commands.insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)));
    }
}

/// 指定した地方平均太陽時に対応する UTC。
fn local_time(scene: &Scene) -> flightsim_render::JulianDate {
    let (year, month, day) = scene.date;
    let hour = scene.hour.rem_euclid(24.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0..24 に畳んだ後なので u8 に収まる"
    )]
    let civil = UtcDateTime::new(
        year,
        month,
        day,
        hour.floor() as u8,
        (hour.fract() * 60.0).floor() as u8,
        0.0,
    );
    flightsim_render::JulianDate::from_local_mean_solar_time(civil, scene.observer.longitude)
}

fn capture(
    time: Res<Time>,
    scene: Res<Scene>,
    mut commands: Commands,
    mut elapsed: Local<f64>,
    mut shot: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = scene.screenshot.as_ref() else {
        return;
    };
    *elapsed += f64::from(time.delta_secs());
    if !*shot && *elapsed >= scene.delay {
        *shot = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    } else if *shot && *elapsed >= scene.delay + 1.5 {
        // 保存が終わる余裕を取ってから閉じる。
        exit.write(AppExit::Success);
    }
}

fn parse_arguments() -> Scene {
    let mut scene = Scene::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--hour" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse().ok()) {
                    scene.hour = value;
                }
            }
            "--date" => {
                if let Some(value) = arguments.next() {
                    let parts: Vec<&str> = value.split('-').collect();
                    if let [year, month, day] = parts[..]
                        && let (Ok(year), Ok(month), Ok(day)) =
                            (year.parse(), month.parse(), day.parse())
                    {
                        scene.date = (year, month, day);
                    }
                }
            }
            "--at" => {
                if let Some(value) = arguments.next() {
                    let parts: Vec<&str> = value.split(',').collect();
                    if let [latitude, longitude] = parts[..]
                        && let (Ok(latitude), Ok(longitude)) = (latitude.parse(), longitude.parse())
                    {
                        scene.observer = Geodetic::from_degrees(latitude, longitude, 0.0);
                    }
                }
            }
            "--rate" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse().ok()) {
                    scene.rate = TimeRate(value);
                }
            }
            "--illuminance" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse().ok()) {
                    scene.illuminance = Some(value);
                }
            }
            "--no-atmosphere" => scene.atmosphere = false,
            "--out" => scene.screenshot = arguments.next().map(PathBuf::from),
            "--delay" => {
                if let Some(value) = arguments.next().and_then(|v| v.parse().ok()) {
                    scene.delay = value;
                }
            }
            other => eprintln!("ignoring unknown argument `{other}`"),
        }
    }
    scene
}
