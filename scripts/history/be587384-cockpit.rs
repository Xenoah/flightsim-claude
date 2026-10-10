//! コックピット内装。
//!
//! # 何を再現しているか
//!
//! **Cessna 172 スカイホーク**（1975 年の 172M 以降、標準の 3.125 インチ計器が
//! 6 つ入るようになった型以降）。FDM が解いているのがこの級の機体
//! （`AircraftConfig::light_single`: 1043 kg、翼幅 11 m、失速角 16°）で、
//! **画面に出ている 6 つの丸型計器はまさにこの機体のシックスパック**だから。
//!
//! ## 実機から取った寸法
//!
//! | 値 | 出どころ |
//! |---|---|
//! | 客室幅 40 in = 1.016 m | 172 の公表値（肩の高さで 39.5〜40 in） |
//! | 客室高 48 in = 1.219 m | 同上 |
//! | 計器の直径 3.125 in = 79.4 mm | 標準計器。172M 以降でこれが 6 つ入る |
//! | 翼幅 11 m | `AircraftConfig` と一致（実機どおり） |
//!
//! ## 実機から取っていない寸法
//!
//! **計器盤の外形、風防の傾き、操縦輪の大きさ、座席位置は公表寸法が無い。**
//! 客室幅・高さと計器の直径を骨格にして、写真から妥当に見える比率で置いた。
//! ここは「再現」ではなく「それらしく組んだ」部分である。
//!
//! ## 計器盤の配置
//!
//! 実機の並びに合わせてある。
//!
//! ```text
//!   [breakers]  ASI ATT ALT   [radio]  [engine]
//!               TC  HDG VSI   [stack]  [gauges]
//!                  ^ yoke
//! ```
//!
//! - シックスパックは**操縦士の正面**。上段が対気速度・姿勢・高度、
//!   下段が旋回計・方位・昇降（T 字配置）
//! - 無線と航法は**中央**
//! - エンジン計器は**右**
//! - スイッチとブレーカーは**左端と操縦輪の後ろ**
//!
//! # 座標系
//!
//! [`crate::aircraft`] と同じく**機体軸**（X = 前、Y = 右、Z = 下）。
//! **Bevy の Y-up で組まないこと。** ここを混ぜると内装が横倒しになる。
//!
//! # 目線との関係
//!
//! すべての寸法は**操縦士の目**を基準に置いてある
//! （`CameraRig::eye_offset`）。目の位置を動かすなら、ここも一緒に動かす
//! こと。**片方だけ変えると、顔が計器盤にめり込むか、盤が視界から消える。**

use bevy::prelude::*;
use flightsim_core::Meters;

/// 1 インチ `m`。**定義値。**
///
/// 実機の寸法はインチで公表されている。**丸めたメートル値を直接書かない**
/// こと。書き写すたびに端数がずれ、出典と食い違う（一度そうなった）。
const INCH: f64 = 0.0254;

/// 客室の幅 `m`。実機の公表値 40 インチ。
pub const CABIN_WIDTH: f64 = 40.0 * INCH;

/// 客室の高さ `m`。実機の公表値 48 インチ。
pub const CABIN_HEIGHT: f64 = 48.0 * INCH;

/// 標準計器の直径 `m`。3.125 インチ。
///
/// **172M 以降でこれが 6 つ収まるようになった。** それ以前は入らず、
/// シックスパックが成立しなかった。
pub const INSTRUMENT_DIAMETER: f64 = 3.125 * INCH;

/// 操縦士の目から計器盤の面までの距離 `m`。
///
/// **公表寸法ではない。** 手が操縦輪に届き、盤の文字が読める距離として
/// 置いた。近すぎると盤が視界を覆い、遠いと計器が読めない。
pub const EYE_TO_PANEL: f64 = 0.78;

/// 操縦士の目から床までの距離 `m`。
///
/// 着座した目の高さ（約 0.78 m）＋座面の高さ（約 0.17 m）。
/// **公表寸法ではない。**
pub const EYE_TO_FLOOR: f64 = 0.95;

/// 操縦士の席が中心線からどれだけ左か `m`。
///
/// 客室幅 1.016 m に 2 席なので、席の中心は中心線から約 4 分の 1。
/// **172 は左席が機長席。**
pub const PILOT_SEAT_OFFSET: f64 = 0.25;

