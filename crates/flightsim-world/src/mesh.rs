//! 地形タイルから描画用メッシュ**データ**を作る。
//!
//! # ここは GPU に触らない
//!
//! 頂点配列を返すところまでが `flightsim-world` の責務で、GPU バッファへの投入は
//! `flightsim-render` が行う（ARCHITECTURE.md §2）。この境界のおかげで、
//! メッシュ生成のバグを **GUI 無しで、`cargo test` 数秒で**追い込める。
//!
//! # 座標系
//!
//! 頂点は **タイル中心を原点とする ECEF 相対位置**（`f32`, m）。
//!
//! 世界座標そのもの（`f64` ECEF）を `f32` に落とすと地表で 76cm に量子化するが
//! （[ADR-0002](../../../../docs/adr/0002-coordinate-system.md)）、タイル中心からの
//! 相対なら level 9 のタイル（約 78 km 四方）でも半径 39 km、`f32` の分解能は
//! `39_000 / 2^23 ≒ 4.6 mm`。描画には十分。
//!
//! メッシュを世界のどこへ置くかは `origin` が持ち、floating origin を適用して
//! `Transform` になる。
//!
//! # スカート
//!
//! 隣り合うタイルの LOD が違うと、境界の頂点が一致せず**隙間（crack）から
//! 地面の裏側や空が見える**。これを塞ぐため、タイルの縁を地心方向へ垂らした
//! 帯（スカート）を付ける。
//!
//! ```text
//!     ____________         上から見た地形
//!    /           /|
//!   /___________/ |  ← 縁から下へ垂らした帯がスカート
//!   |           | |
//!   |___________|/
//! ```
//!
//! 隙間を「埋める」のではなく「裏が見えないように隠す」手法なので、
//! 深さが足りないと隙間が残る。LOD 差 1 段ぶんの標高差を賄える深さが要る。

use crate::dem::DemTile;
use crate::tile::TileId;
use flightsim_core::{Ecef, Geodetic, Meters, Radians};
use glam::DVec3;

/// 描画用のメッシュデータ。
///
/// 配列の並びは Bevy の `Mesh` にそのまま渡せる形にしてある。
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainMesh {
    /// メッシュ原点の世界位置（タイル中心、地形標高の平均高度）。
    pub origin: Ecef,
    /// `origin` からの相対位置 `m`。
    pub positions: Vec<[f32; 3]>,
    /// 単位法線。地心と逆向き（外向き）。
    pub normals: Vec<[f32; 3]>,
    /// タイル内の正規化座標。`u = 0` が西端、`v = 0` が北端。
    pub uvs: Vec<[f32; 2]>,
    /// 各頂点の楕円体高 `m`。
    ///
    /// **地形の素性であって見た目ではない。** 描画側はこれと [`Self::slopes`]
    /// から色を決めるが、どう塗るかを決めるのはここではない。
    /// 生成時には手元にある値なので、描画側で ECEF から測地座標へ
    /// 逆変換し直さずに済む。
    pub elevations: Vec<f32>,
    /// 各頂点の傾斜 `rad`。0 が水平、`PI/2` が垂直。
    ///
    /// 法線と「上」（楕円体法線）のなす角。**斜面を岩肌にするなど、
    /// 高度だけでは決められない塗り分けに要る。**
    pub slopes: Vec<f32>,
    /// 三角形リスト。反時計回りが表（wgpu の既定）。
    pub indices: Vec<u32>,
    /// スカートを除いた地形面の頂点数。デバッグ表示と検査用。
    pub surface_vertex_count: usize,
}

impl TerrainMesh {
    /// 三角形の数。
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// 概算のメモリ使用量 `バイト`。
    #[must_use]
    pub fn memory_footprint(&self) -> usize {
        self.positions.len() * core::mem::size_of::<[f32; 3]>()
            + self.normals.len() * core::mem::size_of::<[f32; 3]>()
            + self.uvs.len() * core::mem::size_of::<[f32; 2]>()
            + self.indices.len() * core::mem::size_of::<u32>()
    }
}

