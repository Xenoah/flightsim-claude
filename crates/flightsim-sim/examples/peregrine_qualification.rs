//! Standalone qualification only. No new runtime law, controls or app registration.
//! Run with PROFILE OUTPUT_DIR [quick]; all failures are retained as evidence.
use flightsim_core::{Attitude, Ecef, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::subsonic::{
    DryThrottle, JetAircraftConfig, JetConditions, JetFlightDynamics, MachNumber, PressureRatio,
    TemperatureRatio,
};
use flightsim_fdm::{
    AeroAngles, Atmosphere, ControlInputs, Environment, GroundSlope, RigidBodyState, aero,
};
use flightsim_sim::aircraft_profile::AircraftProfileV2;
use flightsim_sim::model_identity::{ModelIdentity, canonical_jet_bytes};
use flightsim_sim::model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation};
use flightsim_sim::replay_v4::{JetRecorder, JetRecording, JetReplayPlayer, state_bits_equal};
use glam::{DMat3, DQuat, DVec3};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};

fn controls(c: ControlInputs) -> [f64; 6] {
    [
        c.elevator(),
        c.aileron(),
        c.rudder(),
        c.throttle(),
        c.flaps(),
        c.brakes(),
    ]
}
fn bits(s: &RigidBodyState) -> Vec<String> {
    s.position
        .as_vec()
        .to_array()
        .into_iter()
        .chain(s.velocity.to_array())
        .chain(s.orientation.to_array())
        .chain(s.angular_velocity.to_array())
        .map(|x| format!("{:016x}", x.to_bits()))
        .collect()
}
fn airborne_env() -> Environment {
    Environment::still_air().with_ground(Meters(-1000.))
}
fn state_at(h: f64, v: f64, pitch: f64, descent: f64) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0., 0., h),
        Attitude::new(Radians::ZERO, Radians(pitch), Radians::ZERO),
        Ned::new((v * v - descent * descent).sqrt(), 0., descent),
    )
}
// Independent qualification trim solves the same three longitudinal physical
// residuals, with explicit environment so sea-level flight has no ground contact.
fn trim(
    cfg: &JetAircraftConfig,
    h: f64,
    mach: f64,
    descent: f64,
    flaps: f64,
    env: &Environment,
) -> Result<(RigidBodyState, ControlInputs, [f64; 6]), String> {
    let v = env.atmosphere.sample(Meters(h)).speed_of_sound.get() * mach;
    let model = JetFlightDynamics::new(cfg.clone(), state_at(h, v, 0., descent))
        .map_err(|e| e.to_string())?;
    let eval = |x: DVec3| -> Result<(DVec3, [f64; 6]), String> {
        if x.y.abs() > 1. || !(0. ..=1.).contains(&x.z) {
            return Err("unreachable: control limits".into());
        }
        let s = state_at(h, v, x.x, descent);
        let c = ControlInputs::neutral()
            .with_elevator(x.y)
            .with_throttle(x.z)
            .with_flaps(flaps);
        let d = model.derivative(&s, c, env).map_err(|e| e.to_string())?;
        let a = s.local_frame().ecef_to_ned_vector(d.acceleration).0;
        Ok((
            DVec3::new(a.x, a.z, d.angular_acceleration.y),
            [
                a.x,
                a.y,
                a.z,
                d.angular_acceleration.x,
                d.angular_acceleration.y,
                d.angular_acceleration.z,
            ],
        ))
    };
    let mut x = DVec3::new(0.03, 0.05, 0.5);
    for _ in 0..40 {
        let (r, full) = eval(x)?;
        if r.abs().max_element() < 1e-9 {
            return Ok((
                state_at(h, v, x.x, descent),
                ControlInputs::neutral()
                    .with_elevator(x.y)
                    .with_throttle(x.z)
                    .with_flaps(flaps),
                full,
            ));
        }
        let delta = 1e-5;
        let mut cols = [DVec3::ZERO; 3];
        for j in 0..3 {
            let mut xp = x;
            xp[j] += delta;
            cols[j] = (eval(xp)?.0 - r) / delta;
        }
        let jac = DMat3::from_cols(cols[0], cols[1], cols[2]);
        if jac.determinant().abs() < 1e-12 {
            return Err("unreachable: singular Jacobian".into());
        }
        let update = jac.inverse() * r;
        let mut scale = 1.;
        let mut found = None;
        for _ in 0..20 {
            let xn = x - update * scale;
            if let Ok((rn, _)) = eval(xn)
                && rn.length_squared() < r.length_squared()
            {
                found = Some(xn);
                break;
            }
            scale *= 0.5;
        }
        x = found.ok_or_else(|| "unreachable: bounded line search".to_string())?;
    }
    Err("unreachable: iteration limit".into())
}

