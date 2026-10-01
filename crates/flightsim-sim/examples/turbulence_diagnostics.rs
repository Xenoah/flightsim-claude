//! Diagnostic probes for turbulence integration and the director's wind limitation.
//! See docs/qa/turbulence-2026-10-01.md for before/after values and regression tests.
//! cargo run -p flightsim-sim --example turbulence_diagnostics

use flightsim_core::{
    Attitude, Geodetic, LocalFrame, Meters, MetersPerSecond, Ned, Radians, Seconds,
};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState, Turbulence};
use flightsim_sim::{
    DirectorTargets, FlightDirector, GroundSampler, Simulation, VerticalTarget, Wind,
};
use flightsim_world::{MemoryTileSource, Terrain};

fn sim() -> Simulation<MemoryTileSource> {
    Simulation::from_state(
        AircraftConfig::light_single(),
        RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 1500.0),
            Attitude::from_degrees(0.0, 2.0, 0.0),
            Ned::new(50.0, 0.0, 0.0),
        ),
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    )
}

fn frame_partition(hz: u32, turbulence: Turbulence) -> RigidBodyState {
    let mut sim = sim();
    sim.set_turbulence(turbulence);
    // Hold identical controls for the entire flight, eliminating controller sampling differences.
    let controls = ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65);
    let mut steps = 0;
    for _ in 0..60 * hz {
        let report = sim.advance(Seconds(1.0 / f64::from(hz)), controls);
        assert!(!report.diverged && !sim.crashed());
        steps += report.steps;
    }
    assert_eq!(steps, 7200);
    *sim.state()
}

fn main() {
    let mut sim = sim();
    sim.set_turbulence(Turbulence::severe(1));
    println!("initial_air_speed_reported_mps={:.9}", sim.airspeed().get());
    println!(
        "initial_air_speed_aerodynamic_mps={:.9}",
        sim.aero_angles().true_airspeed.get()
    );
    // The existing test director explicitly uses ground-relative body speed. These three
    // environments hold the aircraft state fixed to expose that limitation independently.
    sim.set_turbulence(Turbulence::CALM);
    for (name, wind) in [
        ("calm", Wind::CALM),
        (
            "headwind",
            Wind {
                from: Radians::ZERO,
                speed: MetersPerSecond(10.0),
            },
        ),
        (
            "tailwind",
            Wind {
                from: Radians(core::f64::consts::PI),
                speed: MetersPerSecond(10.0),
            },
        ),
    ] {
        sim.set_wind(wind);
        let controls = FlightDirector::default().control(
            sim.state(),
            sim.agl(),
            DirectorTargets {
                vertical: VerticalTarget::AltitudeAgl(Meters(1500.0)),
                heading: Radians::ZERO,
                airspeed: MetersPerSecond(50.0),
                flaps: 0.0,
                brakes: 0.0,
                throttle_override: None,
                wings_level: false,
            },
        );
        println!(
            "director_{name}_tas_mps={:.9}; throttle={:.9}",
            sim.airspeed().get(),
            controls.throttle()
        );
    }
    for (name, turbulence) in [
        ("calm", Turbulence::CALM),
        ("severe", Turbulence::severe(1)),
    ] {
        let reference = frame_partition(120, turbulence);
        for hz in [30, 60] {
            let result = frame_partition(hz, turbulence);
            println!(
                "frame_partition_{name}_{hz}_vs_120_position_m={:.12}",
                (result.position.as_vec() - reference.position.as_vec()).length()
            );
            println!(
                "frame_partition_{name}_{hz}_vs_120_velocity_mps={:.12}",
                (result.velocity - reference.velocity).length()
            );
        }
    }
    let turbulence = Turbulence::severe(1);
    for (label, a, b) in [
        (
            "dateline",
            Geodetic::from_degrees(0.0, 179.999_999, 1000.0),
            Geodetic::from_degrees(0.0, -179.999_999, 1000.0),
        ),
        (
            "north_pole",
            Geodetic::from_degrees(89.999_999, 0.0, 1000.0),
            Geodetic::from_degrees(89.999_999, 180.0, 1000.0),
        ),
    ] {
        let first = LocalFrame::new(a).ned_to_ecef_vector(turbulence.gust_at(Seconds(10.0), a));
        let second = LocalFrame::new(b).ned_to_ecef_vector(turbulence.gust_at(Seconds(10.0), b));
        println!(
            "{label}_separation_m={:.9}",
            (a.to_ecef().as_vec() - b.to_ecef().as_vec()).length()
        );
        println!(
            "{label}_gust_ecef_jump_mps={:.9}",
            (second - first).length()
        );
    }
}
