//! 滑走路灯。
//!
//! # なぜ要るのか
//!
//! **夜間に滑走路が見えないと降りられない。** 環境光の階調は入れたが、
//! 夜そのものが暗いのは物理的に正しい（実際の市民薄明の照度は正午の
//! 0.4% 程度）。世界を明るくするのは誤魔化しで、実機と同じく
//! **滑走路の側が光る**のが正しい答え。
//!
//! # 色の規格
//!
//! ICAO / FAA の規格に従う。**この色は好みではなく決まりごと**で、
//! パイロットは色で滑走路のどちら側かを判断する。
//!
//! | 灯火 | 色 | 意味 |
//! |---|---|---|
//! | 縁灯 | 白 | 滑走路の両縁 |
//! | 進入端灯 | 緑 | ここから始まる |
//! | 末端灯 | 赤 | ここで終わる |
//!
//! **進入端と末端は同じ灯器を両側から見たもの**で、実機では片面が緑・
//! 反対面が赤に見える。ここでは面の向きを持たない発光板で近似し、
//! 進入端側を緑、末端側を赤に塗り分ける。
//!
//! # 発光の表現
//!
//! 光源（`PointLight`）を灯火の数だけ置くと、Bevy の前方クラスタリングの
//! 上限を軽く超える（滑走路 1 本で 100 個以上になる）。**自己発光する板**
//! （`StandardMaterial::emissive`）で表す。地面を照らさないが、
//! 遠方から滑走路の位置と向きが読める、という目的には足りる。

use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, Meters, Radians};

/// 灯火を舗装面からどれだけ浮かせるか。実機の埋込灯とほぼ同じ高さ。
const LIGHT_LIFT: f64 = 0.12;

/// 灯火 1 つの大きさ。実機の灯器より大きいが、**遠方から見えないと
/// 意味が無い**ので視認性を優先する。
const LIGHT_SIZE: f64 = 1.6;

/// 縁灯の間隔。実機の規格は 60 m 以下。
const EDGE_SPACING: f64 = 60.0;

/// 縁灯を滑走路の縁からどれだけ外へ置くか。
const EDGE_MARGIN: f64 = 1.5;

/// 末端灯の本数（片側）。中心線を挟んで左右対称に並べる。
const THRESHOLD_LIGHT_COUNT: usize = 5;

/// 縁灯の色（sRGB）。**規格で決まっている。**
const EDGE_COLOR: [f32; 3] = [1.0, 0.98, 0.90];
/// 進入端灯の色（sRGB）。
const THRESHOLD_GREEN: [f32; 3] = [0.10, 1.0, 0.25];
/// 末端灯の色（sRGB）。
const END_RED: [f32; 3] = [1.0, 0.12, 0.10];

/// 灯火の明るさ。`emissive` は線形の輝度なので、露出
/// （`Exposure::SUNLIGHT`）に対して見える強さを持たせる。
const EMISSIVE_STRENGTH: f32 = 6_000.0;

/// 灯火が完全に点く太陽高度（度）。市民薄明の下限。
const FULL_ON_ELEVATION_DEGREES: f64 = -6.0;

/// 灯火が完全に消える太陽高度（度）。
///
/// **日没ちょうどではなく、少し上で消す。** 実機も日没前から点けるが、
/// 昼間に光って見えるのは不自然なので、地平線より上で切る。
const FULL_OFF_ELEVATION_DEGREES: f64 = 3.0;

/// 滑走路灯の実体につける印。
///
/// **全点灯時の発光色を持たせる。** 明るさを動かすとき、現在値から
/// 逆算すると誤差が溜まるうえ、一度 0 にすると二度と戻らない。
/// 常に「基準色 × 比率」で計算する。
#[derive(Component, Debug, Clone, Copy)]
pub struct AirportLights {
    /// 全点灯時の線形 RGB。
    pub full_emissive: LinearRgba,
}

impl AirportLights {
    /// 指定した比率での発光色。
    #[must_use]
    pub fn emissive_at(self, fraction: f32) -> LinearRgba {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        LinearRgba::rgb(
            self.full_emissive.red * fraction,
            self.full_emissive.green * fraction,
            self.full_emissive.blue * fraction,
        )
    }
}

