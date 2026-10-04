//! Original Cedar qualification helpers, deliberately outside production APIs.
//! Trim searches evaluate candidate states only. No time advance or controller.
use flightsim_core::{
    Attitude, Ecef, Geodetic, LocalFrame, Meters, Ned, Radians, RadiansPerSecond, Seconds,
};
use flightsim_fdm::turboprop::{
    AdvanceRatio, PowerConditions, PressureRatio, PropellerQuery, TemperatureRatio,
    TurbineFraction, TurbopropAircraftConfig, TurbopropFlightDynamics, TurbopropState,
};
use flightsim_fdm::{Atmosphere, ControlInputs, Environment, GroundSlope, RigidBodyState};
use flightsim_sim::aircraft_profile_v3::AircraftProfileV3;
use glam::DVec3;
use serde_json::{Value, json};

pub const HZ: u32 = 120;
pub const PROFILE: &str = include_str!("../../tests/fixtures/cedar-law1-profile.json");

pub fn profile() -> AircraftProfileV3 {
    AircraftProfileV3::parse(PROFILE).expect("authored Cedar profile")
}

pub fn state(
    altitude: f64,
    speed: f64,
    pitch: f64,
    bank: f64,
    gamma: f64,
    engine: [f64; 3],
) -> TurbopropState {
    let [x, omega, beta] = engine;
    TurbopropState {
        rigid_body: RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.0, 139.0, altitude),
            Attitude::new(Radians(bank), Radians(pitch), Radians::ZERO),
            Ned::new(speed * gamma.cos(), 0.0, -speed * gamma.sin()),
        ),
        turbine_fraction: TurbineFraction::new(x).unwrap(),
        shaft_rad_s: RadiansPerSecond(omega),
        blade_pitch_rad: Radians(beta),
    }
}

