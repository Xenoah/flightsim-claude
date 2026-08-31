//! タイル生成 CLI。
//!
//! 詳細は [`flightsim_tilegen`] のクレートドキュメントを参照。

use clap::Parser;
use flightsim_core::Meters;
use flightsim_tilegen::vertical_datum::VerticalDatumMismatch;
use flightsim_tilegen::{RasterSet, Region, TileGenOptions, generate_tiles};
use std::path::PathBuf;
use std::process::ExitCode;

/// Copernicus DEM の GeoTIFF から実行時タイル (.fsdem) を焼く。
#[derive(Debug, Parser)]
#[command(name = "flightsim-tilegen", version, about, long_about = None)]
struct Cli {
    /// 入力 GeoTIFF。複数指定でき、先に指定したものが優先される。
    #[arg(short, long, required = true, value_name = "GEOTIFF")]
    input: Vec<PathBuf>,

    /// タイルの出力先ディレクトリ。`{level}/{x}/{y}.fsdem` が作られる。
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,

    /// 生成する最も粗いレベル。
    #[arg(long, default_value_t = 8, value_name = "N")]
    min_level: u8,

    /// 生成する最も細かいレベル。深くするとタイル数が 4 倍ずつ増える。
    #[arg(long, default_value_t = 12, value_name = "N")]
    max_level: u8,

    /// タイル 1 辺の格子点数。`2^n + 1` が扱いやすい。
    #[arg(long, default_value_t = 65, value_name = "N")]
    grid_size: u32,

    /// 元データに被覆が無い格子点を埋める標高 [m]。
    #[arg(long, default_value_t = 0.0, value_name = "METRES")]
    fill: f64,

    /// タイルを書くのに必要な被覆率 `0.0..=1.0`。これを下回るタイルは書かない。
    ///
    /// ほとんどが fill のタイルは、実データとの境界が崖になる。実測では焼いた
    /// 範囲の縁で 179 m の段差が飛行中に現れた。しかもタイルは存在するため、
    /// 実行時からは「地形データがある」ようにしか見えない。
    /// 縁の崖が問題になる場合は 0.9 以上を指定する。
    #[arg(long, default_value_t = 0.0, value_name = "FRACTION")]
    min_coverage: f64,

    /// 鉛直基準が WGS84 楕円体高でない DEM を、そのまま焼くことを許す。
    ///
    /// # 何を受け入れることになるか
    ///
    /// `.fsdem` は WGS84 楕円体高で保存する（ADR-0002）。ジオイド基準の
    /// 高さをそのまま焼くと、**ジオイド高ぶんの系統誤差**が入ったまま
    /// 実行時に「正しい標高」として扱われる。世界で -107〜+86 m、
    /// 日本付近で約 +30〜+40 m。
    ///
    /// 局所的には気付けない。滑走路も機体も同じだけずれるので描画と接地は
    /// 辻褄が合う。効くのは絶対高度と ECEF 半径。
    ///
    /// 合成 DEM のように**基準の無い試験データ**を焼くときは、これを付ける。
    #[arg(long, default_value_t = false)]
    assume_ellipsoidal: bool,

    /// 対象範囲 `west,south,east,north` [度]。省略時は入力ラスタの被覆範囲。
    ///
    /// west > east は日付変更線をまたぐ範囲として扱う。
    #[arg(long, value_name = "W,S,E,N", allow_hyphen_values = true)]
    bounds: Option<String>,

    /// ファイルを書かずに、生成されるタイル数と容量だけを見積もる。
    #[arg(long)]
    dry_run: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    if cli.min_level > cli.max_level {
        return Err(format!(
            "--min-level ({}) is deeper than --max-level ({})",
            cli.min_level, cli.max_level
        ));
    }

    eprintln!("reading {} raster(s)...", cli.input.len());
    let rasters = RasterSet::load(&cli.input).map_err(|error| error.to_string())?;

    let region = match &cli.bounds {
        Some(text) => parse_bounds(text)?,
        None => rasters
            .coverage()
            .ok_or_else(|| "no input rasters, so there is no region to bake".to_owned())?,
    };

    // 方位の接尾辞（°W / °E）は付けない。西端が正の経度（東経）のことが普通にあり、
    // 「138.68°W」のような嘘の表示になる。範囲の端であることだけを示す。
    eprintln!(
        "region: longitude {:.4}°..{:.4}°, latitude {:.4}°..{:.4}°{}",
        region.west().to_degrees().get(),
        region.east().to_degrees().get(),
        region.south().to_degrees().get(),
        region.north().to_degrees().get(),
        if region.crosses_dateline() {
            " (crosses the dateline)"
        } else {
            ""
        }
    );