/// 後方互換のための滑走路灯名。
///
/// 発光の明暗制御は空港面灯火で共通なので、実体は [`AirportLights`] である。
pub type RunwayLights = AirportLights;

/// 太陽高度から灯火の明るさの比率を出す。
///
/// 1 が全点灯、0 が消灯。**両端で滑らかに繋ぐ**（線形だと点灯の
/// 瞬間に段差が見える）。非有限な入力は消灯側へ倒す。
#[must_use]
pub fn light_intensity_fraction(sun_elevation: Radians) -> f32 {
    let degrees = sun_elevation.to_degrees().get();
    if !degrees.is_finite() {
        return 0.0;
    }
    // 高いほど暗く。FULL_OFF で 0、FULL_ON で 1。
    let span = FULL_OFF_ELEVATION_DEGREES - FULL_ON_ELEVATION_DEGREES;
    let t = ((FULL_OFF_ELEVATION_DEGREES - degrees) / span).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, reason = "0..=1 の比率。f32 で十分")]
    let t = t as f32;
    // smoothstep。両端で微分が 0 になり、点灯・消灯に折れ目が出ない。
    t * t * (3.0 - 2.0 * t)
}

/// 灯火 1 つの配置（滑走路基準）と色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunwayLight {
    /// 進入端からの前方距離。
    pub along: Meters,
    /// 中心線からの横ずれ。右が正。
    pub across: Meters,
    /// sRGB の色。
    pub color: [f32; 3],
}

/// 滑走路灯の配置を決める。**Bevy に依存しない純関数。**
///
/// 縁灯は両縁に `EDGE_SPACING` 間隔、進入端に緑、末端に赤を並べる。
/// 長さか幅が非有限・非正なら空を返す（灯火の無い滑走路になるだけで、
/// 描画は壊れない）。
#[must_use]
pub fn runway_light_layout(length: Meters, width: Meters) -> Vec<RunwayLight> {
    if !length.get().is_finite() || length.get() <= 0.0 {
        return Vec::new();
    }
    if !width.get().is_finite() || width.get() <= 0.0 {
        return Vec::new();
    }
    let length = length.get();
    let width = width.get();

    let mut lights = Vec::new();
    let edge = width * 0.5 + EDGE_MARGIN;

    // 縁灯。両端も必ず置くよう、区間数から刻む。
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "滑走路長は数 km、間隔は 60 m。区間数は 3 桁に収まる"
    )]
    let intervals = (length / EDGE_SPACING).ceil().max(1.0) as usize;
    for step in 0..=intervals {
        #[allow(clippy::cast_precision_loss, reason = "区間数は 3 桁")]
        let along = length * (step as f64) / (intervals as f64);
        for side in [-1.0, 1.0] {
            lights.push(RunwayLight {
                along: Meters(along),
                across: Meters(side * edge),
                color: EDGE_COLOR,
            });
        }
    }

    // 進入端（緑）と末端（赤）。中心線を挟んで左右対称。
    for (along, color) in [(0.0, THRESHOLD_GREEN), (length, END_RED)] {
        for index in 0..THRESHOLD_LIGHT_COUNT {
            #[allow(clippy::cast_precision_loss, reason = "灯火の本数は 1 桁")]
            let fraction = (index as f64 + 0.5) / (THRESHOLD_LIGHT_COUNT as f64);
            let offset = fraction * width * 0.5;
            for side in [-1.0, 1.0] {
                lights.push(RunwayLight {
                    along: Meters(along),
                    across: Meters(side * offset),
                    color,
                });
            }
        }
    }

    lights
}

/// 灯火 1 色ぶんのメッシュと材質。
#[derive(Debug)]
pub struct RunwayLightGroup {
    pub mesh: Mesh,
    pub material: StandardMaterial,
    /// この群の sRGB 色。検査とデバッグ用。
    pub color: [f32; 3],
    /// 全点灯時の発光色。`RunwayLights` に持たせて明るさを動かす。
    pub marker: RunwayLights,
}

