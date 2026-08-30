//! 決定論的な雲レイヤーと、雲中の視程。
//!
//! # M3 で扱う範囲
//!
//! ここで描く雲は、雲底と雲頂に置いた 2 枚の PBR 面である。高品質な
//! ボリューム雲ではない。形を表すマスクは、シード・位置・シミュレーション時刻
//! だけから作るため、同じ入力なら同じ雲になる。
//!
//! 雲の中では [`DistanceFog`] で地物のコントラストを落とす。視程から消散係数を
//! 求める式は Koschmieder の式で、Bevy の [`FogFalloff::from_visibility`] と同じ
//! 5% コントラスト閾値を使う。
//!
//! # 昼夜
//!
//! 雲面は `unlit` でも emissive でもない。太陽光と環境光で照らされるので、夜に
//! 自己発光しない。霧の色も [`crate::SunLighting`] の環境光から導く。
//! [`ClearColor`] は大気散乱の背景なので、天候からは変更しない。

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use flightsim_core::{Geodetic, Meters, Radians};
use glam::Affine2;
use std::error::Error;
use std::fmt;

use crate::{RenderOrigin, SunDirection, SunLighting, TimeOfDay};

/// 雲模様 1 周のおおよその大きさ。
///
/// 地球一周を整数個に分けているため、経度 ±180 度で位相が一致する。
const CLOUD_PATTERN_METRES: f64 = std::f64::consts::TAU * EARTH_RADIUS_METRES / 1024.0;
/// 雲面の一辺。巡航高度からの地平線（約 200 km）を余裕を持って覆う。
const CLOUD_DECK_SIZE_METRES: f32 = 600_000.0;
const CLOUD_TEXTURE_SIZE: u32 = 256;
const CLOUD_NOISE_PERIOD: i32 = 8;
const EARTH_RADIUS_METRES: f64 = 6_371_000.0;
const SECONDS_PER_DAY: f64 = 86_400.0;
/// 雲場の移流。物理風ではなく、時刻変化を目で読める程度の固定値。
const CLOUD_DRIFT_EAST_METRES_PER_SECOND: f64 = 8.0;
const CLOUD_DRIFT_NORTH_METRES_PER_SECOND: f64 = 2.0;
const CLOUD_EDGE_FADE_METRES: f64 = 100.0;
const MASK_SOFTNESS: f32 = 0.08;

/// 雲レイヤーの描画設定。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CloudLayer {
    /// 雲量。0 は快晴、1 は全面を覆う。
    pub cover: f32,
    /// 雲底の WGS84 楕円体高（metres）。
    pub base: Meters,
    /// 雲頂の WGS84 楕円体高（metres）。
    pub top: Meters,
    /// 雲中での気象視程（metres）。
    pub visibility: Meters,
    /// 雲模様を決める種。同じ種と入力なら同じ雲になる。
    pub seed: u64,
}

impl CloudLayer {
    /// 検証済みの雲レイヤーを作る。
    ///
    /// # Errors
    ///
    /// 非有限値、範囲外の雲量、負の雲底、雲底以下の雲頂、正でない視程を
    /// [`CloudLayerError`] として返す。
    pub fn try_new(
        cover: f32,
        base: Meters,
        top: Meters,
        visibility: Meters,
        seed: u64,
    ) -> Result<Self, CloudLayerError> {
        let layer = Self {
            cover,
            base,
            top,
            visibility,
            seed,
        };
        layer.validate()?;
        Ok(layer)
    }

    /// 現在の値が描画に使えるか検証する。
    ///
    /// フィールドは CLI の既定値を組み立てやすいよう公開しているため、生成後に
    /// 直接変更された場合も描画システムがこの検証を通す。
    pub fn validate(self) -> Result<(), CloudLayerError> {
        if !self.cover.is_finite() {
            return Err(CloudLayerError::NonFiniteCover);
        }
        if !(0.0..=1.0).contains(&self.cover) {
            return Err(CloudLayerError::CoverOutOfRange(self.cover));
        }
        if !self.base.get().is_finite() {
            return Err(CloudLayerError::NonFiniteBase);
        }
        if self.base.get() < 0.0 {
            return Err(CloudLayerError::NegativeBase(self.base));
        }
        if !self.top.get().is_finite() {
            return Err(CloudLayerError::NonFiniteTop);
        }
        if self.top.get() <= self.base.get() {
            return Err(CloudLayerError::TopNotAboveBase {
                base: self.base,
                top: self.top,
            });
        }
        if !self.visibility.get().is_finite() {
            return Err(CloudLayerError::NonFiniteVisibility);
        }
        if self.visibility.get() <= 0.0 {
            return Err(CloudLayerError::NonPositiveVisibility(self.visibility));
        }
        Ok(())
    }

