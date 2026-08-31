//! 滑走路待機位置の路面標示。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Radians};
use flightsim_world::RunwaySide;

const MARKING_LIFT: f64 = 0.115;
const BAR_WIDTH: f64 = 0.30;
const BAR_CENTRES: [f64; 4] = [-0.75, -0.25, 0.25, 0.75];
const DASH_PITCH: f64 = 2.5;
const DASH_FRACTION: f64 = 0.62;
const YELLOW: [f32; 3] = [0.95, 0.69, 0.04];
const MAX_TAXIWAY_WIDTH: f64 = 200.0;
const MAX_MARKING_QUADS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingMarkingError {
    InvalidPosition,
    InvalidHeading,
    InvalidWidth,
    TooComplex,
    AllocationFailed,
}

impl core::fmt::Display for HoldingMarkingError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPosition => "holding marking position is invalid",
            Self::InvalidHeading => "holding marking heading is invalid",
            Self::InvalidWidth => "holding marking taxiway width is invalid",
            Self::TooComplex => "holding marking exceeds the quad limit",
            Self::AllocationFailed => "could not allocate holding marking mesh",
        })
    }
}

impl std::error::Error for HoldingMarkingError {}

/// 二本の実線と二本の破線からなる滑走路待機位置標示を作る。
///
/// `Forward` は方位の正方向を滑走路側、`Backward` は負方向を滑走路側とする。
/// `Unknown` では実線側を捏造せず `Ok(None)` として標示を省略する。
///
/// # Errors
///
/// 座標・方位・幅が不正、上限超過、または allocation failure の場合。
pub fn holding_position_mesh(
    position: Geodetic,
    taxiway_heading: Radians,
    taxiway_width: Meters,
    runway_side: RunwaySide,
) -> Result<Option<(Mesh, Ecef)>, HoldingMarkingError> {
    if !valid_point(position) {
        return Err(HoldingMarkingError::InvalidPosition);
    }
    if !taxiway_heading.is_finite() {
        return Err(HoldingMarkingError::InvalidHeading);
    }
    let width = taxiway_width.get();
    if !width.is_finite() || width <= 0.0 || width > MAX_TAXIWAY_WIDTH {
        return Err(HoldingMarkingError::InvalidWidth);
    }
    if runway_side == RunwaySide::Unknown {
        return Ok(None);
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "width は 0..=200 m に検査済み"
    )]
    let dash_count = (width / DASH_PITCH).ceil().max(1.0) as usize;
    let dash_count_u32 = u32::try_from(dash_count).map_err(|_| HoldingMarkingError::TooComplex)?;
    let quad_count = 2_usize
        .checked_add(
            dash_count
                .checked_mul(2)
                .ok_or(HoldingMarkingError::TooComplex)?,
        )
        .ok_or(HoldingMarkingError::TooComplex)?;
    if quad_count > MAX_MARKING_QUADS {
        return Err(HoldingMarkingError::TooComplex);
    }

    let origin_point = lifted(position, MARKING_LIFT);
    let origin = origin_point.to_ecef();
    let mut builder = MarkingMeshBuilder::new(origin, quad_count)?;
    let forward_is_runway = runway_side == RunwaySide::Forward;
    for (index, along) in BAR_CENTRES.into_iter().enumerate() {
        let positive_side = along > 0.0;
        let dashed = positive_side == forward_is_runway;
        if dashed {
            let cell = width / f64::from(dash_count_u32);
            let dash = cell * DASH_FRACTION;
            for dash_index in 0..dash_count_u32 {
                let centre = -width * 0.5 + (f64::from(dash_index) + 0.5) * cell;
                builder.quad(
                    position,
                    taxiway_heading,
                    along - BAR_WIDTH * 0.5,
                    along + BAR_WIDTH * 0.5,
                    centre - dash * 0.5,
                    centre + dash * 0.5,
                )?;
            }
        } else {
            builder.quad(
                position,
                taxiway_heading,
                along - BAR_WIDTH * 0.5,
                along + BAR_WIDTH * 0.5,
                -width * 0.5,
                width * 0.5,
            )?;
        }
        debug_assert!(index < BAR_CENTRES.len());
    }

    Ok(Some((builder.build(), origin)))
}

