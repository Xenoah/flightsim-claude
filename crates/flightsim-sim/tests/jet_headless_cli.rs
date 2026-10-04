use flightsim_sim::replay_v4::JetRecording;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "flightsim-jet-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flightsim-jet-headless"))
        .arg("--profile")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/examples/aircraft-profiles-v2/numerical-jet.json"),
        )
        .args(arguments)
        .output()
        .unwrap()
}
#[test]
fn standalone_loads_public_profile_writes_trajectory_and_verifies_v4() {
    let scratch = Scratch::new();
    let replay = scratch.0.join("flight.fsreplay");
    let csv = scratch.0.join("flight.csv");
    let output = run(&[
        "--scenario".as_ref(),
        "trim".as_ref(),
        "--seconds".as_ref(),
        "1".as_ref(),
        "--record".as_ref(),
        replay.as_os_str(),
        "--trajectory".as_ref(),
        csv.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("committed_steps=120"));
    let recording = JetRecording::read_from(&mut std::fs::File::open(&replay).unwrap()).unwrap();
    assert_eq!(recording.controls().len(), 120);
    assert!(recording.terminal().is_none());
    let trajectory = std::fs::read_to_string(csv).unwrap();
    assert_eq!(trajectory.lines().count(), 121);
    assert!(
        trajectory
            .lines()
            .next()
            .unwrap()
            .contains("minimum_gear_clearance_m")
    );
    let output = run(&["--replay".as_ref(), replay.as_os_str()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("replay verified"));
}
#[test]
fn standalone_domain_exit_is_visible_zero_duration_and_replayable() {
    let scratch = Scratch::new();
    let replay = scratch.0.join("terminal.fsreplay");
    let output = run(&[
        "--scenario".as_ref(),
        "domain-exit".as_ref(),
        "--seconds".as_ref(),
        "1".as_ref(),
        "--record".as_ref(),
        replay.as_os_str(),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("status=rejected committed_steps=0"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("OutsideOperatingEnvelope"));
    let recording = JetRecording::read_from(&mut std::fs::File::open(&replay).unwrap()).unwrap();
    assert!(recording.controls().is_empty());
    assert_eq!(recording.terminal().unwrap().cursor, 0);
    let output = run(&["--replay".as_ref(), replay.as_os_str()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("terminal=Some"));
}
