//! Bake an offline compact world atlas from explicitly aligned raw arrays.
use clap::Parser;
use flightsim_core::Degrees;
use flightsim_tilegen::global::{
    GlobalAtlasGeometry, canonical_global_geometry, encode_canonical_global_atlas_from_raw,
    encode_global_atlas_from_raw, write_global_atlas,
};
use flightsim_world::global::{GLOBAL_TERRAIN_DATASET_ID, global_atlas_fingerprint};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "flightsim-globalgen",
    version,
    about = "Bake aligned ETOPO EGM2008 / geoid / land raw arrays to a complete FSGT world atlas (offline)"
)]
struct Cli {
    /// Row-major north-to-south EGM2008 orthometric metres, f32 little-endian.
    #[arg(long)]
    elevation: PathBuf,
    /// Matching EGM2008 geoid N above WGS84 ellipsoid, metres, f32 little-endian.
    #[arg(long)]
    geoid: PathBuf,
    /// Independent land mask, one byte per node: 0 ocean, 1 land.
    #[arg(long)]
    land_mask: PathBuf,
    /// Independent lake/reservoir mask, 0/1 bytes; required with --canonical.
    #[arg(long)]
    inland_water_mask: Option<PathBuf>,
    #[arg(long)]
    width: u32,
    #[arg(long)]
    height: u32,
    /// Longitude of the first sample, degrees (not west edge).
    #[arg(long, allow_hyphen_values = true)]
    origin_lon: f64,
    /// Latitude of the northernmost sample, degrees (not north edge).
    #[arg(long, allow_hyphen_values = true)]
    origin_lat: f64,
    /// Mandatory declaration, never guessed from negative or positive heights.
    #[arg(long, value_parser = ["egm2008"])]
    source_vertical_datum: String,
    /// Preparation manifest with official source URLs/hashes and alignment steps.
    #[arg(long)]
    source_manifest: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Resample to the canonical 2048x1024 lattice aligned with render tiles.
    #[arg(long)]
    canonical: bool,
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let geometry = GlobalAtlasGeometry {
        width: cli.width,
        height: cli.height,
        longitude_origin: Degrees(cli.origin_lon),
        north_latitude: Degrees(cli.origin_lat),
    };
    let count = geometry.validate()?;
    // An explicit named datum is required even though v1 supports only this one.
    if cli.source_vertical_datum != "egm2008" {
        return Err("source datum must be egm2008".into());
    }
    let provenance_path = cli.output.with_extension("provenance.txt");
    if provenance_path == cli.output {
        return Err("atlas output must not use the provenance.txt extension".into());
    }
    if cli.canonical && cli.inland_water_mask.is_none() {
        return Err(
            "--canonical requires --inland-water-mask, even if all its bytes are zero".into(),
        );
    }
    for source in [
        &cli.elevation,
        &cli.geoid,
        &cli.land_mask,
        &cli.source_manifest,
    ]
    .into_iter()
    .chain(cli.inland_water_mask.iter())
    {
        for destination in [&cli.output, &provenance_path] {
            if source == destination
                || (destination.exists() && same_file::is_same_file(source, destination)?)
            {
                return Err("atlas and provenance outputs must not overwrite a source file".into());
            }
        }
    }
    let mut manifest = String::new();
    std::fs::File::open(&cli.source_manifest)?
        .take(1_048_577)
        .read_to_string(&mut manifest)?;
    if manifest.trim().is_empty() || manifest.len() > 1_048_576 {
        return Err("source manifest must be nonempty UTF-8, at most 1 MiB".into());
    }
    let inland_water_fingerprint = if let Some(path) = &cli.inland_water_mask {
        format!("{:016x}", file_fingerprint(path, count)?)
    } else {
        "none (all zero)".to_owned()
    };
    let input_fingerprints = format!(
        "elevation_fnv1a64={:016x}\ngeoid_fnv1a64={:016x}\nland_mask_fnv1a64={:016x}\ninland_water_mask_fnv1a64={inland_water_fingerprint}\n",
        file_fingerprint(&cli.elevation, count * 4)?,
        file_fingerprint(&cli.geoid, count * 4)?,
        file_fingerprint(&cli.land_mask, count)?,
    );
    let dataset_id = if cli.canonical {
        "externally-prepared-canonical-global-atlas"
    } else {
        "externally-prepared-global-atlas"
    };
    let resampling = if cli.canonical {
        "bilinear physical surface H (source ocean H=0) and geoid N; nearest independent dry-land/inland-water masks and lake levels; canonical 2048x1024 lattice"
    } else {
        "none; input lattice retained"
    };
    write_marker(
        &provenance_path,
        &format!("status=INCOMPLETE\ndataset={dataset_id}\n{manifest}\n"),
    )?;
    let bytes = if cli.canonical {
        encode_canonical_global_atlas_from_raw(
            geometry,
            &cli.elevation,
            &cli.geoid,
            &cli.land_mask,
            cli.inland_water_mask.as_deref(),
        )?
    } else {
        encode_global_atlas_from_raw(
            geometry,
            &cli.elevation,
            &cli.geoid,
            &cli.land_mask,
            cli.inland_water_mask.as_deref(),
        )?
    };
    let output_geometry = if cli.canonical {
        canonical_global_geometry()
    } else {
        geometry
    };
    // Only exact bytes, not a caller's text manifest or a non-cryptographic
    // fingerprint alone, identify this output as the reviewed bundled asset.
    let matches_bundled =
        bytes.as_slice() == include_bytes!("../../../flightsim-world/data/global-terrain.fsgt");
    let dataset_id = if matches_bundled {
        GLOBAL_TERRAIN_DATASET_ID
    } else {
        dataset_id
    };
    write_global_atlas(&cli.output, &bytes)?;
    let hash = global_atlas_fingerprint(&bytes);
    write_marker(
        &provenance_path,
        &format!(
            "status=COMPLETE\ndataset={dataset_id}\nformat=FSGT-v2\nbytes={}\nfingerprint_fnv1a64={hash:016x}\nwidth={}\nheight={}\nlongitude_origin_degrees={:.17}\nnorth_latitude_degrees={:.17}\nsource_vertical_datum=EGM2008-EPSG3855\nheight_contract=h=H+N; ocean-mask H=0; below-sea-level land retained\nquantization=H 1m, N 0.01m; halfway-away-from-zero\nresampling={resampling}\nmatches_reviewed_bundled_bytes={matches_bundled}\nsource_attestation=caller-provided manifest; raw CLI does not authenticate original data sources\n{input_fingerprints}source_manifest_begin\n{manifest}\nsource_manifest_end\n",
            bytes.len(),
            output_geometry.width,
            output_geometry.height,
            output_geometry.longitude_origin.get(),
            output_geometry.north_latitude.get(),
        ),
    )?;
    eprintln!(
        "wrote {} bytes to {}; fingerprint {hash:016x}",
        bytes.len(),
        cli.output.display()
    );
    Ok(())
}