/// 計器盤の面が目より下に始まる量 `m`（グレアシールドの高さ）。
///
/// **盤の上端が目線より少し下**に来るようにする。ここが目線より上だと
/// 前が見えず、下がりすぎると計器を見るのに顎を引くことになる。
const GLARESHIELD_DROP: f64 = 0.10;

/// 窓の下端が目より下に始まる量 `m`。
///
/// 肩の少し上。**ここが高いと横が見えず、低いと壁が無くなる。**
const WINDOW_SILL_DROP: f64 = 0.14;

/// シックスパックの中心が目より下にある量 `m`。
///
/// **`flightsim-ui` の 2D 計器の位置と対で決めてある。**
/// あちらは画面下端から 120 px の位置に置いてあり、ここを動かすなら
/// 一緒に動かすこと。**片方だけ動かすと、計器が盤から浮く。**
///
/// 実機の六つ組はもっと下にあるが、視野角 60 度では画面外へ出る。
/// **人の視野（約 120 度）より狭いレンズで見ているぶん、幾何どおりには
/// 置けない。** ここは実機の再現ではなく、画面に収めるための妥協。
pub const SIX_PACK_DROP: f64 = 0.25;

/// 計器盤の高さ `m`。上段と下段の計器が入る。
const PANEL_HEIGHT: f64 = 0.42;

/// 内装のパーツ 1 つ。
///
/// [`crate::aircraft::AircraftPart`] と同じ形。**機体軸**で置く。
#[derive(Debug, Clone)]
pub struct CockpitPart {
    /// 名前。デバッグ表示用。
    pub name: &'static str,
    /// 形。
    pub mesh: Mesh,
    /// **機体軸**での配置。
    pub transform: Transform,
    /// 色。
    pub color: Color,
    /// 光を出すか。計器盤の目盛りなど、暗くても読めるべきもの。
    pub emissive: bool,
}