struct Run {
    summary: Value,
    history: Vec<Value>,
    states: Vec<RigidBodyState>,
}
fn run<F: Fn(f64, &RigidBodyState) -> ControlInputs>(
    cfg: &JetAircraftConfig,
    initial: RigidBodyState,
    env: Environment,
    hz: u32,
    seconds: u32,
    control: F,
) -> Run {
    let mut model = JetFlightDynamics::new(cfg.clone(), initial).unwrap();
    let v0 = (initial.velocity - env.wind_ecef).length();
    let h0 = initial.altitude().get();
    let mut history = Vec::new();
    let mut states = Vec::new();
    let mut terminal = None;
    let mut steps = 0;
    let mut substeps = [0_u64; 9];
    let mut dh: f64 = 0.;
    let mut dv: f64 = 0.;
    let mut alpha: f64 = 0.;
    let mut beta: f64 = 0.;
    let mut bank: f64 = 0.;
    let mut min_h = h0;
    let mut stationary = 0_u32;
    let mut max_stationary = 0_u32;
    let mut final10_v: f64 = 0.;
    let mut final10_start = initial.position.as_vec();
    let mut compress = [0_f64; 3];
    let mut first_contact = None;
    for i in 0..hz * seconds {
        let t = f64::from(i) / f64::from(hz);
        let c = control(t, model.state());
        match model.step(Seconds(1. / f64::from(hz)), c, &env) {
            Ok(report) => {
                steps += 1;
                substeps[report.substeps as usize] += 1;
                for (j, gap) in report.gear_clearances.iter().enumerate() {
                    compress[j] = compress[j].max(-gap.get());
                }
                if first_contact.is_none() && report.gear_clearances.iter().any(|g| g.get() <= 0.) {
                    first_contact = Some(
                        json!({"time_s":t+1./f64::from(hz),"sink_mps":-model.state().vertical_speed().get()}),
                    );
                }
            }
            Err(e) => {
                terminal = Some(
                    json!({"time_s":t,"reason":format!("{:?}",e.reason),"stage":format!("{:?}",e.stage),"substep":e.substep,"query":format!("{:?}",e.query)}),
                );
                break;
            }
        }
        let s = model.state();
        let a = flightsim_fdm::aero_angles_of(s, &env);
        let h = s.altitude().get();
        let v = a.true_airspeed.get();
        dh = dh.max((h - h0).abs());
        dv = dv.max((v - v0).abs());
        min_h = min_h.min(h);
        alpha = alpha.max(a.angle_of_attack.get().abs());
        beta = beta.max(a.sideslip.get().abs());
        bank = bank.max(s.attitude().roll.get().abs());
        let gv = s.velocity_ned().0.abs().max_element();
        if gv < 0.001 {
            stationary += 1;
        } else {
            stationary = 0;
        }
        max_stationary = max_stationary.max(stationary);
        if i == hz * seconds.saturating_sub(10) {
            final10_start = s.position.as_vec();
        }
        if i >= hz * seconds.saturating_sub(10) {
            final10_v = final10_v.max(gv);
        }
        if (i + 1) % (hz / 120) == 0 {
            states.push(*s);
        }
        if (i + 1) % (hz / 20) == 0 {
            history.push(json!({"t":f64::from(i+1)/f64::from(hz),"h":h,"v":v,"alpha":a.angle_of_attack.get(),"beta":a.sideslip.get(),"attitude":[s.attitude().roll.get(),s.attitude().pitch.get(),s.attitude().yaw.get()],"omega":s.angular_velocity.to_array(),"controls":controls(c),"position":s.position.as_vec().to_array(),"velocity_ned":s.velocity_ned().0.to_array(),"state_bits":bits(s)}));
        }
    }
    let s = *model.state();
    let summary = json!({"hz":hz,"requested_seconds":seconds,"steps":steps,"terminal":terminal,"substep_histogram":substeps,"max_altitude_deviation_m":dh,"max_speed_deviation_mps":dv,"min_altitude_m":min_h,"final_altitude_m":s.altitude().get(),"final_speed_mps":(s.velocity-env.wind_ecef).length(),"final_climb_mps":s.vertical_speed().get(),"max_abs_alpha_deg":alpha.to_degrees(),"max_abs_beta_deg":beta.to_degrees(),"max_abs_bank_deg":bank.to_degrees(),"stationary_continuous_max_s":f64::from(max_stationary)/f64::from(hz),"parking_pass":max_stationary>=10*hz&&steps==hz*seconds&&terminal.is_none(),"final10s_max_ground_component_mps":final10_v,"final10s_displacement_m":(s.position.as_vec()-final10_start).length(),"displacement_m":(s.position.as_vec()-initial.position.as_vec()).length(),"max_compression_m":compress,"first_contact":first_contact,"final_state_bits":bits(&s)});
    Run {
        summary,
        history,
        states,
    }
}
fn save_run(out: &Path, name: &str, r: &Run) -> Result<(), Box<dyn Error>> {
    fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_vec(&json!({"summary":r.summary,"history_20hz":r.history}))?,
    )?;
    Ok(())
}
fn compare(a: &Run, b: &Run) -> Value {
    let mut p: f64 = 0.;
    let mut v: f64 = 0.;
    let mut q: f64 = 0.;
    for (x, y) in a.states.iter().zip(&b.states) {
        p = p.max((x.position.as_vec() - y.position.as_vec()).length());
        v = v.max((x.velocity - y.velocity).length());
        q = q.max(x.orientation.angle_between(y.orientation).abs());
    }
    json!({"max_position_m":p,"max_velocity_mps":v,"max_orientation_deg":q.to_degrees(),"complete":a.states.len()==1200&&b.states.len()==1200,"within_tolerance":a.states.len()==1200&&b.states.len()==1200&&p<=0.1&&v<=0.01&&q.to_degrees()<=0.01})
}
fn refinement(a: &Run, b: &Run, c: &Run) -> Value {
    let coarse = compare(a, c);
    let fine = compare(b, c);
    let monotonic = [
        ("max_position_m", 1e-6),
        ("max_velocity_mps", 1e-7),
        ("max_orientation_deg", 1e-7_f64.to_degrees()),
    ]
    .iter()
    .all(|(key, floor)| fine[*key].as_f64().unwrap() <= coarse[*key].as_f64().unwrap() + floor);
    json!({"120_vs480":coarse,"240_vs480":fine,"declining_error_with_floors":monotonic,
        "pass":coarse["within_tolerance"]==true&&fine["within_tolerance"]==true&&monotonic})
}

