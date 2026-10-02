//! Reproduce the open-loop longitudinal diagnostic without GUI or autopilot.
//! Redirect stdout to a local CSV; no output files are created by this example.
#[path = "../tests/longitudinal_support/mod.rs"]
mod support;
use flightsim_core::{Attitude, Geodetic, Knots, Meters, MetersPerSecond, Ned, Seconds};
use flightsim_fdm::{
    Atmosphere, ControlInputs, Environment, FlightDynamics, RigidBodyState, aero_angles_of,
};

fn main() {
    if std::env::args().any(|arg| arg == "--release") {
        release_sweep();
        return;
    }
    if std::env::args().any(|arg| arg == "--pulse") {
        pulse_sweep();
        return;
    }
    println!("kind,aircraft,case,t,alt_m,ias_kt,vs_mps,pitch_deg,alpha_deg,elevator,throttle");
    for (config, default_trim) in support::configs().into_iter().zip(support::default_trims()) {
        for throttle in [0.0, 0.0001, 0.1, 0.5, 1.0] {
            eprintln!(
                "STATIC,{},{throttle},{:.3}",
                config.name,
                config.engine.thrust(throttle, 0.0, 1.0).get()
            );
        }
        let atmosphere = Atmosphere::standard();
        for altitude in [0.0, 2000.0, 4000.0, 6000.0] {
            for ias_kt in [60.0, 70.0, 80.0, 90.0, 110.0, 130.0] {
                let speed = Knots(ias_kt).to_meters_per_second().get();
                if let Some(trim) = support::climb(&config, speed, 1.0, altitude, atmosphere) {
                    eprintln!(
                        "CLIMB,{},{altitude},{ias_kt},{:.3},{:.3},{:.5}",
                        config.name,
                        trim.tas * trim.gamma.sin(),
                        (trim.alpha + trim.gamma).to_degrees(),
                        trim.controls.elevator()
                    );
                }
            }
        }
        let level = support::level(
            &config,
            Knots(85.0).to_meters_per_second().get(),
            1000.0,
            atmosphere,
        )
        .unwrap();
        for (case, delta_pitch, scale_speed, elevator, throttle) in [
            (
                "level",
                0.0,
                1.0,
                level.controls.elevator(),
                level.controls.throttle(),
            ),
            (
                "pitch_plus_2",
                2.0,
                1.0,
                level.controls.elevator(),
                level.controls.throttle(),
            ),
            (
                "speed_plus_10pct",
                0.0,
                1.1,
                level.controls.elevator(),
                level.controls.throttle(),
            ),
            (
                "neutral_from_trim",
                0.0,
                1.0,
                0.0,
                level.controls.throttle(),
            ),
        ] {
            let mut state = level.state(1000.0);
            let a = state.attitude();
            state.orientation = RigidBodyState::from_geodetic(
                state.geodetic(),
                Attitude::from_degrees(0.0, a.pitch.to_degrees().get() + delta_pitch, 0.0),
                state.velocity_ned(),
            )
            .orientation;
            state.velocity *= scale_speed;
            trace(&config, case, state, elevator, throttle, 600);
        }
        let trim = default_trim;
        for (case, ias, pitch, gamma, elevator, throttle) in [
            ("climb_release", 80.0, 10.0, 5.0, trim, 1.0),
            ("climb_release_neutral", 80.0, 10.0, 5.0, 0.0, 1.0),
            ("overpull_release", 45.0, 40.0, 20.0, trim, 1.0),
            ("stall_hold", 65.0, 5.0, 0.0, 1.0, 0.5),
        ] {
            let speed = Knots(ias).to_meters_per_second().get()
                / atmosphere.sample(Meters(1000.0)).density_ratio().sqrt();
            let gamma: f64 = gamma;
            let state = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(35.0, 139.0, 1000.0),
                Attitude::from_degrees(0.0, pitch, 0.0),
                Ned::new(
                    speed * gamma.to_radians().cos(),
                    0.0,
                    -speed * gamma.to_radians().sin(),
                ),
            );
            trace(&config, case, state, elevator, throttle, 90);
        }
    }
}
fn trace(
    c: &flightsim_fdm::AircraftConfig,
    case: &str,
    state: RigidBodyState,
    elevator: f64,
    throttle: f64,
    seconds: u32,
) {
    let env = Environment::still_air().with_ground(Meters(-10000.0));
    let mut fdm = FlightDynamics::new(c.clone(), state);
    let controls = ControlInputs::neutral()
        .with_elevator(elevator)
        .with_throttle(throttle);
    let mut min_alt = state.altitude().get();
    let mut max_alt = min_alt;
    let mut peak_alpha: f64 = 0.0;
    for step in 0..=seconds * 120 {
        let s = fdm.state();
        let a = aero_angles_of(s, &env);
        let ias =
            a.true_airspeed.get() * env.atmosphere.sample(s.altitude()).density_ratio().sqrt();
        min_alt = min_alt.min(s.altitude().get());
        max_alt = max_alt.max(s.altitude().get());
        peak_alpha = peak_alpha.max(a.angle_of_attack.to_degrees().get());
        if step % 120 == 0 {
            println!(
                "TRACE,{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.8},{:.8}",
                c.name,
                case,
                f64::from(step) / 120.0,
                s.altitude().get(),
                MetersPerSecond(ias).to_knots().get(),
                s.vertical_speed().get(),
                s.attitude().pitch.to_degrees().get(),
                a.angle_of_attack.to_degrees().get(),
                elevator,
                throttle
            );
        }
        if step < seconds * 120 {
            fdm.step(Seconds(1.0 / 120.0), controls, &env);
        }
    }
    eprintln!(
        "DYNAMIC,{},{case},range_alt={min_alt:.3}..{max_alt:.3},final_alt={:.3},final_vs={:.4},peak_alpha={peak_alpha:.3}",
        c.name,
        fdm.state().altitude().get(),
        fdm.state().vertical_speed().get()
    );
}

