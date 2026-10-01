//! Build a deterministic, settled, real-DEM cruise fixture for app visual QA.
//!
//! This is a fixture generator, not visual evidence. Run the app with the same
//! tile directory and inspect actual captures throughout the recorded flight.
//! Existing output files are never overwritten.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use flightsim_core::{
    Attitude, Degrees, FloatingOrigin, Geodetic, Meters, MetersPerSecond, Ned, Seconds,
};
use flightsim_fdm::{AircraftConfig, RigidBodyState, Turbulence};
use flightsim_sim::replay::{Conditions, Recording};
use flightsim_sim::{
    DirectorTargets, FlightDirector, GroundSampler, Recorder, Simulation, VerticalTarget, Wind,
};
use flightsim_world::{DiskTileSource, Terrain};

const HZ: u32 = 60;
const DT: Seconds = Seconds(1.0 / 60.0);
const WARMUP_SECONDS: u32 = 120;
const CRUISE_ELLIPSOIDAL_ALTITUDE: Meters = Meters(3035.0);
const CACHE_BYTES: usize = 256 * 1024 * 1024;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Parser)]
#[command(about = "Record a settled westbound cruise over normalized Haneda DEM tiles")]
struct Cli {
    /// Normalized WGS84 ellipsoidal .fsdem directory, levels 8 through 12.
    #[arg(long)]
    tiles: PathBuf,
    /// New .fsreplay file; .csv, .metadata.txt and .terrain.csv are also created.
    #[arg(long)]
    output: PathBuf,
    /// Recorded seconds, excluding the 120-second warmup.
    #[arg(long, default_value_t = 360, value_parser = clap::value_parser!(u32).range(300..=480))]
    duration: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerrainIdentity {
    hash: u64,
    bytes: u64,
    tiles: usize,
    manifest: String,
}

#[derive(Debug, Clone, Copy)]
struct Range {
    min: f64,
    max: f64,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }
}

impl Range {
    fn add(&mut self, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("non-finite telemetry".into());
        }
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        Ok(())
    }

    fn peak(self) -> f64 {
        self.min.abs().max(self.max.abs())
    }
}

#[derive(Debug, Default)]
struct Envelope {
    agl: Range,
    altitude: Range,
    roll: Range,
    pitch: Range,
    heading: Range,
    airspeed: Range,
    vertical_speed: Range,
    roll_rate: Range,
    pitch_rate: Range,
    yaw_rate: Range,
}

impl Envelope {
    fn observe(&mut self, sim: &Simulation<DiskTileSource>) -> Result<(), String> {
        let state = sim.state();
        let attitude = state.attitude();
        self.agl.add(sim.agl().get())?;
        self.altitude.add(state.altitude().get())?;
        self.roll.add(attitude.roll.to_degrees().get())?;
        self.pitch.add(attitude.pitch.to_degrees().get())?;
        self.heading
            .add(attitude.yaw.to_degrees().get().rem_euclid(360.0))?;
        self.airspeed.add(sim.airspeed().get())?;
        self.vertical_speed.add(state.vertical_speed().get())?;
        self.roll_rate.add(state.angular_velocity.x.to_degrees())?;
        self.pitch_rate.add(state.angular_velocity.y.to_degrees())?;
        self.yaw_rate.add(state.angular_velocity.z.to_degrees())?;
        let position = state.geodetic();
        if !(35.15..=35.85).contains(&position.latitude_degrees())
            || !(139.15..=139.95).contains(&position.longitude_degrees())
        {
            return Err("flight left the documented DEM bounds".into());
        }
        if !sim.ground().from_terrain || !sim.terrain().load_failures().is_empty() {
            return Err("DEM fallback or tile read failure".into());
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.agl.min < 2850.0
            || self.agl.max > 3100.0
            || self.altitude.max - self.altitude.min > 1.0
            || self.roll.peak() > 1.0
            || self.pitch.peak() > 5.0
            || self.vertical_speed.peak() > 0.1
            || self.roll_rate.peak() > 0.1
            || self.pitch_rate.peak() > 0.1
            || self.yaw_rate.peak() > 0.1
        {
            return Err(format!("cruise envelope exceeded: {self:#?}"));
        }
        Ok(())
    }
}

fn targets(ground_elevation: Meters) -> DirectorTargets {
    DirectorTargets {
        // Hold an ellipsoidal altitude, rather than pitching to follow every DSM
        // bump three kilometres below. The director accepts an AGL target, so
        // subtract the same sampled ground used by Simulation::agl().
        vertical: VerticalTarget::AltitudeAgl(Meters(
            CRUISE_ELLIPSOIDAL_ALTITUDE.get() - ground_elevation.get(),
        )),
        heading: Degrees(270.0).to_radians(),
        airspeed: MetersPerSecond(50.0),
        flaps: 0.0,
        brakes: 0.0,
        throttle_override: None,
        wings_level: false,
    }
}

fn terrain(path: &Path) -> Terrain<DiskTileSource> {
    Terrain::new(DiskTileSource::new(path), CACHE_BYTES, 8..=12)
}

fn simulation(path: &Path, state: RigidBodyState) -> Simulation<DiskTileSource> {
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        terrain(path),
        GroundSampler::default(),
    );
    sim.set_wind(Wind::CALM);
    sim.set_turbulence(Turbulence::CALM);
    sim
}

