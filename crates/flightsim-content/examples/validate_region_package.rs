//! GUI-independent consumer of the existing local terrain package boundary.
//! Validation is not an accuracy, runtime, publisher, or redistribution approval.
use std::path::Path;

use flightsim_content::{FileKind, ImportPhase, InstalledPackage, Manifest};

fn describe(manifest: &Manifest, identity: &flightsim_content::RegionalIdentity) {
    let tiles = manifest
        .files
        .iter()
        .filter(|file| file.kind == FileKind::TerrainDem)
        .count();
    println!(
        "{}@{}: {tiles} terrain tiles; manifest SHA256 {}",
        identity.id, identity.version, identity.manifest_sha256
    );
    println!(
        "Declared bounds {:?}; nominal source resolution {} m; datum {}",
        manifest.terrain.bounds_degrees,
        manifest.terrain.nominal_resolution_m,
        manifest.terrain.datum
    );
    println!(
        "No flight activated. Regional replay, runtime quality and rights remain separate gates."
    );
}

fn describe_installed(package: &InstalledPackage) {
    describe(package.manifest(), package.identity());
    println!("Installed directory: {}", package.directory().display());
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, path] if command == "validate" => {
            let temporary_store = tempfile::tempdir()?;
            let staged = flightsim_content::stage_zip_with_progress(
                Path::new(path),
                temporary_store.path(),
                |_| true,
            )?;
            describe(staged.manifest(), staged.identity());
            println!("Validated only; temporary staging is discarded without installation.");
        }
        [command, path] if command == "inspect" => {
            describe_installed(&flightsim_content::inspect_installed(Path::new(path))?);
        }
        [command, path] if command == "cancel" => {
            let temporary_store = tempfile::tempdir()?;
            let result = flightsim_content::stage_zip_with_progress(
                Path::new(path),
                temporary_store.path(),
                |progress| progress.phase != ImportPhase::Extracting,
            );
            if !matches!(result, Err(flightsim_content::Error::Cancelled))
                || std::fs::read_dir(temporary_store.path())?.next().is_some()
            {
                return Err("cancellation did not discard every staged file".into());
            }
            println!("Cancelled; no installed or staged files remain.");
        }
        [command, path, store] if command == "install" => {
            describe_installed(&flightsim_content::install_zip(
                Path::new(path),
                Path::new(store),
            )?);
            println!(
                "Choose the package in the application's Installed list, then explicitly Start."
            );
        }
        _ => {
            return Err(
                "usage: validate_region_package validate|inspect|cancel PATH, or install ZIP STORE"
                    .into(),
            );
        }
    }
    Ok(())
}
