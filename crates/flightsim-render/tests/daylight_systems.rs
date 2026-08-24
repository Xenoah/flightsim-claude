//! 時刻 → 太陽の位置 → 光 の結線を ECS ごと確かめる。
//!
//! 純関数の検査（`src/sun.rs` と `src/daylight.rs`）は「値が正しいか」しか見ない。
//! **system として登録し忘れていても、印を付け忘れていても、そちらは全部緑になる。**
//! ここでは実際に `App` を回して、光源に届いているかを見る。

use bevy::light::{GlobalAmbientLight, light_consts::lux};
use bevy::prelude::*;
use flightsim_core::{Geodetic, Seconds};
use flightsim_render::daylight::{SunLight, SunLighting, TimeOfDay, TimeRate};
use flightsim_render::sun::UtcDateTime;
use flightsim_render::{CameraWorldPosition, FlightsimRenderPlugin, RenderOrigin, SunDirection};

/// 東京。
fn tokyo() -> Geodetic {
    Geodetic::from_degrees(35.6895, 139.6917, 0.0)
}

/// 描画層のプラグインだけを載せた最小の App。
fn harness(clock: TimeOfDay) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(FlightsimRenderPlugin)
        .insert_resource(RenderOrigin::new(tokyo()))
        .insert_resource(CameraWorldPosition(tokyo()))
        .insert_resource(clock);
    app
}

/// 太陽の平行光源を 1 つ置く。
fn spawn_sun(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            SunLight,
            DirectionalLight::default(),
            Transform::default(),
            Name::new("sun"),
        ))
        .id()
}

fn light_forward(app: &App, entity: Entity) -> Vec3 {
    Vec3::from(
        app.world()
            .entity(entity)
            .get::<Transform>()
            .unwrap()
            .forward(),
    )
}

#[test]
fn the_light_follows_the_clock() {
    // 正午。太陽はほぼ真上なので、光は下を向く。
    let noon = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 12, 0, 0.0),
        tokyo().longitude,
    );
    let mut app = harness(noon);
    let sun = spawn_sun(&mut app);
    app.update();

    let direction = *app.world().resource::<SunDirection>();
    let elevation = direction.elevation.to_degrees().get();
    assert!(
        elevation > 70.0,
        "the midsummer noon sun should be high, got {elevation:.1}°"
    );

    let forward = light_forward(&app, sun);
    assert!(
        forward.dot(Vec3::NEG_Y) > 0.9,
        "a high sun should light downward, got {forward:?}"
    );

    let light = app.world().entity(sun).get::<DirectionalLight>().unwrap();
    assert!(
        (light.illuminance - lux::RAW_SUNLIGHT).abs() < f32::EPSILON,
        "the atmosphere wants the unfiltered illuminance, got {}",
        light.illuminance
    );

    let ambient = app.world().resource::<GlobalAmbientLight>();
    let lighting = SunLighting::default();
    // 天頂に届かないぶん（`sqrt(sin h)`）だけ満額より少し暗い。
    assert!(
        ambient.brightness > lighting.daylight_ambient * 0.95,
        "daylight should be close to the full ambient {}, got {}",
        lighting.daylight_ambient,
        ambient.brightness
    );
}

#[test]
fn the_night_turns_the_scene_down_but_not_off() {
    let midnight = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 0, 0, 0.0),
        tokyo().longitude,
    );
    let mut app = harness(midnight);
    let sun = spawn_sun(&mut app);
    app.update();

    let direction = *app.world().resource::<SunDirection>();
    assert!(
        direction.elevation.get() < 0.0,
        "local midnight should put the sun below the horizon, got {direction:?}"
    );

    // 太陽が下にあるので、光は上を向く。
    let forward = light_forward(&app, sun);
    assert!(
        forward.dot(Vec3::Y) > 0.5,
        "a sun below the horizon should light upward, got {forward:?}"
    );

    // **真っ暗にしない。** 機体の輪郭が残る程度の環境光が要る。
    let ambient = app.world().resource::<GlobalAmbientLight>();
    let lighting = SunLighting::default();
    assert!(
        ambient.brightness > 0.0 && ambient.brightness <= lighting.night_ambient,
        "the night ambient should sit at the floor, got {}",
        ambient.brightness
    );
}