fn checked_step(
    sim: &mut Simulation<DiskTileSource>,
    frame: flightsim_sim::replay::Frame,
) -> Result<(), String> {
    let report = sim.advance(frame.frame_time, frame.controls);
    if report.diverged || sim.crashed() || report.terrain_missing {
        return Err(format!(
            "invalid flight at {:.6} s: {report:?}, crash={:?}",
            sim.elapsed().get(),
            sim.crash()
        ));
    }
    Ok(())
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn collect_tiles(root: &Path, directory: &Path, paths: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "symlink in tile directory: {}",
                entry.path().display()
            ));
        }
        if kind.is_dir() {
            collect_tiles(root, &entry.path(), paths)?;
        } else if entry.path().extension().is_some_and(|ext| ext == "fsdem") {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_path_buf();
            let relative = relative
                .to_str()
                .ok_or("non-UTF-8 tile filename")?
                .replace('\\', "/");
            paths.push(relative);
        }
    }
    Ok(())
}

fn terrain_identity(root: &Path) -> Result<TerrainIdentity, String> {
    let mut paths = Vec::new();
    collect_tiles(root, root, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err("tile directory contains no .fsdem files".into());
    }
    let mut identity = TerrainIdentity {
        hash: FNV_OFFSET,
        bytes: 0,
        tiles: paths.len(),
        manifest: "relative_path,bytes,fnv1a64\n".into(),
    };
    for relative in paths {
        let bytes = fs::read(root.join(&relative)).map_err(|e| e.to_string())?;
        let length = u64::try_from(bytes.len()).map_err(|e| e.to_string())?;
        let mut file_hash = FNV_OFFSET;
        hash_bytes(&mut file_hash, &bytes);
        hash_bytes(&mut identity.hash, relative.as_bytes());
        hash_bytes(&mut identity.hash, &[0]);
        hash_bytes(&mut identity.hash, &length.to_le_bytes());
        hash_bytes(&mut identity.hash, &bytes);
        identity.bytes += length;
        writeln!(identity.manifest, "{relative},{length},{file_hash:016x}")
            .map_err(|e| e.to_string())?;
    }
    Ok(identity)
}

fn csv_row(
    csv: &mut String,
    frame: u32,
    sim: &Simulation<DiskTileSource>,
    rebases: u32,
) -> Result<(), String> {
    let state = sim.state();
    let p = state.geodetic();
    let a = state.attitude();
    writeln!(csv, "{frame},{:.9},{:.9},{:.9},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{rebases}",
        f64::from(frame) / f64::from(HZ), p.latitude_degrees(), p.longitude_degrees(),
        state.altitude().get(), sim.agl().get(), a.roll.to_degrees().get(), a.pitch.to_degrees().get(),
        a.yaw.to_degrees().get().rem_euclid(360.0), sim.airspeed().get(), state.vertical_speed().get(),
        state.angular_velocity.x.to_degrees(), state.angular_velocity.y.to_degrees(),
        state.angular_velocity.z.to_degrees(), sim.log().distance.get(),
    ).map_err(|e| e.to_string())
}

