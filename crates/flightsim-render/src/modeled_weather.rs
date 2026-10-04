//! Opt-in authored weather presentation. Never changes atmosphere or flight dynamics.
//!
//! Fog is a camera-local homogeneous approximation: distant fog banks cannot be
//! seen from outside. Layer entry/exit is smooth; cloud quality changes rendering
//! detail, never the ambient/fog parameters or the nominal optical convention.

use bevy::prelude::*;
use flightsim_core::{Geodetic, Meters, Seconds};
use flightsim_sim::weather::{CloudMorphology, WeatherScenario, WeatherSelection};

use crate::cloud_field::{CloudField, cloud_cover_threshold, cloud_horizontal_density};
use crate::weather::{camera_geodetic, cloud_fog_color, vertical_cloud_density};
use crate::{CloudDistanceFog, CloudLayer, RenderOrigin, SunDirection, SunLighting};

/// Validated selection and executed simulation time, supplied by the app.
///
/// Update before `RenderSet::Weather`, after simulation/replay seek/restart.
/// Solar time and wall time must not be substituted for `elapsed`.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct RenderWeather {
    pub selection: WeatherSelection,
    pub elapsed: Seconds,
}

/// Independent optical contributions, in inverse metres. Never add visibilities.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeatherExtinction {
    pub ambient: f32,
    pub fog: f32,
    pub cloud: f32,
}
impl WeatherExtinction {
    #[must_use]
    pub fn total(self) -> f32 {
        self.ambient + self.fog + self.cloud
    }
    #[must_use]
    pub fn without_cloud(self) -> f32 {
        self.ambient + self.fog
    }
}

/// Camera fog that must remain when upper cloud rendering owns cloud extinction.
/// The render-world readiness gate copies this, rather than clearing all fog.
#[derive(Component, Debug, Clone)]
pub(crate) struct NonCloudWeatherFog(pub DistanceFog);

/// Resolved deck is distinct from the climate/manual CloudLayer resource, so an
/// app climate assignment cannot overwrite a modeled scenario. Legacy is copied
/// unchanged. Negative authored ellipsoidal heights bypass only Legacy's old
/// nonnegative-base rule, after the complete scenario validator has accepted them.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Deref)]
pub(crate) struct ResolvedCloudLayer {
    #[deref]
    pub layer: CloudLayer,
    pub modeled: bool,
}
impl ResolvedCloudLayer {
    pub fn is_clear(self) -> bool {
        if self.modeled {
            self.layer.cover <= 0.0
        } else {
            self.layer.is_clear()
        }
    }
    pub fn is_valid(self) -> bool {
        self.modeled || self.layer.validate().is_ok()
    }
}

pub(crate) fn resolve_weather_cloud(
    legacy: Res<CloudLayer>,
    weather: Res<RenderWeather>,
    mut resolved: ResMut<ResolvedCloudLayer>,
) {
    let next = match weather.selection {
        WeatherSelection::Legacy => ResolvedCloudLayer {
            layer: *legacy,
            modeled: false,
        },
        WeatherSelection::Modeled(scenario) => {
            let p = scenario.parameters();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "validated coverage in (0,1]"
            )]
            let layer = p
                .cloud
                .map_or_else(CloudLayer::default, |cloud| CloudLayer {
                    cover: cloud.coverage as f32,
                    base: cloud.base,
                    top: cloud.top,
                    visibility: cloud.visibility,
                    seed: p.seed,
                });
            ResolvedCloudLayer {
                layer,
                modeled: true,
            }
        }
    };
    if *resolved != next {
        *resolved = next;
    }
}

/// Authored optical convention: this contribution transmits 2% over `visibility`.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "validated visibility produces a bounded f32 coefficient"
)]
pub fn modeled_extinction(visibility: Meters) -> f32 {
    let v = visibility.get();
    if !v.is_finite() || v < 10.0 {
        return 0.0;
    }
    (-0.02_f64.ln() / v) as f32
}