/// メッシュ生成の設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshOptions {
    /// 1 辺あたりの頂点数。DEM の格子より粗くも細かくもできる。
    ///
    /// `2^n + 1` にすると隣接 LOD で頂点位置が揃う。
    pub resolution: u32,
    /// スカートの深さ。`None` なら幾何誤差から自動で決める。
    pub skirt_depth: Option<Meters>,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            resolution: 33,
            skirt_depth: None,
        }
    }
}

/// スカート深さの下限。
///
/// 平坦なタイルは幾何誤差がほぼ 0 になるが、隣が粗い LOD なら境界には必ず段差が出る。
/// 深さ 0 のスカートは何も隠さないので、最低限の depth を確保する。
const MIN_SKIRT_DEPTH: f64 = 20.0;

/// 幾何誤差に対するスカート深さの倍率。
///
/// 隣接タイルが 1 段粗い場合、境界の標高差は最大でおよそ「その粗いタイルの
/// 幾何誤差」ぶん。粗い側の誤差は細かい側の 2 倍程度なので、余裕を見て 4 倍取る。
const SKIRT_DEPTH_FACTOR: f64 = 4.0;

/// DEM タイルから描画メッシュを作る。
///
/// # Panics
///
/// `options.resolution` が 2 未満の場合。1 頂点では面を張れない。
#[must_use]
pub fn build_mesh(id: TileId, dem: &DemTile, options: &MeshOptions) -> TerrainMesh {
    assert!(
        options.resolution >= 2,
        "mesh resolution must be at least 2, got {}",
        options.resolution
    );

    let resolution = options.resolution;
    let bounds = id.bounds();
    let steps = f64::from(resolution - 1);

    // 原点はタイル中心の地表。ここからの相対で f32 に落とす。
    let centre_geodetic = bounds.center();
    let centre_elevation = dem.elevation_at(centre_geodetic);
    let origin = Geodetic::new(
        centre_geodetic.latitude,
        centre_geodetic.longitude,
        centre_elevation,
    )
    .to_ecef();

    let vertex_count = (resolution as usize) * (resolution as usize);
    let mut positions = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut elevations: Vec<f32> = Vec::with_capacity(uvs.capacity());
    // 法線は面法線の平均で求める。曲率のある地球でも素直に正しくなる。
    let mut normal_accumulator = vec![DVec3::ZERO; vertex_count];
    // 地心方向の単位ベクトル（スカートを垂らす向き）を頂点ごとに持つ。
    let mut up_vectors = Vec::with_capacity(vertex_count);
    let mut world_positions = Vec::with_capacity(vertex_count);

    for row in 0..resolution {
        for column in 0..resolution {
            let u = f64::from(column) / steps;
            let v = f64::from(row) / steps;
            // v = 0 が北端。DEM の行順に合わせている。
            let position = Geodetic::new(
                Radians(bounds.north.get() - v * bounds.height().get()),
                Radians(bounds.west.get() + u * bounds.width().get()),
                Meters::ZERO,
            );
            let elevation = dem.elevation_at(position);
            let surface = Geodetic::new(position.latitude, position.longitude, elevation).to_ecef();

            #[allow(
                clippy::cast_possible_truncation,
                reason = "標高は数千 m。f32 の分解能は cm 未満"
            )]
            elevations.push(elevation.get() as f32);
            world_positions.push(surface.as_vec());
            // 楕円体の法線ではなく、地表点における「上」を測地座標から作る。
            up_vectors.push(local_up(position));
            positions.push(to_local(surface.as_vec(), origin.as_vec()));
            #[allow(
                clippy::cast_possible_truncation,
                reason = "u, v は [0,1] の正規化座標。f32 で表現できる"
            )]
            uvs.push([u as f32, v as f32]);
        }
    }

    // --- 地形面の三角形 ---

    let quads = ((resolution - 1) as usize) * ((resolution - 1) as usize);
    let mut indices = Vec::with_capacity(quads * 6);

    for row in 0..resolution - 1 {
        for column in 0..resolution - 1 {
            let north_west = row * resolution + column;
            let north_east = north_west + 1;
            let south_west = north_west + resolution;
            let south_east = south_west + 1;

            // 反時計回り（上から見て）が表。wgpu の既定の front face に合わせる。
            push_triangle(
                &mut indices,
                &mut normal_accumulator,
                &world_positions,
                north_west,
                south_west,
                north_east,
            );
            push_triangle(
                &mut indices,
                &mut normal_accumulator,
                &world_positions,
                north_east,
                south_west,
                south_east,
            );
        }
    }

    // 面法線の平均を正規化する。孤立点（起こらないはず）は上向きで代用する。
    let mut normals: Vec<[f32; 3]> = normal_accumulator
        .iter()
        .zip(&up_vectors)
        .map(|(accumulated, up)| {
            let normal = if accumulated.length_squared() > 0.0 {
                accumulated.normalize()
            } else {
                *up
            };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "単位ベクトルの成分は [-1, 1]。f32 で十分"
            )]
            [normal.x as f32, normal.y as f32, normal.z as f32]
        })
        .collect();

    // 傾斜は法線と「上」のなす角。**高度だけでは崖と台地を区別できない。**
    let mut slopes: Vec<f32> = normals
        .iter()
        .zip(&up_vectors)
        .map(|(normal, up)| {
            let normal = DVec3::new(
                f64::from(normal[0]),
                f64::from(normal[1]),
                f64::from(normal[2]),
            );
            #[allow(
                clippy::cast_possible_truncation,
                reason = "角度は [0, PI]。f32 で十分"
            )]
            let angle = normal.dot(*up).clamp(-1.0, 1.0).acos() as f32;
            angle
        })
        .collect();

    // --- スカート ---

    let skirt_depth = options.skirt_depth.map_or_else(
        || Meters((dem.geometric_error().get() * SKIRT_DEPTH_FACTOR).max(MIN_SKIRT_DEPTH)),
        |depth| Meters(depth.get().max(0.0)),
    );

    if skirt_depth.get() > 0.0 {
        append_skirt(
            resolution,
            skirt_depth.get(),
            &world_positions,
            &up_vectors,
            origin.as_vec(),
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut elevations,
            &mut slopes,
            &mut indices,
        );
    }

    TerrainMesh {
        origin,
        positions,
        normals,
        uvs,
        elevations,
        slopes,
        indices,
        surface_vertex_count: vertex_count,
    }
}