fn height_for_pressure(target: f64) -> f64 {
    let mut low = -1000.;
    let mut high = 8000.;
    for _ in 0..64 {
        let mid = (low + high) * 0.5;
        if Atmosphere::standard().sample(Meters(mid)).pressure.get() / 101325. > target {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) * 0.5
}

fn domain_and_corners(cfg: &JetAircraftConfig, out: &Path) -> Result<Value, Box<dyn Error>> {
    let mut edges = Vec::new();
    // Pressure is altitude-derived in this frozen law. Record the actual query,
    // including roundoff, instead of calling a nearby ratio the exact boundary.
    for axis in 0..3 {
        for edge in 0..2 {
            for side in [-1., 0., 1.] {
                let p = if axis == 0 {
                    cfg.envelope().definition().pressure_ratio[edge] + side * 1e-9
                } else {
                    0.7
                };
                let t = if axis == 1 {
                    cfg.envelope().definition().temperature_ratio[edge] + side * 1e-9
                } else {
                    0.9
                };
                let m = if axis == 2 {
                    cfg.envelope().definition().mach[edge] + side * 1e-9
                } else {
                    0.55
                };
                if m < 0. {
                    continue;
                }
                let h = height_for_pressure(p);
                let standard = Atmosphere::standard().sample(Meters(h));
                let env = Environment::with_wind_ned(
                    Atmosphere::with_temperature_offset(288.15 * t - standard.temperature.get()),
                    Geodetic::from_degrees(0., 0., h),
                    Ned::new(0., 0., 0.),
                )
                .with_ground(Meters(-1000.));
                let s = state_at(
                    h,
                    env.atmosphere.sample(Meters(h)).speed_of_sound.get() * m,
                    0.,
                    0.,
                );
                let mut model = JetFlightDynamics::new(cfg.clone(), s)?;
                let query = JetConditions::from_atmosphere(
                    env.atmosphere.sample(s.altitude()),
                    MetersPerSecond(s.body_velocity().length()),
                )?;
                let status = model.step(Seconds::ZERO, ControlInputs::neutral(), &env);
                edges.push(json!({"axis":axis,"edge":edge,"side":side,"requested":[p,t,m],"actual":[query.pressure_ratio.0,query.temperature_ratio.0,query.mach.0],"accepted":status.is_ok(),"status":format!("{status:?}"),"rollback":state_bits_equal(&s,model.state())}));
            }
        }
    }
    let mut witnesses = std::collections::BTreeMap::new();
    let env = airborne_env();
    let h = 1000.;
    let sound = env.atmosphere.sample(Meters(h)).speed_of_sound.get();
    // Bounded near-edge search is stage coverage, not aircraft tuning. Every
    // failure must leave all13 source state words unchanged.
    let mut all_rollback = true;
    let mut attempts = 0;
    for pitch in [-0.5, -0.48, -0.45, -0.3] {
        for speed_gap in 0..600 {
            let m = 0.65 - f64::from(speed_gap) * 1e-7;
            let s = state_at(h, sound * m, pitch, 100.);
            let mut model = JetFlightDynamics::new(cfg.clone(), s)?;
            let c = ControlInputs::neutral()
                .with_throttle(1.)
                .with_elevator(-0.5);
            attempts += 1;
            if let Err(error) = model.step(Seconds(1. / 120.), c, &env) {
                all_rollback &= state_bits_equal(&s, model.state());
                let key = format!("{:?}", error.stage);
                witnesses.entry(key).or_insert_with(||json!({"state_bits":bits(&s),"controls":controls(c),"error":format!("{error:?}"),"rollback":state_bits_equal(&s,model.state())}));
            }
        }
    }
    // Refine diagnostic stage transitions; profile/domain/control law unchanged.
    for pitch in [-0.5, -0.48, -0.45, -0.3] {
        for target_stage in [
            flightsim_fdm::subsonic::JetStage::K2,
            flightsim_fdm::subsonic::JetStage::K3,
            flightsim_fdm::subsonic::JetStage::K4,
        ] {
            let mut low = 0.649;
            let mut high = 0.65;
            for _ in 0..52 {
                let m = (low + high) * 0.5;
                let s = state_at(h, sound * m, pitch, 100.);
                let mut model = JetFlightDynamics::new(cfg.clone(), s)?;
                let c = ControlInputs::neutral()
                    .with_throttle(1.)
                    .with_elevator(-0.5);
                attempts += 1;
                match model.step(Seconds(1. / 120.), c, &env) {
                    Err(error) => {
                        all_rollback &= state_bits_equal(&s, model.state());
                        let key = format!("{:?}", error.stage);
                        witnesses.entry(key).or_insert_with(||json!({"state_bits":bits(&s),"controls":controls(c),"error":format!("{error:?}"),"rollback":state_bits_equal(&s,model.state())}));
                        if error.stage as u8 <= target_stage as u8 {
                            high = m;
                        } else {
                            low = m;
                        }
                    }
                    Ok(_) => {
                        low = m;
                    }
                }
            }
        }
    }
    let h = height_for_pressure(1.07);
    let offset = 288.15 * 0.76 - Atmosphere::standard().sample(Meters(h)).temperature.get();
    let env = Environment::with_wind_ned(
        Atmosphere::with_temperature_offset(offset),
        Geodetic::from_degrees(0., 0., h),
        Ned::new(0., 0., 0.),
    )
    .with_ground(Meters(-1000.));
    let s = state_at(
        h,
        env.atmosphere.sample(Meters(h)).speed_of_sound.get() * 0.645,
        -0.02,
        0.,
    );
    let coeff = cfg.aero().sample(MachNumber(0.645))?.coefficients;
    let c = ControlInputs::neutral()
        .with_elevator(-(coeff.pitch_zero + coeff.pitch_alpha * (-0.02)) / coeff.pitch_elevator)
        .with_throttle(0.9);
    let mut runs = Vec::new();
    for hz in [120, 240, 480] {
        let r = run(cfg, s, env, hz, 10, |_, _| c);
        save_run(out, &format!("high-q-inward-corner-{hz}"), &r)?;
        runs.push(r);
    }
    let query = JetConditions::from_atmosphere(
        env.atmosphere.sample(s.altitude()),
        MetersPerSecond(s.body_velocity().length()),
    )?;
    Ok(
        json!({"edges":edges,"stage_search_attempts":attempts,"all_failures_rollback":all_rollback,"stage_witnesses":witnesses,
        "corner":{"initial":[query.pressure_ratio.0,query.temperature_ratio.0,query.mach.0],"q_pa":0.7*101325.*query.pressure_ratio.0*query.mach.0.powi(2),"control_script":"pitch-.02, zero initial pitch moment, throttle.9 held10s; untrimmed near-corner response","refinement":refinement(&runs[0],&runs[1],&runs[2]),"runs":runs.iter().map(|r|r.summary.clone()).collect::<Vec<_>>()},
        "corner_jacobians":[jacobian(cfg,s,c,env,1.),jacobian(cfg,s,c,env,0.5)],
        "corner_jacobian_status":"near-corner p1.07/T.76/M.645; exact hard-corner local dynamics remain unqualified"}),
    )
}
fn supplementary(cfg: &JetAircraftConfig, out: &Path) -> Result<Value, Box<dyn Error>> {
    let mut modes = Vec::new();
    let mut polars = Vec::new();
    for knot in &cfg.aero().definition().knots {
        for flaps in [0., 0.5, 1.] {
            for alpha in [-3.0, -0.6, -0.3, -0.1, 0., 0.1, 0.2, 0.3, 0.6, 3.0] {
                let a = cfg.aero().sample(MachNumber(knot.mach))?.coefficients;
                let value = aero::coefficients(
                    &a,
                    cfg.airframe().geometry(),
                    AeroAngles {
                        angle_of_attack: Radians(alpha),
                        sideslip: Radians::ZERO,
                        true_airspeed: MetersPerSecond(100.),
                    },
                    DVec3::ZERO,
                    ControlInputs::neutral().with_flaps(flaps),
                );
                polars.push(json!({"mach":knot.mach,"alpha_rad":alpha,"flaps":flaps,"lift":value.lift,"drag":value.drag}));
            }
        }
    }
    // Re-trim on each side so local differences do not span a Mach knot.
    for h in [0., 3000., 6000.] {
        for m in [0.2, 0.35, 0.5, 0.6] {
            for side in [-1., 1.] {
                let selected = m + side * 2e-5;
                match trim(cfg,h,selected,0.,0.,&airborne_env()) {
            Ok((s,c,_))=>modes.push(json!({"altitude_m":h,"knot":m,"side":side,"actual_mach":selected,
                "jacobians":[jacobian(cfg,s,c,airborne_env(),1.),jacobian(cfg,s,c,airborne_env(),0.5)]})),
            Err(e)=>modes.push(json!({"altitude_m":h,"knot":m,"side":side,"unreachable":e})),
        }
            }
        }
    }
    Ok(
        json!({"domain_and_corners":domain_and_corners(cfg,out)?,"one_sided_knot_modes":modes,"polar_samples":polars}),
    )
}
fn jacobian(
    cfg: &JetAircraftConfig,
    s: RigidBodyState,
    c: ControlInputs,
    env: Environment,
    scale: f64,
) -> Value {
    let frame = s.local_frame();
    let model = JetFlightDynamics::new(cfg.clone(), s).unwrap();
    let mut matrix = vec![vec![0.; 12]; 12];
    let eval = |state: &RigidBodyState| -> Vec<f64> {
        let d = model.derivative(state, c, &env).unwrap();
        frame
            .ecef_to_ned_vector(d.velocity)
            .0
            .to_array()
            .into_iter()
            .chain(frame.ecef_to_ned_vector(d.acceleration).0.to_array())
            .chain(state.angular_velocity.to_array())
            .chain(d.angular_acceleration.to_array())
            .collect()
    };
    for (j, step) in [
        0.1, 0.1, 0.1, 0.001, 0.001, 0.001, 0.00001, 0.00001, 0.00001, 0.00001, 0.00001, 0.00001,
    ]
    .into_iter()
    .enumerate()
    {
        let h = scale * step;
        let shift = |sign: f64| {
            let mut p = s;
            let axis = DVec3::AXES[j % 3];
            if j < 3 {
                p.position = Ecef::from_vec(
                    s.position.as_vec() + frame.ned_to_ecef_vector(Ned(axis * h * sign)),
                );
            } else if j < 6 {
                p.velocity += frame.ned_to_ecef_vector(Ned(axis * h * sign));
            } else if j < 9 {
                p.orientation =
                    (s.orientation * DQuat::from_scaled_axis(axis * h * sign)).normalize();
            } else {
                p.angular_velocity += axis * h * sign;
            }
            p
        };
        let plus = eval(&shift(1.));
        let minus = eval(&shift(-1.));
        for i in 0..12 {
            matrix[i][j] = (plus[i] - minus[i]) / (2. * h);
        }
    }
    json!({"order":"NED position, NED velocity, right body rotation vector, body angular velocity","step_scale":scale,"matrix":matrix})
}

// Eight-corner barycentric sum is independent of production's nested lerps.
fn reference_thrust(cfg: &JetAircraftConfig, p: f64, t: f64, m: f64, command: f64) -> f64 {
    let d = cfg.thrust().definition();
    let bracket = |a: &[f64], x: f64| {
        let i = a.windows(2).position(|w| x >= w[0] && x <= w[1]).unwrap();
        (i, (x - a[i]) / (a[i + 1] - a[i]))
    };
    let (ip, wp) = bracket(&d.pressure_ratios, p);
    let (it, wt) = bracket(&d.temperature_ratios, t);
    let (im, wm) = bracket(&d.mach, m);
    let mut value = 0.;
    for dp in 0..2 {
        for dt in 0..2 {
            for dm in 0..2 {
                let w = if dp == 0 { 1. - wp } else { wp }
                    * if dt == 0 { 1. - wt } else { wt }
                    * if dm == 0 { 1. - wm } else { wm };
                let cell = d.cells
                    [((ip + dp) * d.temperature_ratios.len() + it + dt) * d.mach.len() + im + dm];
                value += w * ((1. - command) * cell.idle_n + command * cell.maximum_dry_n);
            }
        }
    }
    value
}
fn algebra(cfg: &JetAircraftConfig) -> Value {
    let mut rows = Vec::new();
    let mut max_thrust: f64 = 0.;
    let mut max_force: f64 = 0.;
    let mut max_moment: f64 = 0.;
    for m in [
        0.,
        0.2 - 1e-8,
        0.2,
        0.2 + 1e-8,
        0.35,
        0.475,
        0.5,
        0.55,
        0.6,
        0.65,
    ] {
        for (p, t, command) in [
            (0.45, 0.75, 0.),
            (0.61, 0.93, 0.37),
            (1., 1., 1.),
            (1.08, 1.1, 0.8),
        ] {
            let query = JetConditions {
                pressure_ratio: PressureRatio(p),
                temperature_ratio: TemperatureRatio(t),
                mach: MachNumber(m),
            };
            let got = cfg
                .thrust()
                .sample(query, DryThrottle(command))
                .unwrap()
                .net_thrust
                .get();
            let expected = reference_thrust(cfg, p, t, m, command);
            max_thrust = max_thrust.max((got - expected).abs());
            let a = cfg.aero().sample(MachNumber(m)).unwrap().coefficients;
            let speed = m * (1.4_f64 * 287.052874 * 288.15 * t).sqrt();
            let rho = 101325. * p / (287.052874 * 288.15 * t);
            let q = 0.5 * rho * speed * speed;
            let angles = AeroAngles {
                angle_of_attack: Radians(0.06),
                sideslip: Radians(0.025),
                true_airspeed: MetersPerSecond(speed),
            };
            let omega = DVec3::new(0.03, -0.02, 0.01);
            let c = ControlInputs::neutral()
                .with_aileron(0.1)
                .with_elevator(-0.05)
                .with_rudder(0.07)
                .with_flaps(0.3);
            let g = cfg.airframe().geometry();
            let alpha: f64 = 0.06;
            let flap = 0.3;
            // Ratio-form sigmoid is independent of production's logistic product.
            let left = (-a.stall_blend_rate * (alpha - a.stall_angle.get())).exp();
            let right = (a.stall_blend_rate * (alpha + a.stall_angle.get())).exp();
            let sigma = (1. + left + right) / ((1. + left) * (1. + right));
            let cl = a.lift_zero + a.lift_alpha * alpha + a.lift_flaps * flap;
            let lift = (1. - sigma) * cl + sigma * 2. * alpha.sin().powi(2) * alpha.cos();
            let drag = (1. - sigma)
                * (a.drag_min
                    + cl * cl / (std::f64::consts::PI * a.oswald_efficiency * g.aspect_ratio())
                    + a.drag_flaps * flap)
                + sigma * 2. * alpha.sin().powi(2);
            let norm = 1. / (2. * speed.max(1.));
            let pr = omega.x * g.wing_span.get() * norm;
            let qr = omega.y * g.mean_chord.get() * norm;
            let rr = omega.z * g.wing_span.get() * norm;
            let side = a.side_beta * 0.025 + a.side_rudder * 0.07;
            let roll = a.roll_beta * 0.025
                + a.roll_rate_p * pr
                + a.roll_rate_r * rr
                + a.roll_aileron * 0.1
                + a.roll_rudder * 0.07;
            let pitch = a.pitch_zero
                + a.pitch_alpha * alpha
                + a.pitch_rate_q * qr
                + a.pitch_elevator * (-0.05)
                + a.pitch_flaps * flap;
            let yaw = a.yaw_beta * 0.025
                + a.yaw_rate_p * pr
                + a.yaw_rate_r * rr
                + a.yaw_aileron * 0.1
                + a.yaw_rudder * 0.07;
            let f = DVec3::new(
                lift * alpha.sin() - drag * alpha.cos(),
                side,
                -lift * alpha.cos() - drag * alpha.sin(),
            ) * q
                * g.wing_area.get();
            let moment = DVec3::new(
                roll * g.wing_span.get(),
                pitch * g.mean_chord.get(),
                yaw * g.wing_span.get(),
            ) * q
                * g.wing_area.get();
            let actual = aero::body_force_and_moment(
                &a,
                g,
                angles,
                omega,
                c,
                flightsim_core::KilogramsPerCubicMeter(rho),
            );
            max_force = max_force.max((f - actual.0).abs().max_element());
            max_moment = max_moment.max((moment - actual.1).abs().max_element());
            rows.push(json!({"p":p,"t":t,"mach":m,"throttle":command,"q_pa":q,"net_thrust_n":got,"reference_n":expected,"drag_coefficient":drag}));
        }
    }
    json!({"cases":rows,"max_thrust_error_n":max_thrust,"max_force_error_n":max_force,"max_moment_error_nm":max_moment,"pass":max_thrust<1e-7&&max_force<1e-7&&max_moment<1e-7})
}

fn replay(
    cfg: &JetAircraftConfig,
    s: RigidBodyState,
    c: ControlInputs,
    out: &Path,
    name: &str,
    limit: u32,
) -> Value {
    let mut sim = JetSimulation::from_state(cfg.clone(), s, JetEnvironment::default()).unwrap();
    let mut recorder = JetRecorder::new(&sim).unwrap();
    let mut exact_states = vec![s];
    let mut controller = 0_u32;
    let mut transaction = true;
    for _ in 0..limit {
        let before = sim.snapshot();
        let before_control = controller;
        let report = sim.advance_with_controller(JET_FIXED_DT, &mut controller, |count, _, _| {
            *count += 1;
            c
        });
        if report.terminal().is_some() {
            transaction &= before == sim.snapshot() && controller == before_control;
        }
        recorder.record(&report).unwrap();
        if report.terminal().is_some() {
            break;
        }
        exact_states.push(*sim.state());
    }
    let recording = recorder.finish();
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    fs::write(out.join(format!("{name}.fsreplay")), &bytes).unwrap();
    let parsed = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    let mut again = Vec::new();
    parsed.write_to(&mut again).unwrap();
    let n = u32::try_from(recording.controls().len()).unwrap();
    let mut player = JetReplayPlayer::new(cfg.clone(), parsed).unwrap();
    let mut exact = true;
    let mut max_work = 0;
    while !player.finished() {
        let work = player.advance(Seconds(0.25)).unwrap();
        max_work = max_work.max(work);
        exact &= state_bits_equal(
            player.simulation().state(),
            &exact_states[player.cursor() as usize],
        );
    }
    let terminal_exact = player.simulation().terminal() == sim.terminal();
    for target in [0, n / 2, n, n.saturating_sub(1), 0, n] {
        let work = player.seek_to(target).unwrap();
        max_work = max_work.max(work);
        while player.seeking() {
            max_work = max_work.max(player.continue_seek().unwrap());
        }
        exact &= state_bits_equal(player.simulation().state(), &exact_states[target as usize]);
    }
    player.set_paused(true);
    let before = *player.simulation().state();
    let cursor = player.cursor();
    player.advance(Seconds(5.)).unwrap();
    let pause_exact =
        cursor == player.cursor() && state_bits_equal(&before, player.simulation().state());
    player.restart().unwrap();
    let restart_exact = state_bits_equal(&s, player.simulation().state());
    json!({"frames":n,"terminal":format!("{:?}",sim.terminal()),"bytes":bytes.len(),"byte_roundtrip":bytes==again,"exact_full_state":exact,"terminal_exact":terminal_exact,"transaction":transaction,"pause_exact":pause_exact,"restart_exact":restart_exact,"max_work":max_work,"pass":bytes==again&&exact&&terminal_exact&&transaction&&pause_exact&&restart_exact&&max_work<=240})
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: peregine_qualification PROFILE OUTPUT_DIR [quick]".into());
    }
    let profile = AircraftProfileV2::parse(&fs::read_to_string(&args[1])?)?;
    let cfg = profile.configuration();
    let out = Path::new(&args[2]);
    fs::create_dir_all(out)?;
    if args.get(3).is_some_and(|x| x == "diagnostics") {
        let result = supplementary(cfg, out)?;
        fs::write(
            out.join("supplementary.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        return Ok(());
    }
    let quick = args.get(3).is_some_and(|x| x == "quick");
    let id = ModelIdentity::for_jet(cfg);
    fs::write(out.join("canonical.bin"), canonical_jet_bytes(cfg))?;
    let mut result = json!({"profile":args[1],"identity":format!("{:016x}",id.fingerprint),"identity_schema":id.schema,"law_revision":id.law_revision,"model_kind":id.kind,"mode":if quick{"quick"}else{"full"},"algebra":algebra(cfg)});
    let mut grid = Vec::new();
    let altitudes: Vec<f64> = if quick {
        vec![3000.]
    } else {
        vec![0., 3000., 6000.]
    };
    let machs = if quick {
        vec![0.55, 0.6]
    } else {
        vec![0.2, 0.35, 0.5, 0.55, 0.6]
    };
    for h in altitudes {
        for &m in &machs {
            let name = format!("trim-h{h}-m{m}");
            match trim(cfg, h, m, 0., 0., &airborne_env()) {
                Ok((s, c, residual)) => {
                    let r = run(cfg, s, airborne_env(), 120, 60, |_, _| c);
                    save_run(out, &name, &r)?;
                    let held = r.summary["max_altitude_deviation_m"].as_f64().unwrap() <= 10.
                        && r.summary["max_speed_deviation_mps"].as_f64().unwrap() <= 1.5
                        && r.summary["terminal"].is_null();
                    let mut row = json!({"altitude_m":h,"mach":m,"trimmed":true,"residual":residual,"controls":controls(c),"pitch_rad":s.attitude().pitch.get(),"cruise_margin":c.throttle()<0.9&&c.elevator().abs()<0.5&&s.attitude().pitch.get().abs()<8_f64.to_radians(),"held60_pass":held,"held60":r.summary});
                    if !quick {
                        let r = run(cfg, s, airborne_env(), 120, 600, |_, _| c);
                        save_run(out, &format!("{name}-600s"), &r)?;
                        row["held600"] = r.summary;
                        row["jacobian"] = json!([
                            jacobian(cfg, s, c, airborne_env(), 1.),
                            jacobian(cfg, s, c, airborne_env(), 0.5)
                        ]);
                    }
                    eprintln!(
                        "trim {h}m M{m}: throttle {:0.6} elevator {:0.6}, held60 {held}",
                        c.throttle(),
                        c.elevator()
                    );
                    grid.push(row);
                }
                Err(e) => {
                    eprintln!("trim {h}m M{m}: {e}");
                    grid.push(json!({"altitude_m":h,"mach":m,"trimmed":false,"reason":e}));
                }
            }
        }
    }
    result["trim_grid"] = json!(grid);
    let mut parking = Vec::new();
    for idle in [250., 500., 1000.] {
        let mut td = cfg.thrust().definition().clone();
        for (index, cell) in td.cells.iter_mut().enumerate() {
            let mi = index % td.mach.len();
            let ti = (index / td.mach.len()) % td.temperature_ratios.len();
            let pi = index / (td.mach.len() * td.temperature_ratios.len());
            cell.idle_n = td.pressure_ratios[pi] * (idle - 900. * td.mach[mi])
                / td.temperature_ratios[ti].sqrt();
        }
        let model = JetAircraftConfig::new(
            cfg.airframe().clone(),
            flightsim_fdm::subsonic::DryJetTable::from_definition(td)?,
            cfg.aero().clone(),
            *cfg.envelope(),
        )?;
        fs::write(
            out.join(format!("idle-{idle}-canonical.bin")),
            canonical_jet_bytes(&model),
        )?;
        for speed in [0., 25.] {
            let mut s = *JetSimulation::parked(
                model.clone(),
                Geodetic::from_degrees(0., 0., 0.),
                Radians::ZERO,
                JetEnvironment::default(),
            )?
            .state();
            s.velocity = s.local_frame().ned_to_ecef_vector(Ned::new(speed, 0., 0.));
            let r = run(&model, s, Environment::still_air(), 120, 60, |_, _| {
                ControlInputs::neutral().with_brakes(1.)
            });
            let name = format!("brake-idle{idle}-v{speed}");
            save_run(out, &name, &r)?;
            eprintln!(
                "parking {idle}N start{speed}: {} final10 max{}",
                r.summary["parking_pass"], r.summary["final10s_max_ground_component_mps"]
            );
            parking.push(json!({"idle_static_n":idle,"initial_mps":speed,"identity":format!("{:016x}",ModelIdentity::for_jet(&model).fingerprint),"run":r.summary}));
        }
    }
    result["parking_sensitivity"] = json!(parking);
    if !quick {
        fs::write(
            out.join("summary-before-full.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        full(cfg, out, &mut result)?;
    }
    fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(())
}

fn full(cfg: &JetAircraftConfig, out: &Path, result: &mut Value) -> Result<(), Box<dyn Error>> {
    let mut convergence = Vec::new();
    let mut responses = Vec::new();
    for (h, m) in [(0., 0.2), (3000., 0.55), (0., 0.6), (6000., 0.6)] {
        let (s, c, _) = match trim(cfg, h, m, 0., 0., &airborne_env()) {
            Ok(value) => value,
            Err(error) => {
                convergence.push(json!({"altitude_m":h,"mach":m,"unreachable":error,"pass":false}));
                responses.push(json!({"altitude_m":h,"mach":m,"unreachable":error,"pass":false}));
                continue;
            }
        };
        let mut start = s;
        start.orientation =
            (s.orientation * DQuat::from_scaled_axis(DVec3::new(0.01, 0.01, 0.01))).normalize();
        start.angular_velocity = DVec3::new(0.02, 0.02, 0.02);
        let mut runs = Vec::new();
        for hz in [120, 240, 480] {
            let r = run(cfg, start, airborne_env(), hz, 10, |_, _| c);
            save_run(out, &format!("convergence-h{h}-m{m}-{hz}"), &r)?;
            runs.push(r);
        }
        convergence.push(json!({"altitude_m":h,"mach":m,"refinement":refinement(&runs[0],&runs[1],&runs[2]),"runs":runs.iter().map(|r|r.summary.clone()).collect::<Vec<_>>() }));
        for axis in 0..3 {
            for sign in [-1., 1.] {
                for (magnitude, duration, full_command) in [(0.02, 0.25, false), (1., 0.1, true)] {
                    let r = run(cfg, s, airborne_env(), 120, 10, |t, _| {
                        let u = if t < duration { sign * magnitude } else { 0. };
                        match axis {
                            0 => c.with_elevator(if full_command && t < duration {
                                sign
                            } else {
                                (c.elevator() + u).clamp(-1., 1.)
                            }),
                            1 => c.with_aileron(u),
                            _ => c.with_rudder(u),
                        }
                    });
                    save_run(
                        out,
                        &format!("pulse-h{h}-m{m}-a{axis}-s{sign}-u{magnitude}"),
                        &r,
                    )?;
                    responses.push(json!({"altitude_m":h,"mach":m,"axis":axis,"command":sign*magnitude,"duration_s":duration,"run":r.summary}));
                }
            }
        }
        for throttle in [0., 1.] {
            let r = run(cfg, s, airborne_env(), 120, 3, |_, _| {
                c.with_throttle(throttle)
            });
            save_run(out, &format!("throttle-h{h}-m{m}-{throttle}"), &r)?;
            responses.push(json!({"altitude_m":h,"mach":m,"throttle":throttle,"run":r.summary}));
        }
        for flaps in [0., 1.] {
            let r = run(cfg, s, airborne_env(), 120, 10, |_, _| c.with_flaps(flaps));
            save_run(out, &format!("flaps-h{h}-m{m}-{flaps}"), &r)?;
            responses.push(json!({"altitude_m":h,"mach":m,"flaps":flaps,"run":r.summary}));
        }
    }
    result["domain_and_corners"] = domain_and_corners(cfg, out)?;
    result["convergence"] = json!(convergence);
    result["control_responses"] = json!(responses);
    let mut stalls = Vec::new();
    for knot in &cfg.aero().definition().knots {
        for f in [0., 0.5, 1.] {
            let a = cfg.aero().sample(MachNumber(knot.mach))?.coefficients;
            let peak = aero::positive_stall_peak_angle(&a, cfg.airframe().geometry(), f);
            stalls.push(json!({"mach":knot.mach,"flaps":f,"peak_rad":peak.map(|p|p.get()),"proposed_warning_rad":peak.map(|p|0.85*p.get())}));
        }
    }
    result["stall_peaks"] = json!(stalls);
    let (s, c, _) = trim(cfg, 1500., 0.2, 0., 0., &airborne_env())?;
    let mut stalled = s;
    stalled.orientation = (s.orientation * DQuat::from_rotation_y(0.3)).normalize();
    let r = run(cfg, stalled, airborne_env(), 120, 30, |t, _| {
        if t < 2. {
            c.with_elevator(-0.3).with_throttle(1.)
        } else {
            c.with_throttle(0.7)
        }
    });
    save_run(out, "poststall-explicit-recovery", &r)?;
    result["poststall_recovery"] = r.summary;
    let parked = *JetSimulation::parked(
        cfg.clone(),
        Geodetic::from_degrees(0., 0., 0.),
        Radians::ZERO,
        JetEnvironment::default(),
    )?
    .state();
    let r = run(cfg, parked, Environment::still_air(), 120, 60, |t, _| {
        if t < 10. {
            ControlInputs::neutral().with_throttle(0.08)
        } else {
            ControlInputs::neutral().with_brakes(1.)
        }
    });
    save_run(out, "taxi-brake", &r)?;
    result["taxi_brake"] = r.summary;
    let r = run(cfg, parked, Environment::still_air(), 120, 60, |t, _| {
        ControlInputs::neutral()
            .with_throttle(if t < 25. { 1. } else { 0.55 })
            .with_flaps(0.25)
            .with_elevator(if t < 9. {
                0.
            } else if t < 12. {
                0.18
            } else {
                0.08
            })
    });
    save_run(out, "takeoff-time-script", &r)?;
    result["takeoff"] = r.summary;
    let mut adverse = Vec::new();
    for (label, wind, slope) in [
        ("headcross", Ned::new(-10., 5., 0.), 0.),
        ("slope2deg", Ned::new(0., 0., 0.), 2_f64.to_radians().tan()),
    ] {
        let mut s = parked;
        if slope != 0. {
            s = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(0., 0., 1.15 * (1. + slope * slope).sqrt()),
                Attitude::new(Radians::ZERO, Radians(slope.atan()), Radians::ZERO),
                Ned::new(0., 0., 0.),
            );
        }
        let env = Environment::with_wind_ned(Atmosphere::standard(), s.geodetic(), wind)
            .with_ground_plane(
                Geodetic::from_degrees(0., 0., 0.),
                Meters::ZERO,
                GroundSlope::new(slope, 0.),
            );
        let r = run(cfg, s, env, 120, 60, |_, _| {
            ControlInputs::neutral().with_brakes(1.)
        });
        save_run(out, label, &r)?;
        adverse.push(json!({"case":label,"run":r.summary}));
    }
    result["adverse_parking"] = json!(adverse);
    let approach_mach = 60.
        / Atmosphere::standard()
            .sample(Meters(60.))
            .speed_of_sound
            .get();
    match trim(cfg, 60., approach_mach, 2., 0.5, &airborne_env()) {
        Ok((s, c, residual)) => {
            let r = run(cfg, s, Environment::still_air(), 120, 80, |_, state| {
                if state.altitude().get() < 1.2 {
                    c.with_throttle(0.).with_brakes(1.)
                } else {
                    c
                }
            });
            save_run(out, "approach-contact-brake", &r)?;
            result["approach"] = json!({"controls":controls(c),"pitch_rad":s.attitude().pitch.get(),"residual":residual,"run":r.summary});
            let r = run(cfg, s, Environment::still_air(), 120, 30, |t, _| {
                c.with_throttle(1.).with_elevator(if t < 3. {
                    c.elevator() + 0.04
                } else {
                    c.elevator()
                })
            });
            save_run(out, "go-around-script", &r)?;
            result["go_around"] = r.summary;
        }
        Err(e) => result["approach"] = json!({"unreachable":e}),
    };
    let (s, c, _) = trim(cfg, 3000., 0.6, 0., 0., &airborne_env())?;
    result["replay_cruise"] = replay(cfg, s, c, out, "cruise", 1200);
    let mut s = state_at(
        1000.,
        Atmosphere::standard()
            .sample(Meters(1000.))
            .speed_of_sound
            .get()
            * 0.65
            * 1.000001,
        0.,
        0.,
    );
    result["replay_terminal_zero"] =
        replay(cfg, s, ControlInputs::neutral(), out, "terminal-zero", 1);
    s = state_at(
        1000.,
        Atmosphere::standard()
            .sample(Meters(1000.))
            .speed_of_sound
            .get()
            * 0.6499,
        -0.5,
        100.,
    );
    result["replay_domain_dive"] = replay(
        cfg,
        s,
        ControlInputs::neutral().with_throttle(1.),
        out,
        "domain-dive",
        12000,
    );
    Ok(())
}