/// 目の位置（機体軸）から内装を組む。
///
/// `eye` は `CameraRig::eye_offset` と同じ意味（前・右・下）。
///
/// # なぜ目を引数に取るのか
///
/// **内装は目の位置に対して置かないと意味がない。** 重心を基準にすると、
/// 目の位置を動かしたときに顔が盤にめり込む。
#[must_use]
pub fn interior_parts(eye: [Meters; 3]) -> Vec<CockpitPart> {
    let (eye_x, eye_y, eye_z) = (eye[0].get(), eye[1].get(), eye[2].get());

    // 内装の色。**実機の 172 は灰色系の樹脂とビニール。**
    let panel_face = Color::srgb(0.13, 0.13, 0.14);
    let trim = Color::srgb(0.32, 0.31, 0.29);
    let upholstery = Color::srgb(0.24, 0.23, 0.22);
    let metal = Color::srgb(0.55, 0.55, 0.57);
    let glare = Color::srgb(0.08, 0.08, 0.09);

    let panel_x = eye_x + EYE_TO_PANEL;
    let floor_z = eye_z + EYE_TO_FLOOR;
    let glareshield_z = eye_z + GLARESHIELD_DROP;
    let panel_centre_z = glareshield_z + PANEL_HEIGHT * 0.5;
    let half_width = CABIN_WIDTH * 0.5;

    let mut parts = vec![
        CockpitPart {
            name: "instrument panel",
            // 盤は客室いっぱいの幅。厚みは薄い板。
            mesh: box_mesh(0.05, CABIN_WIDTH, PANEL_HEIGHT),
            transform: at(panel_x, 0.0, panel_centre_z),
            color: panel_face,
            emissive: false,
        },
        CockpitPart {
            name: "glareshield",
            // 盤の上に前へ張り出す庇。**日射で盤が白飛びするのを防ぐもの。**
            // 手前へ 0.16 m 出る。
            mesh: box_mesh(0.16, CABIN_WIDTH, 0.035),
            transform: at(panel_x - 0.08, 0.0, glareshield_z - 0.02),
            color: glare,
            emissive: false,
        },
        CockpitPart {
            name: "floor",
            mesh: box_mesh(1.6, CABIN_WIDTH, 0.03),
            transform: at(eye_x - 0.1, 0.0, floor_z),
            color: upholstery,
            emissive: false,
        },
        // 側面は**窓の下だけ**。172 は左右とも大きな窓で、肩の高さから
        // 上は開いている。**壁で塞ぐと、旋回中に外が見えなくなる**
        // （実機で最も見たい方向が見えない）。
        CockpitPart {
            name: "left wall",
            mesh: box_mesh(1.7, 0.04, WINDOW_SILL_DROP + 0.55),
            transform: at(
                eye_x - 0.1,
                -half_width,
                eye_z + WINDOW_SILL_DROP + (WINDOW_SILL_DROP + 0.55) * 0.5,
            ),
            color: trim,
            emissive: false,
        },
        CockpitPart {
            name: "right wall",
            mesh: box_mesh(1.7, 0.04, WINDOW_SILL_DROP + 0.55),
            transform: at(
                eye_x - 0.1,
                half_width,
                eye_z + WINDOW_SILL_DROP + (WINDOW_SILL_DROP + 0.55) * 0.5,
            ),
            color: trim,
            emissive: false,
        },
        // 窓の後ろの柱（ドアの後端）。**窓の縁が見えないと、
        // ガラスがあるのか穴が開いているのか分からない。**
        CockpitPart {
            name: "left door post",
            mesh: box_mesh(0.05, 0.05, 0.55),
            transform: at(eye_x - 0.55, -half_width, eye_z - 0.02),
            color: trim,
            emissive: false,
        },
        CockpitPart {
            name: "right door post",
            mesh: box_mesh(0.05, 0.05, 0.55),
            transform: at(eye_x - 0.55, half_width, eye_z - 0.02),
            color: trim,
            emissive: false,
        },
        CockpitPart {
            name: "roof",
            // 高翼機なので頭上は主翼の桁。**低翼機と違って上が見えない。**
            mesh: box_mesh(1.7, CABIN_WIDTH, 0.04),
            transform: at(eye_x - 0.1, 0.0, eye_z - 0.32),
            color: trim,
            emissive: false,
        },
        CockpitPart {
            name: "windscreen post",
            // 風防中央の支柱。**172 は中心線に細いものが 1 本。**
            // 太くすると正面が塞がる（最初 6 cm 角にして、滑走路が
            // 見えなくなった）。
            mesh: box_mesh(0.05, 0.028, 0.55),
            transform: at(panel_x + 0.10, 0.0, glareshield_z - 0.30),
            color: trim,
            emissive: false,
        },
        CockpitPart {
            name: "centre stack",
            // 無線と航法。**盤の中央、2 席のあいだ。**
            mesh: box_mesh(0.06, 0.17, 0.30),
            transform: at(panel_x - 0.03, 0.0, panel_centre_z + 0.02),
            color: Color::srgb(0.09, 0.09, 0.10),
            emissive: false,
        },
        CockpitPart {
            name: "control yoke column",
            // 操縦輪の軸。盤から手前へ出る。
            mesh: box_mesh(0.28, 0.05, 0.05),
            transform: at(
                panel_x - 0.16,
                -PILOT_SEAT_OFFSET,
                panel_centre_z + PANEL_HEIGHT * 0.42,
            ),
            color: metal,
            emissive: false,
        },
        CockpitPart {
            name: "control yoke",
            // 172 の操縦輪は W 字。ここでは横棒 1 本で表す。
            mesh: box_mesh(0.04, 0.32, 0.045),
            transform: at(
                panel_x - 0.30,
                -PILOT_SEAT_OFFSET,
                panel_centre_z + PANEL_HEIGHT * 0.42,
            ),
            color: Color::srgb(0.10, 0.10, 0.11),
            emissive: false,
        },
        CockpitPart {
            name: "throttle quadrant",
            // スロットル・ミクスチャ。**盤の中央下、2 席から届く位置。**
            mesh: box_mesh(0.12, 0.14, 0.06),
            transform: at(panel_x - 0.06, 0.02, panel_centre_z + PANEL_HEIGHT * 0.55),
            color: metal,
            emissive: false,
        },
        CockpitPart {
            name: "pilot seat back",
            mesh: box_mesh(0.06, 0.44, 0.55),
            transform: at(eye_x - 0.45, -PILOT_SEAT_OFFSET, eye_z + 0.30),
            color: upholstery,
            emissive: false,
        },
        CockpitPart {
            name: "copilot seat back",
            mesh: box_mesh(0.06, 0.44, 0.55),
            transform: at(eye_x - 0.45, PILOT_SEAT_OFFSET, eye_z + 0.30),
            color: upholstery,
            emissive: false,
        },
    ];

    // 計器の座ぐり。**実機と同じ並びで、盤の面より少し手前に出す。**
    // 見た目の穴であって、計器そのものは `flightsim-ui` が 2D で描く。
    for bezel in six_pack_bezels(panel_x, eye_z, eye_y) {
        parts.push(bezel);
    }
    parts
}

