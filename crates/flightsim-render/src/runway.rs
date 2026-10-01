//! 滑走路の舗装と標示。頂点は f64 ECEF から滑走路中心相対の f32 へ変換する。
//!
//! [`runway_mesh`] は従来の一定楕円体高、[`runway_mesh_with_elevation`] は呼び出し側が
//! 渡す地面標高に沿う。描画層は DEM や接地状態を取得・変更しない。
//! 舗装と標示を同じ格子へ分割することで、標示の下に別の補間面を作らない。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Radians};

/// 空港面の積層順を保つ。apron 0.04 m、taxiway 0.06 m の上。
const PAVEMENT_LIFT: f64 = 0.08;
const MARKING_LIFT: f64 = 0.05;
const ASPHALT: [f32; 3] = [0.17, 0.17, 0.18];
const PAINT: [f32; 3] = [0.87, 0.87, 0.85];
const DASH_LENGTH: f64 = 30.0;
const DASH_GAP: f64 = 20.0;
const DASH_WIDTH: f64 = 0.9;
const KEY_COUNT: u32 = 4;
const KEY_LENGTH: f64 = 30.0;
const KEY_WIDTH: f64 = 1.8;
const KEY_START: f64 = 6.0;

/// GLO-30 の公称 30 m 画素より細かく、面の途中の起伏も取り込む。
/// これは DEM の精度を上げるものでも、任意の地形を完全に覆う保証でもない。
const SURFACE_STEP: f64 = 10.0;
/// 1 枚の舗装・標示合わせて最大 524,288 頂点（各 cell に最大 2 quad）。
const MAX_SURFACE_CELLS: usize = 65_536;
/// core の近距離 offset と有限な描画・標示予算の範囲。異常値をループへ入れない。
const MAX_LENGTH: f64 = 10_000.0;
const MAX_WIDTH: f64 = 200.0;

/// 一定の楕円体高に置く従来の入口。地形に沿わせる場合は
/// [`runway_mesh_with_elevation`] に app が標高を渡す。
#[must_use]
pub fn runway_mesh(
    threshold: Geodetic,
    heading: Radians,
    length: Meters,
    width: Meters,
) -> (Mesh, Ecef) {
    runway_mesh_with_elevation(threshold, heading, length, width, |_| threshold.altitude)
}

/// 呼び出し側が渡す地面楕円体高へ舗装と標示を沿わせる。
///
/// `elevation` の入力は標高 lift 前の測地点、出力はその点の地面楕円体高 (m)。
/// app は使用中の地形と同じ標高源・鉛直基準・欠測時の規約を使用すること。
/// 各格子点で一度だけ取得し、同じ頂点から舗装 (+0.08 m) と標示 (+0.13 m) を作る。
/// 最大 10 m の格子間隔に加え、全標示の境界も格子へ含めるので、別々に補間した
/// 標示が舗装へ埋まることはない。標高を physical runway / FDM へ書き戻さない。
///
/// 10 km × 200 m を超える寸法、非正・非有限寸法、非有限方位、不正な測地点、
/// 65,536 cell を超える格子、core の東西 offset が縮退する極付近 (89.99° 以上) は
/// 空メッシュにする。非有限・絶対値 1,000 km 超の
/// 標高標本は threshold の標高へ戻す。原点は有効な中心点、入力不正時は threshold
/// （それも不正なら緯度経度高度 0）なので、空でも非有限値を GPU へ渡さない。
///
/// 戻り値は [`crate::terrain_mesh_bundle`] へ渡せる ECEF 相対メッシュと原点。
#[must_use]
pub fn runway_mesh_with_elevation(
    threshold: Geodetic,
    heading: Radians,
    length: Meters,
    width: Meters,
    mut elevation: impl FnMut(Geodetic) -> Meters,
) -> (Mesh, Ecef) {
    let mut builder = QuadBuilder::new(safe_origin(threshold));
    if !valid_runway(threshold, heading, length, width) {
        return builder.build();
    }
    // 境界を先に検査して、範囲外の緯度を app の sampler へ渡さない。
    for along in [0.0, length.get()] {
        for across in [-width.get() * 0.5, width.get() * 0.5] {
            if !valid_point(runway_point(threshold, heading, along, across)) {
                return builder.build();
            }
        }
    }
    let markings = marking_rectangles(length.get(), width.get());
    let along = grid_axis(length.get(), markings.iter().flat_map(|r| [r.near, r.far]));
    let across = grid_axis(
        width.get(),
        markings
            .iter()
            .flat_map(|r| [r.left + width.get() * 0.5, r.right + width.get() * 0.5]),
    );
    if (along.len() - 1) * (across.len() - 1) > MAX_SURFACE_CELLS {
        return builder.build();
    }
    let centre = sampled_point(
        runway_point(threshold, heading, length.get() * 0.5, 0.0),
        &mut elevation,
    );
    builder.origin = lifted(centre, PAVEMENT_LIFT).to_ecef();
    let mut points = Vec::with_capacity(along.len() * across.len());
    for &a in &along {
        for &x in &across {
            let point = runway_point(threshold, heading, a, x - width.get() * 0.5);
            if !valid_point(point) {
                return QuadBuilder::new(safe_origin(threshold)).build();
            }
            points.push(sampled_point(point, &mut elevation));
        }
    }
    for (a, along_pair) in along.windows(2).enumerate() {
        for (x, across_pair) in across.windows(2).enumerate() {
            let index = a * across.len() + x;
            let corners = [
                points[index],
                points[index + 1],
                points[index + across.len() + 1],
                points[index + across.len()],
            ];
            builder.quad(corners, PAVEMENT_LIFT, ASPHALT);
            let mid_along = (along_pair[0] + along_pair[1]) * 0.5;
            let mid_across = (across_pair[0] + across_pair[1] - width.get()) * 0.5;
            if markings.iter().any(|r| r.contains(mid_along, mid_across)) {
                builder.quad(corners, PAVEMENT_LIFT + MARKING_LIFT, PAINT);
            }
        }
    }
    builder.build()
}

