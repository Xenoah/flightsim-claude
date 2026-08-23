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
//! ```bash
//! cargo run -p flightsim-tilegen --example synthetic_dem -- data/synthetic.tif
//! cargo run -p flightsim-tilegen -- --input data/synthetic.tif --output data/tiles \
//!     --min-level 8 --max-level 12
//! cargo run -p flightsim-app --release -- --tiles data/tiles --start 35.553,139.781
//! ```

use flightsim_tilegen::testing::GeoTiffBuilder;
use std::path::PathBuf;

/// 覆う範囲。羽田を含む 1 度四方。
const WEST: f64 = 139.0;
const NORTH: f64 = 36.0;
const SPAN_DEGREES: f64 = 1.0;

/// 1 辺の格子点数。3 秒グリッド（SRTM 相当）に近い密度。
const SAMPLES: u32 = 1201;

/// 標高を返す。**実在しない地形。**
///
/// 東は海（0 m）、西へ向かって平野から山地へ上がる。目印になるよう
/// 単独峰を 1 つ置いてある。高度による色分けと陰影を見るための形。
fn elevation(longitude: f64, latitude: f64) -> f32 {
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

    #[allow(
        clippy::cast_possible_truncation,
        reason = "標高は数千 m。f32 の分解能で十分"
    )]
    let total = (plain + peak + ridges).max(0.0) as f32;
    total
}

fn main() {
    let output = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("data/synthetic.tif"), PathBuf::from);

    let step = SPAN_DEGREES / f64::from(SAMPLES - 1);
    let mut samples = Vec::with_capacity((SAMPLES as usize) * (SAMPLES as usize));
    for row in 0..SAMPLES {
        // 画像の行 0 が北端。
        let latitude = NORTH - f64::from(row) * step;
        for column in 0..SAMPLES {
            let longitude = WEST + f64::from(column) * step;
            samples.push(elevation(longitude, latitude));
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
        }
        Err(error) => {
            eprintln!("error: could not write {}: {error}", output.display());
            std::process::exit(1);
        }
    }
}