pub fn start(altitude: f64, speed: f64) -> TurbopropState {
    let p = profile();
    let s = p.running_start();
    state(
        altitude,
        speed,
        0.0,
        0.0,
        0.0,
        [
            s.turbine_fraction().get(),
            s.shaft_speed().get(),
            s.blade_pitch().get(),
        ],
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Trim {
    pub state: TurbopropState,
    pub controls: ControlInputs,
    pub residual: [f64; 7],
    pub iterations: usize,
}

fn candidate(
    v: [f64; 7],
    altitude: f64,
    speed: f64,
    gamma: f64,
    flaps: f64,
) -> Option<(TurbopropState, ControlInputs)> {
    let bounds = [
        (-0.35, 0.35),
        (-0.10, 0.40),
        (-0.8, 0.8),
        (-0.8, 0.8),
        (-0.8, 0.8),
        (0.0, 1.0),
        (0.07, 0.72),
    ];
    if v.iter()
        .zip(bounds)
        .any(|(x, (lo, hi))| !x.is_finite() || *x < lo || *x > hi)
    {
        return None;
    }
    Some((
        state(altitude, speed, v[1], v[0], gamma, [v[5], 180.0, v[6]]),
        ControlInputs::new(v[2], v[3], v[4], v[5], flaps),
    ))
}

fn residual(
    config: &TurbopropAircraftConfig,
    s: TurbopropState,
    c: ControlInputs,
    env: &Environment,
) -> Option<[f64; 7]> {
    let model = TurbopropFlightDynamics::new(config.clone(), s).ok()?;
    let d = model.derivative(&s, c, env).ok()?;
    let a = s
        .rigid_body
        .local_frame()
        .ecef_to_ned_vector(d.rigid_body.acceleration)
        .0;
    let w = d.rigid_body.angular_acceleration;
    Some([a.x, a.y, a.z, w.x, w.y, w.z, d.shaft_acceleration.get()])
}

fn norm(r: [f64; 7]) -> f64 {
    r.iter().map(|v| v * v).sum::<f64>().sqrt()
}

fn linear(mut a: [[f64; 8]; 7]) -> Option<[f64; 7]> {
    for k in 0..7 {
        let pivot = (k..7).max_by(|i, j| a[*i][k].abs().total_cmp(&a[*j][k].abs()))?;
        a.swap(k, pivot);
        if a[k][k].abs() < 1e-12 {
            return None;
        }
        let d = a[k][k];
        for x in &mut a[k][k..] {
            *x /= d;
        }
        for i in 0..7 {
            if i == k {
                continue;
            }
            let f = a[i][k];
            let pivot_row = a[k];
            for (x, y) in a[i][k..].iter_mut().zip(&pivot_row[k..]) {
                *x -= f * y;
            }
        }
    }
    Some(std::array::from_fn(|i| a[i][7]))
}

/// At most 30 Newton iterations, 7 forward differences and 12 line-search
/// evaluations each. Complete valid candidate or honest failure; no clamp,
/// warmup, live simulator, attitude stabilizer or hidden control is involved.
pub fn trim(
    config: &TurbopropAircraftConfig,
    altitude: f64,
    speed: f64,
    gamma: f64,
    flaps: f64,
    env: &Environment,
) -> Result<Trim, String> {
    let mut v = [0.0, 0.08 + gamma, 0.025, 0.10, 0.002, 0.4, 0.40];
    for iteration in 0..30 {
        let (s, c) = candidate(v, altitude, speed, gamma, flaps).ok_or("candidate bounds")?;
        let r = residual(config, s, c, env).ok_or("unsupported trim candidate")?;
        if norm(r) < 1e-8 {
            return Ok(Trim {
                state: s,
                controls: c,
                residual: r,
                iterations: iteration,
            });
        }
        let mut a = [[0.0; 8]; 7];
        for j in 0..7 {
            let mut next = v;
            next[j] += 1e-5;
            let (ss, cc) =
                candidate(next, altitude, speed, gamma, flaps).ok_or("finite-difference bounds")?;
            let rr = residual(config, ss, cc, env).ok_or("unsupported finite difference")?;
            for i in 0..7 {
                a[i][j] = (rr[i] - r[i]) / 1e-5;
            }
        }
        for i in 0..7 {
            a[i][7] = -r[i];
        }
        let delta = linear(a).ok_or("singular trim Jacobian")?;
        let mut accepted = false;
        for backtrack in 0_i32..12 {
            let f = 0.5_f64.powi(backtrack);
            let next = std::array::from_fn(|i| v[i] + f * delta[i]);
            if let Some((ss, cc)) = candidate(next, altitude, speed, gamma, flaps)
                && let Some(rr) = residual(config, ss, cc, env)
                && norm(rr) < norm(r)
            {
                v = next;
                accepted = true;
                break;
            }
        }
        if !accepted {
            return Err(format!("bounded line search failed, residual {}", norm(r)));
        }
    }
    Err("30-iteration trim limit".to_owned())
}

pub fn controls_json(c: ControlInputs) -> Value {
    json!({"aileron":c.aileron(),"elevator":c.elevator(),"rudder":c.rudder(),"throttle":c.throttle(),"flaps":c.flaps(),"brakes":c.brakes()})
}

pub fn state_json(s: &TurbopropState) -> Value {
    let b = &s.rigid_body;
    let a = b.attitude();
    let v = b.velocity_ned();
    json!({"canonical_state_bits_hex":state_bits(s).map(|v|format!("{v:016x}")),
        "position_ecef_m":b.position.0.to_array(),"velocity_ecef_mps":b.velocity.to_array(),
        "orientation_xyzw":b.orientation.to_array(),"angular_velocity_body_rad_s":b.angular_velocity.to_array(),
        "turbine_fraction":s.turbine_fraction.get(),"shaft_rad_s":s.shaft_rad_s.get(),"blade_pitch_rad":s.blade_pitch_rad.get(),
        "altitude_m":b.altitude().get(),"speed_mps":b.velocity.length(),"vertical_speed_mps":v.up(),
        "bank_rad":a.roll.get(),"pitch_rad":a.pitch.get(),"heading_rad":a.yaw.get()})
}

/// Explicit fixture order: ECEF position xyz, velocity xyz, quaternion xyzw,
/// body rates pqr, turbine fraction, relative shaft rad/s, blade pitch rad.
pub fn state_bits(s: &TurbopropState) -> [u64; 16] {
    let b = &s.rigid_body;
    [
        b.position.0.x,
        b.position.0.y,
        b.position.0.z,
        b.velocity.x,
        b.velocity.y,
        b.velocity.z,
        b.orientation.x,
        b.orientation.y,
        b.orientation.z,
        b.orientation.w,
        b.angular_velocity.x,
        b.angular_velocity.y,
        b.angular_velocity.z,
        s.turbine_fraction.get(),
        s.shaft_rad_s.get(),
        s.blade_pitch_rad.get(),
    ]
    .map(f64::to_bits)
}

/// Minimum signed distance of the ENTIRE radius-1.2 m disk at body (3.52,0,0)
/// to the local ground tangent plane, not a tyre/contact-point proxy. Also checks
/// 360 rim points against the same ellipsoidal-height/slope ground convention
/// used by contact, and returns the smaller value. Planar sag/pitch/roll works
/// at any blade azimuth; curved Earth difference is bounded by measured samples.
pub fn disk_clearance(s: &TurbopropState, env: &Environment) -> f64 {
    let b = &s.rigid_body;
    let reference = env.ground_reference().unwrap_or_else(|| b.geodetic());
    let frame = LocalFrame::new(reference);
    let slope = env.ground_slope();
    let normal_ned = DVec3::new(-slope.north(), -slope.east(), -1.0).normalize();
    let normal = frame.ned_to_ecef_vector(Ned(normal_ned));
    let origin = Geodetic::new(
        reference.latitude,
        reference.longitude,
        env.ground_elevation,
    )
    .to_ecef()
    .0;
    let center = b.position.0 + b.orientation * DVec3::new(3.52, 0.0, 0.0);
    let normal_body = b.orientation.inverse() * normal;
    let analytic = (center - origin).dot(normal) - 1.2 * normal_body.y.hypot(normal_body.z);
    let scale = (1.0 + slope.north().powi(2) + slope.east().powi(2)).sqrt();
    let mut sampled = f64::INFINITY;
    for i in 0..360 {
        let t = f64::from(i) * std::f64::consts::TAU / 360.0;
        let point = center + b.orientation * DVec3::new(0.0, 1.2 * t.cos(), 1.2 * t.sin());
        let delta = frame.ecef_to_ned_vector(point - reference.to_ecef().0).0;
        let terrain = env.ground_elevation.get() + slope.north() * delta.x + slope.east() * delta.y;
        sampled = sampled.min((Ecef(point).to_geodetic().altitude.get() - terrain) / scale);
    }
    analytic.min(sampled)
}

pub fn run(
    config: &TurbopropAircraftConfig,
    initial: TurbopropState,
    env: &Environment,
    seconds: u32,
    control: impl Fn(f64, &TurbopropState) -> ControlInputs,
) -> Value {
    run_at_hz(config, initial, env, seconds, HZ, control)
}

pub fn run_at_hz(
    config: &TurbopropAircraftConfig,
    initial: TurbopropState,
    env: &Environment,
    seconds: u32,
    hz: u32,
    control: impl Fn(f64, &TurbopropState) -> ControlInputs,
) -> Value {
    assert!((120..=960).contains(&hz) && seconds <= 300);
    let dt = Seconds(1.0 / f64::from(hz));
    let mut model = TurbopropFlightDynamics::new(config.clone(), initial).unwrap();
    let mut minimum_clearance = disk_clearance(&initial, env);
    let mut minimum_blade_clearance =
        minimum_clearance - static_blade_axial_projection(&initial, env);
    let mut minimum_altitude = initial.rigid_body.altitude().get();
    let mut minimum_gear_clearance = 0.0_f64;
    let mut min_w = initial.shaft_rad_s.get();
    let mut max_w = min_w;
    let mut max_bank = initial.rigid_body.attitude().roll.get().abs();
    let mut max_substeps = 0;
    let mut samples = Vec::new();
    let mut completed = 0_u32;
    let mut failure = None;
    let mut failure_fixture = None;
    let mut min_alpha = f64::INFINITY;
    let mut max_alpha = f64::NEG_INFINITY;
    let tail_start = seconds.saturating_sub(10) * hz;
    let mut tail_peak_speed = 0.0_f64;
    let mut tail_distance = 0.0_f64;
    for tick in 0..seconds * hz {
        let time = f64::from(tick) / f64::from(hz);
        let c = control(time, model.state());
        let before = *model.state();
        match model.step(dt, c, env) {
            Ok(report) => {
                max_substeps = max_substeps.max(report.substeps);
                completed += 1;
            }
            Err(error) => {
                failure = Some(format!("{error:?}"));
                let values = error.diagnostics.values();
                let rejected_axial =
                    values
                        .advance_ratio
                        .zip(values.absolute_spin)
                        .map(|(j, w)| {
                            j.0 * w.get() / std::f64::consts::TAU
                                * config.propeller().diameter().get()
                        });
                failure_fixture = Some(json!({
                    "last_committed_time_s":time,"attempted_dt_s":dt.get(),
                    "last_committed_state":state_json(&before),"effective_controls":controls_json(c),
                    "wind_ecef_mps":env.wind_ecef.to_array(),"atmosphere_temperature_offset_k":env.atmosphere.temperature_offset(),
                    "ground_elevation_m":env.ground_elevation.get(),"ground_slope":[env.ground_slope().north(),env.ground_slope().east()],
                    "substep":error.substep,"stage":format!("{:?}",error.stage),"reason":format!("{:?}",error.reason),
                    "rejected_stage_axial_velocity_mps":rejected_axial,"rejected_stage_advance_ratio":values.advance_ratio.map(|j|j.0),
                    "rejected_stage_relative_shaft_rad_s":values.relative_shaft.map(|w|w.get()),
                    "rejected_stage_absolute_spin_rad_s":values.absolute_spin.map(|w|w.get()),
                    "rejected_stage_relative_rpm":values.relative_shaft.map(|w|w.get()*60.0/std::f64::consts::TAU),
                    "committed_body_air_velocity_mps":(before.rigid_body.orientation.inverse()*(before.rigid_body.velocity-env.wind_ecef)).to_array(),
                    "committed_gear_clearances_m":model.gear_clearances(&before,env).unwrap().map(|v|v.get()),
                    "whole_state_bits_unchanged":state_bits(model.state())==state_bits(&before),
                    "stage_attitude_note":"The error API exposes stage flow/RPM but not its intermediate attitude. The complete last committed attitude and contact state are preserved; no intermediate state is fabricated."
                }));
                break;
            }
        }
        let s = model.state();
        if completed > tail_start {
            tail_peak_speed = tail_peak_speed.max(s.rigid_body.velocity.length());
            tail_distance += s.rigid_body.velocity.length() * dt.get();
        }
        let disk = disk_clearance(s, env);
        minimum_clearance = minimum_clearance.min(disk);
        minimum_blade_clearance =
            minimum_blade_clearance.min(disk - static_blade_axial_projection(s, env));
        minimum_altitude = minimum_altitude.min(s.rigid_body.altitude().get());
        for c in model.gear_clearances(s, env).unwrap() {
            minimum_gear_clearance = minimum_gear_clearance.min(c.get());
        }
        min_w = min_w.min(s.shaft_rad_s.get());
        max_w = max_w.max(s.shaft_rad_s.get());
        max_bank = max_bank.max(s.rigid_body.attitude().roll.get().abs());
        let alpha = flightsim_fdm::aero_angles_of(&s.rigid_body, env)
            .angle_of_attack
            .get();
        if flightsim_fdm::aero_angles_of(&s.rigid_body, env)
            .true_airspeed
            .get()
            > 10.0
        {
            min_alpha = min_alpha.min(alpha);
            max_alpha = max_alpha.max(alpha);
        }
        if completed % hz == 0 {
            samples.push(json!({"time_s":f64::from(completed)/f64::from(hz),"state":sample_json(s,alpha),"controls":controls_json(c)}));
        }
    }
    json!({"completed_s":f64::from(completed)/f64::from(hz),"failure":failure,"failure_fixture":failure_fixture,
        "initial":state_json(&initial),"final":state_json(model.state()),"minimum_disk_clearance_m":minimum_clearance,
        "shaft_range_rad_s":[min_w,max_w],"maximum_abs_bank_rad":max_bank,"maximum_substeps":max_substeps,
        "alpha_range_above_10mps_rad":[if min_alpha.is_finite(){Some(min_alpha)}else{None},if max_alpha.is_finite(){Some(max_alpha)}else{None}],
        "minimum_static_blade_swept_clearance_m":minimum_blade_clearance, "minimum_altitude_m":minimum_altitude,"maximum_gear_compression_m":-minimum_gear_clearance,
        "final_10s_observed_s":f64::from(completed.saturating_sub(tail_start))/f64::from(hz),
        "final_10s_peak_ground_speed_mps":tail_peak_speed,"final_10s_traveled_distance_m":tail_distance,"samples":samples})
}

// 0.055 m encloses the exact accepted static blade mesh axial half-extent
// 0.054359694 m. It does not claim a future variable-pitch/coning mesh envelope.
fn static_blade_axial_projection(s: &TurbopropState, env: &Environment) -> f64 {
    let frame = LocalFrame::new(
        env.ground_reference()
            .unwrap_or_else(|| s.rigid_body.geodetic()),
    );
    let slope = env.ground_slope();
    let n = frame.ned_to_ecef_vector(Ned(
        DVec3::new(-slope.north(), -slope.east(), -1.0).normalize()
    ));
    0.055 * (s.rigid_body.orientation * DVec3::X).dot(n).abs()
}

fn sample_json(s: &TurbopropState, alpha: f64) -> Value {
    let b = &s.rigid_body;
    let a = b.attitude();
    json!({"altitude_m":b.altitude().get(),"speed_mps":b.velocity.length(),"vertical_speed_mps":b.vertical_speed().get(),
        "bank_rad":a.roll.get(),"pitch_rad":a.pitch.get(),"heading_rad":a.yaw.get(),"alpha_rad":alpha,
        "turbine_fraction":s.turbine_fraction.get(),"shaft_rad_s":s.shaft_rad_s.get(),"blade_pitch_rad":s.blade_pitch_rad.get()})
}

pub fn qualify() -> Value {
    let profile = profile();
    let config = profile.configuration();
    let env = Environment::still_air();
    let identity = flightsim_sim::model_identity::ModelIdentity::for_turboprop(config);
    let mut operating_queries = Vec::new();
    for altitude in [0.0, 1500.0, 3000.0] {
        let air = Atmosphere::standard().sample(Meters(altitude));
        for speed in [0.0, 20.0, 40.0, 60.0] {
            for fraction in [0.0, 0.5, 1.0] {
                let power = config
                    .turbine()
                    .sample(
                        PowerConditions {
                            pressure_ratio: PressureRatio(air.pressure.get() / 101325.0),
                            temperature_ratio: TemperatureRatio(air.temperature.get() / 288.15),
                        },
                        TurbineFraction::new(fraction).unwrap(),
                        RadiansPerSecond(180.0),
                    )
                    .unwrap();
                let j = speed / (180.0 / std::f64::consts::TAU * 2.4);
                let load = |beta| {
                    config
                        .propeller()
                        .sample(
                            PropellerQuery {
                                advance_ratio: AdvanceRatio(j),
                                blade_pitch: Radians(beta),
                            },
                            air.density,
                            RadiansPerSecond(180.0),
                        )
                        .unwrap()
                };
                let mut low = 0.07;
                let mut high = 0.72;
                if load(low).absorbed_power.get() > power.delivered.get()
                    || load(high).absorbed_power.get() < power.delivered.get()
                {
                    operating_queries.push(json!({"altitude_m":altitude,"speed_mps":speed,"turbine_fraction":fraction,"shaft_rad_s":180.0,
                        "reference_power_balance_found":false,"reason":"shaft power outside pitch-stop load range; no reference-speed torque balance"}));
                    continue;
                }
                for _ in 0..50 {
                    let middle = (low + high) / 2.0;
                    if load(middle).absorbed_power.get() > power.delivered.get() {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                let beta = (low + high) / 2.0;
                let prop = load(beta);
                let s = state(altitude, speed, 0.0, 0.0, 0.0, [fraction, 180.0, beta]);
                let mut model = TurbopropFlightDynamics::new(config.clone(), s).unwrap();
                model
                    .step(
                        Seconds::ZERO,
                        ControlInputs::neutral(),
                        &env.with_ground(Meters(-100.0)),
                    )
                    .unwrap();
                operating_queries.push(json!({"altitude_m":altitude,"speed_mps":speed,"turbine_fraction":fraction,"shaft_rad_s":180.0,
                    "reference_power_balance_found":true,"blade_pitch_rad":beta,"advance_ratio":j,"thrust_n":prop.thrust.get(),
                    "absorbed_power_w":prop.absorbed_power.get(),"available_power_w":power.available.get(),"delivered_power_w":power.delivered.get(),
                    "drive_torque_nm":power.drive_torque.get(),"load_torque_nm":prop.load_torque.get(),
                    "cp":prop.coefficients.cp,"ct":prop.coefficients.ct,"actual_zero_duration_support":true,"query_scope":"component power balance at zero body rates; not full-aircraft free-flight equilibrium"}));
            }
        }
    }
    let mut trims = Vec::new();
    for (speed, gamma, flaps) in [
        (38.0, 0.0, 0.5),
        (40.0, 0.0, 0.0),
        (50.0, 0.0, 0.0),
        (60.0, 0.0, 0.0),
        (45.0, 0.06, 0.5),
    ] {
        match trim(config, 500.0, speed, gamma, flaps, &env) {
            Ok(t) => {
                let flight = run(config, t.state, &env, 60, |_, _| t.controls);
                let release = run(config, t.state, &env, 10, |_, _| {
                    t.controls.with_aileron(0.0).with_rudder(0.0)
                });
                trims.push(json!({"speed_mps":speed,"gamma_rad":gamma,"flaps":flaps,
                    "state":state_json(&t.state),"controls":controls_json(t.controls),
                    "residual":t.residual,"iterations":t.iterations,"held_controls_60s":flight,"roll_rudder_release_10s":release}));
            }
            Err(e) => {
                trims.push(json!({"speed_mps":speed,"gamma_rad":gamma,"flaps":flaps,"failure":e}))
            }
        }
    }
    let ground = start(1.60, 0.0);
    let ground_idle = run(config, ground, &env, 20, |_, _| {
        ControlInputs::neutral().with_brakes(1.0)
    });
    let headwind = Environment::with_wind_ned(
        Atmosphere::standard(),
        Geodetic::from_degrees(35.0, 139.0, 1.6),
        Ned::new(-5.0, 2.0, 0.0),
    );
    let ground_wind = run(config, ground, &headwind, 20, |_, _| {
        ControlInputs::neutral().with_brakes(1.0)
    });
    let takeoff = run(config, ground, &env, 40, |time, _| {
        if time < 17.0 {
            ControlInputs::new(0.055, if time > 12.0 { 0.15 } else { 0.0 }, 0.004, 1.0, 0.5)
        } else {
            ControlInputs::new(0.035, 0.043, 0.0026, 0.70, 0.5)
        }
    });
    let ground_power_cycle = run(config, ground, &env, 70, |time, _| {
        ControlInputs::neutral()
            .with_brakes(1.0)
            .with_throttle(if time < 30.0 { 1.0 } else { 0.0 })
    });
    let braking = run(config, start(1.6, 25.0), &env, 10, |_, _| {
        ControlInputs::neutral().with_brakes(1.0)
    });
    let braking_headwind = run(config, start(1.6, 25.0), &headwind, 10, |_, _| {
        ControlInputs::neutral().with_brakes(1.0)
    });
    let braking_refinement:Vec<_>=[240,480,960].into_iter().map(|hz|json!({"hz":hz,
        "result":run_at_hz(config,start(1.6,25.0),&env,10,hz,|_,_|ControlInputs::neutral().with_brakes(1.0))})).collect();
    let slope_env = headwind.with_ground_plane(
        Geodetic::from_degrees(35.0, 139.0, 0.0),
        Meters(0.0),
        GroundSlope::new(0.02, 0.01),
    );
    let slope = run(config, ground, &slope_env, 20, |_, _| {
        ControlInputs::neutral().with_brakes(1.0)
    });
    let mut touchdowns = Vec::new();
    for sink in [1.0, 2.0, 3.0] {
        let t = trim(config, 2.5, 38.0, 0.0, 0.5, &env).unwrap();
        let mut s = t.state;
        s.rigid_body.velocity = s
            .rigid_body
            .local_frame()
            .ned_to_ecef_vector(Ned::new(38.0, 0.0, sink));
        let result = run(config, s, &env, 8, |time, _| {
            t.controls
                .with_throttle(0.0)
                .with_elevator(0.0)
                .with_brakes(if time > 2.0 { 0.6 } else { 0.0 })
        });
        touchdowns.push(json!({"sink_mps":sink,"result":result}));
    }
    let t = trim(config, 500.0, 50.0, 0.0, 0.0, &env).unwrap();
    let flight_power_cycle = run(config, t.state, &env, 60, |time, _| {
        t.controls.with_throttle(if (10.0..30.0).contains(&time) {
            t.controls.throttle() + 0.10
        } else {
            t.controls.throttle()
        })
    });
    let stalled = state(500.0, 35.0, 0.36, 0.0, 0.0, [0.35, 180.0, 0.4]);
    let stall_recovery = run(config, stalled, &env, 20, |time, _| {
        ControlInputs::new(
            0.035,
            if time < 0.5 {
                -0.02
            } else if time < 8.0 {
                0.12
            } else {
                0.06
            },
            0.0026,
            0.6,
            0.5,
        )
    });
    let mut rejects = Vec::new();
    for (name, speed, wind) in [
        ("parked_tailwind", 0.0, 3.0),
        ("backward_taxi", -1.0, 0.0),
        ("too_fast", 100.0, 0.0),
    ] {
        let s = start(1.6, speed);
        let mut m = TurbopropFlightDynamics::new(config.clone(), s).unwrap();
        let e = Environment::with_wind_ned(
            Atmosphere::standard(),
            s.rigid_body.geodetic(),
            Ned::new(wind, 0.0, 0.0),
        );
        let error = m
            .step(flightsim_core::Seconds::ZERO, ControlInputs::neutral(), &e)
            .unwrap_err();
        rejects.push(
            json!({"case":name,"error":format!("{error:?}"),"state_unchanged":*m.state()==s}),
        );
    }
    let mut minimum_margin = f64::INFINITY;
    let mut minimum_power_margin = f64::INFINITY;
    let mut count = 0;
    for ji in 0..=140 {
        for bi in 0..=130 {
            let j = f64::from(ji) / 100.0;
            let beta = 0.07 + f64::from(bi) * 0.005;
            let pair = config
                .propeller()
                .coefficients(PropellerQuery {
                    advance_ratio: AdvanceRatio(j),
                    blade_pitch: Radians(beta),
                })
                .unwrap();
            minimum_power_margin = minimum_power_margin.min(pair.cp - j * pair.ct);
            if pair.ct > 0.0 {
                let ideal =
                    pair.ct / 2.0 * (j + (j * j + 8.0 * pair.ct / std::f64::consts::PI).sqrt());
                minimum_margin = minimum_margin.min(pair.cp - ideal);
            }
            count += 1;
        }
    }
    json!({"schema":1,"qualification":"Original experimental approximation, sampled scenarios only; not manufacturer data or certification",
        "physical_identity":{"algorithm":identity.algorithm,"schema":identity.schema,"kind":identity.kind,"law_revision":identity.law_revision,"fnv1a64":format!("{:016x}",identity.fingerprint)},
        "operating_queries":operating_queries,"trims":trims,"ground_idle":ground_idle,
        "ground_head_crosswind":ground_wind,"takeoff":takeoff,"ground_power_cycle":ground_power_cycle,
        "braking_reverse_boundary":braking,"braking_timestep_refinement":braking_refinement,"braking_headwind":braking_headwind,"sloped_ground":slope,"touchdowns":touchdowns,"flight_power_cycle":flight_power_cycle,
        "stall_recovery":stall_recovery,"typed_rejections":rejects,"propeller_samples":{"count":count,"minimum_cp_minus_ideal":minimum_margin,"minimum_cp_minus_j_ct":minimum_power_margin}})
}
