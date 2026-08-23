//! 合成 DEM を書き出す。
//!
//! # なぜ要るのか
//!
//! **地形を目で見るのに、実 DEM の入手が要るのは重い。** Copernicus DEM は
//! 1 タイルでも数十 MB あり、リポジトリにも CI にも置けない。結果として
//! 「地形が映らないまま描画コードを書く」ことになり、実データを通すまで
//! 不具合に気付けない。実際、glTF 経路ではそれで 3 件見落とした。
//!
//! ここで作るのは**実在しない地形**。標高の絶対値に意味は無い。
//! 見たいのは、地形が出るか・LOD が切り替わるか・色や陰影が妥当か。
//!
//! **実データの代わりにはならない。** 投影のずれ、nodata、データ境界の崖と
//! いった実 DEM の面倒は、ここには現れない。
//!
//! # 滑走路
//!
//! M2 の完了条件「1 空港周辺で離陸 → 旋回 → 着陸」のため、合成飛行場の滑走路を
//! 平地として彫ってある。**位置と寸法は [`Runway::synthetic`] が唯一の出所**で、
//! ここには数値を書かない。両方に書くと片方だけ直され、
//! 「滑走路の判定はあるのに地面が斜面」という状態になる。
//!
//! ```bash
//! cargo run -p flightsim-tilegen --example synthetic_dem -- data/synthetic.tif
//! cargo run -p flightsim-tilegen -- --input data/synthetic.tif --output data/tiles \
//!     --min-level 8 --max-level 12
//! cargo run -p flightsim-app --release -- --tiles data/tiles --start 35.553,139.781
//! ```

use flightsim_core::{Geodetic, Meters};
use flightsim_tilegen::testing::GeoTiffBuilder;
use flightsim_world::Runway;
use std::path::PathBuf;

/// 覆う範囲。羽田を含む 1 度四方。
const WEST: f64 = 139.0;
const NORTH: f64 = 36.0;
const SPAN_DEGREES: f64 = 1.0;

/// 1 辺の格子点数。3 秒グリッド（SRTM 相当）に近い密度。
const SAMPLES: u32 = 1201;

/// 滑走路の縦方向（進入方向）に確保する余白 `m`。過走帯とその先の平坦部。
const APRON_LONGITUDINAL_MARGIN: f64 = 300.0;

/// 滑走路の横方向に確保する余白 `m`。滑走路の縁から外側へこの距離。
///
/// **格子間隔より広く取る必要がある。** この DEM は 1 度を 1 200 分割しているので
/// 東西 75 m・南北 93 m 間隔で、滑走路の全幅 45 m は 1 格子にも満たない。
/// 余白なしでは滑走路そのものが格子の隙間に落ちて平らにならない。
const APRON_LATERAL_MARGIN: f64 = 150.0;

/// 平地から元の地形へ戻すまでの距離 `m`。
const BLEND_DISTANCE: f64 = 500.0;

/// 元の地形の標高を返す。**実在しない地形。**
///
/// 東は海（0 m）、西へ向かって平野から山地へ上がる。目印になるよう
/// 単独峰を 1 つ置いてある。高度による色分けと陰影を見るための形。
fn natural_elevation(longitude: f64, latitude: f64) -> f64 {
    // 東ほど 0、西ほど 1。
    let inland = ((WEST + SPAN_DEGREES - longitude) / SPAN_DEGREES).clamp(0.0, 1.0);

    // 海岸線は南東側。ここから内陸へ上がっていく。
    let plain = if inland < 0.25 {
        // 沿岸の低地。わずかに起伏させて、平面と区別が付くようにする。
        inland * 40.0
    } else {
        let rise = (inland - 0.25) / 0.75;
        10.0 + rise.powf(1.8) * 1400.0
    };

    // 単独峰。位置が分かる目印になる。
    let peak_longitude = 139.25;
    let peak_latitude = 35.55;
    let dx = (longitude - peak_longitude) / 0.11;
    let dy = (latitude - peak_latitude) / 0.11;
    let peak = 2200.0 * (-(dx * dx + dy * dy)).exp();

    // 尾根の刻み。LOD が切り替わったときに差が見えるよう、細かい成分を入れる。
    let ridges = (longitude * 47.0).sin() * (latitude * 41.0).cos() * 55.0 * inland;

    (plain + peak + ridges).max(0.0)
}