    /// 雲が無いか。
    ///
    /// 壊れた設定も快晴として扱い、NaN を GPU へ渡さない。
    #[must_use]
    pub fn is_clear(self) -> bool {
        self.validate().is_err() || self.cover <= 0.0
    }
}

impl Default for CloudLayer {
    fn default() -> Self {
        Self {
            cover: 0.0,
            base: Meters(1_000.0),
            top: Meters(2_000.0),
            visibility: Meters(300.0),
            seed: 1,
        }
    }
}

/// [`CloudLayer`] の設定誤り。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloudLayerError {
    NonFiniteCover,
    CoverOutOfRange(f32),
    NonFiniteBase,
    NegativeBase(Meters),
    NonFiniteTop,
    TopNotAboveBase { base: Meters, top: Meters },
    NonFiniteVisibility,
    NonPositiveVisibility(Meters),
}

impl fmt::Display for CloudLayerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteCover => write!(formatter, "cloud cover must be finite"),
            Self::CoverOutOfRange(value) => {
                write!(
                    formatter,
                    "cloud cover must be between 0 and 1, got {value}"
                )
            }
            Self::NonFiniteBase => write!(formatter, "cloud base must be finite"),
            Self::NegativeBase(value) => {
                write!(formatter, "cloud base must not be negative, got {value}")
            }
            Self::NonFiniteTop => write!(formatter, "cloud top must be finite"),
            Self::TopNotAboveBase { base, top } => {
                write!(formatter, "cloud top {top} must be above cloud base {base}")
            }
            Self::NonFiniteVisibility => write!(formatter, "cloud visibility must be finite"),
            Self::NonPositiveVisibility(value) => {
                write!(formatter, "cloud visibility must be positive, got {value}")
            }
        }
    }
}

impl Error for CloudLayerError {}

/// 雲底または雲頂の描画面。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudDeckSurface {
    Base,
    Top,
}

impl CloudDeckSurface {
    fn altitude(self, layer: CloudLayer) -> Meters {
        match self {
            Self::Base => layer.base,
            Self::Top => layer.top,
        }
    }
}

/// 天候が所有するカメラ霧であることを示す印。
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CloudDistanceFog;

/// 雲面に使う GPU 資産と ECS entity。
#[derive(Resource, Debug, Default)]
pub(crate) struct CloudVisuals {
    entities: Vec<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    texture: Option<Handle<Image>>,
    initialised: bool,
}

/// 周期的な雲マスク。`u` と `v` は 1.0 で一周する場の座標。
///
/// 返り値は 0 から 1。同じ引数なら同じビットを返す。`cover` が増えても、同じ
/// 地点のマスクは減らない。
#[must_use]
pub fn cloud_mask(seed: u64, u: f64, v: f64, cover: f32) -> f32 {
    if !u.is_finite() || !v.is_finite() || !cover.is_finite() || cover <= 0.0 {
        return 0.0;
    }
    if cover >= 1.0 {
        return 1.0;
    }

    let noise = fractal_noise(seed, u, v);
    if !noise.is_finite() {
        return 0.0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "正規化した -1..=1 の noise を描画用の f32 へ変換する"
    )]
    let noise = noise.mul_add(0.5, 0.5) as f32;
    let threshold = 1.0 - cover;
    smoothstep_f32(threshold - MASK_SOFTNESS, threshold + MASK_SOFTNESS, noise)
}

