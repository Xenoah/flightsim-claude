//! 誘導路中心線から舗装面と中心線標識を描く。
//!
//! OSM の `aeroway=taxiway` は中心線の折れ線であり、舗装ポリゴンではない。
//! そのため各線分を指定幅の帯へ広げ、曲がり角を円形の継ぎ目で塞ぐ。
//! 頂点は滑走路と同じく測地座標から ECEF へ変換し、地球の曲率に沿わせる。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Radians};

/// 舗装を地形から浮かせる量。
///
/// 滑走路面よりわずかに低くし、交差部では滑走路が上に見えるようにする。
const PAVEMENT_LIFT: f64 = 0.06;

/// 中心線を舗装面から浮かせる量。Z fight を避けるための値。
const MARKING_LIFT: f64 = 0.05;

/// 誘導路舗装の色（sRGB）。
const ASPHALT: [f32; 3] = [0.15, 0.15, 0.16];

/// 誘導路中心線の黄色（sRGB）。
const CENTRELINE_PAINT: [f32; 3] = [0.88, 0.62, 0.08];

/// 遠方からも消えにくい中心線幅。
const CENTRELINE_WIDTH: f64 = 0.3;

/// 曲がり角を塞ぐ円の分割数。12 角形なら頂点数を抑えつつ角が目立たない。
const JOINT_SIDES: usize = 12;

/// 1 way から作るメッシュの上限。
///
/// FSAP の全体上限だけでは、意図的に巨大な 1 way が頂点を数千万個へ膨らませ得る。
/// 実在の空港誘導路には十分余裕を持たせつつ、描画境界でメモリ消費を制限する。
const MAX_TAXIWAY_POINTS: usize = 4_096;

/// 誘導路の折れ線を舗装面と黄色中心線の 1 メッシュへ変換する。
///
/// `points` の高度はそのまま使う。呼び出し側は地形標高を反映してから渡すこと。
/// 戻り値の原点は先頭点に置かれ、[`crate::terrain_mesh_bundle`] で spawn すれば
/// floating origin に追従する。
///
/// 点が 2 個未満、幅が不正、非有限な点がある、または有効な線分が 1 本もない
/// 場合は `None`。外部データの欠損で空メッシュや NaN 頂点を GPU へ渡さない。
#[must_use]
pub fn taxiway_mesh(points: &[Geodetic], width: Meters) -> Option<(Mesh, Ecef)> {
    if points.len() < 2
        || points.len() > MAX_TAXIWAY_POINTS
        || !width.is_finite()
        || width.get() <= 0.0
        || points.iter().any(|point| !valid_point(*point))
    {
        return None;
    }

    let segment_count = points.len() - 1;
    let vertex_count = segment_count
        .checked_mul(8)?
        .checked_add(points.len().checked_mul(2 * (JOINT_SIDES + 1))?)?;
    let index_count = segment_count
        .checked_mul(12)?
        .checked_add(points.len().checked_mul(2 * JOINT_SIDES * 3)?)?;
    let origin_point = lifted(points[0], PAVEMENT_LIFT);
    let mut builder = TaxiwayMeshBuilder::new(origin_point.to_ecef());
    builder.reserve(vertex_count, index_count)?;
    let pavement_half_width = width.get() * 0.5;
    let marking_half_width = CENTRELINE_WIDTH.min(width.get()) * 0.5;
    let mut segment_count = 0_usize;

    for pair in points.windows(2) {
        let near = pair[0];
        let far = pair[1];
        let displacement = LocalFrame::new(near).ecef_to_ned_position(far.to_ecef());
        if !displacement.0.is_finite() || displacement.horizontal_magnitude() <= f64::EPSILON {
            continue;
        }
        let heading = displacement.bearing();
        builder.segment(
            near,
            far,
            heading,
            pavement_half_width,
            PAVEMENT_LIFT,
            ASPHALT,
        )?;
        builder.segment(
            near,
            far,
            heading,
            marking_half_width,
            PAVEMENT_LIFT + MARKING_LIFT,
            CENTRELINE_PAINT,
        )?;
        segment_count += 1;
    }

    if segment_count == 0 {
        return None;
    }

    // 線分ごとの四角形だけでは曲がり角の外側に三角形の隙間ができる。
    // 全点に円を重ねると、way 同士が端点で接続する場合の端面も自然に塞がる。
    for &point in points {
        builder.disc(point, pavement_half_width, PAVEMENT_LIFT, ASPHALT)?;
        builder.disc(
            point,
            marking_half_width,
            PAVEMENT_LIFT + MARKING_LIFT,
            CENTRELINE_PAINT,
        )?;
    }

    Some(builder.build())
}

