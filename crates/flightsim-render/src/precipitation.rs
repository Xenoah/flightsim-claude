//! Bounded, deterministic visual precipitation. No collision, accumulation,
//! history, wind/force feedback, networking, textures or external data.
//!
//! A single mesh contains camera-facing rain streaks or snow diamonds. Particle
//! positions are a periodic field in a fixed departure-local frame, evaluated
//! directly at executed simulation time. Rewind/restart never need warm-up.

use crate::{CloudQuality, CloudVolumeCamera, RenderOrigin, RenderWeather};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::VertexAttributeValues,
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use flightsim_core::{Ecef, LocalFrame, Ned, Seconds};
use flightsim_sim::weather::{PrecipitationKind, WeatherParameters, WeatherSelection};

/// Logical source mesh storage and owned draws, not GPU memory or measured time.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrecipitationDiagnostics {
    pub particles: usize,
    pub particle_cap: usize,
    /// Six vertices each with position/normal = 6 * (12 + 12).
    pub cpu_mesh_bytes: usize,
    /// One precipitation mesh submitted when active; zero otherwise.
    pub draws: usize,
}

#[derive(Component, Debug)]
pub(crate) struct PrecipitationMesh;
#[derive(Resource, Debug, Default)]
pub(crate) struct PrecipitationVisuals {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    kind: Option<PrecipitationKind>,
    count: usize,
}

const VERTICES: usize = 6;
const BYTES_PER_PARTICLE: usize = VERTICES * (12 + 12);
const CELL: f64 = 64.0;
// Every unscaled proxy fits within 1.3 * hypot(0.55, 0.015) < 0.716 m.
// Reserve 0.75 m below the ellipsoidal top before the 2 m area fade. The
// additional margin covers final local-f32 rounding under normal rebasing.
const CEILING_PROXY_MARGIN: f64 = 0.75;
const CEILING_FADE: f64 = 2.0;

fn ceiling_fade(p: WeatherParameters, world: Ecef) -> f64 {
    p.cloud.map_or(1.0, |cloud| {
        let clearance = cloud.top.get() - world.to_geodetic().altitude.get();
        let fade = ((clearance - CEILING_PROXY_MARGIN) / CEILING_FADE).clamp(0.0, 1.0);
        fade * fade * (3.0 - 2.0 * fade)
    })
}

fn field_above_cloud(p: WeatherParameters, camera: Ecef) -> bool {
    // Only centers strictly inside the 32 m sphere can have nonzero area.
    // Ellipsoidal height is the signed normal distance to the WGS84 surface,
    // so its change is bounded by Euclidean distance here. Do not substitute
    // departure-NED down: its tangent plane diverges from height on long flights.
    p.cloud
        .is_some_and(|cloud| camera.to_geodetic().altitude.get() - CELL * 0.5 >= cloud.top.get())
}

fn particle_cap(quality: CloudQuality) -> usize {
    match quality {
        CloudQuality::Off | CloudQuality::Light => 128,
        CloudQuality::High => 256,
        CloudQuality::Ultra => 384,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "bounded visible-particle proxy; cap is at most 384"
)]
fn particle_count(p: WeatherParameters, cap: usize) -> usize {
    if p.precipitation_kind == PrecipitationKind::None {
        return 0;
    }
    // Rate is liquid water equivalent. This is a visual density proxy, not a
    // number concentration, snowfall depth or a diagnosed drop-size spectrum.
    let fraction = (p.precipitation_rate.0 / (0.025 / 3600.0))
        .sqrt()
        .clamp(0.08, 1.0);
    (fraction * cap as f64).ceil() as usize
}

fn unit(seed: u64, index: usize, channel: u64) -> f64 {
    let mut n = seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ channel;
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let bytes = (n ^ (n >> 31)).to_le_bytes();
    let bits = u32::from_le_bytes(bytes[4..].try_into().expect("four hash bytes"));
    f64::from(bits) / 4_294_967_296.0
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    world: Ecef,
    alpha: f32,
    size: f32,
}

