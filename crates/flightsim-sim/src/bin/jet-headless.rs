//! Standalone authored-jet numerical flight, exact v4 recording and verification.
use clap::{Parser, ValueEnum};
use flightsim_core::{Geodetic, Meters, MetersPerSecond, Seconds};
use flightsim_sim::{
    aircraft_profile::AircraftProfileV2,
    jet_scenarios::{JetTakeoffDriver, solve_jet_trim},
    model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation, jet_parked_state},
    replay_v4::{JetRecorder, JetRecording, JetReplayPlayer},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Scenario {
    Takeoff,
    Trim,
    Approach,
    Throttle,
    DomainExit,
}
#[derive(Debug, Parser)]
#[command(
    about = "Numerical authored jet flight and exact v4 replay; no certified aircraft or rendering"
)]
struct Args {
    #[arg(long)]
    profile: PathBuf,
    #[arg(long, value_enum, default_value = "trim")]
    scenario: Scenario,
    #[arg(long, default_value_t = 30.0)]
    seconds: f64,
    #[arg(long, default_value_t = 1000.0)]
    altitude_m: f64,
    #[arg(long, default_value_t = 50.0)]
    speed_mps: f64,
    #[arg(long)]
    trajectory: Option<PathBuf>,
    #[arg(long)]
    record: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["record", "trajectory"])]
    replay: Option<PathBuf>,
}
fn main() -> std::process::ExitCode {
    match run(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let profile = AircraftProfileV2::load(&args.profile)?;
    let config = profile.configuration().clone();
    if let Some(path) = args.replay {
        let recording = JetRecording::read_from(&mut BufReader::new(File::open(path)?))?;
        let mut player = JetReplayPlayer::new(config, recording)?;
        while !player.finished() {
            player.advance(Seconds(0.25))?;
        }
        println!(
            "replay verified: committed_steps={} elapsed_s={:.12} terminal={:?}",
            player.cursor(),
            player.simulation().elapsed().get(),
            player.simulation().terminal()
        );
        return Ok(());
    }
    if !args.seconds.is_finite()
        || args.seconds < 0.0
        || args.seconds > f64::from(flightsim_sim::replay::MAX_FRAMES) / 120.0
    {
        return Err("seconds must be finite and within recording capacity".into());
    }
    let position = Geodetic::from_degrees(0.0, 0.0, args.altitude_m);
    let trim = solve_jet_trim(
        &config,
        position,
        MetersPerSecond(args.speed_mps),
        MetersPerSecond::ZERO,
        0.0,
    );
    let (state, base) = match args.scenario {
        Scenario::Takeoff => (
            jet_parked_state(
                &config,
                position,
                Meters::ZERO,
                flightsim_core::Radians::ZERO,
            ),
            flightsim_fdm::ControlInputs::neutral(),
        ),
        Scenario::Approach => {
            let result = solve_jet_trim(
                &config,
                Geodetic::from_degrees(0., 0., 60.),
                MetersPerSecond(35.),
                MetersPerSecond(2.),
                0.5,
            )?;
            (result.state, result.controls)
        }
        Scenario::DomainExit => {
            let mut state = trim?.state;
            let speed = flightsim_fdm::Atmosphere::standard()
                .sample(state.altitude())
                .speed_of_sound
                .get()
                * (config.envelope().definition().mach[1] + 0.001);
            state.velocity = state
                .local_frame()
                .ned_to_ecef_vector(flightsim_core::Ned::new(speed, 0., 0.));
            (
                state,
                flightsim_fdm::ControlInputs::neutral().with_throttle(1.0),
            )
        }
        Scenario::Trim | Scenario::Throttle => {
            let result = trim?;
            println!(
                "trim residual={:?} elevator={:.12} throttle={:.12}",
                result.residual,
                result.controls.elevator(),
                result.controls.throttle()
            );
            (result.state, result.controls)
        }
    };
    let mut simulation = JetSimulation::from_state(config, state, JetEnvironment::default())?;
    let mut recorder = JetRecorder::new(&simulation)?;
    let mut driver = JetTakeoffDriver::default();
    let mut output = args
        .trajectory
        .map(File::create)
        .transpose()?
        .map(BufWriter::new);
    if let Some(file) = &mut output {
        writeln!(
            file,
            "elapsed_s,latitude_rad,longitude_rad,altitude_m,airspeed_mps,vertical_speed_mps,pitch_rad,minimum_gear_clearance_m,committed_steps"
        )?;
    }
    let mut step = 0_u32;
    while f64::from(step) * JET_FIXED_DT.get() < args.seconds {
        let report = if matches!(args.scenario, Scenario::Takeoff) {
            simulation.advance_with_controller(JET_FIXED_DT, &mut driver, |driver, dt, state| {
                driver.controls(dt, state)
            })
        } else {
            let controls = if matches!(args.scenario, Scenario::Throttle) {
                base.with_throttle(if simulation.elapsed().get() < args.seconds / 2. {
                    0.0
                } else {
                    1.0
                })
            } else {
                base
            };
            simulation.advance(JET_FIXED_DT, controls)
        };
        recorder.record(&report)?;
        if let Some(file) = &mut output {
            let geo = simulation.state().geodetic();
            writeln!(
                file,
                "{:.12},{:.15},{:.15},{:.9},{:.9},{:.9},{:.12},{:.9},{}",
                simulation.elapsed().get(),
                geo.latitude.get(),
                geo.longitude.get(),
                geo.altitude.get(),
                simulation.airspeed().get(),
                simulation.state().vertical_speed().get(),
                simulation.state().attitude().pitch.get(),
                simulation.minimum_gear_clearance().get(),
                simulation.snapshot().committed_steps
            )?;
        }
        if report.terminal().is_some() {
            break;
        }
        step += 1;
    }
    if let Some(file) = &mut output {
        file.flush()?;
    }
    let recording = recorder.finish();
    if let Some(path) = args.record {
        let mut writer = BufWriter::new(File::create(path)?);
        recording.write_to(&mut writer)?;
        writer.flush()?;
    }
    println!(
        "status={} committed_steps={} elapsed_s={:.12} altitude_m={:.6} airspeed_mps={:.6} landings={}",
        if simulation.terminal().is_some() {
            "rejected"
        } else {
            "complete"
        },
        simulation.snapshot().committed_steps,
        simulation.elapsed().get(),
        simulation.state().altitude().get(),
        simulation.airspeed().get(),
        simulation.log().landings
    );
    if let Some(event) = simulation.terminal() {
        return Err(format!(
            "terminal cursor={} {} query={:?}",
            event.cursor, event.failure, event.failure.query
        )
        .into());
    }
    Ok(())
}