/// 雲底・雲頂の境界を滑らかにした鉛直密度。
///
/// 境界そのものとレイヤー外では 0、十分内側では 1。薄いレイヤーでは厚さの
/// 4 分の 1 を遷移に使い、上下の遷移が潰し合わないようにする。
#[must_use]
pub fn vertical_cloud_density(altitude: Meters, base: Meters, top: Meters) -> f32 {
    let altitude = altitude.get();
    let base = base.get();
    let top = top.get();
    if !(altitude.is_finite() && base.is_finite() && top.is_finite() && top > base) {
        return 0.0;
    }
    if altitude <= base || altitude >= top {
        return 0.0;
    }

    let thickness = top - base;
    let fade = CLOUD_EDGE_FADE_METRES
        .min(thickness * 0.25)
        .max(f64::EPSILON);
    let from_base = smoothstep_f64(0.0, fade, altitude - base);
    let from_top = smoothstep_f64(0.0, fade, top - altitude);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "密度は 0..=1 に収まり、描画用の f32 で十分"
    )]
    {
        (from_base * from_top) as f32
    }
}

/// 指定した時刻・位置でカメラが受ける雲密度。
///
/// 壁時計も乱数器も参照しない。非有限な入力や壊れた設定では 0 を返す。
#[must_use]
pub fn cloud_density_at(layer: CloudLayer, clock: TimeOfDay, position: Geodetic) -> f32 {
    if layer.validate().is_err() {
        return 0.0;
    }
    let Some((u, v)) = cloud_field_coordinates(clock, position) else {
        return 0.0;
    };
    cloud_mask(layer.seed, u, v, layer.cover)
        * vertical_cloud_density(position.altitude, layer.base, layer.top)
}

/// 気象視程から消散係数を求める。
///
/// Koschmieder の式 `beta = -ln(0.05) / visibility`。5% は Bevy と同じ
/// revised contrast threshold。壊れた視程では 0 を返す。
#[must_use]
pub fn fog_extinction(visibility: Meters) -> f32 {
    let visibility = visibility.get();
    if !visibility.is_finite() || visibility <= 0.0 {
        return 0.0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "視程から得る小さな描画係数で、f32 の精度で十分"
    )]
    {
        (-0.05_f64.ln() / visibility) as f32
    }
}

/// 雲中視程を表すカメラ霧。
#[must_use]
pub fn cloud_distance_fog(
    layer: CloudLayer,
    local_density: f32,
    lighting: &SunLighting,
    sun_elevation: Radians,
) -> DistanceFog {
    let density = if local_density.is_finite() {
        local_density.clamp(0.0, 1.0)
    } else {
        0.0
    };
    DistanceFog {
        color: cloud_fog_color(lighting, sun_elevation, density),
        // 現在の太陽光源は大気圏外照度を夜も保持し、GPU の大気散乱側で地平線
        // 遮蔽する。DistanceFog の簡易 glow はその遮蔽を知らないため使わない。
        directional_light_color: Color::NONE,
        directional_light_exponent: 8.0,
        falloff: FogFalloff::Exponential {
            density: fog_extinction(layer.visibility),
        },
    }
}

/// 雲面を必要に応じて作り直す。
///
/// `MinimalPlugins` を使う純関数テストでは asset resource が無いため、optional と
/// している。通常の `DefaultPlugins` 構成では最初の Update で作られる。
pub(crate) fn sync_cloud_visuals(
    mut commands: Commands,
    layer: Res<CloudLayer>,
    mut visuals: ResMut<CloudVisuals>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    images: Option<ResMut<Assets<Image>>>,
) {
    if visuals.initialised && !layer.is_changed() {
        return;
    }

    let (Some(mut meshes), Some(mut materials), Some(mut images)) = (meshes, materials, images)
    else {
        // 快晴なら作る物が無いので、asset resource の無い最小 App でも完了扱いに
        // できる。曇天なら resource が現れるまで次フレームに再試行する。
        if layer.is_clear() {
            visuals.initialised = true;
        }
        return;
    };

    clear_cloud_visuals(
        &mut commands,
        &mut visuals,
        &mut meshes,
        &mut materials,
        &mut images,
    );
    visuals.initialised = true;

    if layer.is_clear() {
        return;
    }

    let texture = images.add(cloud_mask_image(*layer));
    let repeats = cloud_texture_repeats();
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.94, 0.98),
        base_color_texture: Some(texture.clone()),
        perceptual_roughness: 1.0,
        metallic: 0.0,
        double_sided: true,
        cull_mode: None,
        alpha_mode: AlphaMode::Mask(0.5),
        uv_transform: Affine2::from_scale(Vec2::new(repeats, -repeats)),
        ..default()
    });
    let mesh = meshes.add(Mesh::from(Plane3d::new(
        Vec3::Y,
        Vec2::splat(CLOUD_DECK_SIZE_METRES * 0.5),
    )));

    for surface in [CloudDeckSurface::Base, CloudDeckSurface::Top] {
        let entity = commands
            .spawn((
                surface,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Name::new(match surface {
                    CloudDeckSurface::Base => "cloud base",
                    CloudDeckSurface::Top => "cloud top",
                }),
            ))
            .id();
        visuals.entities.push(entity);
    }
    visuals.mesh = Some(mesh);
    visuals.material = Some(material);
    visuals.texture = Some(texture);
}

