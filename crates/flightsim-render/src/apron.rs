//! 空港エプロンの面メッシュ。
//!
//! 三角形分割は呼び出し側が行い、ここでは DEM 標高を反映済みの三角形を
//! ECEF 原点相対メッシュへ変換する。外部データ境界なので巻き順・有限性を
//! 検査し、不正な面を GPU へ渡さない。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters};
use flightsim_world::AirportSurface;

/// 誘導路の 0.06 m より低く積層する。
const APRON_LIFT: f64 = 0.04;

/// 1 メッシュで受け付ける最大三角形数。
pub const MAX_APRON_TRIANGLES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApronMeshError {
    Empty,
    TooManyTriangles,
    InvalidVertex,
    DegenerateTriangle,
    AllocationFailed,
}

impl core::fmt::Display for ApronMeshError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "apron mesh has no triangles",
            Self::TooManyTriangles => "apron mesh exceeds the triangle limit",
            Self::InvalidVertex => "apron mesh contains an invalid vertex",
            Self::DegenerateTriangle => "apron mesh contains a degenerate triangle",
            Self::AllocationFailed => "could not allocate apron mesh storage",
        })
    }
}

impl std::error::Error for ApronMeshError {}

/// OSM surface を表示用の sRGB 色へ写像する。
#[must_use]
pub const fn apron_surface_color(surface: AirportSurface) -> [f32; 3] {
    match surface {
        AirportSurface::Asphalt => [0.18, 0.18, 0.19],
        AirportSurface::Concrete => [0.48, 0.47, 0.44],
        AirportSurface::Paved => [0.28, 0.28, 0.29],
        AirportSurface::Grass => [0.28, 0.38, 0.19],
        AirportSurface::Gravel => [0.39, 0.36, 0.31],
        AirportSurface::Dirt => [0.34, 0.25, 0.16],
        AirportSurface::Sand => [0.55, 0.47, 0.31],
        AirportSurface::Unknown => [0.24, 0.24, 0.25],
    }
}

