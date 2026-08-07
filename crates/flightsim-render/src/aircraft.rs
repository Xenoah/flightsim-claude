//! 機体の見た目。
//!
//! # なぜプレースホルダから作るのか
//!
//! 外部視点で機体が映らないと、姿勢が合っているのか、そもそも spawn されて
//! いるのかが分からない。**3D モデルを調達する前に「映る」状態を作っておく**と、
//! モデルを差し替えたときの比較対象になる。
//!
//! ここで作るのは箱を組み合わせただけの形だが、**寸法は `AircraftConfig` から
//! 引く**ので、翼幅を変えれば見た目も変わる。物理と見た目が食い違わない。
//!
//! # 座標系
//!
//! 各パーツの `Transform` は**機体軸**（X = 前、Y = 右、Z = 下）。
//! 機体エンティティの子として付けると、親の `Transform`（機体軸 → 描画座標）が
//! 効いて正しい向きになる。
//!
//! **Bevy の標準的な Y-up で組まないこと。** 機体軸は Z が下向きで、
//! 一般的なモデル座標系とは違う。ここを混ぜると機体が横倒しや逆さまになる。

use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use flightsim_core::Meters;
use flightsim_fdm::AircraftConfig;

/// 機体を構成する 1 パーツ。
#[derive(Debug, Clone)]
pub struct AircraftPart {
    /// パーツの名前。デバッグ表示用。
    pub name: &'static str,
    pub mesh: Mesh,
    /// **機体軸**での配置。
    pub transform: Transform,
    pub color: Color,
}

/// 機体の全長。
///
/// `AircraftConfig` は空力に必要な寸法（翼幅・翼弦・翼面積）しか持たない。
/// 胴体の長さは飛び方に影響しないので入っていない。**見た目のためだけの値**
/// であることを明示するため、ここに置く。
///
/// 軽single機の代表値。翼幅 11 m に対して全長 8.3 m は妥当な比率。
const FUSELAGE_LENGTH: f64 = 8.3;

/// 胴体の太さ（幅・高さ）。
const FUSELAGE_WIDTH: f64 = 1.2;
const FUSELAGE_HEIGHT: f64 = 1.4;

/// 主翼の厚み。
const WING_THICKNESS: f64 = 0.18;

/// 重心から機首までの距離が全長に占める割合。
///
/// 重心は翼の前縁付近にあるので、前より後ろのほうが長い。
const NOSE_FRACTION: f64 = 0.38;

/// 箱を作る。寸法は機体軸の (X 長さ, Y 幅, Z 高さ)。
fn box_mesh(length: f64, width: f64, height: f64) -> Mesh {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "機体の寸法は数メートル。f32 の分解能は十分"
    )]
    Mesh::from(Cuboid::new(length as f32, width as f32, height as f32))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "機体の寸法は数メートル。f32 の分解能は十分"
)]
fn at(x: f64, y: f64, z: f64) -> Transform {
    Transform::from_xyz(x as f32, y as f32, z as f32)
}