#[test]
fn the_sun_moves_when_time_passes() {
    // 時間加速を入れて数フレーム回すと、太陽が動くこと。
    // **ここが動かないと、日の出を待つのに実時間が要る。**
    let dawn = TimeOfDay {
        rate: TimeRate::FASTEST,
        ..TimeOfDay::at_local_mean_solar_time(
            UtcDateTime::new(2026, 6, 21, 6, 0, 0.0),
            tokyo().longitude,
        )
    };
    let mut app = harness(dawn);
    spawn_sun(&mut app);
    app.update();

    let start = app.world().resource::<SunDirection>().elevation;
    let start_time = app.world().resource::<TimeOfDay>().utc;

    for _ in 0..5 {
        // `Time` は実時間で進むので、確実に 0 より大きい刻みを作る。
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.update();
    }

    let moved = app.world().resource::<TimeOfDay>().utc.get() - start_time.get();
    assert!(
        moved > 0.0,
        "the clock did not advance at {}x",
        TimeRate::FASTEST.get()
    );
    let now = app.world().resource::<SunDirection>().elevation;
    assert!(
        now.get() > start.get(),
        "the morning sun should climb, went from {:.3}° to {:.3}°",
        start.to_degrees().get(),
        now.to_degrees().get()
    );
}

#[test]
fn a_paused_clock_freezes_the_light() {
    // スクリーンショットの比較には、時刻が止まることが要る。
    let mut clock = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 9, 0, 0.0),
        tokyo().longitude,
    );
    clock.rate = TimeRate::PAUSED;
    let frozen = clock.utc;

    let mut app = harness(clock);
    spawn_sun(&mut app);
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.update();
    }

    let now = app.world().resource::<TimeOfDay>().utc;
    assert!(
        (now.get() - frozen.get()).abs() < f64::EPSILON,
        "a paused clock moved from {} to {}",
        frozen.get(),
        now.get()
    );
}

#[test]
fn lights_without_the_marker_are_left_alone() {
    // 印の付いていない光源は触らない。夜間の着陸灯を後から足せるようにするため。
    let noon = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 12, 0, 0.0),
        tokyo().longitude,
    );
    let mut app = harness(noon);
    let other = app
        .world_mut()
        .spawn((
            DirectionalLight {
                illuminance: 1234.0,
                ..default()
            },
            Transform::default(),
        ))
        .id();
    app.update();

    let light = app.world().entity(other).get::<DirectionalLight>().unwrap();
    assert!(
        (light.illuminance - 1234.0).abs() < f32::EPSILON,
        "an unmarked light was overwritten with {}",
        light.illuminance
    );
    let transform = app.world().entity(other).get::<Transform>().unwrap();
    assert_eq!(
        *transform,
        Transform::default(),
        "an unmarked light was rotated"
    );
}

#[test]
fn a_broken_clock_does_not_break_the_light() {
    // **NaN は全状態に伝播する。** 時刻が壊れても、描画が NaN の姿勢を掴まないこと。
    let mut clock = TimeOfDay::at_local_mean_solar_time(
        UtcDateTime::new(2026, 6, 21, 12, 0, 0.0),
        tokyo().longitude,
    );
    clock.rate = TimeRate(f64::NAN);
    let mut app = harness(clock);
    let sun = spawn_sun(&mut app);

    app.world_mut()
        .resource_mut::<TimeOfDay>()
        .advance(Seconds(f64::NAN));
    app.update();

    assert!(app.world().resource::<TimeOfDay>().utc.is_finite());
    let transform = app.world().entity(sun).get::<Transform>().unwrap();
    assert!(
        transform.rotation.is_finite() && transform.translation.is_finite(),
        "the sun light picked up a broken transform: {transform:?}"
    );
    let ambient = app.world().resource::<GlobalAmbientLight>();
    assert!(ambient.brightness.is_finite());
}