/// 測地座標における「上」（楕円体法線）。
fn local_up(position: Geodetic) -> DVec3 {
    let (latitude, longitude) = (position.latitude.get(), position.longitude.get());
    DVec3::new(
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    )
}

fn to_local(world: DVec3, origin: DVec3) -> [f32; 3] {
    let relative = world - origin;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "タイル中心からの相対。level 9 でも半径 39 km で f32 の分解能は 4.6 mm"
    )]
    [relative.x as f32, relative.y as f32, relative.z as f32]
}

/// 三角形を追加し、その面法線を各頂点へ積む。
fn push_triangle(
    indices: &mut Vec<u32>,
    normals: &mut [DVec3],
    world_positions: &[DVec3],
    a: u32,
    b: u32,
    c: u32,
) {
    indices.extend_from_slice(&[a, b, c]);

    let (pa, pb, pc) = (
        world_positions[a as usize],
        world_positions[b as usize],
        world_positions[c as usize],
    );
    // 面積で重み付けされた面法線（正規化しない外積）。
    // 大きい面ほど法線への寄与が大きくなり、平均が素直になる。
    let face = (pb - pa).cross(pc - pa);
    for index in [a, b, c] {
        normals[index as usize] += face;
    }
}

/// タイルの縁に沿ってスカートを張る。
///
/// 境界を**上から見て時計回り**に辿り、各区間について
/// `(縁 v0, 縁 v1, スカート s0)` と `(縁 v1, スカート s1, スカート s0)` を張ると
/// 法線が外向きになる。逆順にすると内向きになり、裏面カリングで消えて
/// **スカートが存在しないのと同じ**になる。
#[allow(
    clippy::too_many_arguments,
    reason = "頂点属性を配列ごとに持つ形なので引数が増える。まとめる構造体を作るほどの再利用は無い"
)]
fn append_skirt(
    resolution: u32,
    depth: f64,
    world_positions: &[DVec3],
    up_vectors: &[DVec3],
    origin: DVec3,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    elevations: &mut Vec<f32>,
    slopes: &mut Vec<f32>,
    indices: &mut Vec<u32>,
) {
    let last = resolution - 1;
    let at = |row: u32, column: u32| -> u32 { row * resolution + column };

    // 境界を時計回りに一周する（北縁 西→東、東縁 北→南、南縁 東→西、西縁 南→北）。
    let mut ring: Vec<u32> = Vec::with_capacity((4 * last) as usize);
    ring.extend((0..last).map(|column| at(0, column)));
    ring.extend((0..last).map(|row| at(row, last)));
    ring.extend((0..last).map(|column| at(last, last - column)));
    ring.extend((0..last).map(|row| at(last - row, 0)));

    // 縁の頂点を地心方向へ下ろした複製を作る。
    let skirt_base = u32::try_from(positions.len()).unwrap_or(u32::MAX);
    for &vertex in &ring {
        let index = vertex as usize;
        let lowered = world_positions[index] - up_vectors[index] * depth;
        positions.push(to_local(lowered, origin));
        // 法線は縁の頂点と揃える。スカートは隠すための面で、陰影を主張させない。
        normals.push(normals[index]);
        uvs.push(uvs[index]);
        // 縁と同じ値を引き継ぐ。スカートは隠すための面なので、
        // ここで色が変わると継ぎ目が目立つ。
        elevations.push(elevations[index]);
        slopes.push(slopes[index]);
    }

    let ring_length = u32::try_from(ring.len()).unwrap_or(u32::MAX);
    for step in 0..ring_length {
        let next = (step + 1) % ring_length;
        let (v0, v1) = (ring[step as usize], ring[next as usize]);
        let (s0, s1) = (skirt_base + step, skirt_base + next);

        indices.extend_from_slice(&[v0, v1, s0]);
        indices.extend_from_slice(&[v1, s1, s0]);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の標高データ生成。f32 の精度で十分"
    )]

    use super::*;
    use crate::dem::HeightGrid;

    fn flat_tile(id: TileId, elevation: f64) -> DemTile {
        DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(elevation)))
    }

    fn hilly_tile(id: TileId) -> DemTile {
        let size = 33_u32;
        let samples: Vec<f32> = (0..size)
            .flat_map(|row| {
                (0..size).map(move |column| {
                    let x = f64::from(column) / f64::from(size - 1);
                    let y = f64::from(row) / f64::from(size - 1);
                    (500.0 + 300.0 * (x * 6.0).sin() * (y * 4.0).cos()) as f32
                })
            })
            .collect();
        DemTile::new(id.bounds(), HeightGrid::new(size, size, samples))
    }

    /// スカートが「対応する縁の頂点」から実際に何 m 垂れているか。
    ///
    /// タイル全体の地心半径は緯度で変わる（楕円体）ので、面の最小半径と
    /// スカートの最大半径を引き算すると、その変動ぶんが混ざって正しく測れない。
    /// 対応する頂点ペアの距離で測る。ペアは常に最も近い面の頂点になる
    /// （格子間隔はスカート深さよりずっと大きい）。
    fn skirt_hang(mesh: &TerrainMesh) -> (f64, f64) {
        let point = |index: usize| {
            let p = mesh.positions[index];
            DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        };

        let mut shortest = f64::INFINITY;
        let mut longest = 0.0_f64;
        for skirt in mesh.surface_vertex_count..mesh.positions.len() {
            let nearest = (0..mesh.surface_vertex_count)
                .map(|surface| point(skirt).distance(point(surface)))
                .fold(f64::INFINITY, f64::min);
            shortest = shortest.min(nearest);
            longest = longest.max(nearest);
        }
        (shortest, longest)
    }

    fn triangle_normal(mesh: &TerrainMesh, triangle: usize) -> DVec3 {
        let fetch = |offset: usize| {
            let index = mesh.indices[triangle * 3 + offset] as usize;
            let p = mesh.positions[index];
            DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        };
        let (a, b, c) = (fetch(0), fetch(1), fetch(2));
        (b - a).cross(c - a)
    }

    // --- 形 ---

    #[test]
    fn the_vertex_and_index_counts_follow_the_resolution() {
        let id = TileId::new(10, 500, 300);
        let dem = flat_tile(id, 200.0);

        for resolution in [2_u32, 5, 17, 33] {
            let mesh = build_mesh(
                id,
                &dem,
                &MeshOptions {
                    resolution,
                    skirt_depth: Some(Meters::ZERO),
                },
            );

            let expected = (resolution as usize).pow(2);
            assert_eq!(mesh.positions.len(), expected);
            assert_eq!(mesh.normals.len(), expected);
            assert_eq!(mesh.uvs.len(), expected);
            assert_eq!(mesh.surface_vertex_count, expected);

            // 四角形 1 枚につき三角形 2 枚。
            let quads = ((resolution - 1) as usize).pow(2);
            assert_eq!(mesh.triangle_count(), quads * 2);
        }
    }

    #[test]
    fn the_skirt_adds_a_ring_of_vertices_and_faces() {
        let id = TileId::new(10, 500, 300);
        let dem = hilly_tile(id);
        let resolution = 17_u32;

        let without = build_mesh(
            id,
            &dem,
            &MeshOptions {
                resolution,
                skirt_depth: Some(Meters::ZERO),
            },
        );
        let with = build_mesh(
            id,
            &dem,
            &MeshOptions {
                resolution,
                skirt_depth: Some(Meters(50.0)),
            },
        );

        // 縁の頂点数は 4 * (resolution - 1)。角は 1 度ずつ数える。
        let ring = 4 * (resolution as usize - 1);
        assert_eq!(with.positions.len(), without.positions.len() + ring);
        // 帯 1 区間につき三角形 2 枚。
        assert_eq!(with.triangle_count(), without.triangle_count() + ring * 2);
        // 地形面の頂点数はスカートを含まない。
        assert_eq!(with.surface_vertex_count, without.surface_vertex_count);
    }

    #[test]
    fn every_index_points_at_a_real_vertex() {
        // 範囲外の添字は GPU で未定義動作か描画崩れになる。
        let id = TileId::new(10, 500, 300);
        let mesh = build_mesh(id, &hilly_tile(id), &MeshOptions::default());

        let count = u32::try_from(mesh.positions.len()).expect("vertex count fits in u32");
        for &index in &mesh.indices {
            assert!(index < count, "index {index} is outside {count} vertices");
        }
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "indices do not form whole triangles"
        );
    }

    #[test]
    fn every_attribute_is_finite() {
        let id = TileId::new(12, 3_000, 1_500);
        let mesh = build_mesh(id, &hilly_tile(id), &MeshOptions::default());

        for position in &mesh.positions {
            assert!(
                position.iter().all(|v| v.is_finite()),
                "position {position:?}"
            );
        }
        for normal in &mesh.normals {
            assert!(normal.iter().all(|v| v.is_finite()), "normal {normal:?}");
        }
        for uv in &mesh.uvs {
            assert!(uv.iter().all(|v| v.is_finite()), "uv {uv:?}");
        }
    }

    // --- 向き ---

    #[test]
    fn the_surface_faces_away_from_the_earth() {
        // 巻き順を間違えると、裏面カリングで地面が丸ごと消える。
        // 「何も描かれない」という最も分かりにくい壊れ方になる。
        let id = TileId::new(10, 500, 300);
        let dem = hilly_tile(id);
        let mesh = build_mesh(
            id,
            &dem,
            &MeshOptions {
                resolution: 17,
                skirt_depth: Some(Meters::ZERO),
            },
        );

        let outward = mesh.origin.as_vec().normalize();
        for triangle in 0..mesh.triangle_count() {
            let normal = triangle_normal(&mesh, triangle);
            assert!(
                normal.dot(outward) > 0.0,
                "triangle {triangle} faces inward (normal · up = {})",
                normal.normalize().dot(outward)
            );
        }
    }

    #[test]
    fn vertex_normals_point_outward() {
        let id = TileId::new(10, 500, 300);
        let mesh = build_mesh(
            id,
            &hilly_tile(id),
            &MeshOptions {
                resolution: 17,
                skirt_depth: Some(Meters::ZERO),
            },
        );

        let outward = mesh.origin.as_vec().normalize();
        for (index, normal) in mesh.normals.iter().enumerate() {
            let normal = DVec3::new(
                f64::from(normal[0]),
                f64::from(normal[1]),
                f64::from(normal[2]),
            );
            assert!(
                (normal.length() - 1.0).abs() < 1e-5,
                "normal {index} has length {}",
                normal.length()
            );
            assert!(
                normal.dot(outward) > 0.5,
                "normal {index} points sideways or inward (dot = {})",
                normal.dot(outward)
            );
        }
    }

    #[test]
    fn a_flat_tile_has_normals_that_follow_the_curvature() {
        // 平坦な地形でも地球は丸い。全法線が同一方向なら球面を無視している。
        let id = TileId::new(4, 8, 4);
        let mesh = build_mesh(
            id,
            &flat_tile(id, 0.0),
            &MeshOptions {
                resolution: 17,
                skirt_depth: Some(Meters::ZERO),
            },
        );

        let first = DVec3::new(
            f64::from(mesh.normals[0][0]),
            f64::from(mesh.normals[0][1]),
            f64::from(mesh.normals[0][2]),
        );
        let last = mesh.normals[mesh.surface_vertex_count - 1];
        let last = DVec3::new(f64::from(last[0]), f64::from(last[1]), f64::from(last[2]));

        // level 4 のタイルは 22.5°四方。角どうしの法線はそれなりに開く。
        let angle = first.dot(last).clamp(-1.0, 1.0).acos().to_degrees();
        assert!(
            angle > 10.0,
            "the corners of a 22.5° tile only differ by {angle:.1}°; \
             the mesh is probably flat rather than following the ellipsoid"
        );
    }

    // --- スカート ---

    #[test]
    fn skirt_vertices_sit_below_the_edge_they_hang_from() {
        let id = TileId::new(10, 500, 300);
        let resolution = 9_u32;
        let depth = 100.0;
        let mesh = build_mesh(
            id,
            &flat_tile(id, 300.0),
            &MeshOptions {
                resolution,
                skirt_depth: Some(Meters(depth)),
            },
        );

        // 地心方向へ下ろしているので、対応する縁の頂点より必ず地心に近い。
        let radius = |index: usize| {
            let p = mesh.positions[index];
            (mesh.origin.as_vec() + DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
                .length()
        };
        let point = |index: usize| {
            let p = mesh.positions[index];
            DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        };

        for skirt in mesh.surface_vertex_count..mesh.positions.len() {
            let (paired, _) = (0..mesh.surface_vertex_count)
                .map(|surface| (surface, point(skirt).distance(point(surface))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("the surface always has vertices");
            assert!(
                radius(skirt) < radius(paired),
                "skirt vertex {skirt} sits above the edge vertex it hangs from"
            );
        }

        let (shortest, longest) = skirt_hang(&mesh);
        assert!(
            (shortest - depth).abs() < 0.01 && (longest - depth).abs() < 0.01,
            "the skirt hangs {shortest:.2}..{longest:.2} m rather than {depth} m"
        );
    }

    #[test]
    fn the_skirt_faces_outward_rather_than_into_the_tile() {
        // 内向きだと裏面カリングで消え、スカートが無いのと同じになる。
        // 「隙間が塞がらない理由が分からない」という形で時間を溶かす典型。
        let id = TileId::new(10, 500, 300);
        let resolution = 9_u32;
        let mesh = build_mesh(
            id,
            &flat_tile(id, 0.0),
            &MeshOptions {
                resolution,
                skirt_depth: Some(Meters(200.0)),
            },
        );

        let surface_triangles = ((resolution - 1) as usize).pow(2) * 2;
        let mut checked = 0_u32;

        for triangle in surface_triangles..mesh.triangle_count() {
            let normal = triangle_normal(&mesh, triangle).normalize();

            // 三角形の重心（ローカル座標）。タイル中心が原点なので、
            // 重心の水平成分がそのまま「外向き」を指す。
            let centroid = (0..3)
                .map(|offset| {
                    let index = mesh.indices[triangle * 3 + offset] as usize;
                    let p = mesh.positions[index];
                    DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
                })
                .sum::<DVec3>()
                / 3.0;

            let up = mesh.origin.as_vec().normalize();
            let horizontal = centroid - up * centroid.dot(up);
            if horizontal.length() < 1.0 {
                continue; // 中心付近の縮退。判定できない
            }

            assert!(
                normal.dot(horizontal.normalize()) > 0.0,
                "skirt triangle {triangle} faces inward (dot = {})",
                normal.dot(horizontal.normalize())
            );
            checked += 1;
        }

        assert!(checked > 0, "no skirt triangle was actually checked");
    }

    #[test]
    fn the_automatic_skirt_depth_grows_with_the_terrain_roughness() {
        // 平坦な地形でも最低限の深さが要る。隣が粗い LOD なら段差は必ず出る。
        let id = TileId::new(10, 500, 300);
        let options = MeshOptions {
            resolution: 17,
            skirt_depth: None,
        };

        let flat = build_mesh(id, &flat_tile(id, 100.0), &options);
        let rough = build_mesh(id, &hilly_tile(id), &options);

        let (flat_hang, _) = skirt_hang(&flat);
        let (rough_hang, _) = skirt_hang(&rough);

        assert!(
            (flat_hang - MIN_SKIRT_DEPTH).abs() < 0.01,
            "a flat tile got a {flat_hang:.1} m skirt; it should fall back to the minimum"
        );
        assert!(
            rough_hang > flat_hang,
            "rough terrain ({rough_hang:.1} m) should get a deeper skirt than flat ({flat_hang:.1} m)"
        );
    }

    // --- 位置 ---

    #[test]
    fn the_mesh_sits_where_the_tile_is() {
        let id = TileId::new(12, 3_000, 1_500);
        let dem = flat_tile(id, 750.0);
        let mesh = build_mesh(id, &dem, &MeshOptions::default());

        let origin_geodetic = mesh.origin.to_geodetic();
        let centre = id.center();
        assert!((origin_geodetic.latitude.get() - centre.latitude.get()).abs() < 1e-9);
        assert!((origin_geodetic.longitude.get() - centre.longitude.get()).abs() < 1e-9);
        assert!(
            (origin_geodetic.altitude.get() - 750.0).abs() < 1.0,
            "the mesh origin sits at {} m rather than on the terrain",
            origin_geodetic.altitude
        );
    }

    #[test]
    fn every_vertex_carries_an_elevation_and_a_slope() {
        // 個数がずれると、描画側で頂点と色が食い違う。**スカートも含めて**揃うこと。
        let id = TileId::new(9, 220, 100);
        let mesh = build_mesh(id, &flat_tile(id, 120.0), &MeshOptions::default());

        assert_eq!(mesh.elevations.len(), mesh.positions.len());
        assert_eq!(mesh.slopes.len(), mesh.positions.len());
        assert!(
            mesh.positions.len() > mesh.surface_vertex_count,
            "this tile should have a skirt, otherwise the check above is weaker than it looks"
        );
    }

    #[test]
    fn the_stored_elevation_matches_the_terrain() {
        // 別経路で作った値がずれていないか、外側から確かめる。
        let id = TileId::new(9, 220, 100);
        let mesh = build_mesh(id, &flat_tile(id, 340.0), &MeshOptions::default());

        for elevation in &mesh.elevations[..mesh.surface_vertex_count] {
            assert!(
                (elevation - 340.0).abs() < 0.05,
                "a vertex of a 340 m plateau reports {elevation} m"
            );
        }
    }

    #[test]
    fn a_flat_tile_has_no_slope() {
        // 平らな地形で傾斜が出るなら、法線か「上」のどちらかが狂っている。
        let id = TileId::new(9, 220, 100);
        let mesh = build_mesh(id, &flat_tile(id, 500.0), &MeshOptions::default());

        let steepest = mesh.slopes[..mesh.surface_vertex_count]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        assert!(
            steepest < 0.02,
            "a flat tile reports a slope of {:.2}°",
            steepest.to_degrees()
        );
    }

    #[test]
    fn slopes_are_finite_everywhere() {
        // NaN は全状態に伝播して原因特定が極めて困難になる。
        let id = TileId::new(9, 220, 100);
        let mesh = build_mesh(id, &flat_tile(id, 0.0), &MeshOptions::default());
        assert!(
            mesh.slopes.iter().all(|slope| slope.is_finite()),
            "a slope came out NaN or infinite"
        );
        assert!(mesh.elevations.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn vertices_reproduce_the_terrain_elevation() {
        // メッシュが DEM とずれていたら、見える地形と当たる地形が食い違う。
        let id = TileId::new(12, 3_000, 1_500);
        let dem = hilly_tile(id);
        let resolution = 17_u32;
        let mesh = build_mesh(
            id,
            &dem,
            &MeshOptions {
                resolution,
                skirt_depth: Some(Meters::ZERO),
            },
        );

        let bounds = id.bounds();
        let steps = f64::from(resolution - 1);
        for row in 0..resolution {
            for column in 0..resolution {
                let index = (row * resolution + column) as usize;
                let p = mesh.positions[index];
                let world = mesh.origin.as_vec()
                    + DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                let geodetic = Ecef::from_vec(world).to_geodetic();

                let expected = dem.elevation_at(Geodetic::new(
                    Radians(bounds.north.get() - f64::from(row) / steps * bounds.height().get()),
                    Radians(bounds.west.get() + f64::from(column) / steps * bounds.width().get()),
                    Meters::ZERO,
                ));

                assert!(
                    (geodetic.altitude.get() - expected.get()).abs() < 0.5,
                    "vertex ({column}, {row}) is at {} m but the DEM says {expected}",
                    geodetic.altitude
                );
            }
        }
    }

    #[test]
    fn relative_positions_stay_small_enough_for_f32() {
        // タイル中心からの相対にしている理由そのもの。
        // 世界座標をそのまま f32 にすると 76cm に量子化する（ADR-0002）。
        for level in [8_u8, 10, 12] {
            let id = TileId::new(level, 100, 60);
            let mesh = build_mesh(id, &flat_tile(id, 0.0), &MeshOptions::default());

            let worst = mesh
                .positions
                .iter()
                .map(|p| {
                    f64::from(p[0])
                        .hypot(f64::from(p[1]))
                        .hypot(f64::from(p[2]))
                })
                .fold(0.0_f64, f64::max);

            // f32 の分解能 = 距離 / 2^23。1 cm 以下に収まること。
            let resolution_m = worst / f64::from(1_u32 << 23);
            assert!(
                resolution_m < 0.01,
                "level {level}: vertices reach {worst:.0} m from the tile centre, \
                 giving {resolution_m:.4} m of f32 resolution"
            );
        }
    }

    #[test]
    #[should_panic(expected = "mesh resolution must be at least 2")]
    fn a_degenerate_resolution_is_rejected() {
        let id = TileId::new(10, 500, 300);
        let _ = build_mesh(
            id,
            &flat_tile(id, 0.0),
            &MeshOptions {
                resolution: 1,
                skirt_depth: None,
            },
        );
    }
}