/// 滑走路灯を色ごとにまとめたメッシュを作る。
///
/// **灯火ごとにエンティティを作ると描画コマンドが 100 個を超える**ので、
/// 色で束ねて 3 枚に収める。原点は滑走路中心で、`terrain_mesh_bundle`
/// に渡せば地形タイルと同じ経路で floating origin と回転が付く。
#[must_use]
pub fn runway_light_meshes(
    threshold: Geodetic,
    heading: Radians,
    length: Meters,
    width: Meters,
) -> (Vec<RunwayLightGroup>, Ecef) {
    let layout = runway_light_layout(length, width);
    let centre = light_point(threshold, heading, length.get() * 0.5, 0.0);
    let origin = centre.to_ecef();
    if layout.is_empty() {
        return (Vec::new(), origin);
    }

    // 色ごとに束ねる。色は 3 種類しかないので線形探索で足りる。
    //
    // **ビット比較にする。** 色はすべて同じ定数から来るので、
    // 近似比較より厳密なビット一致のほうが意図に合う（clippy の
    // float_cmp も、浮動小数の等値比較を許さない）。
    let mut groups: Vec<([f32; 3], Vec<RunwayLight>)> = Vec::new();
    for light in layout {
        if let Some(group) = groups
            .iter_mut()
            .find(|(color, _)| same_color(*color, light.color))
        {
            group.1.push(light);
        } else {
            groups.push((light.color, vec![light]));
        }
    }

    let meshes = groups
        .into_iter()
        .map(|(color, lights)| {
            let mesh = quads_for(threshold, heading, origin, &lights);
            let emissive = LinearRgba::rgb(
                crate::srgb_to_linear(color[0]) * EMISSIVE_STRENGTH,
                crate::srgb_to_linear(color[1]) * EMISSIVE_STRENGTH,
                crate::srgb_to_linear(color[2]) * EMISSIVE_STRENGTH,
            );
            let material = StandardMaterial {
                // **base_color は黒。** 灯火は自分で光るのであって、
                // 太陽に照らされて見えるのではない。昼に白い板が
                // 並んで見えるのを防ぐ。
                base_color: Color::BLACK,
                emissive,
                ..default()
            };
            RunwayLightGroup {
                mesh,
                material,
                color,
                marker: RunwayLights {
                    full_emissive: emissive,
                },
            }
        })
        .collect();

    (meshes, origin)
}

/// 灯火の板を並べたメッシュ。
fn quads_for(threshold: Geodetic, heading: Radians, origin: Ecef, lights: &[RunwayLight]) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let half = LIGHT_SIZE * 0.5;
    for light in lights {
        let base = u32::try_from(positions.len()).unwrap_or(u32::MAX);
        for (along_offset, across_offset) in
            [(-half, -half), (-half, half), (half, half), (half, -half)]
        {
            let point = light_point(
                threshold,
                heading,
                light.along.get() + along_offset,
                light.across.get() + across_offset,
            );
            let ecef = point.to_ecef();
            let above = Geodetic::new(
                point.latitude,
                point.longitude,
                Meters(point.altitude.get() + 1.0),
            )
            .to_ecef();
            let up = (above.as_vec() - ecef.as_vec()).normalize();
            let relative = ecef.as_vec() - origin.as_vec();

            #[allow(
                clippy::cast_possible_truncation,
                reason = "原点相対で数 km 以内。f32 の分解能は mm 未満"
            )]
            {
                positions.push([relative.x as f32, relative.y as f32, relative.z as f32]);
                normals.push([up.x as f32, up.y as f32, up.z as f32]);
            }
        }
        // 滑走路の舗装と同じ巻き順。前方 × 右方 = 下向きなので、
        // (0,1,2)/(0,2,3) で上を向く。
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

/// 2 つの色が同じ定数から来たか。
///
/// 浮動小数の等値比較を避けるためビットで見る。色は定数なので、
/// 同じ灯火種別なら必ずビットまで一致する。
fn same_color(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(left, right)| left.to_bits() == right.to_bits())
}

