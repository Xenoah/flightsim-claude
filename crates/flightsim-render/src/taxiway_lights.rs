//! 誘導路灯・滑走路警戒灯の配置と色別メッシュ。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters};
use flightsim_world::{GroundLightKind, TaxiwayLighting};

use crate::runway_lights::AirportLights;

const LIGHT_LIFT: f64 = 0.12;
const LIGHT_SIZE: f64 = 1.35;
const EDGE_MARGIN: f64 = 0.75;
const EDGE_SPACING: f64 = 60.0;
const CENTRE_SPACING: f64 = 30.0;
const EMISSIVE_STRENGTH: f32 = 6_000.0;
const BLUE: [f32; 3] = [0.08, 0.30, 1.0];
const GREEN: [f32; 3] = [0.08, 1.0, 0.24];
const YELLOW: [f32; 3] = [1.0, 0.68, 0.04];

pub const MAX_TAXIWAY_LIGHT_PATH_POINTS: usize = 4_096;
pub const MAX_GROUND_LIGHTS: usize = 100_000;
const MAX_GROUND_LIGHTS_F64: f64 = 100_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxiwayLightError {
    InvalidPath,
    InvalidWidth,
    InvalidLight,
    TooManyLights,
    AllocationFailed,
}

impl core::fmt::Display for TaxiwayLightError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "taxiway light path is invalid",
            Self::InvalidWidth => "taxiway light width is invalid",
            Self::InvalidLight => "ground light position is invalid",
            Self::TooManyLights => "ground light count exceeds the safe limit",
            Self::AllocationFailed => "could not allocate ground light storage",
        })
    }
}

impl std::error::Error for TaxiwayLightError {}

/// 一色・一種別へまとめた灯火メッシュ。
#[derive(Debug)]
pub struct GroundLightMeshGroup {
    pub kind: GroundLightKind,
    pub color: [f32; 3],
    pub mesh: Mesh,
    pub material: StandardMaterial,
    pub marker: AirportLights,
}