fn pulse_sweep() {
    println!("aircraft,rate,pulse_s,peak_alpha_deg,min_ias_kt,alt_loss_m,peak_pitch_deg");
    for c in support::configs() {
        let light = c.name.starts_with("Light");
        let default_rate = if light { 2.5 } else { 2.0 };
        let centering = if light { 1.8 } else { 1.6 };
        let env = Environment::still_air().with_ground(Meters(-10_000.0));
        let trim = support::climb(
            &c,
            Knots(80.0).to_meters_per_second().get(),
            1.0,
            1000.0,
            env.atmosphere,
        )
        .unwrap();
        for rate in [0.8, 1.0, 1.2, default_rate] {
            for pulse_ticks in [12, 30, 60, 120, 360] {
                let mut fdm = FlightDynamics::new(c.clone(), trim.state(1000.0));
                let mut axis = 0.0_f64;
                let mut peak_alpha = 0.0_f64;
                let mut min_ias = f64::INFINITY;
                let mut min_alt = 1000.0_f64;
                let mut peak_pitch = 0.0_f64;
                for tick in 0..7200 {
                    axis = if tick < pulse_ticks {
                        (axis + rate / 120.0).min(1.0)
                    } else {
                        (axis - centering / 120.0).max(0.0)
                    };
                    fdm.step(
                        Seconds(1.0 / 120.0),
                        trim.controls.with_elevator(trim.controls.elevator() + axis),
                        &env,
                    );
                    let s = fdm.state();
                    let angles = aero_angles_of(s, &env);
                    peak_alpha = peak_alpha.max(angles.angle_of_attack.to_degrees().get());
                    min_ias = min_ias.min(
                        MetersPerSecond(
                            angles.true_airspeed.get()
                                * env.atmosphere.sample(s.altitude()).density_ratio().sqrt(),
                        )
                        .to_knots()
                        .get(),
                    );
                    min_alt = min_alt.min(s.altitude().get());
                    peak_pitch = peak_pitch.max(s.attitude().pitch.to_degrees().get());
                }
                println!(
                    "{},{},{},{:.4},{:.4},{:.4},{:.4}",
                    c.name,
                    rate,
                    f64::from(pulse_ticks) / 120.0,
                    peak_alpha,
                    min_ias,
                    1000.0 - min_alt,
                    peak_pitch
                );
            }
        }
    }
}

fn release_sweep() {
    for (c, default_trim) in support::configs().into_iter().zip(support::default_trims()) {
        for kt in [60.0, 65.0, 70.0, 80.0] {
            let env = Environment::still_air();
            let tas = Knots(kt).to_meters_per_second().get()
                / env.atmosphere.sample(Meters(100.0)).density_ratio().sqrt();
            let gamma = 5.0_f64.to_radians();
            let state = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(35.0, 139.0, 100.0),
                Attitude::from_degrees(0.0, 10.0, 0.0),
                Ned::new(tas * gamma.cos(), 0.0, -tas * gamma.sin()),
            );
            trace(
                &c,
                &format!("low_release_{kt}kt"),
                state,
                default_trim,
                1.0,
                90,
            );
        }
    }
}
