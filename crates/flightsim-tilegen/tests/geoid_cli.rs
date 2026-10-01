//! The CLI must require explicit correction, and must retain per-run provenance.
use flightsim_tilegen::testing::GeoTiffBuilder;
use std::process::Command;

fn prepare(directory: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let source = directory.join("dem.tif");
    std::fs::write(
        &source,
        GeoTiffBuilder::new(2, 2, vec![100.0; 4])
            .origin(0.0, 0.001)
            .pixel_size(0.001, 0.001)
            .vertical_cs_type(3855)
            .build(),
    )
    .unwrap();
    let geoid = directory.join("geoid.pgm");
    let mut data =
        b"P5\n# Description WGS84 EGM2008, synthetic\n# Offset 30\n# Scale 1\n4 3\n65535\n"
            .to_vec();
    data.extend([0_u16; 12].iter().flat_map(|x| x.to_be_bytes()));
    std::fs::write(&geoid, data).unwrap();
    (source, geoid)
}

#[test]
fn correction_is_explicit_and_normalized_tiles_record_the_model() {
    let directory = tempfile::tempdir().unwrap();
    let (source, grid) = prepare(directory.path());
    let output = directory.path().join("tiles");
    let base = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_flightsim-tilegen"));
        cmd.arg("--input")
            .arg(&source)
            .arg("--output")
            .arg(&output)
            .args([
                "--min-level",
                "8",
                "--max-level",
                "8",
                "--grid-size",
                "3",
                "--bounds=-0.001,-0.002,0.003,0.002",
            ]);
        cmd
    };
    let rejected = base().output().unwrap();
    assert!(!rejected.status.success());
    assert!(!output.exists());
    let accepted = base()
        .arg("--geoid-grid")
        .arg(&grid)
        .args(["--geoid-model", "egm2008"])
        .output()
        .unwrap();
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let provenance = std::fs::read_to_string(output.join("terrain-provenance.txt")).unwrap();
    assert!(provenance.contains("h = H + N"));
    assert!(provenance.contains("EGM2008"));
    assert!(provenance.contains("fnv1a64="));
    assert!(provenance.contains("Assume ellipsoidal without conversion: false"));
    let files = std::fs::read_dir(output.join("8")).unwrap();
    let mut saw_normalized = false;
    for column in files {
        for tile in std::fs::read_dir(column.unwrap().path()).unwrap() {
            let bytes = std::fs::read(tile.unwrap().path()).unwrap();
            let tile = flightsim_world::dem::io::read_tile(&mut bytes.as_slice()).unwrap();
            let grid = tile.tile.grid();
            for row in 0..grid.height() {
                for column in 0..grid.width() {
                    let height = grid.sample_at(column, row).get();
                    assert!(
                        height.abs() < 0.002 || (height - 130.0).abs() < 0.002,
                        "expected 130 m corrected terrain or 0 m fill, got {height}"
                    );
                    saw_normalized |= (height - 130.0).abs() < 0.002;
                }
            }
        }
    }
    assert!(
        saw_normalized,
        "at least one generated sample must contain corrected terrain"
    );
}

#[test]
fn dry_run_writes_nothing_and_conflicting_options_fail() {
    let directory = tempfile::tempdir().unwrap();
    let (source, grid) = prepare(directory.path());
    let output = directory.path().join("tiles");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flightsim-tilegen"));
    cmd.arg("--input")
        .arg(&source)
        .arg("--output")
        .arg(&output)
        .arg("--geoid-grid")
        .arg(&grid)
        .args([
            "--geoid-model",
            "egm2008",
            "--dry-run",
            "--min-level",
            "0",
            "--max-level",
            "0",
        ]);
    assert!(cmd.output().unwrap().status.success());
    assert!(!output.exists());
    assert!(
        !cmd.arg("--assume-ellipsoidal")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!output.exists());
}