/// 誘導路中心線から決定論的な縁灯・中心線灯を作る。
///
/// `points` は各 node で DEM 標高を反映済みであること。配置間隔は way 全体の
/// 弧長に対して連続で、閉路は始点を末尾へ重複配置しない。
pub fn procedural_taxiway_light_layout(
    points: &[Geodetic],
    width: Meters,
    lighting: TaxiwayLighting,
) -> Result<Vec<(Geodetic, GroundLightKind)>, TaxiwayLightError> {
    if points.len() < 2
        || points.len() > MAX_TAXIWAY_LIGHT_PATH_POINTS
        || points.iter().any(|point| !valid_point(*point))
    {
        return Err(TaxiwayLightError::InvalidPath);
    }
    if !width.is_finite() || width.get() <= 0.0 || width.get() > 200.0 {
        return Err(TaxiwayLightError::InvalidWidth);
    }
    if lighting == TaxiwayLighting::None {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    segments
        .try_reserve_exact(points.len() - 1)
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;
    let mut total = 0.0_f64;
    for pair in points.windows(2) {
        let displacement = LocalFrame::new(pair[0]).ecef_to_ned_position(pair[1].to_ecef());
        let length = displacement.horizontal_magnitude();
        if !length.is_finite() || length <= f64::EPSILON {
            return Err(TaxiwayLightError::InvalidPath);
        }
        total += length;
        if !total.is_finite() {
            return Err(TaxiwayLightError::InvalidPath);
        }
        segments.push(PathSegment {
            start: pair[0],
            end_altitude: pair[1].altitude.get(),
            start_distance: total - length,
            length,
            heading: displacement.bearing(),
        });
    }
    let closed = points[0]
        .to_ecef()
        .distance_to(points[points.len() - 1].to_ecef())
        .get()
        <= 0.01;

    let edge = matches!(
        lighting,
        TaxiwayLighting::Edge | TaxiwayLighting::EdgeAndCenterline
    );
    let centre = matches!(
        lighting,
        TaxiwayLighting::Centerline | TaxiwayLighting::EdgeAndCenterline
    );
    let edge_samples = if edge {
        sample_count(total, EDGE_SPACING, closed)?
            .checked_mul(2)
            .ok_or(TaxiwayLightError::TooManyLights)?
    } else {
        0
    };
    let centre_samples = if centre {
        sample_count(total, CENTRE_SPACING, closed)?
    } else {
        0
    };
    let count = edge_samples
        .checked_add(centre_samples)
        .ok_or(TaxiwayLightError::TooManyLights)?;
    if count > MAX_GROUND_LIGHTS {
        return Err(TaxiwayLightError::TooManyLights);
    }
    let mut lights = Vec::new();
    lights
        .try_reserve_exact(count)
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;

    if edge {
        append_samples(
            &mut lights,
            &segments,
            total,
            EDGE_SPACING,
            closed,
            Some(width.get() * 0.5 + EDGE_MARGIN),
            GroundLightKind::TaxiwayEdge,
        )?;
    }
    if centre {
        append_samples(
            &mut lights,
            &segments,
            total,
            CENTRE_SPACING,
            closed,
            None,
            GroundLightKind::TaxiwayCenterline,
        )?;
    }
    debug_assert_eq!(lights.len(), count);
    Ok(lights)
}

/// DEM 標高付きの明示灯火を、種別（すなわち色）ごとの mesh へまとめる。
///
/// 空入力は原点も定義できないため `Ok(None)`。灯火ごとの entity は作らない。
pub fn ground_light_meshes(
    lights: &[(Geodetic, GroundLightKind)],
) -> Result<Option<(Vec<GroundLightMeshGroup>, Ecef)>, TaxiwayLightError> {
    if lights.is_empty() {
        return Ok(None);
    }
    if lights.len() > MAX_GROUND_LIGHTS {
        return Err(TaxiwayLightError::TooManyLights);
    }
    if lights.iter().any(|(point, _)| !valid_point(*point)) {
        return Err(TaxiwayLightError::InvalidLight);
    }

    let origin = lifted(lights[0].0).to_ecef();
    let kinds = [
        GroundLightKind::TaxiwayEdge,
        GroundLightKind::TaxiwayCenterline,
        GroundLightKind::RunwayGuard,
    ];
    let mut groups = Vec::new();
    groups
        .try_reserve_exact(kinds.len())
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;
    for kind in kinds {
        let count = lights
            .iter()
            .filter(|(_, light_kind)| *light_kind == kind)
            .count();
        if count == 0 {
            continue;
        }
        let color = light_color(kind);
        let mesh = light_mesh(
            origin,
            lights
                .iter()
                .filter_map(|(point, light_kind)| (*light_kind == kind).then_some(*point)),
            count,
        )?;
        let emissive = LinearRgba::rgb(
            crate::srgb_to_linear(color[0]) * EMISSIVE_STRENGTH,
            crate::srgb_to_linear(color[1]) * EMISSIVE_STRENGTH,
            crate::srgb_to_linear(color[2]) * EMISSIVE_STRENGTH,
        );
        groups.push(GroundLightMeshGroup {
            kind,
            color,
            mesh,
            material: StandardMaterial {
                base_color: Color::BLACK,
                emissive,
                ..default()
            },
            marker: AirportLights {
                full_emissive: emissive,
            },
        });
    }
    Ok(Some((groups, origin)))
}

#[derive(Debug, Clone, Copy)]
struct PathSegment {
    start: Geodetic,
    end_altitude: f64,
    start_distance: f64,
    length: f64,
    heading: flightsim_core::Radians,
}

fn sample_count(total: f64, spacing: f64, closed: bool) -> Result<usize, TaxiwayLightError> {
    let intervals = (total / spacing).ceil().max(1.0);
    if !intervals.is_finite() || intervals > MAX_GROUND_LIGHTS_F64 {
        return Err(TaxiwayLightError::TooManyLights);
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "有限かつ MAX_GROUND_LIGHTS 以下へ検査済み"
    )]
    let intervals = intervals as usize;
    Ok(if closed { intervals } else { intervals + 1 })
}