/// 滑走路とその周囲を平らにした標高を返す。
///
/// ```text
///   標高
///    │   ┌──────────────┐ 滑走路 + 余白（elevation で完全に平ら）
///    │  ╱                ╲
///    │ ╱   BLEND_DISTANCE  ╲   smoothstep で元の地形へ戻す
///    │╱                      ╲
///    └────────────────────────── 距離
/// ```
///
/// **段差の崖を作らないこと。** 平地の縁で標高が飛ぶと、そこだけ幾何誤差が跳ね上がり、
/// 平野なのに最大レベルまで細分化される（実データの被覆境界で実際に起きた現象と同じ）。
/// 端点で微分が 0 になる smoothstep を使い、傾斜の不連続もなくしてある。
fn elevation(runway: &Runway, longitude: f64, latitude: f64) -> f64 {
    let natural = natural_elevation(longitude, latitude);

    let offsets = runway.offsets(Geodetic::from_degrees(latitude, longitude, 0.0));
    let longitudinal = offsets.longitudinal.get();
    let lateral = offsets.lateral.get();

    // 平地の矩形からの距離。矩形の内側では 0。
    let half_width = runway.width.get() * 0.5 + APRON_LATERAL_MARGIN;
    let lateral_excess = (lateral.abs() - half_width).max(0.0);
    let longitudinal_excess = (-APRON_LONGITUDINAL_MARGIN - longitudinal)
        .max(longitudinal - (runway.length.get() + APRON_LONGITUDINAL_MARGIN))
        .max(0.0);
    let distance = lateral_excess.hypot(longitudinal_excess);

    if distance >= BLEND_DISTANCE {
        return natural;
    }

    // smoothstep。t = 0 と t = 1 の両端で傾きが 0 になるので、平地の縁にも
    // 元の地形との継ぎ目にも折れ線が残らない。
    let t = distance / BLEND_DISTANCE;
    let blend = t * t * (3.0 - 2.0 * t);

    let flat = runway.elevation.get();
    flat + (natural - flat) * blend
}

fn main() {
    let output = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("data/synthetic.tif"), PathBuf::from);

    let runway = Runway::synthetic();

    let step = SPAN_DEGREES / f64::from(SAMPLES - 1);
    let mut samples = Vec::with_capacity((SAMPLES as usize) * (SAMPLES as usize));
    for row in 0..SAMPLES {
        // 画像の行 0 が北端。
        let latitude = NORTH - f64::from(row) * step;
        for column in 0..SAMPLES {
            let longitude = WEST + f64::from(column) * step;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "標高は数千 m。f32 の分解能で十分"
            )]
            let value = elevation(&runway, longitude, latitude) as f32;
            samples.push(value);
        }
    }

    let highest = samples.iter().copied().fold(f32::MIN, f32::max);
    let lowest = samples.iter().copied().fold(f32::MAX, f32::min);

    let bytes = GeoTiffBuilder::new(SAMPLES, SAMPLES, samples)
        .origin(WEST, NORTH)
        .pixel_size(step, step)
        .build();

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("error: could not create {}: {error}", parent.display());
        std::process::exit(1);
    }

    match std::fs::write(&output, &bytes) {
        Ok(()) => {
            println!("wrote {} ({} bytes)", output.display(), bytes.len());
            println!(
                "  {SAMPLES}x{SAMPLES} samples over {WEST}..{}E, {}..{NORTH}N",
                WEST + SPAN_DEGREES,
                NORTH - SPAN_DEGREES
            );
            println!("  elevation {lowest:.0}..{highest:.0} m — **this terrain is not real**");
            report_runway(&runway);
        }
        Err(error) => {
            eprintln!("error: could not write {}: {error}", output.display());
            std::process::exit(1);
        }
    }
}

/// 彫った滑走路の諸元を出す。**離陸開始位置をここに出しておくのが要点。**
/// アプリの `--start` に何を渡せば滑走路上に置けるのかが、焼いた直後に分かる。
fn report_runway(runway: &Runway) {
    let start = runway.takeoff_start();
    let far = runway.opposite_threshold();

    println!(
        "  runway {:.0} x {:.0} m at {:.0} m, heading {:03.0}/{:03.0}",
        runway.length.get(),
        runway.width.get(),
        runway.elevation.get(),
        runway.heading.to_degrees().get(),
        runway.reciprocal_heading().to_degrees().get(),
    );
    println!(
        "    threshold {:.6},{:.6} -> {:.6},{:.6}",
        runway.threshold.latitude_degrees(),
        runway.threshold.longitude_degrees(),
        far.latitude_degrees(),
        far.longitude_degrees(),
    );
    println!(
        "    flattened {:.0} m beyond each end, {:.0} m to each side, \
         blended over {BLEND_DISTANCE:.0} m",
        APRON_LONGITUDINAL_MARGIN,
        runway.width.get() * 0.5 + APRON_LATERAL_MARGIN,
    );
    println!(
        "    takeoff start: --start {:.6},{:.6} --heading {:.0}",
        start.latitude_degrees(),
        start.longitude_degrees(),
        runway.heading.to_degrees().get(),
    );
    println!(
        "    max gradient across the flattened edge: {:.4} m/m",
        max_lateral_gradient(runway),
    );
}

/// 滑走路中央を横切る断面の最大勾配 `m/m`。
///
/// **崖が残っていないことを、焼いた直後に数字で見るための検査。**
/// 平地の縁で標高が飛ぶと、そこだけ幾何誤差が跳ね上がって平野が過剰に細分化される。
/// 格子ではなく 5 m 刻みの連続サンプルで測るので、DEM の解像度に依存しない。
fn max_lateral_gradient(runway: &Runway) -> f64 {
    const STEP: f64 = 5.0;
    const HALF_SPAN: f64 = 1_500.0;

    let mut max_gradient: f64 = 0.0;
    let mut previous: Option<f64> = None;

    let mut lateral = -HALF_SPAN;
    while lateral <= HALF_SPAN {
        let point = runway.point_at(runway.length * 0.5, Meters(lateral));
        let here = elevation(runway, point.longitude_degrees(), point.latitude_degrees());
        if let Some(previous) = previous {
            max_gradient = max_gradient.max((here - previous).abs() / STEP);
        }
        previous = Some(here);
        lateral += STEP;
    }

    max_gradient
}