fn write_new_bundle(outputs: &[(PathBuf, Vec<u8>)]) -> Result<(), String> {
    let mut created = Vec::new();
    let result = (|| {
        for (path, bytes) in outputs {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
            created.push(path);
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("cannot finish {}: {e}", path.display()))?;
        }
        Ok(())
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn run(cli: &Cli) -> Result<(), String> {
    if cli.output.extension().is_none_or(|ext| ext != "fsreplay") {
        return Err("--output must have the .fsreplay extension".into());
    }
    let csv_path = cli.output.with_extension("csv");
    let metadata_path = cli.output.with_extension("metadata.txt");
    let terrain_path = cli.output.with_extension("terrain.csv");
    for path in [&cli.output, &csv_path, &metadata_path, &terrain_path] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(format!("refusing to overwrite {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot inspect {}: {e}", path.display())),
        }
    }
    let tiles = cli
        .tiles
        .canonicalize()
        .map_err(|e| format!("invalid tile directory: {e}"))?;
    let identity = terrain_identity(&tiles)?;
    let warmup_start = Geodetic::from_degrees(35.55, 139.846, 0.0);
    let mut warmup_terrain = terrain(&tiles);
    let ground = GroundSampler::default().sample(&mut warmup_terrain, warmup_start);
    if !ground.from_terrain {
        return Err("warmup start has no terrain".into());
    }
    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.846, CRUISE_ELLIPSOIDAL_ALTITUDE.get()),
        Attitude::from_degrees(0.0, 2.0, 270.0),
        Ned::new(0.0, -50.0, 0.0),
    );
    let mut warmup = simulation(&tiles, initial);
    let director = FlightDirector::default();
    for _ in 0..WARMUP_SECONDS * HZ {
        let controls = director.control(
            warmup.state(),
            warmup.agl(),
            targets(warmup.ground().elevation),
        );
        checked_step(
            &mut warmup,
            flightsim_sim::replay::Frame {
                frame_time: DT,
                controls,
            },
        )?;
    }
    let frame_zero = *warmup.state();
    // Reconstruct: no warmup time, accumulator, contact state or flight history
    // is carried into the replay. This is the same path used for verification.
    let mut sim = simulation(&tiles, frame_zero);
    let config = AircraftConfig::light_single();
    let mut recorder = Recorder::new(
        Conditions {
            start: frame_zero.geodetic(),
            heading: frame_zero.attitude().yaw,
            wind: Wind::CALM,
            turbulence: Turbulence::CALM,
            start_epoch: 0.0,
            time_rate: 1.0,
            ..Conditions::default()
        }
        .with_aircraft(&config),
    );
    let mut csv = String::from(
        "frame,recorded_seconds,latitude_deg,longitude_deg,ellipsoidal_altitude_m,agl_m,roll_deg,pitch_deg,heading_deg,tas_mps,vertical_speed_mps,roll_rate_deg_s,pitch_rate_deg_s,yaw_rate_deg_s,distance_m,aircraft_anchor_rebases\n",
    );
    let mut envelope = Envelope::default();
    let mut origin = FloatingOrigin::new(frame_zero.position);
    let mut rebases = 0;
    let mut rebase_times = Vec::new();
    let frames = cli.duration * HZ;
    for frame in 0..=frames {
        envelope.observe(&sim)?;
        if frame % HZ == 0 {
            csv_row(&mut csv, frame, &sim, rebases)?;
        }
        if frame == frames {
            break;
        }
        let controls = director.control(sim.state(), sim.agl(), targets(sim.ground().elevation));
        recorder.record(DT, controls, Some(sim.state()));
        checked_step(
            &mut sim,
            flightsim_sim::replay::Frame {
                frame_time: DT,
                controls,
            },
        )?;
        if origin.rebase_if_needed(sim.state().position).is_some() {
            rebases += 1;
            rebase_times.push(f64::from(frame + 1) / f64::from(HZ));
        }
    }
    envelope.validate()?;
    if !(10_000.0..=25_000.0).contains(&sim.log().distance.get()) || rebases < 2 {
        return Err(format!(
            "inadequate traversal: distance={} m, rebases={rebases}",
            sim.log().distance.get()
        ));
    }
    let recording = recorder.finish();
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).map_err(|e| e.to_string())?;
    let restored = Recording::read_from(&mut bytes.as_slice()).map_err(|e| e.to_string())?;
    if restored != recording {
        return Err("serialized roundtrip changed the recording".into());
    }
    restored
        .check_reproducible_with(&config)
        .map_err(|e| e.to_string())?;
    let zero = restored
        .keyframe_exactly_at(0)
        .ok_or("missing frame-zero keyframe")?;
    let mut replay = simulation(&tiles, zero.state);
    for (index, frame) in restored.frames().iter().enumerate() {
        let index = u32::try_from(index).map_err(|e| e.to_string())?;
        if let Some(key) = restored.keyframe_exactly_at(index)
            && key.state != *replay.state()
        {
            return Err(format!("exact keyframe state mismatch at {index}"));
        }
        checked_step(&mut replay, *frame)?;
    }
    if replay.state() != sim.state()
        || replay.elapsed() != sim.elapsed()
        || replay.log() != sim.log()
        || replay.ground() != sim.ground()
        || replay.interpolated() != sim.interpolated()
    {
        return Err("exact final state/time/history/ground/interpolation replay mismatch".into());
    }
    if terrain_identity(&tiles)? != identity {
        return Err("terrain bytes changed during recording/replay".into());
    }
    let start = frame_zero.geodetic();
    let end = sim.state().geodetic();
    let mut metadata = format!(
        "fixture=settled-westbound-Haneda-cruise\n\
         target_ellipsoidal_altitude_m=3035\n\
         generator=flightsim-sim/examples/record_visual_flight.rs\n\
         aircraft={}\naircraft_fingerprint={:016x}\n\
         terrain_root={}\nterrain_levels=8..=12\n\
         terrain_bounds_lon_lat=139.15,35.15,139.95,35.85\n\
         terrain_tiles={}\nterrain_bytes={}\nterrain_inventory_fnv1a64={:016x}\n\
         terrain_identity_algorithm=FNV-1a-64 over sorted UTF-8 relative path, NUL, u64 little-endian byte length, exact file bytes; noncryptographic\n\
         terrain_identity_verified_before_and_after=true\n\
         warmup_seconds={WARMUP_SECONDS}\nwarmup_reset=fresh Simulation::from_state\n\
         recorded_seconds={}\nframes={}\nframe_dt_seconds={:.17}\nkeyframes={}\n\
         start_epoch=0 (sentinel; launch app with --time 09:00)\ntime_rate=1\nwind=calm\nturbulence=calm\n\
         start_lat_lon={:.9},{:.9}\nend_lat_lon={:.9},{:.9}\n\
         start_altitude_agl_m={:.6}\nend_altitude_agl_m={:.6}\ndistance_m={:.6}\n\
         aircraft_anchor_rebases={rebases}\naircraft_anchor_rebase_seconds={rebase_times:?}\n\
         rebase_scope=core FloatingOrigin at aircraft position; actual camera/render rebases require app QA\n\
         serialized_roundtrip_exact=true\nall_keyframes_exact=true\n\
         final_state_elapsed_log_ground_interpolation_exact=true\n\
         missing_terrain_frames=0\ncrashed=false\ndiverged=false\n\
         visual_validation=NOT established by this generator; inspect actual app captures\n",
        config.name,
        restored.conditions().aircraft_fingerprint,
        tiles.display(),
        identity.tiles,
        identity.bytes,
        identity.hash,
        cli.duration,
        frames,
        DT.get(),
        restored.keyframes().len(),
        start.latitude_degrees(),
        start.longitude_degrees(),
        end.latitude_degrees(),
        end.longitude_degrees(),
        simulation(&tiles, frame_zero).agl().get(),
        sim.agl().get(),
        sim.log().distance.get(),
    );
    for (name, range) in [
        ("agl_m", envelope.agl),
        ("ellipsoidal_altitude_m", envelope.altitude),
        ("roll_deg", envelope.roll),
        ("pitch_deg", envelope.pitch),
        ("heading_deg", envelope.heading),
        ("tas_mps", envelope.airspeed),
        ("vertical_speed_mps", envelope.vertical_speed),
        ("roll_rate_deg_s", envelope.roll_rate),
        ("pitch_rate_deg_s", envelope.pitch_rate),
        ("yaw_rate_deg_s", envelope.yaw_rate),
    ] {
        writeln!(metadata, "{name}_min_max={:.9},{:.9}", range.min, range.max)
            .map_err(|e| e.to_string())?;
    }
    // Publish the replay last; all numeric checks run before any output is created.
    write_new_bundle(&[
        (csv_path, csv.into_bytes()),
        (terrain_path, identity.manifest.into_bytes()),
        (metadata_path, metadata.as_bytes().to_vec()),
        (cli.output.clone(), bytes),
    ])?;
    // Check the actual file on disk, not only the in-memory serialization.
    let file = File::open(&cli.output).map_err(|e| e.to_string())?;
    let written =
        Recording::read_from(&mut std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    if written != restored {
        return Err("on-disk replay differs from the verified bytes".into());
    }
    println!("wrote {}\n{metadata}", cli.output.display());
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flightsim-visual-fixture-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn the_cruise_target_does_not_follow_ground_bumps() {
        for ground in [Meters(-100.0), Meters(35.0), Meters(180.0)] {
            let VerticalTarget::AltitudeAgl(target) = targets(ground).vertical else {
                panic!("expected an AGL target");
            };
            assert!((target.get() + ground.get() - 3035.0).abs() < 1.0e-9);
        }
    }

    #[test]
    fn telemetry_rejects_nonfinite_samples() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Range::default().add(value).is_err());
        }
        let mut range = Range::default();
        range.add(-2.0).unwrap();
        range.add(1.0).unwrap();
        assert_eq!(range.peak().to_bits(), 2.0_f64.to_bits());
    }

    #[test]
    fn an_existing_bundle_member_is_never_overwritten_and_new_files_roll_back() {
        let scratch = Scratch::new();
        let first = scratch.0.join("new.csv");
        let occupied = scratch.0.join("existing.fsreplay");
        fs::write(&occupied, b"valuable recording").unwrap();
        assert!(
            write_new_bundle(&[
                (first.clone(), b"new telemetry".to_vec()),
                (occupied.clone(), b"replacement".to_vec()),
            ])
            .is_err()
        );
        assert!(!first.exists());
        assert_eq!(fs::read(occupied).unwrap(), b"valuable recording");
    }

    #[test]
    fn terrain_identity_is_sorted_and_sensitive_to_names_and_bytes() {
        let scratch = Scratch::new();
        fs::create_dir(scratch.0.join("12")).unwrap();
        fs::write(scratch.0.join("12/b.fsdem"), b"bbb").unwrap();
        fs::write(scratch.0.join("12/a.fsdem"), b"aa").unwrap();
        fs::write(scratch.0.join("unrelated.txt"), b"ignored").unwrap();
        let before = terrain_identity(&scratch.0).unwrap();
        assert_eq!(before.tiles, 2);
        assert_eq!(before.bytes, 5);
        assert!(
            before
                .manifest
                .lines()
                .nth(1)
                .unwrap()
                .starts_with("12/a.fsdem,")
        );
        assert_eq!(before, terrain_identity(&scratch.0).unwrap());
        fs::write(scratch.0.join("12/a.fsdem"), b"ac").unwrap();
        assert_ne!(before.hash, terrain_identity(&scratch.0).unwrap().hash);
        fs::write(scratch.0.join("12/a.fsdem"), b"aa").unwrap();
        fs::rename(scratch.0.join("12/a.fsdem"), scratch.0.join("12/c.fsdem")).unwrap();
        assert_ne!(before.hash, terrain_identity(&scratch.0).unwrap().hash);
    }

    #[test]
    fn empty_terrain_does_not_produce_a_fixture() {
        let scratch = Scratch::new();
        assert!(terrain_identity(&scratch.0).is_err());
    }
}