/// 滑走路基準の座標から測地点を作る。舗装より少し高い位置。
fn light_point(threshold: Geodetic, heading: Radians, along: f64, across: f64) -> Geodetic {
    let (sin, cos) = heading.get().sin_cos();
    let north = along * cos - across * sin;
    let east = along * sin + across * cos;
    let moved = threshold.offset_by(Meters(north), Meters(east));
    Geodetic::new(
        moved.latitude,
        moved.longitude,
        Meters(moved.altitude.get() + LIGHT_LIFT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;

    const LENGTH: Meters = Meters(2500.0);
    const WIDTH: Meters = Meters(45.0);

    fn layout() -> Vec<RunwayLight> {
        runway_light_layout(LENGTH, WIDTH)
    }

    // --- 点灯・消灯 ---

    #[test]
    fn the_lights_are_off_in_daylight() {
        // 昼に光る板が並んでいたら、滑走路が読めなくなる。
        for degrees in [10.0, 30.0, 78.0] {
            let fraction = light_intensity_fraction(Degrees(degrees).to_radians());
            assert!(
                fraction.abs() < 1e-6,
                "the lights should be off at {degrees} deg, got {fraction}"
            );
        }
    }

    #[test]
    fn the_lights_are_fully_on_at_night() {
        for degrees in [-6.0, -12.0, -30.0] {
            let fraction = light_intensity_fraction(Degrees(degrees).to_radians());
            assert!(
                (fraction - 1.0).abs() < 1e-6,
                "the lights should be full at {degrees} deg, got {fraction}"
            );
        }
    }

    #[test]
    fn the_lights_come_up_before_the_sun_is_down() {
        // 実機も日没前から点ける。**地平線で既に点き始めていること**が要件で、
        // 「半分以上」ではない。点灯域 -6°..+3° の中で 0° は上寄りなので、
        // ここでの比率は 1/4 程度になる（実測 0.259）。
        // 完全点灯は市民薄明の下限に合わせてある。
        let at_horizon = light_intensity_fraction(Degrees(0.0).to_radians());
        assert!(
            at_horizon > 0.15,
            "the lights should already be coming up at sunset, got {at_horizon}"
        );
        // 薄暮の半ばでは、はっきり見える明るさになっていること。
        // 薄暮の半ば（-3°）は smoothstep(2/3) = 0.741。点灯域の 2/3 まで
        // 進んだ位置なので、この値が正しい。
        let civil_twilight = light_intensity_fraction(Degrees(-3.0).to_radians());
        assert!(
            civil_twilight > 0.7,
            "the lights should be near full in civil twilight, got {civil_twilight}"
        );
    }

    #[test]
    fn the_transition_is_monotonic_and_smooth() {
        // 太陽が下がるほど明るく、段差が無いこと。
        let mut previous = 0.0_f32;
        let mut degrees = 6.0;
        while degrees >= -12.0 {
            let fraction = light_intensity_fraction(Degrees(degrees).to_radians());
            assert!(
                fraction >= previous - 1e-6,
                "the lights dimmed while the sun set: {previous} then {fraction} at {degrees}"
            );
            assert!(
                (fraction - previous) < 0.15,
                "the lights jumped by {} at {degrees} deg",
                fraction - previous
            );
            previous = fraction;
            degrees -= 0.25;
        }
        assert!((previous - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_broken_sun_angle_leaves_the_lights_off() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let fraction = light_intensity_fraction(Radians(value));
            assert!(
                fraction.is_finite() && (0.0..=1.0).contains(&fraction),
                "a broken sun angle produced {fraction}"
            );
        }
    }

    // --- 配置 ---

    #[test]
    fn the_edges_are_lit_along_the_whole_runway() {
        // 実機の規格は 60 m 以下の間隔。両端も点くこと。
        let lights = layout();
        let mut edge_positions: Vec<f64> = lights
            .iter()
            .filter(|light| same_color(light.color, EDGE_COLOR) && light.across.get() > 0.0)
            .map(|light| light.along.get())
            .collect();
        edge_positions.sort_by(|a, b| a.partial_cmp(b).expect("finite"));

        assert!(
            edge_positions
                .first()
                .is_some_and(|value| value.abs() < 1e-6),
            "the approach end must be lit"
        );
        assert!(
            edge_positions
                .last()
                .is_some_and(|value| (value - LENGTH.get()).abs() < 1e-6),
            "the far end must be lit"
        );
        for pair in edge_positions.windows(2) {
            assert!(
                pair[1] - pair[0] <= EDGE_SPACING + 1e-6,
                "edge lights are {} m apart, the standard is {EDGE_SPACING} m",
                pair[1] - pair[0]
            );
        }
    }

    #[test]
    fn the_edge_lights_sit_outside_the_pavement() {
        // 舗装の上に置くと、着陸時に踏む位置になる。
        let lights = layout();
        for light in lights
            .iter()
            .filter(|light| same_color(light.color, EDGE_COLOR))
        {
            assert!(
                light.across.get().abs() > WIDTH.get() * 0.5,
                "an edge light sits {} m off centre, inside the {} m runway",
                light.across.get(),
                WIDTH.get()
            );
        }
    }

    #[test]
    fn the_approach_end_is_green_and_the_far_end_is_red() {
        // **色は規格。** 取り違えるとパイロットが逆向きに降りる。
        let lights = layout();
        let green: Vec<&RunwayLight> = lights
            .iter()
            .filter(|light| same_color(light.color, THRESHOLD_GREEN))
            .collect();
        let red: Vec<&RunwayLight> = lights
            .iter()
            .filter(|light| same_color(light.color, END_RED))
            .collect();

        assert!(!green.is_empty() && !red.is_empty());
        for light in green {
            assert!(
                light.along.get().abs() < 1e-6,
                "a green light is not at the approach end"
            );
        }
        for light in red {
            assert!(
                (light.along.get() - LENGTH.get()).abs() < 1e-6,
                "a red light is not at the far end"
            );
        }
    }

    #[test]
    fn the_threshold_lights_are_symmetric_about_the_centreline() {
        let lights = layout();
        let green: Vec<f64> = lights
            .iter()
            .filter(|light| same_color(light.color, THRESHOLD_GREEN))
            .map(|light| light.across.get())
            .collect();
        let sum: f64 = green.iter().sum();
        assert!(
            sum.abs() < 1e-6,
            "the threshold lights are not symmetric, offsets sum to {sum}"
        );
    }

    #[test]
    fn a_degenerate_runway_produces_no_lights_rather_than_panicking() {
        for (length, width) in [
            (Meters(0.0), WIDTH),
            (Meters(-100.0), WIDTH),
            (Meters(f64::NAN), WIDTH),
            (LENGTH, Meters(0.0)),
            (LENGTH, Meters(f64::INFINITY)),
        ] {
            let lights = runway_light_layout(length, width);
            assert!(
                lights.is_empty(),
                "a degenerate runway ({length}, {width}) produced {} lights",
                lights.len()
            );
        }
    }

    #[test]
    fn every_light_position_is_finite() {
        for light in layout() {
            assert!(
                light.along.get().is_finite() && light.across.get().is_finite(),
                "a light landed at a non-finite position"
            );
        }
    }

    // --- メッシュ ---

    #[test]
    fn the_lights_are_grouped_into_three_meshes() {
        // 灯火ごとにエンティティを作ると描画コマンドが 100 を超える。
        let (groups, _) = runway_light_meshes(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Degrees(50.0).to_radians(),
            LENGTH,
            WIDTH,
        );
        assert_eq!(groups.len(), 3, "expected white, green and red groups");
        let total: usize = groups.iter().map(|group| group.mesh.count_vertices()).sum();
        assert_eq!(total, layout().len() * 4, "each light is one quad");
    }

    #[test]
    fn the_lights_emit_rather_than_reflect() {
        // base_color を残すと、昼に白い板が並んで見える。
        let (groups, _) = runway_light_meshes(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Degrees(50.0).to_radians(),
            LENGTH,
            WIDTH,
        );
        for group in &groups {
            let base = group.material.base_color.to_linear();
            assert!(
                base.red + base.green + base.blue < 1e-6,
                "a light group reflects sunlight: {base:?}"
            );
            let emissive = group.material.emissive;
            assert!(
                emissive.red + emissive.green + emissive.blue > 1.0,
                "a light group does not emit: {emissive:?}"
            );
        }
    }

    #[test]
    fn dimming_keeps_the_full_brightness_recoverable() {
        // **現在値から逆算する実装を防ぐ。** 一度 0 まで落とすと
        // 二度と戻らない、という不具合を実際に作った（夜に灯火が
        // まったく見えなかった）。基準色は marker が持つ。
        let marker = RunwayLights {
            full_emissive: LinearRgba::rgb(100.0, 200.0, 50.0),
        };

        let off = marker.emissive_at(0.0);
        assert!(off.red + off.green + off.blue < 1e-6, "0 で消えること");

        // 消えた後でも、全点灯に戻せること。
        let back = marker.emissive_at(1.0);
        assert!((back.red - 100.0).abs() < 1e-3);
        assert!((back.green - 200.0).abs() < 1e-3);
        assert!((back.blue - 50.0).abs() < 1e-3);

        // 半分なら半分。色相は保たれること（緑が赤の 2 倍のまま）。
        let half = marker.emissive_at(0.5);
        assert!((half.green / half.red - 2.0).abs() < 1e-3, "色相が崩れた");
    }

    #[test]
    fn a_broken_fraction_leaves_the_lights_dark_rather_than_blinding() {
        let marker = RunwayLights {
            full_emissive: LinearRgba::rgb(100.0, 100.0, 100.0),
        };
        for fraction in [f32::NAN, f32::INFINITY, -1.0, 5.0] {
            let emissive = marker.emissive_at(fraction);
            assert!(
                emissive.red.is_finite() && (0.0..=100.0).contains(&emissive.red),
                "fraction {fraction} produced {emissive:?}"
            );
        }
    }

    #[test]
    fn the_group_marker_matches_the_material_it_ships_with() {
        // marker と material が食い違うと、最初のフレームで色が飛ぶ。
        let (groups, _) = runway_light_meshes(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Degrees(50.0).to_radians(),
            LENGTH,
            WIDTH,
        );
        for group in &groups {
            let shipped = group.material.emissive;
            let full = group.marker.full_emissive;
            assert!(
                (shipped.red - full.red).abs() < 1e-3
                    && (shipped.green - full.green).abs() < 1e-3
                    && (shipped.blue - full.blue).abs() < 1e-3,
                "marker {full:?} does not match material {shipped:?}"
            );
        }
    }

    #[test]
    fn the_lights_follow_the_curvature_of_the_earth() {
        // 接平面に置くと 2.5 km の両端が沈み、舗装にめり込む。
        let elevation = 8.0;
        let (groups, origin) = runway_light_meshes(
            Geodetic::from_degrees(35.548, 139.775, elevation),
            Degrees(50.0).to_radians(),
            LENGTH,
            WIDTH,
        );
        for group in &groups {
            let positions = match group.mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values,
                _ => panic!("positions must be f32x3"),
            };
            for position in positions {
                let world = Ecef::from_vec(
                    origin.as_vec()
                        + glam::DVec3::new(
                            f64::from(position[0]),
                            f64::from(position[1]),
                            f64::from(position[2]),
                        ),
                );
                let altitude = world.to_geodetic().altitude.get();
                assert!(
                    (altitude - elevation - LIGHT_LIFT).abs() < 0.3,
                    "a light sits at {altitude} m, expected about {}",
                    elevation + LIGHT_LIFT
                );
            }
        }
    }

    #[test]
    fn a_degenerate_runway_still_reports_an_origin() {
        // 灯火が無くても原点は返す。呼び出し側が Option を剥がさずに済む。
        let (groups, origin) = runway_light_meshes(
            Geodetic::from_degrees(35.548, 139.775, 8.0),
            Degrees(50.0).to_radians(),
            Meters(0.0),
            WIDTH,
        );
        assert!(groups.is_empty());
        assert!(origin.as_vec().is_finite());
    }
}