struct MarkingMeshBuilder {
    origin: Ecef,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MarkingMeshBuilder {
    fn new(origin: Ecef, quads: usize) -> Result<Self, HoldingMarkingError> {
        let vertices = quads
            .checked_mul(4)
            .ok_or(HoldingMarkingError::TooComplex)?;
        let index_count = quads
            .checked_mul(6)
            .ok_or(HoldingMarkingError::TooComplex)?;
        let mut result = Self {
            origin,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        };
        result
            .positions
            .try_reserve_exact(vertices)
            .map_err(|_| HoldingMarkingError::AllocationFailed)?;
        result
            .normals
            .try_reserve_exact(vertices)
            .map_err(|_| HoldingMarkingError::AllocationFailed)?;
        result
            .colors
            .try_reserve_exact(vertices)
            .map_err(|_| HoldingMarkingError::AllocationFailed)?;
        result
            .indices
            .try_reserve_exact(index_count)
            .map_err(|_| HoldingMarkingError::AllocationFailed)?;
        Ok(result)
    }

    fn quad(
        &mut self,
        centre: Geodetic,
        heading: Radians,
        along_near: f64,
        along_far: f64,
        across_left: f64,
        across_right: f64,
    ) -> Result<(), HoldingMarkingError> {
        let points = [
            surface_point(centre, heading, along_near, across_left),
            surface_point(centre, heading, along_far, across_left),
            surface_point(centre, heading, along_far, across_right),
            surface_point(centre, heading, along_near, across_right),
        ];
        let base =
            u32::try_from(self.positions.len()).map_err(|_| HoldingMarkingError::TooComplex)?;
        let linear = Color::srgb(YELLOW[0], YELLOW[1], YELLOW[2]).to_linear();
        for point in points {
            let relative = point.to_ecef().as_vec() - self.origin.as_vec();
            let up = LocalFrame::new(point).up_ecef();
            if !relative.is_finite() || !up.is_finite() {
                return Err(HoldingMarkingError::InvalidPosition);
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "空港面内の原点相対位置と単位法線は f32 で十分"
            )]
            {
                self.positions
                    .push([relative.x as f32, relative.y as f32, relative.z as f32]);
                self.normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
            self.colors
                .push([linear.red, linear.green, linear.blue, 1.0]);
        }
        // 前方 x 右方は下向きなので、右方 x 前方の順にして上へ向ける。
        self.indices
            .extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        Ok(())
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn surface_point(centre: Geodetic, heading: Radians, along: f64, across: f64) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    let north = along * cos - across * sin;
    let east = along * sin + across * cos;
    lifted(centre.offset_by(Meters(north), Meters(east)), MARKING_LIFT)
}

fn lifted(point: Geodetic, amount: f64) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + amount),
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
    use bevy::mesh::VertexAttributeValues;
    use flightsim_core::Degrees;

    fn sample(side: RunwaySide) -> Result<Option<(Mesh, Ecef)>, HoldingMarkingError> {
        holding_position_mesh(
            Geodetic::from_degrees(35.0, 139.0, 8.0),
            Degrees(30.0).to_radians(),
            Meters(20.0),
            side,
        )
    }

    #[test]
    fn unknown_runway_side_is_neutral_omission() {
        assert!(
            sample(RunwaySide::Unknown)
                .expect("valid geometry")
                .is_none()
        );
    }

    #[test]
    fn two_solid_and_two_dashed_bars_are_built() {
        let (mesh, _) = sample(RunwaySide::Forward)
            .expect("valid geometry")
            .expect("known side");
        let dash_count = core::iter::successors(Some(0.0_f64), |offset| Some(*offset + DASH_PITCH))
            .take_while(|offset| *offset < 20.0)
            .count();
        assert_eq!(mesh.count_vertices(), (2 + 2 * dash_count) * 4);
    }

    #[test]
    fn changing_runway_side_mirrors_but_preserves_complexity() {
        let (forward, _) = sample(RunwaySide::Forward)
            .expect("forward")
            .expect("known");
        let (backward, _) = sample(RunwaySide::Backward)
            .expect("backward")
            .expect("known");
        assert_eq!(forward.count_vertices(), backward.count_vertices());
        assert_eq!(
            forward.indices().map(Indices::len),
            backward.indices().map(Indices::len)
        );
    }

    #[test]
    fn triangles_face_up_and_are_yellow() {
        let (mesh, _) = sample(RunwaySide::Forward).expect("valid").expect("known");
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("missing normals"),
        };
        assert!(
            normals
                .iter()
                .all(|normal| normal.iter().all(|v| v.is_finite()))
        );
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("missing colors"),
        };
        let expected = Color::srgb(YELLOW[0], YELLOW[1], YELLOW[2]).to_linear();
        assert!(colors.iter().all(|color| {
            (color[0] - expected.red).abs() < 1e-6
                && (color[1] - expected.green).abs() < 1e-6
                && (color[2] - expected.blue).abs() < 1e-6
        }));
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let point = Geodetic::from_degrees(35.0, 139.0, 8.0);
        assert_eq!(
            holding_position_mesh(point, Radians(f64::NAN), Meters(20.0), RunwaySide::Forward,)
                .expect_err("heading"),
            HoldingMarkingError::InvalidHeading
        );
        assert_eq!(
            holding_position_mesh(point, Radians::ZERO, Meters(201.0), RunwaySide::Forward,)
                .expect_err("cap"),
            HoldingMarkingError::InvalidWidth
        );
    }
}