fn valid_point(point: Geodetic) -> bool {
    point.latitude.is_finite()
        && point.longitude.is_finite()
        && point.altitude.is_finite()
        && point.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && point.longitude.get().abs() <= core::f64::consts::PI
}

struct TaxiwayMeshBuilder {
    origin: Ecef,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl TaxiwayMeshBuilder {
    fn new(origin: Ecef) -> Self {
        Self {
            origin,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn reserve(&mut self, vertex_count: usize, index_count: usize) -> Option<()> {
        self.positions.try_reserve_exact(vertex_count).ok()?;
        self.normals.try_reserve_exact(vertex_count).ok()?;
        self.colors.try_reserve_exact(vertex_count).ok()?;
        self.indices.try_reserve_exact(index_count).ok()?;
        Some(())
    }

    fn segment(
        &mut self,
        near: Geodetic,
        far: Geodetic,
        heading: Radians,
        half_width: f64,
        lift: f64,
        srgb: [f32; 3],
    ) -> Option<()> {
        let near_left = lateral_point(near, heading, -half_width, lift);
        let near_right = lateral_point(near, heading, half_width, lift);
        let far_left = lateral_point(far, heading, -half_width, lift);
        let far_right = lateral_point(far, heading, half_width, lift);
        let base = self.push_vertices([near_left, near_right, far_right, far_left], srgb)?;
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        Some(())
    }

    fn disc(&mut self, centre: Geodetic, radius: f64, lift: f64, srgb: [f32; 3]) -> Option<()> {
        let centre_index = self.push_vertex(lifted(centre, lift), srgb)?;
        let first_ring_index = u32::try_from(self.positions.len()).ok()?;
        for side in 0..JOINT_SIDES {
            #[allow(clippy::cast_precision_loss, reason = "JOINT_SIDES は 12")]
            let angle = core::f64::consts::TAU * side as f64 / JOINT_SIDES as f64;
            let point =
                centre.offset_by(Meters(radius * angle.cos()), Meters(radius * angle.sin()));
            self.push_vertex(lifted(point, lift), srgb)?;
        }

        for side in 0..JOINT_SIDES {
            let current = first_ring_index + u32::try_from(side).ok()?;
            let next = first_ring_index + u32::try_from((side + 1) % JOINT_SIDES).ok()?;
            // north -> east は NED では下向き。順番を反転して表を上へ向ける。
            self.indices
                .extend_from_slice(&[centre_index, next, current]);
        }
        Some(())
    }

    fn push_vertices<const N: usize>(
        &mut self,
        points: [Geodetic; N],
        srgb: [f32; 3],
    ) -> Option<u32> {
        let base = u32::try_from(self.positions.len()).ok()?;
        for point in points {
            self.push_vertex(point, srgb)?;
        }
        Some(base)
    }

    fn push_vertex(&mut self, point: Geodetic, srgb: [f32; 3]) -> Option<u32> {
        let index = u32::try_from(self.positions.len()).ok()?;
        let ecef = point.to_ecef();
        let relative = ecef.as_vec() - self.origin.as_vec();
        let up = LocalFrame::new(point).up_ecef();
        if !relative.is_finite() || !up.is_finite() {
            return None;
        }

        #[allow(
            clippy::cast_possible_truncation,
            reason = "空港内の原点相対座標と単位法線は f32 で十分"
        )]
        {
            self.positions
                .push([relative.x as f32, relative.y as f32, relative.z as f32]);
            self.normals.push([up.x as f32, up.y as f32, up.z as f32]);
        }
        self.colors.push([
            crate::srgb_to_linear(srgb[0]),
            crate::srgb_to_linear(srgb[1]),
            crate::srgb_to_linear(srgb[2]),
            1.0,
        ]);
        Some(index)
    }

    fn build(self) -> (Mesh, Ecef) {
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices));
        (mesh, self.origin)
    }
}

fn lateral_point(point: Geodetic, heading: Radians, across: f64, lift: f64) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    let moved = point.offset_by(Meters(-across * sin), Meters(across * cos));
    lifted(moved, lift)
}