/// 雲面をカメラ付近へ保ち、雲模様を地球上の位置と時刻へ固定する。
pub(crate) fn update_cloud_visuals(
    layer: Res<CloudLayer>,
    clock: Res<TimeOfDay>,
    origin: Option<Res<RenderOrigin>>,
    cameras: Query<&Transform, (With<Camera3d>, Without<CloudDeckSurface>)>,
    mut surfaces: Query<(&CloudDeckSurface, &mut Transform), Without<Camera3d>>,
    visuals: Res<CloudVisuals>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    if layer.is_clear() {
        return;
    }
    let Some(origin) = origin else {
        return;
    };
    let Some(camera_position) = cameras
        .iter()
        .find_map(|transform| camera_geodetic(&origin, transform))
    else {
        return;
    };

    for (surface, mut transform) in &mut surfaces {
        let altitude = surface.altitude(*layer);
        let world = Geodetic::new(
            camera_position.latitude,
            camera_position.longitude,
            altitude,
        )
        .to_ecef();
        let local = origin.0.to_render(world);
        if local.is_finite() {
            transform.translation = local;
        }
    }

    let (Some(material), Some(mut materials), Some((u, v))) = (
        visuals.material.as_ref(),
        materials,
        cloud_field_coordinates(*clock, camera_position),
    ) else {
        return;
    };
    let Some(material) = materials.get_mut(material) else {
        return;
    };
    material.uv_transform = cloud_uv_transform(u, v);
}

/// カメラへ雲中の視程を反映する。
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Bevy system の resource と、相反する marker filter を明示する"
)]
pub(crate) fn update_cloud_distance_fog(
    mut commands: Commands,
    layer: Res<CloudLayer>,
    clock: Res<TimeOfDay>,
    origin: Option<Res<RenderOrigin>>,
    lighting: Res<SunLighting>,
    sun: Res<SunDirection>,
    mut owned_fog: Query<(&Transform, &mut DistanceFog), (With<Camera3d>, With<CloudDistanceFog>)>,
    cameras_without_fog: Query<(Entity, &Transform), (With<Camera3d>, Without<CloudDistanceFog>)>,
) {
    let Some(origin) = origin else {
        return;
    };

    for (transform, mut current) in &mut owned_fog {
        let local_density = camera_geodetic(&origin, transform)
            .map_or(0.0, |position| cloud_density_at(*layer, *clock, position));
        *current = cloud_distance_fog(*layer, local_density, &lighting, sun.elevation);
    }
    for (camera, transform) in &cameras_without_fog {
        let local_density = camera_geodetic(&origin, transform)
            .map_or(0.0, |position| cloud_density_at(*layer, *clock, position));
        let fog = cloud_distance_fog(*layer, local_density, &lighting, sun.elevation);
        commands.entity(camera).insert((CloudDistanceFog, fog));
    }
}

fn clear_cloud_visuals(
    commands: &mut Commands,
    visuals: &mut CloudVisuals,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    for entity in visuals.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    if let Some(mesh) = visuals.mesh.take() {
        meshes.remove(&mesh);
    }
    if let Some(material) = visuals.material.take() {
        materials.remove(&material);
    }
    if let Some(texture) = visuals.texture.take() {
        images.remove(&texture);
    }
}

