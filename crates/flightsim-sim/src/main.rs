//! ヘッドレス飛行ランナー。
//!
//! 焼いた地形タイルの上で場周飛行を 1 回まわし、軌跡を CSV で出す。
//! 詳細は [`flightsim_sim`] のクレートドキュメントを参照。

use clap::Parser;
use flightsim_core::{Degrees, Geodetic, Meters, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::{CircuitPlan, GroundSampler, Phase, SimulationOptions, fly};
use flightsim_world::{DiskTileSource, MemoryTileSource, Terrain, TileSource};
use std::path::PathBuf;
use std::process::ExitCode;

/// 実地形の上で場周飛行を 1 回まわし、軌跡を出力する。
#[derive(Debug, Parser)]
#[command(name = "flightsim-headless", version, about, long_about = None)]
struct Cli {
    /// タイルのルートディレクトリ。省略すると全域を楕円体高 0 m の海面として扱う。
    #[arg(short, long, value_name = "DIR")]
    tiles: Option<PathBuf>,

    /// 出発地点 `緯度,経度`（度）。
    #[arg(short, long, value_name = "LAT,LON", allow_hyphen_values = true)]
    start: String,

    /// 滑走路の方位（真方位・度）。
    #[arg(long, default_value_t = 0.0, value_name = "DEG")]
    heading: f64,

    /// 旋回後の方位（真方位・度）。
    #[arg(long, default_value_t = 90.0, value_name = "DEG")]
    outbound: f64,

    /// 場周高度（重心の対地高度・m）。
    #[arg(long, default_value_t = 300.0, value_name = "METRES")]
    pattern_altitude: f64,

    /// 打ち切り時間（秒）。
    #[arg(long, default_value_t = 600.0, value_name = "SECONDS")]
    duration: f64,

    /// 軌跡の記録間隔（秒）。
    #[arg(long, default_value_t = 0.5, value_name = "SECONDS")]
    sample_interval: f64,

    /// タイルを探す最も粗いレベル。
    #[arg(long, default_value_t = 8, value_name = "N")]
    min_level: u8,

    /// タイルを探す最も細かいレベル。
    #[arg(long, default_value_t = 13, value_name = "N")]
    max_level: u8,

    /// 軌跡の出力先 CSV。省略すると標準出力へ書く。
    #[arg(short, long, value_name = "CSV")]
    output: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(diverged) if diverged => {
            // 発散した軌跡は数値として意味がない。成功として報告しない。
            eprintln!("error: the trajectory diverged; the recorded samples are not trustworthy");
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<bool, String> {
    if cli.min_level > cli.max_level {
        return Err(format!(
            "--min-level ({}) is deeper than --max-level ({})",
            cli.min_level, cli.max_level
        ));
    }

    let start = parse_start(&cli.start)?;

    // タイルを指定しない場合に存在しないパスを捏造しないこと。
    // OS によっては「ファイルが無い」ではなく「パスが不正」というエラーになり、
    // 本物の読み込み失敗と区別がつかなくなる。
    let source: Box<dyn TileSource> = match &cli.tiles {
        Some(path) => Box::new(DiskTileSource::new(path)),
        None => Box::new(MemoryTileSource::new()),
    };
    let mut terrain = Terrain::new(source, 256 * 1024 * 1024, cli.min_level..=cli.max_level);

    match &cli.tiles {
        Some(path) => eprintln!("terrain: {}", path.display()),
        None => eprintln!("terrain: none — treating the whole world as sea level"),
    }
    eprintln!(
        "start:   {:.5}°, {:.5}°  runway heading {:.0}°",
        start.latitude_degrees(),
        start.longitude_degrees(),
        cli.heading
    );

    let plan = CircuitPlan {
        runway_heading: Degrees(cli.heading).to_radians(),
        outbound_heading: Degrees(cli.outbound).to_radians(),
        pattern_altitude_agl: Meters(cli.pattern_altitude),
        ..CircuitPlan::default()
    };
    let options = SimulationOptions {
        max_duration: Seconds(cli.duration),
        sample_interval: Seconds(cli.sample_interval),
        ..SimulationOptions::default()
    };

    let trajectory = fly(
        &AircraftConfig::light_single(),
        &plan,
        start,
        &mut terrain,
        &GroundSampler::default(),
        &options,
    );

    // 何が起きたかを数字で出す。「飛んだ」だけでは検証にならない。
    eprintln!();
    eprintln!("phases:  {}", format_phases(&trajectory));
    eprintln!(
        "flew:    {:.1} s, peak {:.0} m AGL, {} sample(s)",
        trajectory.duration.get(),
        trajectory.peak_agl().get(),
        trajectory.samples.len()
    );
    if let Some(last) = trajectory.samples.last() {
        eprintln!(
            "ended:   {:.5}°, {:.5}°  {:.0} m AGL  {:.0} m/s  phase {}",
            last.position.latitude_degrees(),
            last.position.longitude_degrees(),
            last.agl.get(),
            last.airspeed.get(),
            last.phase.name(),
        );
    }
    if trajectory.final_phase != Phase::Complete {
        // 「飛んだ」と「一周できた」は違う。黙って成功扱いにしない。
        eprintln!(
            "warning: the circuit did not finish — it stopped in `{}` after {:.0} s.",
            trajectory.final_phase.name(),
            trajectory.duration.get()
        );
        eprintln!(
            "         Raise --duration, or check whether the terrain along the route outclimbs the aircraft."
        );
    }
    if trajectory.steps_without_terrain > 0 {
        eprintln!(
            "note:    {} step(s) had no terrain data and used sea level",
            trajectory.steps_without_terrain
        );
    }
    for failure in terrain.load_failures() {
        eprintln!("warning: {failure}");
    }

    match &cli.output {
        Some(path) => {
            let file = std::fs::File::create(path)
                .map_err(|error| format!("could not create {}: {error}", path.display()))?;
            let mut writer = std::io::BufWriter::new(file);
            trajectory
                .write_csv(&mut writer)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
            eprintln!("wrote:   {}", path.display());
        }
        None => {
            let stdout = std::io::stdout();
            trajectory
                .write_csv(&mut stdout.lock())
                .map_err(|error| format!("could not write to stdout: {error}"))?;
        }
    }

    Ok(trajectory.diverged)
}

fn format_phases(trajectory: &flightsim_sim::Trajectory) -> String {
    trajectory
        .phases_visited()
        .iter()
        .map(|phase| phase.name())
        .collect::<Vec<_>>()
        .join(" → ")
}

/// `緯度,経度` を度として解析する。
fn parse_start(text: &str) -> Result<Geodetic, String> {
    let values: Vec<f64> = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .map_err(|_| format!("`{}` is not a number", part.trim()))
        })
        .collect::<Result<_, _>>()?;

    let [latitude, longitude] = values.as_slice() else {
        return Err(format!(
            "expected 2 comma-separated degrees (lat,lon), got {}",
            values.len()
        ));
    };
    if !(-90.0..=90.0).contains(latitude) {
        return Err(format!("latitude {latitude}° is outside ±90°"));
    }
    if !(-180.0..=180.0).contains(longitude) {
        return Err(format!("longitude {longitude}° is outside ±180°"));
    }

    Ok(Geodetic::new(
        Degrees(*latitude).to_radians(),
        Degrees(*longitude).to_radians(),
        Meters::ZERO,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_position_is_parsed_as_latitude_then_longitude() {
        let start = parse_start("35.553,139.781").expect("valid");
        assert!((start.latitude_degrees() - 35.553).abs() < 1e-9);
        assert!((start.longitude_degrees() - 139.781).abs() < 1e-9);
    }

    #[test]
    fn negative_and_spaced_coordinates_are_accepted() {
        let start = parse_start(" -33.39 , -70.79 ").expect("valid");
        assert!((start.latitude_degrees() + 33.39).abs() < 1e-9);
        assert!((start.longitude_degrees() + 70.79).abs() < 1e-9);
    }

    #[test]
    fn malformed_or_out_of_range_positions_are_rejected() {
        assert!(parse_start("35.0").is_err());
        assert!(parse_start("35.0,139.0,0.0").is_err());
        assert!(parse_start("north,east").is_err());
        assert!(parse_start("").is_err());
        assert!(parse_start("91.0,0.0").is_err());
        assert!(parse_start("0.0,181.0").is_err());
    }

    #[test]
    fn the_cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