/// 三角形分割済みのエプロンを Bevy メッシュへ変換する。
///
/// 各頂点は DEM 標高を反映済みであること。入力三角形が下向きでも、出力添字は
/// 局所的な上方向へ揃える。三角形ごとに頂点を持つ形式は FSAP v3 と同じである。
///
/// # Errors
///
/// 空・上限超過・非有限座標・縮退三角形・allocation failure。
pub fn apron_mesh(
    triangles: &[[Geodetic; 3]],
    surface: AirportSurface,
) -> Result<(Mesh, Ecef), ApronMeshError> {
    if triangles.is_empty() {
        return Err(ApronMeshError::Empty);
    }
    if triangles.len() > MAX_APRON_TRIANGLES {
        return Err(ApronMeshError::TooManyTriangles);
    }
    if triangles.iter().flatten().any(|point| !valid_point(*point)) {
        return Err(ApronMeshError::InvalidVertex);
    }

    let vertex_count = triangles
        .len()
        .checked_mul(3)
        .ok_or(ApronMeshError::TooManyTriangles)?;
    let origin = lifted(triangles[0][0]).to_ecef();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    positions
        .try_reserve_exact(vertex_count)
        .map_err(|_| ApronMeshError::AllocationFailed)?;
    normals
        .try_reserve_exact(vertex_count)
        .map_err(|_| ApronMeshError::AllocationFailed)?;
    colors
        .try_reserve_exact(vertex_count)
        .map_err(|_| ApronMeshError::AllocationFailed)?;
    indices
        .try_reserve_exact(vertex_count)
        .map_err(|_| ApronMeshError::AllocationFailed)?;

    let srgb = apron_surface_color(surface);
    let color = Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear();
    for triangle in triangles {
        let mut triangle = triangle.map(lifted);
        let world = triangle.map(Geodetic::to_ecef);
        let cross =
            (world[1].as_vec() - world[0].as_vec()).cross(world[2].as_vec() - world[0].as_vec());
        let facing = cross.dot(LocalFrame::new(triangle[0]).up_ecef());
        if !facing.is_finite() || facing.abs() <= 1.0e-9 {
            return Err(ApronMeshError::DegenerateTriangle);
        }
        if facing < 0.0 {
            triangle.swap(1, 2);
        }

        let base = u32::try_from(positions.len()).map_err(|_| ApronMeshError::TooManyTriangles)?;
        for point in triangle {
            let relative = point.to_ecef().as_vec() - origin.as_vec();
            let up = LocalFrame::new(point).up_ecef();
            if !relative.is_finite() || !up.is_finite() {
                return Err(ApronMeshError::InvalidVertex);
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "空港面内の原点相対位置と単位法線は f32 で十分"
            )]
            {
                positions.push([relative.x as f32, relative.y as f32, relative.z as f32]);
                normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
            colors.push([color.red, color.green, color.blue, 1.0]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    Ok((mesh, origin))
}

fn lifted(point: Geodetic) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + APRON_LIFT),
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

    fn square() -> [Geodetic; 4] {
        let origin = Geodetic::from_degrees(35.0, 139.0, 12.0);
        [
            origin,
            origin.offset_by(Meters(0.0), Meters(20.0)),
            origin.offset_by(Meters(20.0), Meters(20.0)),
            origin.offset_by(Meters(20.0), Meters(0.0)),
        ]
    }

    #[test]
    fn downward_input_is_rewound_upward() {
        let vertices = square();
        let triangles = [
            [vertices[0], vertices[2], vertices[1]],
            [vertices[0], vertices[3], vertices[2]],
        ];
        let (mesh, origin) = apron_mesh(&triangles, AirportSurface::Concrete).expect("valid apron");
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("missing positions"),
        };
        let indices = match mesh.indices() {
            Some(Indices::U32(values)) => values,
            _ => panic!("missing indices"),
        };
        for triangle in indices.chunks_exact(3) {
            let point = |index: u32| {
                let value = positions[index as usize];
                origin.as_vec()
                    + glam::DVec3::new(
                        f64::from(value[0]),
                        f64::from(value[1]),
                        f64::from(value[2]),
                    )
            };
            let a = point(triangle[0]);
            let b = point(triangle[1]);
            let c = point(triangle[2]);
            assert!((b - a).cross(c - a).dot(a.normalize()) > 0.0);
        }
    }

    #[test]
    fn every_surface_has_a_distinct_finite_color() {
        let surfaces = [
            AirportSurface::Unknown,
            AirportSurface::Asphalt,
            AirportSurface::Concrete,
            AirportSurface::Paved,
            AirportSurface::Grass,
            AirportSurface::Gravel,
            AirportSurface::Dirt,
            AirportSurface::Sand,
        ];
        let colors: Vec<[u32; 3]> = surfaces
            .into_iter()
            .map(|surface| apron_surface_color(surface).map(f32::to_bits))
            .collect();
        assert!(
            surfaces
                .into_iter()
                .all(|surface| { apron_surface_color(surface).into_iter().all(f32::is_finite) })
        );
        for (index, color) in colors.iter().enumerate() {
            assert!(!colors[..index].contains(color), "duplicate surface color");
        }
    }

    #[test]
    fn malformed_geometry_is_rejected() {
        let vertices = square();
        assert_eq!(
            apron_mesh(
                &[[vertices[0], vertices[0], vertices[1]]],
                AirportSurface::Asphalt,
            )
            .expect_err("collapsed triangle"),
            ApronMeshError::DegenerateTriangle
        );
        let bad = Geodetic::from_degrees(f64::NAN, 0.0, 0.0);
        assert_eq!(
            apron_mesh(&[[bad, vertices[1], vertices[2]]], AirportSurface::Asphalt,)
                .expect_err("NaN vertex"),
            ApronMeshError::InvalidVertex
        );
    }

    #[test]
    fn cap_is_enforced_before_mesh_allocation() {
        let point = Geodetic::from_degrees(0.0, 0.0, 0.0);
        let triangles = vec![[point; 3]; MAX_APRON_TRIANGLES + 1];
        assert_eq!(
            apron_mesh(&triangles, AirportSurface::Unknown).expect_err("triangle cap"),
            ApronMeshError::TooManyTriangles
        );
    }
}