    // **鉛直基準を黙って誤用しない。** 焼いてしまうと、実行時からは
    // 「正しい標高」と区別が付かない。
    let mismatched = rasters.non_ellipsoidal_sources();
    if !mismatched.is_empty() {
        for (index, datum) in &mismatched {
            let path = cli
                .input
                .get(*index)
                .map_or_else(|| "<unknown>".to_owned(), |p| p.display().to_string());
            eprintln!("vertical datum: {path} is {datum}");
        }
        if !cli.assume_ellipsoidal {
            let (_, datum) = mismatched[0];
            return Err(VerticalDatumMismatch { datum }.to_string());
        }
        eprintln!(
            "warning: --assume-ellipsoidal was given, so the heights are baked unchanged.\n\
             \x20        The geoid undulation stays in the tiles as a systematic error."
        );
    }

    // 深いレベルはタイル数が 4 倍ずつ増える。着手前に規模を見せる。
    let planned: usize = (cli.min_level..=cli.max_level)
        .map(|level| region.tiles(level).len())
        .sum();
    eprintln!(
        "levels {}..={} cover {planned} tile(s){}",
        cli.min_level,
        cli.max_level,
        if cli.dry_run { " (dry run)" } else { "" }
    );

    let options = TileGenOptions {
        grid_size: cli.grid_size,
        fill: Meters(cli.fill),
        min_coverage: cli.min_coverage,
    };
    let report = generate_tiles(
        &rasters,
        region,
        cli.min_level..=cli.max_level,
        &options,
        &cli.output,
        cli.dry_run,
    )
    .map_err(|error| error.to_string())?;

    #[allow(
        clippy::cast_precision_loss,
        reason = "表示用の概算。バイト数の精度は問題にならない"
    )]
    let mebibytes = report.bytes_written as f64 / (1024.0 * 1024.0);
    eprintln!("wrote {} tile(s), {mebibytes:.1} MiB", report.tiles_written);
    if report.tiles_without_coverage > 0 {
        eprintln!(
            "skipped {} tile(s) with no source coverage",
            report.tiles_without_coverage
        );
    }
    if report.tiles_below_min_coverage > 0 {
        eprintln!(
            "skipped {} tile(s) below the {:.0}% coverage threshold",
            report.tiles_below_min_coverage,
            cli.min_coverage * 100.0
        );
    }
    if report.grid_points_filled > 0 {
        // 黙って埋めると、地形に平坦な板が現れた理由が分からなくなる。
        // さらに、埋めた部分との段差が幾何誤差を押し上げ、実データの無い場所ほど
        // 細かく細分化されるという逆転が起きる。これは実測で確認している
        // （被覆完全なタイル最大 10.7 m に対し、fill を含むタイル最大 375.3 m）。
        eprintln!(
            "warning: filled {} grid point(s) with {} m where the source rasters had no coverage.",
            report.grid_points_filled, cli.fill
        );
        eprintln!(
            "         The step between real terrain and fill inflates the geometric error, \
             so those tiles subdivide more than they should."
        );
        eprintln!(
            "         Use --bounds to keep generation inside the covered area, or --min-coverage"
        );
        eprintln!("         to skip mostly-filled tiles (they read as real terrain at runtime).");
    }

    Ok(())
}

/// `west,south,east,north` を度として解析する。
fn parse_bounds(text: &str) -> Result<Region, String> {
    let values: Vec<f64> = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .map_err(|_| format!("`{}` is not a number", part.trim()))
        })
        .collect::<Result<_, _>>()?;

    let [west, south, east, north] = values.as_slice() else {
        return Err(format!(
            "expected 4 comma-separated degrees (west,south,east,north), got {}",
            values.len()
        ));
    };

    Region::from_degrees(*west, *south, *east, *north).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_parsed_as_west_south_east_north() {
        let region = parse_bounds("139.0,35.0,140.5,36.5").expect("valid bounds");
        assert!((region.west().to_degrees().get() - 139.0).abs() < 1e-9);
        assert!((region.south().to_degrees().get() - 35.0).abs() < 1e-9);
        assert!((region.east().to_degrees().get() - 140.5).abs() < 1e-9);
        assert!((region.north().to_degrees().get() - 36.5).abs() < 1e-9);
    }

    #[test]
    fn negative_and_spaced_bounds_are_accepted() {
        let region = parse_bounds(" -70.5 , -34.0 , -70.0 , -33.0 ").expect("valid bounds");
        assert!((region.west().to_degrees().get() + 70.5).abs() < 1e-9);
        assert!(!region.crosses_dateline());
    }

    #[test]
    fn dateline_crossing_bounds_are_recognised() {
        let region = parse_bounds("170,-5,-170,5").expect("valid bounds");
        assert!(region.crosses_dateline());
    }

    #[test]
    fn malformed_bounds_are_reported_clearly() {
        assert!(parse_bounds("1,2,3").is_err());
        assert!(parse_bounds("1,2,3,4,5").is_err());
        assert!(parse_bounds("a,b,c,d").is_err());
        assert!(parse_bounds("").is_err());
        // 緯度が範囲外。
        assert!(parse_bounds("0,-91,1,1").is_err());
    }

    #[test]
    fn the_cli_definition_is_valid() {
        // clap の derive はここで初めて検証される。引数定義の矛盾を起動前に落とす。
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