fn lifted(point: Geodetic, lift: f64) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + lift),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn sample() -> Vec<Geodetic> {
        vec![
            Geodetic::from_degrees(35.5480, 139.7750, 8.0),
            Geodetic::from_degrees(35.5485, 139.7756, 8.0),
            Geodetic::from_degrees(35.5492, 139.7756, 8.0),
        ]
    }

    #[test]
    fn a_polyline_has_pavement_markings_and_joint_geometry() {
        let points = sample();
        let (mesh, _) = taxiway_mesh(&points, Meters(20.0)).expect("valid taxiway");

        // 2 線分 × 2 帯 × 4 頂点 + 3 点 × 2 円 × (中心 1 + 外周 12)。
        assert_eq!(mesh.count_vertices(), 94);
        assert_eq!(mesh.indices().map(Indices::len), Some(240));
    }

    #[test]
    fn every_triangle_faces_away_from_the_earth() {
        let (mesh, origin) = taxiway_mesh(&sample(), Meters(20.0)).expect("valid taxiway");
        let positions: Vec<DVec3> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values
                .iter()
                .map(|value| {
                    DVec3::new(
                        f64::from(value[0]),
                        f64::from(value[1]),
                        f64::from(value[2]),
                    )
                })
                .collect(),
            _ => panic!("positions must be f32x3"),
        };
        let indices = match mesh.indices() {
            Some(Indices::U32(values)) => values,
            _ => panic!("indices must be u32"),
        };

        for triangle in indices.chunks_exact(3) {
            let [a, b, c] =
                [triangle[0], triangle[1], triangle[2]].map(|index| positions[index as usize]);
            let centre = origin.as_vec() + (a + b + c) / 3.0;
            let normal = (b - a).cross(c - a);
            assert!(
                normal.dot(centre.normalize()) > 0.0,
                "a taxiway triangle winds towards the earth"
            );
        }
    }

    #[test]
    fn vertices_follow_the_ellipsoid_instead_of_a_tangent_plane() {
        let (mesh, origin) = taxiway_mesh(&sample(), Meters(20.0)).expect("valid taxiway");
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("positions must be f32x3"),
        };
        for position in positions {
            let world = Ecef::from_vec(
                origin.as_vec()
                    + DVec3::new(
                        f64::from(position[0]),
                        f64::from(position[1]),
                        f64::from(position[2]),
                    ),
            );
            let altitude = world.to_geodetic().altitude.get();
            assert!(
                (8.04..=8.13).contains(&altitude),
                "taxiway vertex altitude {altitude} did not follow the ellipsoid"
            );
        }
    }

    #[test]
    fn each_node_keeps_its_own_surface_elevation() {
        let points = [
            Geodetic::from_degrees(35.5480, 139.7750, 5.0),
            Geodetic::from_degrees(35.5490, 139.7760, 25.0),
        ];
        let (mesh, origin) = taxiway_mesh(&points, Meters(20.0)).expect("valid sloping taxiway");
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("positions must be f32x3"),
        };
        let altitude = |index: usize| {
            let relative = positions[index];
            Ecef::from_vec(
                origin.as_vec()
                    + DVec3::new(
                        f64::from(relative[0]),
                        f64::from(relative[1]),
                        f64::from(relative[2]),
                    ),
            )
            .to_geodetic()
            .altitude
            .get()
        };

        // 最初の pavement quad は near-left, near-right, far-right, far-left の順。
        assert!((altitude(0) - 5.06).abs() < 0.02);
        assert!((altitude(2) - 25.06).abs() < 0.02);
    }

    #[test]
    fn centreline_uses_linear_yellow() {
        let (mesh, _) = taxiway_mesh(&sample(), Meters(20.0)).expect("valid taxiway");
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("colors must be f32x4"),
        };
        let expected = CENTRELINE_PAINT.map(crate::srgb_to_linear);
        assert!(colors.iter().any(|color| color[..3] == expected));
    }

    #[test]
    fn invalid_or_degenerate_geometry_is_skipped() {
        assert!(taxiway_mesh(&[], Meters(20.0)).is_none());
        assert!(taxiway_mesh(&sample()[..1], Meters(20.0)).is_none());
        assert!(taxiway_mesh(&sample(), Meters::ZERO).is_none());
        assert!(taxiway_mesh(&sample(), Meters(f64::NAN)).is_none());

        let duplicate = [sample()[0], sample()[0]];
        assert!(taxiway_mesh(&duplicate, Meters(20.0)).is_none());

        let mut invalid = sample();
        invalid[1].latitude = Radians(f64::NAN);
        assert!(taxiway_mesh(&invalid, Meters(20.0)).is_none());

        let excessive = vec![sample()[0]; MAX_TAXIWAY_POINTS + 1];
        assert!(taxiway_mesh(&excessive, Meters(20.0)).is_none());
    }
}