fn cloud_mask_image(layer: CloudLayer) -> Image {
    let pixel_count = CLOUD_TEXTURE_SIZE as usize * CLOUD_TEXTURE_SIZE as usize;
    let mut data = Vec::with_capacity(pixel_count * 4);
    for y in 0..CLOUD_TEXTURE_SIZE {
        for x in 0..CLOUD_TEXTURE_SIZE {
            let u = f64::from(x) / f64::from(CLOUD_TEXTURE_SIZE);
            let v = f64::from(y) / f64::from(CLOUD_TEXTURE_SIZE);
            let mask = cloud_mask(layer.seed, u, v, layer.cover);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0..=1 のマスクを 8 bit alpha へ量子化する"
            )]
            let alpha = (mask * 255.0).round() as u8;
            data.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: CLOUD_TEXTURE_SIZE,
            height: CLOUD_TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

fn cloud_texture_repeats() -> f32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "数十 km の模様周期を描画用の f32 UV scale へ変換する"
    )]
    let pattern_metres = CLOUD_PATTERN_METRES as f32;
    CLOUD_DECK_SIZE_METRES / pattern_metres
}

fn cloud_uv_transform(u: f64, v: f64) -> Affine2 {
    let repeats = cloud_texture_repeats();
    // RenderFrame は +X が東、-Z が北。一方 Plane3d の UV は +X/+Z へ増える。
    // V を反転しないと、CPU の雲中判定と描画マスクが南北の鏡像になる。
    let scale = Vec2::new(repeats, -repeats);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "rem_euclid で 0..1 に収めた UV 位相"
    )]
    let phase = Vec2::new(u.rem_euclid(1.0) as f32, v.rem_euclid(1.0) as f32);
    Affine2::from_scale_angle_translation(scale, 0.0, phase - scale * 0.5)
}

fn cloud_fog_color(lighting: &SunLighting, elevation: Radians, density: f32) -> Color {
    let ambient = lighting.ambient(elevation);
    let span = lighting.daylight_ambient - lighting.night_ambient;
    let daylight = if span.is_finite() && span.abs() > f32::EPSILON {
        ((ambient.brightness - lighting.night_ambient) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // DistanceFog は物理光ではなく画面上の混色。夜も昼の灰色を足すと雲が自己発光
    // して見えるため、夜はほぼ黒、昼だけ明るい灰色へ上げる。
    let value = 0.004 + (0.58 - 0.004) * daylight;
    let tint = ambient.color.to_linear();
    Color::linear_rgba(
        tint.red * value,
        tint.green * value,
        tint.blue * value,
        density,
    )
}

fn cloud_field_coordinates(clock: TimeOfDay, position: Geodetic) -> Option<(f64, f64)> {
    let latitude = position.latitude.get();
    let longitude = position.longitude.get();
    let time = clock.utc.days_since_j2000() * SECONDS_PER_DAY;
    if !(latitude.is_finite()
        && longitude.is_finite()
        && position.altitude.get().is_finite()
        && time.is_finite())
    {
        return None;
    }

    // 雲模様に厳密な測地投影は要らない。経度方向を地球一周の整数分割にすることで
    // 日付変更線の位相だけは連続させ、局所では metre 規模の滑らかな場にする。
    let east = longitude * EARTH_RADIUS_METRES + time * CLOUD_DRIFT_EAST_METRES_PER_SECOND;
    let north = latitude * EARTH_RADIUS_METRES + time * CLOUD_DRIFT_NORTH_METRES_PER_SECOND;
    Some((east / CLOUD_PATTERN_METRES, north / CLOUD_PATTERN_METRES))
}

fn camera_geodetic(origin: &RenderOrigin, transform: &Transform) -> Option<Geodetic> {
    if !transform.translation.is_finite() {
        return None;
    }
    let position = origin.0.to_world(transform.translation).to_geodetic();
    if position.latitude.get().is_finite()
        && position.longitude.get().is_finite()
        && position.altitude.get().is_finite()
    {
        Some(position)
    } else {
        None
    }
}

fn fractal_noise(seed: u64, u: f64, v: f64) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut normalisation = 0.0;
    let mut frequency = 1_i32;
    for octave in 0..3_u32 {
        let period = CLOUD_NOISE_PERIOD * frequency;
        total += amplitude
            * value_noise_periodic(
                seed ^ (0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(u64::from(octave) + 1)),
                u * f64::from(period),
                v * f64::from(period),
                period,
            );
        normalisation += amplitude;
        amplitude *= 0.5;
        frequency *= 2;
    }
    total / normalisation
}

fn value_noise_periodic(seed: u64, x: f64, y: f64, period: i32) -> f64 {
    // 先に 1 周へ畳む。public API に極端に大きい座標が来ても、その値を直接
    // 整数へ cast せず、必ず小さい有限範囲に収める。
    let period_f64 = f64::from(period);
    let x = x.rem_euclid(period_f64);
    let y = y.rem_euclid(period_f64);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "rem_euclid 後は 0..period（最大 32）なので i64 に正確に収まる"
    )]
    let x0 = x.floor() as i64;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "rem_euclid 後は 0..period（最大 32）なので i64 に正確に収まる"
    )]
    let y0 = y.floor() as i64;
    let sx = smoothstep_f64(0.0, 1.0, x - x.floor());
    let sy = smoothstep_f64(0.0, 1.0, y - y.floor());
    let sample = |dx: i64, dy: i64| {
        lattice(
            seed,
            (x0 + dx).rem_euclid(i64::from(period)),
            (y0 + dy).rem_euclid(i64::from(period)),
        )
    };
    let low = sample(0, 0) + (sample(1, 0) - sample(0, 0)) * sx;
    let high = sample(0, 1) + (sample(1, 1) - sample(0, 1)) * sx;
    low + (high - low) * sy
}