/// The canonical field is evaluated before narrowing into the current origin.
#[cfg(test)]
fn particle(
    p: WeatherParameters,
    elapsed: Seconds,
    index: usize,
    camera: Ecef,
) -> Option<Particle> {
    let frame = LocalFrame::new(p.departure_reference);
    particle_in_frame(
        p,
        elapsed,
        index,
        &frame,
        frame.ecef_to_ned_position(camera),
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "unit opacity and sub-metre visual sizes"
)]
fn particle_in_frame(
    p: WeatherParameters,
    elapsed: Seconds,
    index: usize,
    frame: &LocalFrame,
    at: Ned,
) -> Option<Particle> {
    if !elapsed.is_finite() || elapsed.get() < 0.0 {
        return None;
    }
    let snow = p.precipitation_kind == PrecipitationKind::Snow;
    let speed = if snow {
        0.8 + unit(p.seed, index, 1) * 0.6
    } else {
        8.0 + unit(p.seed, index, 1) * 3.0
    };
    let phase = elapsed.get().rem_euclid(CELL / speed) * speed;
    let sway = if snow {
        (elapsed.get().rem_euclid(8.0) * std::f64::consts::TAU / 8.0 + unit(p.seed, index, 2) * 6.0)
            .sin()
            * 0.6
    } else {
        0.0
    };
    let base = [
        unit(p.seed, index, 3) * CELL + sway,
        unit(p.seed, index, 4) * CELL,
        unit(p.seed, index, 5) * CELL + phase,
    ];
    let center = [at.north(), at.east(), at.down()];
    if center.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let local: [f64; 3] = std::array::from_fn(|axis| {
        base[axis] + ((center[axis] - base[axis]) / CELL + 0.5).floor() * CELL
    });
    let distance = ((local[0] - center[0]).powi(2)
        + (local[1] - center[1]).powi(2)
        + (local[2] - center[2]).powi(2))
    .sqrt();
    let fade = ((CELL * 0.5 - distance) / 8.0).clamp(0.0, 1.0);
    let near = ((distance - 1.5) / 2.0).clamp(0.0, 1.0);
    let world = frame.ned_to_ecef_position(Ned::new(local[0], local[1], local[2]));
    Some(Particle {
        world,
        alpha: (fade * fade * (3.0 - 2.0 * fade) * near * ceiling_fade(p, world)) as f32,
        size: (0.7 + unit(p.seed, index, 6) * 0.6) as f32,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "single bounded visual owner with optional headless asset resources"
)]
pub(crate) fn update_precipitation(
    mut commands: Commands,
    weather: Res<RenderWeather>,
    quality: Res<CloudQuality>,
    origin: Option<Res<RenderOrigin>>,
    cameras: Query<(&Transform, &Camera, Option<&CloudVolumeCamera>), With<Camera3d>>,
    mut visuals: ResMut<PrecipitationVisuals>,
    mut diagnostics: ResMut<PrecipitationDiagnostics>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let parameters = match weather.selection {
        WeatherSelection::Legacy => None,
        WeatherSelection::Modeled(scenario) => Some(scenario.parameters()),
    };
    let cap = particle_cap(*quality);
    let count = parameters.map_or(0, |p| particle_count(p, cap));
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    if count == 0 || !weather.elapsed.is_finite() || weather.elapsed.get() < 0.0 {
        clear(&mut commands, &mut visuals, &mut meshes, &mut materials);
        *diagnostics = PrecipitationDiagnostics::default();
        return;
    }
    // The marked flight view owns precipitation. A suspended flight camera
    // must not silently transfer it to an unrelated active debug camera.
    let designated = cameras.iter().find(|(_, _, marked)| marked.is_some());
    let selected = match designated {
        Some(view) if view.1.is_active => Some(view),
        Some(_) => None,
        None => cameras.iter().find(|(_, camera, _)| camera.is_active),
    };
    let (Some(origin), Some((camera, _, _))) = (origin, selected) else {
        clear(&mut commands, &mut visuals, &mut meshes, &mut materials);
        *diagnostics = PrecipitationDiagnostics::default();
        return;
    };
    let p = parameters.expect("positive count requires modeled parameters");
    if !camera.translation.is_finite() || !camera.rotation.is_finite() {
        clear(&mut commands, &mut visuals, &mut meshes, &mut materials);
        *diagnostics = PrecipitationDiagnostics::default();
        return;
    }
    if field_above_cloud(p, origin.0.to_world(camera.translation)) {
        clear(&mut commands, &mut visuals, &mut meshes, &mut materials);
        *diagnostics = PrecipitationDiagnostics::default();
        return;
    }
    if visuals.kind != Some(p.precipitation_kind) || visuals.count != count {
        clear(&mut commands, &mut visuals, &mut meshes, &mut materials);
        let mesh = meshes.add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0_f32; 3]; count * VERTICES],
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                vec![[0.0_f32, 0.0, 1.0]; count * VERTICES],
            ),
        );
        let snow = p.precipitation_kind == PrecipitationKind::Snow;
        let material = materials.add(StandardMaterial {
            base_color: if snow {
                Color::srgb(0.94, 0.97, 1.0)
            } else {
                Color::srgb(0.66, 0.78, 0.90)
            },
            // The upper cloud composite reads opaque scene depth. A blended
            // particle would be overpainted by cloud behind it. These tiny solid
            // proxies write depth; their area, not alpha, fades at the boundary.
            alpha_mode: AlphaMode::Opaque,
            perceptual_roughness: 1.0,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let entity = commands
            .spawn((
                PrecipitationMesh,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                NoFrustumCulling,
                NotShadowCaster,
                NotShadowReceiver,
                Name::new("authored precipitation"),
            ))
            .id();
        visuals.entity = Some(entity);
        visuals.mesh = Some(mesh);
        visuals.material = Some(material);
        visuals.kind = Some(p.precipitation_kind);
        visuals.count = count;
    }
    let Some(mesh) = visuals
        .mesh
        .as_ref()
        .and_then(|handle| meshes.get_mut(handle))
    else {
        return;
    };
    update_mesh(mesh, p, weather.elapsed, &origin, camera, cap);
    *diagnostics = PrecipitationDiagnostics {
        particles: count,
        particle_cap: cap,
        cpu_mesh_bytes: count * BYTES_PER_PARTICLE,
        draws: 1,
    };
}