/// シックスパックのベゼル 6 つ。
///
/// 実機の並び:
///
/// ```text
///   ASI  ATT  ALT      上段
///   TC   HDG  VSI      下段
/// ```
///
/// **T 字配置**と呼ばれるのは、姿勢・対気速度・高度・方位の 4 つが
/// T の字を作るため。上段中央が姿勢、その真下が方位。
fn six_pack_bezels(panel_x: f64, eye_z: f64, eye_y: f64) -> Vec<CockpitPart> {
    // 隣り合う計器の中心間隔。直径 + 縁の余白。
    let pitch = INSTRUMENT_DIAMETER * 1.12;
    // 2 段の中心は目より `SIX_PACK_DROP` だけ下。
    let centre_z = eye_z + SIX_PACK_DROP;
    let row_top = centre_z - pitch * 0.5;
    let row_bottom = centre_z + pitch * 0.5;
    // 6 つの中心は操縦士の正面。
    let centre_y = eye_y;

    let names: [(&'static str, f64, f64); 6] = [
        ("bezel airspeed", centre_y - pitch, row_top),
        ("bezel attitude", centre_y, row_top),
        ("bezel altimeter", centre_y + pitch, row_top),
        ("bezel turn", centre_y - pitch, row_bottom),
        ("bezel heading", centre_y, row_bottom),
        ("bezel vertical speed", centre_y + pitch, row_bottom),
    ];

    names
        .into_iter()
        .map(|(name, y, z)| CockpitPart {
            name,
            // 縁だけの見た目。**盤より 1 cm 手前に出して、影で縁が立つ。**
            mesh: box_mesh(0.012, INSTRUMENT_DIAMETER, INSTRUMENT_DIAMETER),
            transform: at(panel_x - 0.03, y, z),
            color: Color::srgb(0.05, 0.05, 0.055),
            emissive: false,
        })
        .collect()
}

/// 直方体。引数は**機体軸**での（前後・左右・上下）の長さ。
fn box_mesh(length: f64, width: f64, height: f64) -> Mesh {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "内装の寸法は数メートル。f32 の精度で十分"
    )]
    let size = Vec3::new(length as f32, width as f32, height as f32);
    Mesh::from(Cuboid::from_size(size))
}