fn lattice(seed: u64, x: i64, y: i64) -> f64 {
    let mut hash = seed;
    for component in [x, y] {
        #[allow(
            clippy::cast_sign_loss,
            reason = "ハッシュ入力として整数のビット列を使う"
        )]
        let bits = component as u64;
        hash = hash.wrapping_add(bits).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        hash ^= hash >> 31;
        hash = hash.wrapping_mul(0x94D0_49BB_1331_11EB);
        hash ^= hash >> 29;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "上位 53 bit は f64 の仮数に正確に収まる"
    )]
    let unit = (hash >> 11) as f64 / (1_u64 << 53) as f64;
    unit.mul_add(2.0, -1.0)
}

fn smoothstep_f64(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep_f32(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UtcDateTime;
    use flightsim_core::Degrees;

    fn layer(cover: f32) -> CloudLayer {
        CloudLayer::try_new(cover, Meters(1_000.0), Meters(2_000.0), Meters(300.0), 1)
            .expect("valid cloud layer")
    }

    fn noon() -> TimeOfDay {
        TimeOfDay::new(UtcDateTime::new(2026, 6, 21, 3, 0, 0.0).to_julian_date())
    }

    #[test]
    fn the_default_is_clear_but_keeps_useful_layer_defaults() {
        let default = CloudLayer::default();
        assert!(default.is_clear());
        assert_eq!(default.base, Meters(1_000.0));
        assert_eq!(default.top, Meters(2_000.0));
        assert_eq!(default.visibility, Meters(300.0));
        assert_eq!(default.seed, 1);
        assert!(default.validate().is_ok());
    }

    #[test]
    fn cover_accepts_both_boundaries() {
        assert!(layer(0.0).validate().is_ok());
        assert!(layer(1.0).validate().is_ok());
    }

    #[test]
    fn broken_layer_values_are_rejected() {
        for cover in [-0.001, 1.001, f32::NAN, f32::INFINITY] {
            assert!(
                CloudLayer::try_new(cover, Meters(1_000.0), Meters(2_000.0), Meters(300.0), 1,)
                    .is_err(),
                "cover {cover} was accepted"
            );
        }
        assert!(
            CloudLayer::try_new(0.5, Meters(-1.0), Meters(2_000.0), Meters(300.0), 1,).is_err()
        );
        assert!(
            CloudLayer::try_new(0.5, Meters(1_000.0), Meters(1_000.0), Meters(300.0), 1,).is_err()
        );
        assert!(
            CloudLayer::try_new(0.5, Meters(1_000.0), Meters(2_000.0), Meters(0.0), 1,).is_err()
        );
    }

    #[test]
    fn the_mask_is_deterministic_and_periodic() {
        let a = cloud_mask(42, 0.314, -0.271, 0.55);
        let b = cloud_mask(42, 0.314, -0.271, 0.55);
        assert_eq!(a.to_bits(), b.to_bits());
        assert_eq!(a.to_bits(), cloud_mask(42, 1.314, -2.271, 0.55).to_bits());
    }

    #[test]
    fn cover_boundaries_mean_clear_and_overcast_everywhere() {
        for (u, v) in [(0.0, 0.0), (0.25, 0.75), (-10.2, 31.7)] {
            assert_eq!(cloud_mask(1, u, v, 0.0).to_bits(), 0.0_f32.to_bits());
            assert_eq!(cloud_mask(1, u, v, 1.0).to_bits(), 1.0_f32.to_bits());
        }
    }

    #[test]
    fn increasing_cover_never_clears_a_point() {
        for y in 0..32 {
            for x in 0..32 {
                let u = f64::from(x) / 32.0;
                let v = f64::from(y) / 32.0;
                let mut previous = 0.0;
                for cover in [0.0, 0.2, 0.5, 0.8, 1.0] {
                    let current = cloud_mask(7, u, v, cover);
                    assert!(current >= previous, "mask fell at {u}, {v}");
                    previous = current;
                }
            }
        }
    }

    #[test]
    fn cloud_uv_uses_render_frames_negative_z_as_north() {
        let repeats = cloud_texture_repeats();
        let transform = cloud_uv_transform(0.25, 0.75);
        let centre = transform.transform_point2(Vec2::splat(0.5));
        let one_pattern = 1.0 / repeats;
        let east = transform.transform_point2(Vec2::new(0.5 + one_pattern, 0.5));
        let north = transform.transform_point2(Vec2::new(0.5, 0.5 - one_pattern));

        assert!((centre - Vec2::new(0.25, 0.75)).length() < 1e-5);
        assert!((east - centre - Vec2::X).length() < 1e-5);
        assert!((north - centre - Vec2::Y).length() < 1e-5);
    }

    #[test]
    fn vertical_density_fades_at_both_boundaries() {
        let base = Meters(1_000.0);
        let top = Meters(2_000.0);
        assert_eq!(
            vertical_cloud_density(Meters(999.0), base, top).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            vertical_cloud_density(base, base, top).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            vertical_cloud_density(top, base, top).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            vertical_cloud_density(Meters(2_001.0), base, top).to_bits(),
            0.0_f32.to_bits()
        );
        assert!(vertical_cloud_density(Meters(1_050.0), base, top) > 0.0);
        assert!((vertical_cloud_density(Meters(1_500.0), base, top) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn density_uses_only_layer_time_and_position() {
        let layer = layer(0.65);
        let position = Geodetic::from_degrees(35.0, 139.0, 1_500.0);
        let a = cloud_density_at(layer, noon(), position);
        let b = cloud_density_at(layer, noon(), position);
        assert_eq!(a.to_bits(), b.to_bits());
        assert!(a.is_finite() && (0.0..=1.0).contains(&a));
    }

    #[test]
    fn koschmieder_visibility_reaches_five_percent_contrast() {
        let visibility = Meters(300.0);
        let beta = fog_extinction(visibility);
        let contrast = (-f64::from(beta) * visibility.get()).exp();
        assert!(
            (contrast - 0.05).abs() < 1e-6,
            "300 m retained contrast {contrast}"
        );
    }

    #[test]
    fn fog_is_transparent_outside_cloud_and_dark_at_night() {
        let lighting = SunLighting::default();
        let clear = cloud_distance_fog(layer(0.5), 0.0, &lighting, Degrees(45.0).to_radians());
        assert_eq!(clear.color.alpha().to_bits(), 0.0_f32.to_bits());

        let day = cloud_distance_fog(layer(0.5), 1.0, &lighting, Degrees(45.0).to_radians())
            .color
            .to_linear();
        let night = cloud_distance_fog(layer(0.5), 1.0, &lighting, Degrees(-30.0).to_radians())
            .color
            .to_linear();
        assert_eq!(day.alpha.to_bits(), 1.0_f32.to_bits());
        assert_eq!(night.alpha.to_bits(), 1.0_f32.to_bits());
        assert!(night.red < day.red * 0.1, "night fog glowed: {night:?}");
        assert!(night.blue < day.blue * 0.1, "night fog glowed: {night:?}");
    }

    #[test]
    fn non_finite_inputs_do_not_escape_as_density() {
        assert_eq!(
            cloud_mask(1, f64::NAN, 0.0, 0.5).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            cloud_mask(1, f64::MAX, f64::MAX, 0.5).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            fog_extinction(Meters(f64::NAN)).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            vertical_cloud_density(Meters(f64::INFINITY), Meters(0.0), Meters(1.0)).to_bits(),
            0.0_f32.to_bits()
        );
    }
}
