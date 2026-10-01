//! Per-file atomic replacement and honest status for partially completed batches.
use flightsim_tilegen::testing::{GeoTiffBuilder, PixelConvention};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn source(directory: &Path) -> PathBuf {
    let path = directory.join("dem.tif");
    std::fs::write(
        &path,
        GeoTiffBuilder::new(3, 3, vec![100.0; 9])
            .origin(0.0, 45.0)
            .pixel_size(45.0, 45.0)
            .convention(PixelConvention::Point)
            .vertical_cs_type(4979)
            .build(),
    )
    .unwrap();
    path
}

fn run(source: &Path, output: &Path, dry_run: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flightsim-tilegen"));
    command
        .arg("--input")
        .arg(source)
        .arg("--output")
        .arg(output)
        .args([
            "--min-level",
            "1",
            "--max-level",
            "1",
            "--grid-size",
            "3",
            "--bounds=0,-45,90,45",
        ]);
    if dry_run {
        command.arg("--dry-run");
    }
    command.output().unwrap()
}

#[test]
fn partial_failure_replaces_old_attestation_and_success_completes_it() {
    let directory = tempfile::tempdir().unwrap();
    let source = source(directory.path());
    let output = directory.path().join("tiles");
    let column = output.join("1/2");
    std::fs::create_dir_all(&column).unwrap();
    let first = column.join("0.fsdem");
    let second = column.join("1.fsdem");
    std::fs::write(&first, b"old tile bytes").unwrap();
    let previous = directory.path().join("previous-tile");
    std::fs::hard_link(&first, &previous).unwrap();
    // A directory at the second destination forces persist/rename to fail after
    // the first tile has been replaced, without disk-full or permission timing.
    std::fs::create_dir(&second).unwrap();
    std::fs::write(second.join("preserve"), b"sentinel").unwrap();
    let marker = output.join("terrain-provenance.txt");
    std::fs::write(&marker, "Generation status: COMPLETE\nold successful run\n").unwrap();

    let failed = run(&source, &output, false);
    assert!(!failed.status.success());
    let provenance = std::fs::read_to_string(&marker).unwrap();
    assert!(provenance.starts_with("Generation status: INCOMPLETE\n"));
    assert!(!provenance.contains("old successful run"));
    let bytes = std::fs::read(&first).unwrap();
    assert!(flightsim_world::dem::io::read_tile(&mut bytes.as_slice()).is_ok());
    // Replacement must not truncate the old inode behind other readers/links.
    assert_eq!(std::fs::read(previous).unwrap(), b"old tile bytes");
    assert_eq!(std::fs::read(second.join("preserve")).unwrap(), b"sentinel");
    assert_eq!(
        std::fs::read_dir(&column).unwrap().count(),
        2,
        "temporary file leaked"
    );

    std::fs::remove_dir_all(&second).unwrap();
    let successful = run(&source, &output, false);
    assert!(
        successful.status.success(),
        "{}",
        String::from_utf8_lossy(&successful.stderr)
    );
    assert!(
        std::fs::read_to_string(marker)
            .unwrap()
            .starts_with("Generation status: COMPLETE\n")
    );
    let bytes = std::fs::read(second).unwrap();
    assert!(flightsim_world::dem::io::read_tile(&mut bytes.as_slice()).is_ok());
}

#[test]
fn marker_failure_aborts_before_tiles_and_dry_run_preserves_existing_marker() {
    let directory = tempfile::tempdir().unwrap();
    let source = source(directory.path());
    let output = directory.path().join("tiles");
    let column = output.join("1/2");
    std::fs::create_dir_all(&column).unwrap();
    let first = column.join("0.fsdem");
    std::fs::write(&first, b"old tile bytes").unwrap();
    let marker = output.join("terrain-provenance.txt");
    std::fs::create_dir(&marker).unwrap();
    assert!(!run(&source, &output, false).status.success());
    assert_eq!(std::fs::read(&first).unwrap(), b"old tile bytes");
    assert!(!column.join("1.fsdem").exists());
    assert_eq!(
        std::fs::read_dir(&output).unwrap().count(),
        2,
        "temporary marker leaked"
    );

    std::fs::remove_dir(&marker).unwrap();
    let old = "Generation status: COMPLETE\nprevious invocation\n";
    std::fs::write(&marker, old).unwrap();
    assert!(run(&source, &output, true).status.success());
    assert_eq!(std::fs::read_to_string(marker).unwrap(), old);
    assert_eq!(std::fs::read(first).unwrap(), b"old tile bytes");
    assert!(!column.join("1.fsdem").exists());
}

#[test]
fn automatic_bounds_write_tiles_on_both_sides_of_the_dateline() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("dateline.tif");
    std::fs::write(
        &source,
        GeoTiffBuilder::new(3, 3, vec![100.0; 9])
            .origin(179.0, 1.0)
            .pixel_size(1.0, 1.0)
            .convention(PixelConvention::Point)
            .vertical_cs_type(4979)
            .build(),
    )
    .unwrap();
    let output = directory.path().join("tiles");
    let result = Command::new(env!("CARGO_BIN_EXE_flightsim-tilegen"))
        .arg("--input")
        .arg(&source)
        .arg("--output")
        .arg(&output)
        .args(["--min-level", "5", "--max-level", "5", "--grid-size", "3"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        output.join("5/0").is_dir(),
        "western hemisphere was omitted"
    );
    assert!(
        output.join("5/63").is_dir(),
        "eastern hemisphere was omitted"
    );
    assert_eq!(std::fs::read_dir(output.join("5")).unwrap().count(), 2);
}
