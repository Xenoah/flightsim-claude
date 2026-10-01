//! Issue #5 numerical scenario report. This does not measure human handling quality.
//!
//! cargo run -p flightsim-sim --example turbulence_validation
//! cargo run -p flightsim-sim --example turbulence_validation -- --trace-dir /tmp/turbulence-traces

#[path = "support/turbulence_scenarios.rs"]
mod scenarios;

use clap::Parser;
use scenarios::{Envelope, RUN_SECONDS, SEEDS, Scenario, Severity};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Args {
    /// Optional per-physics-step CSV files. Summary CSV always goes to stdout.
    #[arg(long)]
    trace_dir: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    eprintln!(
        "aircraft=light-single fingerprint={:016x}; warmup={}s; fixed_dt=1/{}s; measured={}s; seeds={SEEDS:?}",
        flightsim_sim::replay::aircraft_fingerprint(&flightsim_fdm::AircraftConfig::light_single()),
        scenarios::WARMUP_SECONDS,
        scenarios::SAMPLE_HZ,
        RUN_SECONDS,
    );
    if let Some(directory) = &args.trace_dir {
        std::fs::create_dir_all(directory)?;
    }
    let stdout = io::stdout();
    let mut summary = io::BufWriter::new(stdout.lock());
    writeln!(
        summary,
        "scenario,severity,seed,seconds,roll_peak_deg,pitch_min_deg,pitch_max_deg,heading_peak_deg,accel_peak_mps2,nz_min_g,nz_max_g,nz_deviation_rms_g,aileron_peak,elevator_peak,rudder_peak,throttle_min,throttle_max,min_surface_margin,surface_saturated_samples,tas_min_mps,tas_max_mps,alt_min_m,stall_fraction_peak,gust_n_rms_mps,gust_e_rms_mps,gust_d_rms_mps,airspeed_disagreement_peak_mps,quaternion_error,envelope"
    )?;
    let mut failure_count = 0;
    for scenario in Scenario::ALL {
        let initial = scenarios::settled_state(scenario);
        eprintln!(
            "{} measured initial state: {:?}; targets: {:?}",
            scenario.name(),
            initial,
            scenario.targets()
        );
        for severity in Severity::ALL {
            for seed in SEEDS {
                let mut trace = args
                    .trace_dir
                    .as_ref()
                    .map(|directory| {
                        std::fs::File::create(directory.join(format!(
                            "{}-{}-{seed}.csv",
                            scenario.name(),
                            severity.name()
                        )))
                        .map(io::BufWriter::new)
                    })
                    .transpose()?;
                if let Some(writer) = &mut trace {
                    writeln!(
                        writer,
                        "elapsed_s,roll_deg,pitch_deg,heading_error_deg,tas_mps,reported_airspeed_mps,altitude_m,acceleration_mps2,nz_g,stall_fraction,aileron,elevator,rudder,throttle,gust_n_mps,gust_e_mps,gust_d_mps"
                    )?;
                }
                let mut trace_error = None;
                let metrics = scenarios::run(
                    scenario,
                    severity.turbulence(seed),
                    RUN_SECONDS,
                    |s| {
                        if let Some(writer) = &mut trace {
                            if trace_error.is_none() {
                                trace_error = writeln!(writer, "{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9}",
                                s.elapsed.get(), s.roll_deg, s.pitch_deg, s.heading_error_deg, s.true_airspeed_mps,
                                s.reported_airspeed_mps, s.altitude_m, s.acceleration_mps2, s.normal_load_g,
                                s.stall_fraction, s.controls.aileron(), s.controls.elevator(), s.controls.rudder(),
                                s.controls.throttle(), s.gust.north(), s.gust.east(), s.gust.down()).err();
                            }
                        }
                    },
                );
                if let Some(error) = trace_error {
                    return Err(error.into());
                }
                if let Some(writer) = &mut trace {
                    writer.flush()?;
                }
                let failures = Envelope::for_severity(severity).failures(&metrics);
                failure_count += usize::from(!failures.is_empty());
                let margin = metrics.controls[..3]
                    .iter()
                    .map(|range| 1.0 - range.peak())
                    .fold(1.0_f64, f64::min);
                writeln!(
                    summary,
                    "{},{},{seed},{RUN_SECONDS},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.3e},{}",
                    scenario.name(),
                    severity.name(),
                    metrics.roll_deg.peak(),
                    metrics.pitch_deg.min,
                    metrics.pitch_deg.max,
                    metrics.heading_error_deg.peak(),
                    metrics.acceleration_mps2.max,
                    metrics.normal_load_g.min,
                    metrics.normal_load_g.max,
                    metrics.load_deviation_g.rms(metrics.samples),
                    metrics.controls[0].peak(),
                    metrics.controls[1].peak(),
                    metrics.controls[2].peak(),
                    metrics.controls[3].min,
                    metrics.controls[3].max,
                    margin,
                    metrics.saturation_samples[..3].iter().sum::<u32>(),
                    metrics.true_airspeed_mps.min,
                    metrics.true_airspeed_mps.max,
                    metrics.altitude_m.min,
                    metrics.stall_fraction.max,
                    metrics.gust[0].rms(metrics.samples),
                    metrics.gust[1].rms(metrics.samples),
                    metrics.gust[2].rms(metrics.samples),
                    metrics.airspeed_disagreement_mps.peak(),
                    metrics.quaternion_error,
                    if failures.is_empty() {
                        "PASS".to_owned()
                    } else {
                        failures.join(";")
                    }
                )?;
                summary.flush()?;
            }
        }
    }
    if failure_count > 0 {
        return Err(format!("{failure_count} scenarios exceeded the numerical envelopes").into());
    }
    Ok(())
}