fn quad_axes(snow: bool, physical_up: Vec3, rotation: Quat) -> (Vec3, Vec3) {
    if snow {
        return (rotation * Vec3::X * 0.09, rotation * Vec3::Y * 0.09);
    }
    let view = rotation * Vec3::Z;
    // Project falling streaks into the image plane. Screen-right alone becomes
    // parallel to physical-up at 90 degree bank and collapses every triangle.
    let projected = physical_up - view * physical_up.dot(view);
    let length = projected.length();
    let tall = if length > 1e-4 {
        projected / length
    } else {
        rotation * Vec3::Y
    };
    (
        tall.cross(view).normalize_or_zero() * 0.015,
        tall * (0.04 + 0.51 * length),
    )
}

fn update_mesh(
    mesh: &mut Mesh,
    p: WeatherParameters,
    elapsed: Seconds,
    origin: &RenderOrigin,
    camera: &Transform,
    cap: usize,
) {
    let frame = LocalFrame::new(p.departure_reference);
    let at = frame.ecef_to_ned_position(origin.0.to_world(camera.translation));
    #[allow(clippy::cast_precision_loss, reason = "cap is at most 384")]
    let sampling_weight = 128.0 / cap as f32;
    let snow = p.precipitation_kind == PrecipitationKind::Snow;
    let physical_up = origin
        .0
        .vector_to_render(frame.ned_to_ecef_vector(Ned::new(0.0, 0.0, -1.0)));
    let (right, up) = quad_axes(snow, physical_up, camera.rotation);
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for (index, vertices) in positions.chunks_exact_mut(VERTICES).enumerate() {
            let Some(drop) = particle_in_frame(p, elapsed, index, &frame, at) else {
                vertices.fill([0.0; 3]);
                continue;
            };
            let center = origin.0.to_render(drop.world);
            let weight = (drop.alpha * sampling_weight).sqrt();
            let wide = right * drop.size * weight;
            let tall = up * drop.size * weight;
            let points = if snow {
                [
                    center - tall,
                    center + wide,
                    center + tall,
                    center - tall,
                    center + tall,
                    center - wide,
                ]
            } else {
                [
                    center - tall - wide,
                    center - tall + wide,
                    center + tall + wide,
                    center - tall - wide,
                    center + tall + wide,
                    center + tall - wide,
                ]
            };
            for (vertex, point) in vertices.iter_mut().zip(points) {
                *vertex = point.to_array();
            }
        }
    }
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        normals.fill((camera.rotation * Vec3::Z).to_array());
    }
}