/// 機体設定から見た目のパーツを組み立てる。
///
/// 箱を並べただけの仮の形。**3D モデルを入れたら差し替える。**
/// 寸法は設定から引くので、機体を差し替えれば見た目も追随する。
#[must_use]
pub fn placeholder_parts(config: &AircraftConfig) -> Vec<AircraftPart> {
    let span = config.geometry.wing_span.get();
    let chord = config.geometry.mean_chord.get();

    let nose = FUSELAGE_LENGTH * NOSE_FRACTION;
    let tail = FUSELAGE_LENGTH - nose;
    // 胴体の中心は重心より少し後ろ。
    let fuselage_centre = (nose - tail) * 0.5;

    // 尾翼の寸法は主翼に対する比率で決める。実機の尾翼容積比の目安から。
    let tail_span = span * 0.35;
    let tail_chord = chord * 0.7;
    let fin_height = FUSELAGE_HEIGHT * 1.1;

    let body = Color::srgb(0.88, 0.88, 0.90);
    let accent = Color::srgb(0.20, 0.35, 0.65);

    vec![
        AircraftPart {
            name: "fuselage",
            mesh: box_mesh(FUSELAGE_LENGTH, FUSELAGE_WIDTH, FUSELAGE_HEIGHT),
            transform: at(fuselage_centre, 0.0, 0.0),
            color: body,
        },
        AircraftPart {
            name: "wing",
            // 主翼は重心のすぐ後ろ。高翼機なので胴体の上（機体軸では -Z）。
            mesh: box_mesh(chord, span, WING_THICKNESS),
            transform: at(-0.1, 0.0, -FUSELAGE_HEIGHT * 0.45),
            color: body,
        },
        AircraftPart {
            name: "horizontal stabiliser",
            mesh: box_mesh(tail_chord, tail_span, WING_THICKNESS * 0.8),
            transform: at(-tail * 0.86, 0.0, -FUSELAGE_HEIGHT * 0.2),
            color: body,
        },
        AircraftPart {
            name: "vertical stabiliser",
            // 垂直尾翼は上へ伸びる。機体軸の Z は下向きなので中心は負。
            mesh: box_mesh(tail_chord * 1.2, WING_THICKNESS * 0.8, fin_height),
            transform: at(
                -tail * 0.82,
                0.0,
                -FUSELAGE_HEIGHT * 0.35 - fin_height * 0.5,
            ),
            color: accent,
        },
        AircraftPart {
            name: "propeller disc",
            // 機首の円盤。回転はしないが、前がどちらか一目で分かる。
            mesh: box_mesh(0.08, WING_THICKNESS, FUSELAGE_HEIGHT * 1.3),
            transform: at(nose, 0.0, 0.0),
            color: accent,
        },
    ]
}