#[derive(Clone, Copy)]
struct MarkingRect {
    near: f64,
    far: f64,
    left: f64,
    right: f64,
}

impl MarkingRect {
    fn contains(self, along: f64, across: f64) -> bool {
        along > self.near && along < self.far && across > self.left && across < self.right
    }
}

fn marking_rectangles(length: f64, width: f64) -> Vec<MarkingRect> {
    let mut rectangles = Vec::new();
    let marked_end = KEY_START + KEY_LENGTH + DASH_GAP;
    let mut along = marked_end;
    while along + DASH_LENGTH < length - marked_end {
        rectangles.push(MarkingRect {
            near: along,
            far: along + DASH_LENGTH,
            left: -DASH_WIDTH * 0.5,
            right: DASH_WIDTH * 0.5,
        });
        along += DASH_LENGTH + DASH_GAP;
    }
    // 小さすぎる面へ実寸のピアノキーをはみ出させない。
    if length >= 2.0 * (KEY_START + KEY_LENGTH) && width >= 12.0 {
        for far_end in [false, true] {
            let near = if far_end {
                length - KEY_START - KEY_LENGTH
            } else {
                KEY_START
            };
            for key in 0..KEY_COUNT {
                let offset =
                    (f64::from(key) + 0.5) * (width * 0.5 - 2.0) / f64::from(KEY_COUNT) + 1.5;
                for side in [-1.0, 1.0] {
                    rectangles.push(MarkingRect {
                        near,
                        far: near + KEY_LENGTH,
                        left: side * offset - KEY_WIDTH * 0.5,
                        right: side * offset + KEY_WIDTH * 0.5,
                    });
                }
            }
        }
    }
    rectangles
}

fn grid_axis(span: f64, marking_edges: impl Iterator<Item = f64>) -> Vec<f64> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "validated dimensions cap intervals at 1,000"
    )]
    let intervals = (span / SURFACE_STEP).ceil().max(1.0) as u32;
    let mut points: Vec<f64> = (0..=intervals)
        .map(|i| span * f64::from(i) / f64::from(intervals))
        .collect();
    points.extend(marking_edges.filter(|&v| v > 0.0 && v < span));
    points.sort_by(f64::total_cmp);
    // より小さな差は f32 相対座標へ落とすと縮退する。端点・標示の形は µm まで保つ。
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    points
}

