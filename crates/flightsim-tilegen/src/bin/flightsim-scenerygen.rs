//! Offline, bounded regional OSM roads/buildings/landcover importer.
use clap::Parser;
use flightsim_tilegen::{SceneryBakeOptions, generate_scenery_database};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "flightsim-scenerygen",
    version,
    about = "Bake a bounded regional OSM PBF into an offline .fsscenery database. Simple way geometry only; omissions are reported."
)]
struct Cli {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Human-readable source region and snapshot identity, not a claim of survey accuracy.
    #[arg(long)]
    source_name: String,
    /// Official download/source provenance URL, stored in the runtime database.
    #[arg(long)]
    source_url: String,
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = SceneryBakeOptions {
        source_name: cli.source_name,
        source_url: cli.source_url,
    };
    match generate_scenery_database(&cli.input, &cli.output, &options) {
        Ok(report) => {
            println!("{report:#?}");
            println!(
                "Wrote {}. Map data © OpenStreetMap contributors; derived database ODbL1.0. Heights from levels/defaults are display estimates. No multipolygon, bridge or tunnel reconstruction.",
                cli.output.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_definition() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        assert!(Cli::try_parse_from(["flightsim-scenerygen"]).is_err());
    }
}