/// 機体の全長・全幅・全高（機体軸）。テストと視点距離の決定に使う。
#[must_use]
pub fn placeholder_extents(config: &AircraftConfig) -> (Meters, Meters, Meters) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for part in placeholder_parts(config) {
        let Some(bounds) = part.mesh.compute_aabb() else {
            continue;
        };
        let centre = part.transform.translation + Vec3::from(bounds.center);
        let extent = Vec3::from(bounds.half_extents);
        min = min.min(centre - extent);
        max = max.max(centre + extent);
    }

    let size = max - min;
    (
        Meters(f64::from(size.x)),
        Meters(f64::from(size.y)),
        Meters(f64::from(size.z)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AircraftConfig {
        AircraftConfig::light_single()
    }

    #[test]
    fn the_wing_spans_what_the_configuration_says() {
        // 見た目と物理が食い違うと、翼端が地面に接触する位置がずれる。
        let config = config();
        let parts = placeholder_parts(&config);
        let wing = parts
            .iter()
            .find(|part| part.name == "wing")
            .expect("there is a wing");

        let bounds = wing.mesh.compute_aabb().expect("the wing has geometry");
        let span = f64::from(bounds.half_extents.y) * 2.0;
        assert!(
            (span - config.geometry.wing_span.get()).abs() < 0.01,
            "the wing spans {span} m but the configuration says {}",
            config.geometry.wing_span
        );
    }

    #[test]
    fn the_wing_chord_matches_the_configuration() {
        let config = config();
        let wing = placeholder_parts(&config)
            .into_iter()
            .find(|part| part.name == "wing")
            .expect("there is a wing");
        let bounds = wing.mesh.compute_aabb().expect("geometry");
        let chord = f64::from(bounds.half_extents.x) * 2.0;
        assert!((chord - config.geometry.mean_chord.get()).abs() < 0.01);
    }

    #[test]
    fn changing_the_aircraft_changes_the_model() {
        // 寸法をハードコードしていたら、この検査で気付ける。
        let mut wide = config();
        wide.geometry.wing_span = Meters(20.0);

        let (_, narrow_width, _) = placeholder_extents(&config());
        let (_, wide_width, _) = placeholder_extents(&wide);
        assert!(
            wide_width.get() > narrow_width.get() + 8.0,
            "doubling the span barely changed the model ({narrow_width} → {wide_width})"
        );
    }

    // --- 向き ---

    #[test]
    fn the_propeller_sits_ahead_of_everything_else() {
        // 前後を取り違えると機体が後ろ向きに飛ぶ。
        let parts = placeholder_parts(&config());
        let propeller = parts
            .iter()
            .find(|part| part.name == "propeller disc")
            .expect("there is a propeller");

        for part in &parts {
            if part.name == "propeller disc" {
                continue;
            }
            assert!(
                propeller.transform.translation.x > part.transform.translation.x,
                "the propeller ({}) is not ahead of `{}` ({})",
                propeller.transform.translation.x,
                part.name,
                part.transform.translation.x
            );
        }
    }

    #[test]
    fn the_tail_sits_behind_the_wing() {
        let parts = placeholder_parts(&config());
        let find = |name: &str| {
            parts
                .iter()
                .find(|part| part.name == name)
                .unwrap_or_else(|| panic!("`{name}` is missing"))
                .transform
                .translation
                .x
        };
        assert!(find("horizontal stabiliser") < find("wing"));
        assert!(find("vertical stabiliser") < find("wing"));
    }

    #[test]
    fn the_fin_points_up_in_body_axes() {
        // 機体軸の Z は**下向き**。Bevy の Y-up の感覚で組むと尾翼が下に生える。
        let parts = placeholder_parts(&config());
        let fin = parts
            .iter()
            .find(|part| part.name == "vertical stabiliser")
            .expect("there is a fin");
        assert!(
            fin.transform.translation.z < 0.0,
            "the fin sits at z = {} in body axes; positive z is downward",
            fin.transform.translation.z
        );
    }

    #[test]
    fn the_wing_is_wider_than_it_is_long() {
        // 翼弦と翼幅を取り違えると、翼が進行方向に伸びた妙な形になる。
        let wing = placeholder_parts(&config())
            .into_iter()
            .find(|part| part.name == "wing")
            .expect("there is a wing");
        let bounds = wing.mesh.compute_aabb().expect("geometry");
        assert!(
            bounds.half_extents.y > bounds.half_extents.x * 3.0,
            "the wing is {} long and {} wide; the axes are probably swapped",
            bounds.half_extents.x * 2.0,
            bounds.half_extents.y * 2.0
        );
    }

    // --- 大きさ ---

    #[test]
    fn the_model_is_the_size_of_a_light_aircraft() {
        let (length, width, height) = placeholder_extents(&config());

        assert!(
            (7.0..12.0).contains(&length.get()),
            "the aircraft is {length} long; a light single is about 8 m"
        );
        assert!(
            (10.0..13.0).contains(&width.get()),
            "the aircraft is {width} wide; the span should be about 11 m"
        );
        assert!(
            (2.0..5.0).contains(&height.get()),
            "the aircraft is {height} tall; about 3 m is expected"
        );
    }

    #[test]
    fn the_model_fits_around_the_landing_gear() {
        // 脚の接地点より機体が小さいと、車輪が胴体の外に浮いて見える。
        let config = config();
        let (length, width, _) = placeholder_extents(&config);

        let widest = config
            .landing_gear
            .legs()
            .iter()
            .map(|leg| leg.contact_point().as_vec().y.abs())
            .fold(0.0_f64, f64::max);
        let longest = config
            .landing_gear
            .legs()
            .iter()
            .map(|leg| leg.contact_point().as_vec().x.abs())
            .fold(0.0_f64, f64::max);

        assert!(
            width.get() > widest * 2.0,
            "the gear is {} m wide but the model is only {width}",
            widest * 2.0
        );
        assert!(length.get() > longest * 2.0);
    }

    #[test]
    fn every_part_has_geometry_and_a_finite_transform() {
        for part in placeholder_parts(&config()) {
            assert!(
                part.mesh.compute_aabb().is_some(),
                "`{}` has no geometry",
                part.name
            );
            assert!(
                part.transform.translation.is_finite(),
                "`{}` sits at {:?}",
                part.name,
                part.transform.translation
            );
        }
    }
}