struct QuadBuilder {
    origin: Ecef,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl QuadBuilder {
    fn new(origin: Ecef) -> Self {
        Self {
            origin,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn quad(&mut self, points: [Geodetic; 4], lift: f64, srgb: [f32; 3]) {
        let base = u32::try_from(self.positions.len()).expect("bounded runway vertex count");
        let color = [
            crate::srgb_to_linear(srgb[0]),
            crate::srgb_to_linear(srgb[1]),
            crate::srgb_to_linear(srgb[2]),
            1.0,
        ];
        for point in points {
            let relative = lifted(point, lift).to_ecef().as_vec() - self.origin.as_vec();
            let up = LocalFrame::new(point).up_ecef();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded airport-relative positions and unit normals"
            )]
            {
                self.positions
                    .push([relative.x as f32, relative.y as f32, relative.z as f32]);
                self.normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
            self.colors.push(color);
        }
        // 左近・右近・右遠・左遠。前方 × 右方は下向きなので、この順序で上向き。
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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

/// 前方・右方を N/E に回すだけ。測地・ECEF 変換は core に任せる。
pub(crate) fn runway_point(
    threshold: Geodetic,
    heading: Radians,
    along: f64,
    across: f64,
) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    threshold.offset_by(
        Meters(along * cos - across * sin),
        Meters(along * sin + across * cos),
    )
}

pub(crate) fn lifted(point: Geodetic, lift: f64) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + lift),
    )
}

pub(crate) fn sampled_point(
    point: Geodetic,
    elevation: &mut impl FnMut(Geodetic) -> Meters,
) -> Geodetic {
    let altitude = elevation(point);
    Geodetic::new(
        point.latitude,
        point.longitude,
        if valid_altitude(altitude) {
            altitude
        } else {
            point.altitude
        },
    )
}

fn valid_altitude(altitude: Meters) -> bool {
    altitude.get().is_finite() && altitude.get().abs() <= 1_000_000.0
}

pub(crate) fn valid_point(point: Geodetic) -> bool {
    point.latitude.get().is_finite()
        && point.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && point.longitude.get().is_finite()
        && valid_altitude(point.altitude)
}

pub(crate) fn safe_origin(threshold: Geodetic) -> Ecef {
    if valid_point(threshold) {
        threshold.to_ecef()
    } else {
        Geodetic::from_degrees(0.0, 0.0, 0.0).to_ecef()
    }
}

pub(crate) fn valid_dimensions(length: Meters, width: Meters) -> bool {
    length.get().is_finite()
        && length.get() > 0.0
        && length.get() <= MAX_LENGTH
        && width.get().is_finite()
        && width.get() > 0.0
        && width.get() <= MAX_WIDTH
}