#[allow(clippy::too_many_arguments, reason = "配置種別と横位置を明示する")]
fn append_samples(
    output: &mut Vec<(Geodetic, GroundLightKind)>,
    segments: &[PathSegment],
    total: f64,
    spacing: f64,
    closed: bool,
    edge_offset: Option<f64>,
    kind: GroundLightKind,
) -> Result<(), TaxiwayLightError> {
    let count = sample_count(total, spacing, closed)?;
    let intervals = if closed { count } else { count - 1 };
    let intervals_u32 = u32::try_from(intervals).map_err(|_| TaxiwayLightError::TooManyLights)?;
    let step = total / f64::from(intervals_u32);
    let mut segment_index = 0_usize;
    for index in 0..count {
        let index = u32::try_from(index).map_err(|_| TaxiwayLightError::TooManyLights)?;
        let distance = f64::from(index) * step;
        while segment_index + 1 < segments.len()
            && distance > segments[segment_index].start_distance + segments[segment_index].length
        {
            segment_index += 1;
        }
        let segment = segments[segment_index];
        let along = (distance - segment.start_distance).clamp(0.0, segment.length);
        let fraction = along / segment.length;
        let altitude = segment.start.altitude.get()
            + (segment.end_altitude - segment.start.altitude.get()) * fraction;
        let centre = path_point(segment.start, segment.heading, along, 0.0, altitude);
        if let Some(offset) = edge_offset {
            for across in [-offset, offset] {
                output.push((
                    path_point(centre, segment.heading, 0.0, across, altitude),
                    kind,
                ));
            }
        } else {
            output.push((centre, kind));
        }
    }
    Ok(())
}

fn path_point(
    start: Geodetic,
    heading: flightsim_core::Radians,
    along: f64,
    across: f64,
    altitude: f64,
) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    let north = along * cos - across * sin;
    let east = along * sin + across * cos;
    let point = start.offset_by(Meters(north), Meters(east));
    Geodetic::new(point.latitude, point.longitude, Meters(altitude))
}

fn light_mesh(
    origin: Ecef,
    points: impl Iterator<Item = Geodetic>,
    count: usize,
) -> Result<Mesh, TaxiwayLightError> {
    let vertex_count = count
        .checked_mul(4)
        .ok_or(TaxiwayLightError::TooManyLights)?;
    let index_count = count
        .checked_mul(6)
        .ok_or(TaxiwayLightError::TooManyLights)?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    positions
        .try_reserve_exact(vertex_count)
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;
    normals
        .try_reserve_exact(vertex_count)
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;
    indices
        .try_reserve_exact(index_count)
        .map_err(|_| TaxiwayLightError::AllocationFailed)?;
    let half = LIGHT_SIZE * 0.5;
    for point in points {
        let base = u32::try_from(positions.len()).map_err(|_| TaxiwayLightError::TooManyLights)?;
        for (north, east) in [(-half, -half), (-half, half), (half, half), (half, -half)] {
            let corner = lifted(point.offset_by(Meters(north), Meters(east)));
            let relative = corner.to_ecef().as_vec() - origin.as_vec();
            let up = LocalFrame::new(corner).up_ecef();
            if !relative.is_finite() || !up.is_finite() {
                return Err(TaxiwayLightError::InvalidLight);
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "空港内の原点相対位置と単位法線は f32 で十分"
            )]
            {
                positions.push([relative.x as f32, relative.y as f32, relative.z as f32]);
                normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
        }
        // 北 x 東は下向きなので反転する。
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    Ok(Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices)))
}

const fn light_color(kind: GroundLightKind) -> [f32; 3] {
    match kind {
        GroundLightKind::TaxiwayEdge => BLUE,
        GroundLightKind::TaxiwayCenterline => GREEN,
        GroundLightKind::RunwayGuard => YELLOW,
    }
}

fn lifted(point: Geodetic) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + LIGHT_LIFT),
    )
}

