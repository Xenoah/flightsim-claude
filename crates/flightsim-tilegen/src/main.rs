//! タイル生成 CLI。
//!
//! 詳細は [`flightsim_tilegen`] のクレートドキュメントを参照。

use clap::{Parser, ValueEnum};
use flightsim_core::Meters;
use flightsim_tilegen::geoid::GeoidGrid;
use flightsim_tilegen::vertical_datum::{GeoidModel, VerticalDatum, VerticalDatumMismatch};
use flightsim_tilegen::{RasterSet, Region, TileGenOptions, generate_tiles};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GeoidKind {
    Egm2008,
    Egm96,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceDatum {
    Ellipsoidal,
    Egm2008,
    Egm96,
}

impl SourceDatum {
    const fn datum(self) -> VerticalDatum {
        match self {
            Self::Ellipsoidal => VerticalDatum::Ellipsoidal,
            Self::Egm2008 => VerticalDatum::Geoid(GeoidModel::Egm2008),
            Self::Egm96 => VerticalDatum::Geoid(GeoidModel::Egm96),
        }
    }
}

impl GeoidKind {
    const fn model(self) -> GeoidModel {
        match self {
            Self::Egm2008 => GeoidModel::Egm2008,
            Self::Egm96 => GeoidModel::Egm96,
        }
    }
}

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
    #[arg(long, default_value_t = false, conflicts_with = "geoid_grid")]
    assume_ellipsoidal: bool,

    /// Local GeographicLib 16-bit PGM geoid grid. Applies h = H + N before resampling.
    #[arg(long, value_name = "PGM", requires = "geoid_model")]
    geoid_grid: Option<PathBuf>,

    /// Model of the supplied grid; must match its Description and the DEM datum.
    #[arg(long, value_enum, requires = "geoid_grid")]
    geoid_model: Option<GeoidKind>,

    /// Declare missing source vertical metadata from verified provider documentation.
    /// Existing conflicting GeoTIFF metadata is rejected, never overwritten.
    #[arg(long, value_enum, conflicts_with = "assume_ellipsoidal")]
    source_vertical_datum: Option<SourceDatum>,

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
    let mut rasters = RasterSet::load(&cli.input).map_err(|error| error.to_string())?;
    let original_datums = rasters.non_ellipsoidal_sources();
    if let Some(datum) = cli.source_vertical_datum {
        rasters = rasters
            .declare_vertical_datum(datum.datum())
            .map_err(|error| error.to_string())?;
    }
    let grid = match (&cli.geoid_grid, cli.geoid_model) {
        (Some(path), Some(kind)) => Some(
            GeoidGrid::open(path, kind.model())
                .map_err(|error| format!("geoid {}: {error}", path.display()))?,
        ),
        _ => None,
    };
    if let Some(grid) = &grid {
        eprintln!(
            "normalizing heights with {} (bilinear; h = H + N)...",
            grid.description()
        );
        rasters = rasters
            .normalize_to_ellipsoid(grid)
            .map_err(|error| error.to_string())?;
    }

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
        rasters = rasters.assume_ellipsoidal();
    }

    // 深いレベルはタイル数が 4 倍ずつ増える。着手前に規模を見せる。
    let options = TileGenOptions {
        grid_size: cli.grid_size,
        fill: Meters(cli.fill),
        min_coverage: cli.min_coverage,
    };
    let planned = flightsim_tilegen::generate::planned_tile_count(
        region,
        cli.min_level..=cli.max_level,
        &options,
    )
    .map_err(|error| error.to_string())?;
    eprintln!(
        "levels {}..={} cover {planned} tile(s){}",
        cli.min_level,
        cli.max_level,
        if cli.dry_run { " (dry run)" } else { "" }
    );

    let provenance = generation_provenance(cli, &original_datums, grid.as_ref())?;
    if !cli.dry_run {
        // A batch may replace some tiles before a later write fails. Retire any
        // old success attestation before touching its first tile, and leave this
        // marker in place on errors or process interruption.
        write_provenance(
            &cli.output,
            &format!(
                "Generation status: INCOMPLETE\nTiles may be a partial mixture of this invocation and earlier output.\n{provenance}"
            ),
        )?;
    }
    let report = generate_tiles(
        &rasters,
        region,
        cli.min_level..=cli.max_level,
        &options,
        &cli.output,
        cli.dry_run,
    )
    .map_err(|error| error.to_string())?;
    if !cli.dry_run {
        write_provenance(
            &cli.output,
            &format!("Generation status: COMPLETE\n{provenance}"),
        )?;
    }

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

/// This per-invocation record deliberately stays outside the runtime tile format.
/// Fingerprints detect accidental input changes; they are not authentication.
fn generation_provenance(
    cli: &Cli,
    mismatched: &[(usize, flightsim_tilegen::vertical_datum::VerticalDatum)],
    grid: Option<&GeoidGrid>,
) -> Result<String, String> {
    use std::fmt::Write;
    let destination = cli.output.join("terrain-provenance.txt");
    let mut text = format!(
        "flightsim-tilegen {}\nRuntime height contract: WGS84 ellipsoidal metres (ADR-0005)\n\
         This record describes only the latest invocation; existing tiles outside its scope are not attested.\n\
         Source fingerprint: FNV-1a 64-bit over file bytes (not cryptographic authentication)\n\
         Levels: {}..={}\nGrid size: {}\nFill metres: {}\nMinimum coverage: {}\nBounds: {:?}\n\
         Assume ellipsoidal without conversion: {}\nDeclared source datum: {:?}\n",
        env!("CARGO_PKG_VERSION"),
        cli.min_level,
        cli.max_level,
        cli.grid_size,
        cli.fill,
        cli.min_coverage,
        cli.bounds,
        cli.assume_ellipsoidal,
        cli.source_vertical_datum
    );
    for (index, path) in cli.input.iter().enumerate() {
        reject_provenance_alias(path, &destination)?;
        let datum = mismatched
            .iter()
            .find(|(source, _)| *source == index)
            .map_or(
                flightsim_tilegen::vertical_datum::VerticalDatum::Ellipsoidal,
                |(_, datum)| *datum,
            );
        let (length, fingerprint) = source_fingerprint(path)?;
        writeln!(text, "Input {index}: {path:?}; bytes={length}; fnv1a64={fingerprint:016x}; source datum={datum}").expect("String write");
    }
    if let (Some(path), Some(grid)) = (&cli.geoid_grid, grid) {
        reject_provenance_alias(path, &destination)?;
        let (length, fingerprint) = source_fingerprint(path)?;
        writeln!(text, "Geoid: {path:?}; bytes={length}; fnv1a64={fingerprint:016x}; description={:?}; dimensions={:?}", grid.description(), grid.dimensions()).expect("String write");
        text.push_str("Conversion: h = H + N at source pixel centres before resampling; bilinear geoid interpolation\n");
    }
    Ok(text)
}

fn reject_provenance_alias(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    if destination.exists()
        && same_file::is_same_file(source, destination).map_err(|error| error.to_string())?
    {
        return Err("the provenance output would overwrite an input file".to_owned());
    }
    Ok(())
}

fn source_fingerprint(path: &std::path::Path) -> Result<(u64, u64), String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let (mut length, mut hash) = (0_u64, 0xcbf2_9ce4_8422_2325_u64);
    let mut buffer = [0_u8; 16_384];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok((length, hash));
        }
        length += count as u64;
        for &byte in &buffer[..count] {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
}

fn write_provenance(directory: &std::path::Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist(directory.join("terrain-provenance.txt"))
        .map_err(|error| error.to_string())?;
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