pub(crate) fn valid_runway(
    threshold: Geodetic,
    heading: Radians,
    length: Meters,
    width: Meters,
) -> bool {
    valid_point(threshold)
        && threshold.latitude.to_degrees().get().abs() < 89.99
        && heading.get().is_finite()
        && valid_dimensions(length, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn sample() -> (Mesh, Ecef) {
        runway_mesh(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Radians(50.0_f64.to_radians()),
            Meters(2500.0),
            Meters(45.0),
        )
    }

    #[test]
    fn the_mesh_has_pavement_and_markings() {
        let (mesh, _) = sample();
        let vertices = mesh.count_vertices();
        // 舗装 4 + 破線多数 + ピアノキー 2 端 × 4 本 × 2 側 × 4 頂点。
        assert!(
            vertices > 100,
            "the runway should carry markings, got only {vertices} vertices"
        );
    }

    #[test]
    fn every_triangle_faces_up() {
        // 裏返った三角形は背面カリングで消える。「滑走路が見えない」の典型原因。
        let (mesh, origin) = sample();
        let positions: Vec<DVec3> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values
                .iter()
                .map(|v| DVec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2])))
                .collect(),
            _ => panic!("positions must be f32x3"),
        };
        let up = origin.as_vec().normalize();

        let indices: Vec<u32> = match mesh.indices() {
            Some(Indices::U32(values)) => values.clone(),
            _ => panic!("indices must be u32"),
        };
        for triangle in indices.chunks(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| positions[i as usize]);
            let normal = (b - a).cross(c - a);
            assert!(
                normal.dot(up) > 0.0,
                "a triangle winds the wrong way and will be culled"
            );
        }
    }

    #[test]
    fn the_far_end_follows_the_curvature_of_the_earth() {
        // 接平面に置くと両端が沈む。端の頂点も楕円体高 8 m 付近にあること。
        let (mesh, origin) = sample();
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
                (7.9..=8.3).contains(&altitude),
                "a runway vertex sits at {altitude} m — it does not follow the ellipsoid"
            );
        }
    }

    #[test]
    fn markings_are_painted_in_linear_colour() {
        // sRGB のまま渡すと明るく浅くなる（地形で実際に踏んだ）。
        let (mesh, _) = sample();
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("colors must be f32x4"),
        };
        let brightest = colors.iter().map(|c| c[0]).fold(0.0_f32, f32::max);
        let expected = crate::srgb_to_linear(PAINT[0]);
        assert!(
            (brightest - expected).abs() < 1e-6,
            "the paint colour {brightest} is not the linear form of sRGB {}",
            PAINT[0]
        );
    }

    fn positions(mesh: &Mesh) -> Vec<DVec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values
                .iter()
                .map(|v| DVec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2])))
                .collect(),
            _ => panic!("positions must be f32x3"),
        }
    }

    fn terrain_height(point: Geodetic) -> Meters {
        // 起点と遠端が同じ高さでも途中を覆えなかった旧 1 quad を検出する起伏。
        let north = (point.latitude_degrees() - 35.548) * 10_000.0;
        let east = (point.longitude_degrees() - 139.775) * 10_000.0;
        Meters(8.0 + 0.08 * north + 0.02 * east + (north * 0.12).sin() * 2.0)
    }

    #[test]
    fn draped_surface_samples_the_interior_and_retains_both_lifts() {
        let (mesh, origin) = runway_mesh_with_elevation(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Radians(0.0),
            Meters(2500.0),
            Meters(45.0),
            terrain_height,
        );
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("colors must be f32x4"),
        };
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for (position, color) in positions(&mesh).iter().zip(colors) {
            let point = Ecef::from_vec(origin.as_vec() + position).to_geodetic();
            let is_paint = color[0] > 0.5;
            let lift = PAVEMENT_LIFT + if is_paint { MARKING_LIFT } else { 0.0 };
            assert!((point.altitude.get() - terrain_height(point).get() - lift).abs() < 0.002);
            lowest = lowest.min(point.altitude.get());
            highest = highest.max(point.altitude.get());
        }
        assert!(
            highest - lowest > 10.0,
            "threshold altitude must not flatten the runway"
        );
    }

    #[test]
    fn paint_reuses_exact_pavement_cells_even_on_nonplanar_terrain() {
        let (mesh, origin) = runway_mesh_with_elevation(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Radians(0.0),
            Meters(500.0),
            Meters(45.0),
            terrain_height,
        );
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("colors must be f32x4"),
        };
        let vertices = positions(&mesh);
        let mut paint_cells = 0;
        for (cell, color) in colors.chunks_exact(4).enumerate() {
            if color[0][0] <= 0.5 {
                continue;
            }
            paint_cells += 1;
            assert!(cell > 0 && colors[(cell - 1) * 4][0] < 0.5);
            for corner in 0..4 {
                let pavement = vertices[(cell - 1) * 4 + corner];
                let paint = vertices[cell * 4 + corner];
                let point = Ecef::from_vec(origin.as_vec() + pavement).to_geodetic();
                let up = LocalFrame::new(point).up_ecef();
                assert!(
                    (paint - pavement - up * MARKING_LIFT).length() < 0.0001,
                    "paint and pavement must share corners, diagonal, and lift"
                );
            }
        }
        assert!(paint_cells > 20);
    }

    #[test]
    fn draped_cells_are_small_and_their_triangles_face_up() {
        let (mesh, origin) = runway_mesh_with_elevation(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Radians(0.8),
            Meters(2500.0),
            Meters(45.0),
            terrain_height,
        );
        let up = LocalFrame::new(origin.to_geodetic()).up_ecef();
        for cell in positions(&mesh).chunks_exact(4) {
            for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
                assert!((cell[b] - cell[a]).cross(cell[c] - cell[a]).dot(up) > 0.0);
            }
            for side in 0..4 {
                assert!((cell[(side + 1) % 4] - cell[side]).length() < SURFACE_STEP + 0.5);
            }
        }
    }

    #[test]
    fn flat_callback_is_the_legacy_helper() {
        let threshold = Geodetic::from_degrees(35.548, 139.775, 8.0);
        let (legacy, legacy_origin) =
            runway_mesh(threshold, Radians(0.8), Meters(2500.0), Meters(45.0));
        let (draped, draped_origin) = runway_mesh_with_elevation(
            threshold,
            Radians(0.8),
            Meters(2500.0),
            Meters(45.0),
            |_| threshold.altitude,
        );
        assert_eq!(legacy_origin, draped_origin);
        assert_eq!(positions(&legacy), positions(&draped));
    }

    #[test]
    fn invalid_dimensions_do_not_sample_or_allocate_a_surface() {
        for (length, width) in [
            (f64::NAN, 45.0),
            (f64::INFINITY, 45.0),
            (1e200, 45.0),
            (0.0, 45.0),
            (-1.0, 45.0),
            (MAX_LENGTH + 1.0, 45.0),
            (2500.0, f64::NAN),
            (2500.0, MAX_WIDTH + 1.0),
        ] {
            let (mesh, origin) = runway_mesh_with_elevation(
                Geodetic::from_degrees(35.548, 139.775, 8.0),
                Radians(0.0),
                Meters(length),
                Meters(width),
                |_| panic!("invalid dimensions must be rejected before sampling"),
            );
            assert_eq!(mesh.count_vertices(), 0);
            assert!(origin.as_vec().is_finite());
        }
    }

    #[test]
    fn invalid_samples_fall_back_without_nonfinite_gpu_positions() {
        for altitude in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let (mesh, origin) = runway_mesh_with_elevation(
                Geodetic::from_degrees(35.548, 139.775, 8.0),
                Radians(0.0),
                Meters(2500.0),
                Meters(45.0),
                |_| Meters(altitude),
            );
            assert!(origin.as_vec().is_finite());
            for position in positions(&mesh) {
                assert!(position.is_finite());
                let altitude = Ecef::from_vec(origin.as_vec() + position)
                    .to_geodetic()
                    .altitude
                    .get();
                assert!((8.07..8.14).contains(&altitude));
            }
        }
    }

    #[test]
    fn all_supported_dimensions_have_a_hard_mesh_budget() {
        let (mesh, _) = runway_mesh_with_elevation(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Radians(0.0),
            Meters(MAX_LENGTH),
            Meters(MAX_WIDTH),
            |_| Meters(8.0),
        );
        assert!(mesh.count_vertices() > 0);
        assert!(mesh.count_vertices() <= MAX_SURFACE_CELLS * 8);
    }

    #[test]
    fn dateline_and_high_latitude_meshes_keep_finite_local_precision() {
        for (latitude, longitude) in [
            (0.0, 179.999),
            (0.0, -179.999),
            (80.0, 179.999),
            (-80.0, -179.999),
        ] {
            let threshold = Geodetic::from_degrees(latitude, longitude, 1800.0);
            let (mesh, origin) = runway_mesh_with_elevation(
                threshold,
                Radians(1.1),
                Meters(2500.0),
                Meters(45.0),
                |_| Meters(1800.0),
            );
            assert!(mesh.count_vertices() > 0);
            for position in positions(&mesh) {
                assert!(position.is_finite() && position.length() < 1300.0);
                let point = Ecef::from_vec(origin.as_vec() + position).to_geodetic();
                assert!((1800.07..1800.14).contains(&point.altitude.get()));
            }
        }
    }

    #[test]
    fn invalid_coordinate_and_polar_geometry_remain_empty_and_finite() {
        for threshold in [
            Geodetic::from_degrees(f64::NAN, 0.0, 0.0),
            Geodetic::from_degrees(0.0, f64::INFINITY, 0.0),
            Geodetic::from_degrees(91.0, 0.0, 0.0),
            Geodetic::from_degrees(0.0, 0.0, f64::MAX),
            Geodetic::from_degrees(90.0, 0.0, 0.0),
            Geodetic::from_degrees(-90.0, 0.0, 0.0),
        ] {
            let (mesh, origin) = runway_mesh_with_elevation(
                threshold,
                Radians(core::f64::consts::PI),
                Meters(2500.0),
                Meters(45.0),
                |_| panic!("invalid coordinates must be rejected before sampling"),
            );
            assert_eq!(mesh.count_vertices(), 0);
            assert!(origin.as_vec().is_finite());
        }
    }
}