/// **機体軸**での配置。
fn at(x: f64, y: f64, z: f64) -> Transform {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "内装の寸法は数メートル。f32 の精度で十分"
    )]
    let translation = Vec3::new(x as f32, y as f32, z as f32);
    Transform::from_translation(translation)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 既定の目の位置（`CameraRig::eye_offset` と同じ）。
    fn eye() -> [Meters; 3] {
        [Meters(0.6), Meters(-PILOT_SEAT_OFFSET), Meters(-0.9)]
    }

    fn part(parts: &[CockpitPart], name: &str) -> Transform {
        parts
            .iter()
            .find(|part| part.name == name)
            .unwrap_or_else(|| panic!("no part named {name}"))
            .transform
    }

    // --- 実機の寸法 ---

    #[test]
    fn the_cabin_matches_the_published_dimensions() {
        // 40 インチ幅・48 インチ高。**ここを勝手に広げない。**
        // 172 の狭さは操縦席の印象そのもので、広げると別の機体になる。
        assert!((CABIN_WIDTH - 1.016).abs() < 1e-9, "got {CABIN_WIDTH}");
        assert!((CABIN_HEIGHT - 1.2192).abs() < 1e-9, "got {CABIN_HEIGHT}");
    }

    #[test]
    fn the_instruments_are_the_standard_three_and_an_eighth_inch() {
        // 3.125 インチ。**172M 以降でこれが 6 つ入るようになった。**
        assert!(
            (INSTRUMENT_DIAMETER - 0.079_375).abs() < 1e-9,
            "got {INSTRUMENT_DIAMETER}"
        );
    }

    // --- 配置 ---

    #[test]
    fn the_panel_sits_in_front_of_the_pilot_not_behind() {
        // **符号を取り違えると、盤が背中側に出て何も見えなくなる。**
        let parts = interior_parts(eye());
        let panel = part(&parts, "instrument panel");
        assert!(
            f64::from(panel.translation.x) > eye()[0].get(),
            "the panel must be ahead of the eye"
        );
    }

    #[test]
    fn the_panel_top_is_just_below_the_eye_line() {
        // **目線より上だと前が見えない。** 下がりすぎると計器を見るのに
        // 顎を引くことになる。機体軸の Z は下向きなので、盤の上端は
        // 目より「大きい Z」＝下にある。
        let parts = interior_parts(eye());
        let panel = part(&parts, "instrument panel");
        let panel_top = f64::from(panel.translation.z) - PANEL_HEIGHT * 0.5;
        let eye_z = eye()[2].get();
        assert!(
            panel_top > eye_z,
            "the panel top {panel_top} must be below the eye {eye_z}"
        );
        assert!(
            panel_top - eye_z < 0.2,
            "but not so far below that the instruments are out of view"
        );
    }

    #[test]
    fn the_floor_is_below_the_eye_and_the_roof_above() {
        let parts = interior_parts(eye());
        let eye_z = eye()[2].get();
        assert!(f64::from(part(&parts, "floor").translation.z) > eye_z);
        assert!(f64::from(part(&parts, "roof").translation.z) < eye_z);
    }

    #[test]
    fn the_walls_are_a_cabin_width_apart() {
        let parts = interior_parts(eye());
        let left = f64::from(part(&parts, "left wall").translation.y);
        let right = f64::from(part(&parts, "right wall").translation.y);
        assert!(
            (right - left - CABIN_WIDTH).abs() < 1e-6,
            "{left} to {right}"
        );
    }

    #[test]
    fn the_pilot_sits_on_the_left() {
        // **172 は左席が機長席。** 操縦輪と席が目の側に来ること。
        let parts = interior_parts(eye());
        assert!(f64::from(part(&parts, "control yoke").translation.y) < 0.0);
        assert!(f64::from(part(&parts, "pilot seat back").translation.y) < 0.0);
        assert!(f64::from(part(&parts, "copilot seat back").translation.y) > 0.0);
    }

    #[test]
    fn the_yoke_is_between_the_pilot_and_the_panel() {
        // **盤より奥に出ると、操縦輪が盤を突き抜けて見える。**
        let parts = interior_parts(eye());
        let yoke = f64::from(part(&parts, "control yoke").translation.x);
        let panel = f64::from(part(&parts, "instrument panel").translation.x);
        assert!(
            yoke < panel,
            "the yoke {yoke} must be nearer than the panel {panel}"
        );
        assert!(yoke > eye()[0].get(), "but still ahead of the eye");
    }

    #[test]
    fn the_glareshield_hangs_over_the_panel() {
        // 庇は盤より手前へ出ていること。**出ていないと庇の意味がない。**
        let parts = interior_parts(eye());
        let shield = f64::from(part(&parts, "glareshield").translation.x);
        let panel = f64::from(part(&parts, "instrument panel").translation.x);
        assert!(shield < panel, "{shield} against {panel}");
    }

    // --- シックスパック ---

    #[test]
    fn the_six_pack_is_laid_out_the_way_the_real_one_is() {
        // 上段 ASI / ATT / ALT、下段 TC / HDG / VSI。
        // **並びを崩すと、実機で訓練した人が読めなくなる。**
        let parts = interior_parts(eye());
        let asi = part(&parts, "bezel airspeed").translation;
        let att = part(&parts, "bezel attitude").translation;
        let alt = part(&parts, "bezel altimeter").translation;
        let turn = part(&parts, "bezel turn").translation;
        let hdg = part(&parts, "bezel heading").translation;
        let vsi = part(&parts, "bezel vertical speed").translation;

        // 上段が 3 つとも同じ高さ、下段も同じ高さ。
        assert!((asi.z - att.z).abs() < 1e-5 && (att.z - alt.z).abs() < 1e-5);
        assert!((turn.z - hdg.z).abs() < 1e-5 && (hdg.z - vsi.z).abs() < 1e-5);
        // 上段が下段より上（機体軸の Z は下向き）。
        assert!(asi.z < turn.z, "the top row must sit above the bottom row");

        // 左から ASI・ATT・ALT の順。
        assert!(asi.y < att.y && att.y < alt.y);
        assert!(turn.y < hdg.y && hdg.y < vsi.y);
        // 姿勢計の真下が方位計。**T 字配置の縦棒。**
        assert!((att.y - hdg.y).abs() < 1e-5);
    }

    #[test]
    fn the_six_pack_is_in_front_of_the_pilot_not_the_middle_of_the_cabin() {
        // **中央に置くと、左席から見て右にずれる。**
        let parts = interior_parts(eye());
        let attitude = f64::from(part(&parts, "bezel attitude").translation.y);
        assert!(
            (attitude - eye()[1].get()).abs() < 1e-6,
            "the attitude indicator should be dead ahead of the eye, got {attitude}"
        );
    }

    #[test]
    fn the_six_pack_fits_inside_the_panel() {
        // **盤からはみ出したら、計器が宙に浮いて見える。**
        let parts = interior_parts(eye());
        let panel = part(&parts, "instrument panel");
        let half_width = CABIN_WIDTH * 0.5;
        let panel_top = f64::from(panel.translation.z) - PANEL_HEIGHT * 0.5;
        let panel_bottom = f64::from(panel.translation.z) + PANEL_HEIGHT * 0.5;

        for bezel in parts.iter().filter(|part| part.name.starts_with("bezel")) {
            let y = f64::from(bezel.transform.translation.y);
            let z = f64::from(bezel.transform.translation.z);
            let radius = INSTRUMENT_DIAMETER * 0.5;
            assert!(
                y - radius > -half_width && y + radius < half_width,
                "{} runs off the side of the panel at y {y}",
                bezel.name
            );
            assert!(
                z - radius > panel_top && z + radius < panel_bottom,
                "{} runs off the top or bottom of the panel at z {z}",
                bezel.name
            );
        }
    }

    // --- 全体 ---

    #[test]
    fn nothing_is_built_behind_the_pilots_head() {
        // 席の背もたれより後ろに物を置いても見えない。**無駄な描画。**
        let parts = interior_parts(eye());
        let eye_x = eye()[0].get();
        for part in &parts {
            let x = f64::from(part.transform.translation.x);
            assert!(
                x > eye_x - 0.6,
                "{} sits {x} m, far behind the eye at {eye_x}",
                part.name
            );
        }
    }

    #[test]
    fn every_part_is_named_and_finite() {
        let parts = interior_parts(eye());
        assert!(parts.len() >= 15, "got only {} parts", parts.len());
        for part in &parts {
            assert!(!part.name.is_empty());
            assert!(
                part.transform.translation.is_finite(),
                "{} has a broken transform",
                part.name
            );
        }
    }

    #[test]
    fn moving_the_eye_moves_the_whole_interior_with_it() {
        // **内装は目に対して置く。** 重心基準にすると、目を動かした
        // ときに顔が盤にめり込む。
        let moved = [Meters(1.2), Meters(-PILOT_SEAT_OFFSET), Meters(-1.4)];
        let base = interior_parts(eye());
        let shifted = interior_parts(moved);
        let panel_base = part(&base, "instrument panel").translation;
        let panel_shifted = part(&shifted, "instrument panel").translation;
        assert!((panel_shifted.x - panel_base.x - 0.6).abs() < 1e-5);
        assert!((panel_shifted.z - panel_base.z + 0.5).abs() < 1e-5);
    }
}
