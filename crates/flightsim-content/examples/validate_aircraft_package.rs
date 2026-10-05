//! Content-only inspection tool. Never installs or claims physical validation.
use std::path::Path;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = (|| -> flightsim_content::Result<()> {
        if args.len() != 2 {
            return Err(flightsim_content::Error::Invalid(
                "usage: validate_aircraft_package validate|inspect|cancel PATH".into(),
            ));
        }
        match args[0].as_str() {
            "validate" => {
                let temp = tempfile::tempdir()?;
                let p = flightsim_content::aircraft::stage_zip_with_progress(
                    Path::new(&args[1]),
                    temp.path(),
                    |_| true,
                )?;
                println!(
                    "content-only valid: {:?}; {:?}; profile semantics, runtime and rights unverified",
                    p.identity(),
                    p.geometry()
                );
            }
            "inspect" => {
                let p = flightsim_content::aircraft::inspect_installed(Path::new(&args[1]))?;
                println!(
                    "content-only valid: {:?}; profile semantics, runtime and rights unverified",
                    p.identity()
                );
            }
            "cancel" => {
                let temp = tempfile::tempdir()?;
                let result = flightsim_content::aircraft::stage_zip_with_progress(
                    Path::new(&args[1]),
                    temp.path(),
                    |p| p.phase != flightsim_content::ImportPhase::Extracting,
                );
                if !matches!(result, Err(flightsim_content::Error::Cancelled))
                    || std::fs::read_dir(temp.path())?.next().is_some()
                {
                    return Err(flightsim_content::Error::Invalid(
                        "cancellation failed".into(),
                    ));
                }
                println!("cancelled; no published or staged data remains");
            }
            _ => return Err(flightsim_content::Error::Invalid("unknown command".into())),
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