fn clear(
    commands: &mut Commands,
    visuals: &mut PrecipitationVisuals,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    if let Some(entity) = visuals.entity.take() {
        commands.entity(entity).despawn();
    }
    if let Some(mesh) = visuals.mesh.take() {
        meshes.remove(&mesh);
    }
    if let Some(material) = visuals.material.take() {
        materials.remove(&material);
    }
    visuals.kind = None;
    visuals.count = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Geodetic, Meters};
    use flightsim_sim::weather::{WeatherPreset, WeatherScenario};

    fn parameters(preset: WeatherPreset) -> WeatherParameters {
        WeatherScenario::from_preset(preset, Geodetic::from_degrees(35.0, 139.0, 0.0), 71)
            .unwrap()
            .parameters()
    }
    #[test]
    fn pause_seek_restart_and_cadence_need_no_particle_history() {
        let p = parameters(WeatherPreset::Rain);
        let camera = Geodetic::from_degrees(35.0, 139.0, 20.0).to_ecef();
        let first = particle(p, Seconds(17.0), 3, camera).unwrap();
        for time in [17.0, 17.1, 25.0, 1000.0, 0.0, 17.0] {
            let _ = particle(p, Seconds(time), 3, camera);
        }
        let again = particle(p, Seconds(17.0), 3, camera).unwrap();
        assert_eq!(first.world.as_vec(), again.world.as_vec());
        assert_eq!(first.alpha.to_bits(), again.alpha.to_bits());
        assert!(particle(p, Seconds(f64::NAN), 3, camera).is_none());
        assert!(particle(p, Seconds(-1.0), 3, camera).is_none());
    }
    #[test]
    fn rebasing_changes_only_final_render_coordinates() {
        let p = parameters(WeatherPreset::Snow);
        let camera = Geodetic::from_degrees(35.0, 139.0, 20.0).to_ecef();
        let drop = particle(p, Seconds(32.0), 9, camera).unwrap();
        let first = RenderOrigin::new(p.departure_reference);
        let shifted = RenderOrigin::new(Geodetic::from_degrees(35.02, 139.01, 0.0));
        for origin in [first, shifted] {
            let restored = origin.0.to_world(origin.0.to_render(drop.world));
            assert!(restored.distance_to(drop.world).get() < 0.002);
        }
    }
    #[test]
    fn rate_and_tier_only_choose_a_bounded_prefix_of_the_same_field() {
        let p = parameters(WeatherPreset::Storm);
        let camera = p.departure_reference.to_ecef();
        for quality in [
            CloudQuality::Off,
            CloudQuality::Light,
            CloudQuality::High,
            CloudQuality::Ultra,
        ] {
            let cap = particle_cap(quality);
            assert!(particle_count(p, cap) <= cap && cap <= 384);
            for index in 0..cap {
                let a = particle(p, Seconds(100.0), index, camera).unwrap();
                assert!(a.world.as_vec().is_finite() && a.alpha.is_finite());
                assert!((0.0..=1.0).contains(&a.alpha));
            }
        }
        assert_eq!(particle_count(parameters(WeatherPreset::Clear), 384), 0);
    }
    #[test]
    fn rain_billboards_keep_area_at_steep_bank_and_vertical_sight() {
        for rotation in [
            Quat::IDENTITY,
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ] {
            let (wide, tall) = quad_axes(false, Vec3::Y, rotation);
            assert!(wide.is_finite() && tall.is_finite());
            assert!(wide.cross(tall).length() > 0.0001);
            assert!(wide.dot(tall).abs() < 1e-6);
        }
    }
    #[test]
    fn ceiling_feather_reserves_the_entire_proxy_before_fading() {
        let p = parameters(WeatherPreset::Rain);
        let top = p.cloud.unwrap().top.get();
        for (clearance, expected) in [
            (10.0, 1.0),
            (1.75, 0.5),
            (0.5, 0.0),
            (0.0, 0.0),
            (-10.0, 0.0),
        ] {
            let world = Geodetic::from_degrees(35.0, 139.0, top - clearance).to_ecef();
            assert!((ceiling_fade(p, world) - expected).abs() < 1e-8);
        }
        for (offset, above) in [(31.99, false), (32.0, true), (32.01, true)] {
            let camera = Geodetic::from_degrees(0.0, 0.0, top + offset).to_ecef();
            assert_eq!(field_above_cloud(p, camera), above);
        }
        for snow in [false, true] {
            for rotation in [
                Quat::IDENTITY,
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                Quat::from_euler(EulerRot::XYZ, 0.4, 1.2, -0.9),
            ] {
                let (wide, tall) = quad_axes(snow, Vec3::Y, rotation);
                // Triangle inequality is conservative for both streak corners
                // and diamond tips; size <= 1.3, sampling/area weights <= 1.
                assert!(f64::from(1.3 * (wide.length() + tall.length())) < CEILING_PROXY_MARGIN);
            }
        }
    }

    #[test]
    fn ceiling_uses_global_ellipsoidal_height_at_poles_dateline_and_negative_layers() {
        let mut negative = parameters(WeatherPreset::Rain);
        negative.preset = WeatherPreset::Custom;
        negative.departure_reference = Geodetic::from_degrees(-25.0, 50.0, -900.0);
        negative.cloud.as_mut().unwrap().base = Meters(-800.0);
        negative.cloud.as_mut().unwrap().top = Meters(-400.0);
        let negative = WeatherScenario::try_from(negative).unwrap().parameters();
        for p in [
            parameters(WeatherPreset::Rain),
            parameters(WeatherPreset::Snow),
            parameters(WeatherPreset::Storm),
            negative,
        ] {
            let top = p.cloud.unwrap().top.get();
            let mut unbounded = p;
            unbounded.preset = WeatherPreset::Custom;
            unbounded.cloud = None;
            let unbounded = WeatherScenario::try_from(unbounded).unwrap().parameters();
            // These cameras are far from the fixed departure: a tangent-down
            // ceiling would give the wrong answer, particularly at the poles.
            for (latitude, longitude) in [
                (90.0, 180.0),
                (-90.0, -180.0),
                (0.0, 180.0),
                (0.0, -180.0),
                (35.0, 139.0),
            ] {
                for offset in [-40.0, 0.0, 33.0] {
                    let camera =
                        Geodetic::from_degrees(latitude, longitude, top + offset).to_ecef();
                    assert_eq!(field_above_cloud(p, camera), offset > 32.0);
                    assert!(!field_above_cloud(unbounded, camera));
                    let mut visible = 0;
                    let mut custom_visible = 0;
                    for index in 0..384 {
                        let drop = particle(p, Seconds(17.0), index, camera).unwrap();
                        let custom = particle(unbounded, Seconds(17.0), index, camera).unwrap();
                        assert_eq!(drop.world.as_vec(), custom.world.as_vec());
                        if custom.alpha > 0.0 {
                            custom_visible += 1;
                        }
                        if drop.alpha > 0.0 {
                            visible += 1;
                            assert!(
                                drop.world.to_geodetic().altitude.get() + CEILING_PROXY_MARGIN
                                    <= top
                            );
                        }
                        if offset < -32.0 {
                            assert_eq!(drop.alpha.to_bits(), custom.alpha.to_bits());
                        }
                    }
                    assert!(custom_visible > 0);
                    if offset > 32.0 {
                        assert_eq!(visible, 0);
                    } else {
                        assert!(visible > 0);
                    }
                }
            }
        }
    }

    #[test]
    fn banked_mesh_vertices_stay_below_cloud_top_across_rebases() {
        for (preset, departure, latitude, longitude) in [
            (
                WeatherPreset::Rain,
                Geodetic::from_degrees(90.0, 180.0, 17.0),
                90.0,
                0.0,
            ),
            (
                WeatherPreset::Snow,
                Geodetic::from_degrees(-90.0, -180.0, -900.0),
                -90.0,
                0.0,
            ),
            (
                WeatherPreset::Storm,
                Geodetic::from_degrees(0.0, 180.0, 17.0),
                0.0,
                -180.0,
            ),
            (
                WeatherPreset::Rain,
                Geodetic::from_degrees(35.0, 139.0, 17.0),
                0.0,
                -180.0,
            ),
        ] {
            let mut p = WeatherScenario::from_preset(preset, departure, 71)
                .unwrap()
                .parameters();
            if preset == WeatherPreset::Snow {
                p.preset = WeatherPreset::Custom;
                p.cloud.as_mut().unwrap().base = Meters(-800.0);
                p.cloud.as_mut().unwrap().top = Meters(-400.0);
                p = WeatherScenario::try_from(p).unwrap().parameters();
            }
            let top = p.cloud.unwrap().top.get();
            for height in [top - 40.0, top, top + 20.0, top + 33.0] {
                let location = Geodetic::from_degrees(latitude, longitude, height);
                let camera_world = location.to_ecef();
                let shifted = LocalFrame::new(location)
                    .ned_to_ecef_position(Ned::new(500.0, 250.0, 100.0))
                    .to_geodetic();
                for rotation in [
                    glam::DQuat::IDENTITY,
                    glam::DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2),
                ] {
                    let mut previous: Option<Vec<Ecef>> = None;
                    for origin in [RenderOrigin::new(location), RenderOrigin::new(shifted)] {
                        let camera = Transform {
                            translation: origin.0.to_render(camera_world),
                            rotation: origin.0.rotation_to_render(rotation),
                            ..default()
                        };
                        let mut mesh = Mesh::new(
                            PrimitiveTopology::TriangleList,
                            RenderAssetUsages::MAIN_WORLD,
                        )
                        .with_inserted_attribute(
                            Mesh::ATTRIBUTE_POSITION,
                            vec![[0.0_f32; 3]; 128 * VERTICES],
                        );
                        // Off/Light have the largest geometric sample area.
                        update_mesh(&mut mesh, p, Seconds(17.0), &origin, &camera, 128);
                        let Some(VertexAttributeValues::Float32x3(positions)) =
                            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("position buffer");
                        };
                        let mut visible = 0;
                        for triangle in positions.chunks_exact(3) {
                            let [a, b, c] = [
                                Vec3::from(triangle[0]),
                                Vec3::from(triangle[1]),
                                Vec3::from(triangle[2]),
                            ];
                            if (b - a).cross(c - a).length_squared() == 0.0 {
                                continue;
                            }
                            visible += 1;
                            for vertex in [a, b, c] {
                                assert!(
                                    origin.0.to_world(vertex).to_geodetic().altitude.get() <= top
                                );
                            }
                        }
                        assert_eq!(visible > 0, height < top + 32.0);
                        let world: Vec<_> = positions
                            .iter()
                            .map(|v| origin.0.to_world(Vec3::from(*v)))
                            .collect();
                        if let Some(previous) = &previous {
                            for (before, after) in previous.iter().zip(&world) {
                                assert!(before.distance_to(*after).get() < 0.005);
                            }
                        }
                        previous = Some(world);
                    }
                }
            }
        }
    }
    #[test]
    fn phases_have_distinct_fall_speed_and_geometry() {
        let rain = parameters(WeatherPreset::Rain);
        let mut snow = rain;
        snow.precipitation_kind = PrecipitationKind::Snow;
        let camera = rain.departure_reference.to_ecef();
        let frame = LocalFrame::new(rain.departure_reference);
        let fall = |p| {
            let a = frame
                .ecef_to_ned_position(particle(p, Seconds(0.0), 0, camera).unwrap().world)
                .down();
            let b = frame
                .ecef_to_ned_position(particle(p, Seconds(0.1), 0, camera).unwrap().world)
                .down();
            (b - a).abs()
        };
        assert!(fall(rain) > fall(snow) * 4.0);
    }
}
