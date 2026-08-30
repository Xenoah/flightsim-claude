//! OSM PBF から実行時空港 DB を焼く CLI。

use clap::Parser;
use flightsim_tilegen::{AirportGenerationReport, generate_airport_database};
use std::path::PathBuf;
use std::process::ExitCode;

/// 地域 OSM PBF の滑走路中心線を実行時空港 DB へ変換する。
#[derive(Debug, Parser)]
#[command(
    name = "flightsim-airportgen",
    version,
    about = "地域 OSM PBF の滑走路中心線を実行時空港 DB へ変換する。",
    long_about = None
)]
struct Cli {
    /// 入力する地域 OpenStreetMap PBF。
    #[arg(short, long, value_name = "OSM_PBF")]
    input: PathBuf,

    /// 出力する実行時空港 DB（通常は `.fsairports`）。
    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,
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
    eprintln!("reading OSM PBF {}...", cli.input.display());
    let report =
        generate_airport_database(&cli.input, &cli.output).map_err(|error| error.to_string())?;
    print_report(report, &cli.output);
    Ok(())
}

fn print_report(report: AirportGenerationReport, output: &std::path::Path) {
    eprintln!(
        "wrote {} runway(s) to {}",
        report.runways_written,
        output.display()
    );
    eprintln!("matched {} aeroway=runway way(s)", report.runway_ways_seen);
    if report.widths_defaulted > 0 {
        eprintln!(
            "warning: used the 45 m width fallback for {} runway(s)",
            report.widths_defaulted
        );
    }
    report_skipped("area=yes", report.skipped_areas);
    report_skipped("closed area geometry", report.skipped_closed);
    report_skipped("missing endpoint nodes", report.skipped_missing_nodes);
    report_skipped(
        "invalid endpoint coordinates",
        report.skipped_bad_coordinates,
    );
    report_skipped("degenerate endpoint geometry", report.skipped_degenerate);
}

fn report_skipped(reason: &str, count: usize) {
    if count > 0 {
        eprintln!("skipped {count} way(s): {reason}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_definition_is_valid() {
        use clap::CommandFactory;
        let command = Cli::command();
        assert!(
            command
                .get_about()
                .expect("airportgen has an about string")
                .to_string()
                .contains("OSM PBF")
        );
        command.debug_assert();
    }

    #[test]
    fn input_and_output_are_required_and_named_options() {
        let cli = Cli::try_parse_from([
            "flightsim-airportgen",
            "--input",
            "region.osm.pbf",
            "--output",
            "region.fsairports",
        ])
        .expect("valid command line");
        assert_eq!(cli.input, PathBuf::from("region.osm.pbf"));
        assert_eq!(cli.output, PathBuf::from("region.fsairports"));

        assert!(Cli::try_parse_from(["flightsim-airportgen"]).is_err());
    }
}