fn write_marker(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.flush()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn file_fingerprint(path: &Path, expected_bytes: usize) -> std::io::Result<u64> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() != expected_bytes as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "input length does not match the declared grid",
        ));
    }
    let mut file = file.take(expected_bytes as u64 + 1);
    let mut total = 0_usize;
    let mut buffer = [0_u8; 65_536];
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    loop {
        let count = file.read(&mut buffer)?;
        total += count;
        if total > expected_bytes || (count == 0 && total != expected_bytes) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "input changed while fingerprinting",
            ));
        }
        if count == 0 {
            return Ok(value);
        }
        for byte in &buffer[..count] {
            value = (value ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Geodetic, Meters};
    use flightsim_world::global::GlobalTerrain;

    fn fixture(directory: &Path) -> Cli {
        let elevation = directory.join("height.f32le");
        let geoid = directory.join("geoid.f32le");
        let land_mask = directory.join("land.u8");
        let source_manifest = directory.join("source.json");
        std::fs::write(
            &elevation,
            (0..8)
                .flat_map(|_| (-420.5_f32).to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            &geoid,
            (0..8)
                .flat_map(|_| 32.125_f32.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(&land_mask, [1_u8; 8]).unwrap();
        std::fs::write(
            &source_manifest,
            "{\"source\":\"explicit synthetic test\"}\n",
        )
        .unwrap();
        Cli {
            elevation,
            geoid,
            land_mask,
            source_manifest,
            inland_water_mask: None,
            width: 4,
            height: 2,
            origin_lon: -135.0,
            origin_lat: 45.0,
            source_vertical_datum: "egm2008".to_owned(),
            output: directory.join("world.fsgt"),
            canonical: false,
        }
    }

    #[test]
    fn complete_bake_matches_runtime_and_has_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let cli = fixture(directory.path());
        run(&cli).unwrap();
        let atlas = GlobalTerrain::from_bytes(std::fs::read(&cli.output).unwrap()).unwrap();
        let sample = atlas
            .sample(Geodetic::from_degrees(45.0, -135.0, 0.0))
            .unwrap();
        assert_eq!(sample.elevation_msl, Meters(-421.0));
        let marker = std::fs::read_to_string(cli.output.with_extension("provenance.txt")).unwrap();
        assert!(marker.starts_with("status=COMPLETE\n"));
        assert!(marker.contains("dataset=externally-prepared-global-atlas\n"));
        assert!(marker.contains("matches_reviewed_bundled_bytes=false"));
        assert!(marker.contains("raw CLI does not authenticate original data sources"));
        assert!(marker.contains("elevation_fnv1a64="));
        assert!(marker.contains("geoid_fnv1a64="));
        assert!(marker.contains("land_mask_fnv1a64="));
        assert!(marker.contains(&format!("fingerprint_fnv1a64={:016x}", atlas.fingerprint())));
    }

    #[test]
    fn failed_validation_marks_incomplete_and_preserves_previous_atlas() {
        let directory = tempfile::tempdir().unwrap();
        let cli = fixture(directory.path());
        run(&cli).unwrap();
        let previous = std::fs::read(&cli.output).unwrap();
        std::fs::write(
            &cli.geoid,
            (0..8)
                .flat_map(|_| f32::NAN.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(run(&cli).is_err());
        assert_eq!(std::fs::read(&cli.output).unwrap(), previous);
        let marker = std::fs::read_to_string(cli.output.with_extension("provenance.txt")).unwrap();
        assert!(marker.starts_with("status=INCOMPLETE\n"));
    }

    #[test]
    fn output_and_provenance_cannot_overwrite_source_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut cli = fixture(directory.path());
        cli.output = cli.elevation.clone();
        assert!(run(&cli).is_err());
        assert_eq!(std::fs::metadata(&cli.elevation).unwrap().len(), 32);
        cli.output = directory.path().join("source.fsgt");
        cli.source_manifest = cli.output.with_extension("provenance.txt");
        std::fs::write(&cli.source_manifest, "source manifest retained").unwrap();
        assert!(run(&cli).is_err());
        assert_eq!(
            std::fs::read_to_string(&cli.source_manifest).unwrap(),
            "source manifest retained"
        );
    }
}