fn valid_point(point: Geodetic) -> bool {
    point.latitude.is_finite()
        && point.longitude.is_finite()
        && point.altitude.is_finite()
        && point.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && point.longitude.get().abs() <= core::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_path() -> Vec<Geodetic> {
        let start = Geodetic::from_degrees(35.0, 139.0, 8.0);
        vec![
            start,
            start.offset_by(Meters(0.0), Meters(95.0)),
            start.offset_by(Meters(80.0), Meters(95.0)),
        ]
    }

    #[test]
    fn procedural_both_mode_has_blue_edges_and_green_centres() {
        let lights = procedural_taxiway_light_layout(
            &open_path(),
            Meters(18.0),
            TaxiwayLighting::EdgeAndCenterline,
        )
        .expect("valid path");
        assert!(
            lights
                .iter()
                .any(|(_, kind)| *kind == GroundLightKind::TaxiwayEdge)
        );
        assert!(
            lights
                .iter()
                .any(|(_, kind)| *kind == GroundLightKind::TaxiwayCenterline)
        );
        assert!(
            !lights
                .iter()
                .any(|(_, kind)| *kind == GroundLightKind::RunwayGuard)
        );
    }

    #[test]
    fn none_mode_produces_no_lights() {
        assert!(
            procedural_taxiway_light_layout(&open_path(), Meters(18.0), TaxiwayLighting::None,)
                .expect("valid path")
                .is_empty()
        );
    }

    #[test]
    fn closed_loop_does_not_duplicate_its_start() {
        let start = Geodetic::from_degrees(35.0, 139.0, 8.0);
        let east = start.offset_by(Meters(0.0), Meters(100.0));
        let north_east = east.offset_by(Meters(100.0), Meters(0.0));
        let north = start.offset_by(Meters(100.0), Meters(0.0));
        let lights = procedural_taxiway_light_layout(
            &[start, east, north_east, north, start],
            Meters(18.0),
            TaxiwayLighting::Centerline,
        )
        .expect("closed path");
        for (index, (point, _)) in lights.iter().enumerate() {
            assert!(
                !lights[..index].iter().any(|(previous, _)| {
                    previous.to_ecef().distance_to(point.to_ecef()).get() < 0.01
                }),
                "duplicated closed-loop light at {index}"
            );
        }
    }

    #[test]
    fn explicit_lights_are_grouped_into_three_meshes() {
        let point = Geodetic::from_degrees(35.0, 139.0, 8.0);
        let lights = [
            (point, GroundLightKind::TaxiwayEdge),
            (
                point.offset_by(Meters(2.0), Meters(0.0)),
                GroundLightKind::TaxiwayCenterline,
            ),
            (
                point.offset_by(Meters(4.0), Meters(0.0)),
                GroundLightKind::RunwayGuard,
            ),
            (
                point.offset_by(Meters(6.0), Meters(0.0)),
                GroundLightKind::TaxiwayEdge,
            ),
        ];
        let (groups, _) = ground_light_meshes(&lights)
            .expect("valid lights")
            .expect("non-empty");
        assert_eq!(groups.len(), 3);
        assert_eq!(
            groups
                .iter()
                .map(|group| group.mesh.count_vertices())
                .sum::<usize>(),
            16
        );
        assert!(groups.iter().all(|group| {
            let base = group.material.base_color.to_linear();
            base.red + base.green + base.blue < 1e-6
        }));
    }

    #[test]
    fn light_marker_recovers_after_full_daylight_off() {
        let point = Geodetic::from_degrees(35.0, 139.0, 8.0);
        let (groups, _) = ground_light_meshes(&[(point, GroundLightKind::TaxiwayEdge)])
            .expect("valid")
            .expect("non-empty");
        let marker = groups[0].marker;
        let off = marker.emissive_at(0.0);
        assert!(off.red + off.green + off.blue < 1e-6);
        let back = marker.emissive_at(1.0);
        assert!((back.blue - marker.full_emissive.blue).abs() < 1e-3);
    }

    #[test]
    fn invalid_and_excessive_inputs_are_rejected() {
        assert_eq!(
            procedural_taxiway_light_layout(&open_path(), Meters(f64::NAN), TaxiwayLighting::Edge,)
                .expect_err("width"),
            TaxiwayLightError::InvalidWidth
        );
        let point = Geodetic::from_degrees(0.0, 0.0, 0.0);
        let lights = vec![(point, GroundLightKind::TaxiwayEdge); MAX_GROUND_LIGHTS + 1];
        assert_eq!(
            ground_light_meshes(&lights).expect_err("cap"),
            TaxiwayLightError::TooManyLights
        );
    }
}
