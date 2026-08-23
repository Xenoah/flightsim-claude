//! 滑走路の見た目。
//!
//! # なぜテクスチャではなくジオメトリか
//!
//! 滑走路に要るのは「どこに降りるか」が遠くから分かること。舗装面と
//! 中心線の破線と滑走路端標識があれば読める。画像アセットの調達と
//! 権利確認を待たずに、頂点色だけで成立させる。
//!
//! # 座標
//!
//! 頂点は ECEF 軸で、滑走路中心を原点とする相対値。地球の曲率に沿わせる
//! ため、各頂点を測地座標（`Geodetic::offset_by`）で置いてから ECEF へ
//! 変換する。**接平面に置くと 2.5 km の滑走路の両端が約 12 cm 沈む。**
//!
//! 色は [`crate::srgb_to_linear`] を通す。**sRGB のまま頂点色に入れると
//! 明るく浅くなる**（地形の塗り分けで実際に踏んだ）。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, Meters, Radians};

/// 舗装面を地形からどれだけ浮かせるか。
///
/// 地形は滑走路の下を平らに彫ってあるが、メッシュの分割点がずれると
/// 数 cm の起伏が残る。Z ファイトと埋没の両方を避ける高さ。
const PAVEMENT_LIFT: f64 = 0.08;

/// 標識を舗装面からどれだけ浮かせるか。
const MARKING_LIFT: f64 = 0.05;

/// 舗装の色（sRGB）。夜明けでも地面と見分けられる暗い灰色。
const ASPHALT: [f32; 3] = [0.17, 0.17, 0.18];

/// 標識の色（sRGB）。実際の滑走路標識は白。
const PAINT: [f32; 3] = [0.87, 0.87, 0.85];

/// 中心線の破線。実際の規格（30 m 線 + 20 m 間隔）に合わせる。
const DASH_LENGTH: f64 = 30.0;
const DASH_GAP: f64 = 20.0;
const DASH_WIDTH: f64 = 0.9;

/// 滑走路端標識（ピアノキー）。
const KEY_COUNT: usize = 4;
const KEY_LENGTH: f64 = 30.0;
const KEY_WIDTH: f64 = 1.8;
const KEY_START: f64 = 6.0;

/// 滑走路の見た目を組み立てる。
///
/// 戻り値はメッシュと、その原点（滑走路中心・舗装の高さ）。
/// [`crate::terrain_mesh_bundle`] で spawn すれば、地形タイルと同じ経路で
/// floating origin と回転が正しく付く。
#[must_use]
pub fn runway_mesh(
    threshold: Geodetic,
    heading: Radians,
    length: Meters,
    width: Meters,
) -> (Mesh, Ecef) {
    let mut builder = QuadBuilder::new(threshold, heading, length);

    // 舗装面。
    builder.quad(
        0.0,
        length.get(),
        -width.get() * 0.5,
        width.get() * 0.5,
        PAVEMENT_LIFT,
        ASPHALT,
    );

    // 中心線の破線。両端の標識帯は空けておく。
    let marked_end = KEY_START + KEY_LENGTH + DASH_GAP;
    let mut along = marked_end;
    while along + DASH_LENGTH < length.get() - marked_end {
        builder.quad(
            along,
            along + DASH_LENGTH,
            -DASH_WIDTH * 0.5,
            DASH_WIDTH * 0.5,
            PAVEMENT_LIFT + MARKING_LIFT,
            PAINT,
        );
        along += DASH_LENGTH + DASH_GAP;
    }

    // 両端のピアノキー。中心線を挟んで左右対称に並べる。
    for far_end in [false, true] {
        let (near, far) = if far_end {
            (
                length.get() - KEY_START - KEY_LENGTH,
                length.get() - KEY_START,
            )
        } else {
            (KEY_START, KEY_START + KEY_LENGTH)
        };
        for key in 0..KEY_COUNT {
            #[allow(clippy::cast_precision_loss, reason = "鍵の本数は 1 桁")]
            let offset = (key as f64 + 0.5) * (width.get() * 0.5 - 2.0) / KEY_COUNT as f64 + 1.5;
            for side in [-1.0, 1.0] {
                builder.quad(
                    near,
                    far,
                    side * offset - KEY_WIDTH * 0.5,
                    side * offset + KEY_WIDTH * 0.5,
                    PAVEMENT_LIFT + MARKING_LIFT,
                    PAINT,
                );
            }
        }
    }

    builder.build()
}

/// ECEF 相対の四角形を積んでいく。
struct QuadBuilder {
    threshold: Geodetic,
    heading: Radians,
    origin: Ecef,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl QuadBuilder {
    fn new(threshold: Geodetic, heading: Radians, length: Meters) -> Self {
        // 原点は滑走路中心・舗装の高さ。
        let centre = surface_point(threshold, heading, length.get() * 0.5, 0.0, PAVEMENT_LIFT);
        Self {
            threshold,
            heading,
            origin: centre.to_ecef(),
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// threshold から前方 `along`、右方 `across` の帯を置く。
    fn quad(
        &mut self,
        along_near: f64,
        along_far: f64,
        across_left: f64,
        across_right: f64,
        lift: f64,
        srgb: [f32; 3],
    ) {
        let base = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        let color = [
            crate::srgb_to_linear(srgb[0]),
            crate::srgb_to_linear(srgb[1]),
            crate::srgb_to_linear(srgb[2]),
            1.0,
        ];

        // 左近・右近・右遠・左遠の順。
        for (along, across) in [
            (along_near, across_left),
            (along_near, across_right),
            (along_far, across_right),
            (along_far, across_left),
        ] {
            let surface = surface_point(self.threshold, self.heading, along, across, lift);
            let ecef = surface.to_ecef();
            // 測地座標の「上」。高度を 1 m 上げた点との差から作る（楕円体法線）。
            let above = Geodetic::new(
                surface.latitude,
                surface.longitude,
                Meters(surface.altitude.get() + 1.0),
            )
            .to_ecef();
            let up = (above.as_vec() - ecef.as_vec()).normalize();
            let relative = ecef.as_vec() - self.origin.as_vec();

            #[allow(
                clippy::cast_possible_truncation,
                reason = "原点相対で数 km 以内。f32 の分解能は mm 未満"
            )]
            {
                self.positions
                    .push([relative.x as f32, relative.y as f32, relative.z as f32]);
                self.normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
            self.colors.push(color);
        }

        // 上から見て反時計回りが表（wgpu の既定、地形タイルと同じ）。
        // 前方 × 右方 = **下向き**なので、(0,2,1) 順だと裏を向く。
        // 下の巻き順テストが実際にこれを捕まえた。
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

/// threshold 基準の滑走路座標（前方・右方・持ち上げ）から測地点を作る。
fn surface_point(
    threshold: Geodetic,
    heading: Radians,
    along: f64,
    across: f64,
    lift: f64,
) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    let north = along * cos - across * sin;
    let east = along * sin + across * cos;
    let moved = threshold.offset_by(Meters(north), Meters(east));
    Geodetic::new(
        moved.latitude,
        moved.longitude,
        Meters(moved.altitude.get() + lift),
    )
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
}