/// Same normalized vertical shape used by authored High/Ultra shader branches.
/// These are authored shapes, not meteorologically diagnosed cloud classes.
#[allow(
    clippy::cast_possible_truncation,
    reason = "bounded normalized layer values"
)]
pub(crate) fn modeled_cloud_profile(
    altitude: Meters,
    base: Meters,
    top: Meters,
    noise: f32,
    morphology: CloudMorphology,
) -> f32 {
    let h = ((altitude.get() - base.get()) / (top.get() - base.get())) as f32;
    let smooth = |a: f32, b: f32, x: f32| {
        let x = ((x - a) / (b - a)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    match morphology {
        CloudMorphology::Layered => vertical_cloud_density(altitude, base, top),
        CloudMorphology::Puffy => crate::cloud_field::cloud_vertical_profile(h, noise),
        CloudMorphology::Towering => {
            let ceiling = 0.8 + 0.18 * noise;
            smooth(0.01, 0.07, h) * (1.0 - smooth(ceiling - 0.16, ceiling, h))
        }
    }
}

fn fog_layer_density(altitude: Meters, bottom: Meters, top: Meters) -> f32 {
    if !altitude.is_finite() {
        return 0.0;
    }
    let edge = 2.0_f64.min((top - bottom).get() * 0.25);
    let smooth = |distance: f64| {
        let x = (distance / edge).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    #[allow(clippy::cast_possible_truncation, reason = "bounded unit density")]
    {
        (smooth((altitude - bottom).get()) * smooth((top - altitude).get())) as f32
    }
}

/// Tier-independent camera-local coefficients for the validated scenario.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "validated coverage in (0,1]"
)]
pub fn weather_extinction(
    scenario: WeatherScenario,
    elapsed: Seconds,
    position: Geodetic,
) -> WeatherExtinction {
    let p = scenario.parameters();
    let fog = p.fog.map_or(0.0, |layer| {
        modeled_extinction(layer.visibility)
            * fog_layer_density(position.altitude, layer.bottom, layer.top)
    });
    let cloud = p.cloud.map_or(0.0, |layer| {
        let Some(q) = CloudField::coordinates(position, elapsed) else {
            return 0.0;
        };
        let noise = CloudField::new(p.seed).noise(q);
        let horizontal =
            cloud_horizontal_density(noise, cloud_cover_threshold(layer.coverage as f32));
        modeled_extinction(layer.visibility)
            * horizontal
            * modeled_cloud_profile(
                position.altitude,
                layer.base,
                layer.top,
                noise,
                layer.morphology,
            )
    });
    WeatherExtinction {
        ambient: modeled_extinction(p.ambient_visibility),
        fog,
        cloud,
    }
}

pub(crate) fn distance_fog(
    extinction: f32,
    lighting: &SunLighting,
    sun: &SunDirection,
) -> DistanceFog {
    DistanceFog {
        color: cloud_fog_color(lighting, sun.elevation, 1.0),
        directional_light_color: Color::NONE,
        directional_light_exponent: 8.0,
        falloff: FogFalloff::Exponential {
            density: extinction,
        },
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "separate owned and untouched camera fog queries"
)]
pub(crate) fn update_modeled_weather_fog(
    mut commands: Commands,
    weather: Res<RenderWeather>,
    origin: Option<Res<RenderOrigin>>,
    lighting: Res<SunLighting>,
    sun: Res<SunDirection>,
    mut owned: Query<
        (Entity, &Transform, &mut DistanceFog),
        (With<Camera3d>, With<CloudDistanceFog>),
    >,
    unowned: Query<(Entity, &Transform), (With<Camera3d>, Without<DistanceFog>)>,
    non_cloud: Query<Entity, With<NonCloudWeatherFog>>,
) {
    let WeatherSelection::Modeled(scenario) = weather.selection else {
        for entity in &non_cloud {
            commands.entity(entity).remove::<NonCloudWeatherFog>();
        }
        return;
    };
    let Some(origin) = origin else {
        return;
    };
    let fogs = |transform: &Transform| {
        let extinction = camera_geodetic(&origin, transform).map_or_else(
            || WeatherExtinction {
                ambient: modeled_extinction(scenario.parameters().ambient_visibility),
                ..default()
            },
            |position| weather_extinction(scenario, weather.elapsed, position),
        );
        (
            distance_fog(extinction.total(), &lighting, &sun),
            NonCloudWeatherFog(distance_fog(extinction.without_cloud(), &lighting, &sun)),
        )
    };
    for (entity, transform, mut fog) in &mut owned {
        let (next, remaining) = fogs(transform);
        *fog = next;
        commands.entity(entity).insert(remaining);
    }
    for (entity, transform) in &unowned {
        let (fog, remaining) = fogs(transform);
        commands
            .entity(entity)
            .insert((CloudDistanceFog, fog, remaining));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_sim::weather::{ModeledFogLayer, WeatherPreset};

    fn scenario(preset: WeatherPreset) -> WeatherScenario {
        WeatherScenario::from_preset(preset, Geodetic::from_degrees(35.0, 139.0, 0.0), 7).unwrap()
    }
    #[test]
    fn extinction_addition_matches_independent_beer_lambert_transmission() {
        let mut p = scenario(WeatherPreset::Rain).parameters();
        p.preset = WeatherPreset::Custom;
        p.fog = Some(ModeledFogLayer {
            bottom: Meters(0.0),
            top: Meters(3000.0),
            visibility: Meters(250.0),
        });
        let weather = WeatherScenario::try_from(p).unwrap();
        let extinction = weather_extinction(
            weather,
            Seconds(0.0),
            Geodetic::from_degrees(35.0, 139.0, 1500.0),
        );
        for (coefficient, visibility) in [
            (modeled_extinction(Meters(250.0)), 250.0_f32),
            (modeled_extinction(Meters(10000.0)), 10000.0),
        ] {
            assert!(((-coefficient * visibility).exp() - 0.02).abs() < 1e-6);
        }
        assert!(extinction.cloud > 0.0 && extinction.fog > 0.0);
        let total_transmission = (-extinction.total() * 100.0).exp();
        let independent = (-extinction.ambient * 100.0).exp()
            * (-extinction.fog * 100.0).exp()
            * (-extinction.cloud * 100.0).exp();
        assert!((total_transmission - independent).abs() < 1e-6);
        assert!(extinction.total() > extinction.without_cloud());
    }
    #[test]
    fn fog_is_dense_at_normal_eye_height_and_smooth_at_both_edges() {
        let layer = (Meters(-500.0), Meters(-200.0));
        assert_eq!(
            fog_layer_density(Meters(-498.0), layer.0, layer.1).to_bits(),
            1.0_f32.to_bits()
        );
        for boundary in [-500.0, -200.0] {
            assert!(fog_layer_density(Meters(boundary - 1e-4), layer.0, layer.1) < 1e-6);
            assert!(fog_layer_density(Meters(boundary + 1e-4), layer.0, layer.1) < 1e-6);
        }
    }
    #[test]
    fn negative_authored_cloud_heights_do_not_relax_legacy_validation() {
        let scenario = WeatherScenario::from_preset(
            WeatherPreset::Snow,
            Geodetic::from_degrees(0.0, 0.0, -800.0),
            1,
        )
        .unwrap();
        let mut app = App::new();
        app.init_resource::<CloudLayer>()
            .init_resource::<ResolvedCloudLayer>()
            .insert_resource(RenderWeather {
                selection: WeatherSelection::Modeled(scenario),
                elapsed: Seconds::ZERO,
            })
            .add_systems(Update, resolve_weather_cloud);
        app.update();
        let deck = *app.world().resource::<ResolvedCloudLayer>();
        assert!(deck.base.get() < 0.0 && deck.is_valid() && !deck.is_clear());
        assert!(deck.layer.validate().is_err());
        let extinction = weather_extinction(
            scenario,
            Seconds::ZERO,
            Geodetic::from_degrees(0.0, 0.0, 0.0),
        );
        assert!(extinction.cloud > 0.0);
    }
    #[test]
    fn elapsed_and_position_are_sufficient_after_pause_seek_and_restart() {
        let scenario = scenario(WeatherPreset::Cloud);
        let position = Geodetic::from_degrees(35.0, 139.0, 2000.0);
        let initial = weather_extinction(scenario, Seconds(17.0), position);
        for elapsed in [17.0, 52.0, 0.0, 400.0, 17.0] {
            let _ = weather_extinction(scenario, Seconds(elapsed), position);
        }
        assert_eq!(
            initial,
            weather_extinction(scenario, Seconds(17.0), position)
        );
    }
}
