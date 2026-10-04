//! Qualification only: production model steps, independently observed stages.
//! The observer never writes back into the model. Every reconstructed endpoint
//! must match all 16 production state words before its stage evidence is used.
#[allow(dead_code)]
#[path = "cedar_law1_baseline.rs"]
pub mod baseline;

use flightsim_core::{
    Ecef, LocalFrame, Radians, RadiansPerSecond, RadiansPerSecondSquared, Seconds,
};
use flightsim_fdm as cedar_witness_fdm;
use flightsim_fdm::turboprop::{self as law1, near_static as law2};
use flightsim_fdm::{ControlInputs, Environment, RigidBodyState};
#[path = "../../../flightsim-fdm/tests/support/cedar_boundary_witness.rs"]
mod cedar_boundary_witness;
use glam::{DQuat, DVec3};
use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};
use std::f64::consts::{PI, TAU};

pub const BRAKE_RELEASE_S: f64 = 20.0;
pub const DURATION_S: u32 = 30;

#[derive(Deserialize)]
struct ExactCell {
    ct: Box<RawValue>,
    cp: Box<RawValue>,
}

pub fn configuration() -> law2::TurbopropAircraftConfig {
    let p = baseline::profile();
    let original = p.configuration();
    let identity = flightsim_sim::model_identity::ModelIdentity::for_turboprop(original);
    assert_eq!(identity.fingerprint, 0xf83d_082e_8148_71c0);
    let rows: [Vec<ExactCell>; 2] = serde_json::from_str(include_str!(
        "../../tests/fixtures/cedar-law2-negative-rows.json"
    ))
    .unwrap();
    let rows = rows.map(|r| {
        r.into_iter()
            .map(|c| law1::PropellerCellDefinition {
                ct: c.ct.get().parse().unwrap(),
                cp: c.cp.get().parse().unwrap(),
            })
            .collect()
    });
    let result = law2::TurbopropAircraftConfig::from_forward(original.clone(), rows).unwrap();
    assert_eq!(
        flightsim_sim::turboprop_identity::canonical_turboprop_bytes(original),
        flightsim_sim::turboprop_identity::canonical_turboprop_bytes(result.forward_config()),
    );
    result
}

pub fn braking_controls(time: f64, _: &law1::TurbopropState) -> ControlInputs {
    // Identical calm full-brake/idle command through 20 s, then brake release
    // only. Renewed acceleration is not bought by changing power or surfaces.
    ControlInputs::neutral().with_brakes(if time < BRAKE_RELEASE_S { 1.0 } else { 0.0 })
}

pub fn boundary_state() -> law1::TurbopropState {
    let f: Value = serde_json::from_str(include_str!(
        "../../../flightsim-fdm/tests/fixtures/cedar-law1-calm-braking-boundary.json"
    ))
    .unwrap();
    let a: Vec<_> = f["last_committed_state"]["canonical_state_bits_hex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap()))
        .collect();
    law1::TurbopropState {
        rigid_body: RigidBodyState {
            position: Ecef(DVec3::new(a[0], a[1], a[2])),
            velocity: DVec3::new(a[3], a[4], a[5]),
            orientation: DQuat::from_xyzw(a[6], a[7], a[8], a[9]),
            angular_velocity: DVec3::new(a[10], a[11], a[12]),
        },
        turbine_fraction: law1::TurbineFraction::new(a[13]).unwrap(),
        shaft_rad_s: RadiansPerSecond(a[14]),
        blade_pitch_rad: Radians(a[15]),
    }
}

pub fn boundary_witness(config: &law2::TurbopropAircraftConfig) -> Value {
    let start = boundary_state();
    let controls = ControlInputs::neutral().with_brakes(1.0);
    let env = Environment::still_air();
    let mut old =
        law1::TurbopropFlightDynamics::new(config.forward_config().clone(), start).unwrap();
    let error = old.step(Seconds(1.0 / 120.0), controls, &env).unwrap_err();
    assert_eq!(error.stage, law1::TurbopropStage::K2);
    assert_eq!(error.substep, 2);
    assert!(
        matches!(error.reason, law1::TurbopropFailureReason::OutsidePropellerDomain(d)
        if d.advance_ratio == law1::AxisStatus::Below)
    );
    cedar_boundary_witness::assert_rejection(
        old.config(),
        start,
        controls,
        &env,
        error,
        -4.878_198_680_520_810_5e-6,
    );
    assert_eq!(
        baseline::state_bits(old.state()),
        baseline::state_bits(&start)
    );
    let mut new = law2::TurbopropFlightDynamics::new(config.clone(), start).unwrap();
    let accepted = new.step(Seconds(1.0 / 120.0), controls, &env).unwrap();
    assert_eq!(accepted.substeps, cedar_boundary_witness::SUBSTEPS);
    json!({"law1_error":format!("{error:?}"), "law1_rollback_all_16_words":true,
        "initial":baseline::state_json(&start), "law2_substeps":accepted.substeps,
        "law2_endpoint":baseline::state_json(new.state())})
}

fn offset(
    config: &law2::TurbopropAircraftConfig,
    start: &law1::TurbopropState,
    d: law1::TurbopropDerivative,
    h: f64,
    c: ControlInputs,
    g: law1::GovernorSample,
) -> law1::TurbopropState {
    law1::TurbopropState {
        rigid_body: flightsim_fdm::state::offset(&start.rigid_body, &d.rigid_body, h),
        shaft_rad_s: RadiansPerSecond(start.shaft_rad_s.get() + d.shaft_acceleration.get() * h),
        turbine_fraction: config
            .turbine()
            .fraction_after(
                start.turbine_fraction,
                law1::TurbineFraction::new(c.throttle()).unwrap(),
                Seconds(h),
            )
            .unwrap(),
        blade_pitch_rad: g.pitch_after(Seconds(h)).unwrap(),
    }
}

/// Reconstruct the states queried by the production RK4 driver, using its
/// reported subdivision count. Production acceptance is the authority; this
/// observer does not replace phase/domain guards or establish correctness alone.
fn observe_stages(
    model: &law2::TurbopropFlightDynamics,
    start: law1::TurbopropState,
    dt: Seconds,
    substeps: u32,
    controls: ControlInputs,
    env: &Environment,
    mut observe: impl FnMut(&law1::TurbopropState, u32, &str, f64),
) -> law1::TurbopropState {
    observe(&start, 0, "Initial", 0.0);
    let mut s = start;
    let h = dt.get() / f64::from(substeps);
    for i in 0..substeps {
        let t = f64::from(i) * h;
        let governor = model
            .config()
            .governor()
            .sample(s.shaft_rad_s, s.blade_pitch_rad)
            .unwrap();
        observe(&s, i, "K1", t);
        let k1 = model.derivative(&s, controls, env).unwrap();
        let s2 = offset(model.config(), &s, k1, h * 0.5, controls, governor);
        observe(&s2, i, "K2", t + h * 0.5);
        let k2 = model.derivative(&s2, controls, env).unwrap();
        let s3 = offset(model.config(), &s, k2, h * 0.5, controls, governor);
        observe(&s3, i, "K3", t + h * 0.5);
        let k3 = model.derivative(&s3, controls, env).unwrap();
        let s4 = offset(model.config(), &s, k3, h, controls, governor);
        observe(&s4, i, "K4", t + h);
        let k4 = model.derivative(&s4, controls, env).unwrap();
        let weighted = law1::TurbopropDerivative {
            rigid_body: (k1.rigid_body + k2.rigid_body * 2.0 + k3.rigid_body * 2.0 + k4.rigid_body)
                * (1.0 / 6.0),
            shaft_acceleration: RadiansPerSecondSquared(
                (k1.shaft_acceleration.get()
                    + 2.0 * k2.shaft_acceleration.get()
                    + 2.0 * k3.shaft_acceleration.get()
                    + k4.shaft_acceleration.get())
                    * (1.0 / 6.0),
            ),
        };
        s = offset(model.config(), &s, weighted, h, controls, governor);
        observe(&s, i, "Endpoint", t + h);
    }
    s
}

fn flow(
    config: &law2::TurbopropAircraftConfig,
    s: &law1::TurbopropState,
    env: &Environment,
) -> Value {
    let v = s.rigid_body.orientation.inverse() * (s.rigid_body.velocity - env.wind_ecef);
    let absolute = s.shaft_rad_s.get()
        + config.propeller().rotation_sense().sign() * s.rigid_body.angular_velocity.x;
    let diameter = config.propeller().diameter().get();
    let j = v.x / ((absolute / TAU) * diameter);
    let density = env.atmosphere.sample(s.rigid_body.altitude()).density;
    let load = config
        .propeller()
        .sample(
            law1::PropellerQuery {
                advance_ratio: law1::AdvanceRatio(j),
                blade_pitch: s.blade_pitch_rad,
            },
            density,
            RadiansPerSecond(absolute),
        )
        .unwrap();
    let hover =
        (load.thrust.get() / (2.0 * density.get() * (PI * diameter * diameter / 4.0))).sqrt();
    json!({"body_air_velocity_mps":v.to_array(), "signed_j":j,
        "hover_velocity_mps":if hover.is_finite(){Some(hover)}else{None},
        "signed_adverse_ratio":if hover.is_finite(){Some(-v.x / hover)}else{None},
        "transverse_ratio":if hover.is_finite(){Some(v.y.hypot(v.z) / hover)}else{None},
        "thrust_n":load.thrust.get(), "absorbed_power_w":load.absorbed_power.get(),
        "load_torque_nm":load.load_torque.get(), "signed_thrust_work_w":load.thrust.get() * v.x,
        "relative_rpm":s.shaft_rad_s.get() * 60.0 / TAU, "absolute_spin_rad_s":absolute,
        "blade_pitch_rad":s.blade_pitch_rad.get()})
}

fn swept_clearance(s: &law1::TurbopropState, env: &Environment, disk: f64) -> f64 {
    let frame = LocalFrame::new(
        env.ground_reference()
            .unwrap_or_else(|| s.rigid_body.geodetic()),
    );
    let slope = env.ground_slope();
    let normal = frame.ned_to_ecef_vector(flightsim_core::Ned(
        DVec3::new(-slope.north(), -slope.east(), -1.0).normalize(),
    ));
    // The original static-mesh envelope, not a variable-pitch blade model.
    disk - 0.055 * (s.rigid_body.orientation * DVec3::X).dot(normal).abs()
}

pub fn sample(
    model: &law2::TurbopropFlightDynamics,
    s: &law1::TurbopropState,
    controls: ControlInputs,
    env: &Environment,
    time: f64,
) -> Value {
    let config = model.config();
    let b = &s.rigid_body;
    let d = model.derivative(s, controls, env).unwrap();
    let air = env.atmosphere.sample(b.altitude());
    let relative = b.orientation.inverse() * (b.velocity - env.wind_ecef);
    let coefficients = config
        .aero()
        .sample(law1::MachNumber(
            relative.length() / air.speed_of_sound.get(),
        ))
        .unwrap()
        .coefficients;
    let (aero_force, _) = flightsim_fdm::aero::body_force_and_moment(
        &coefficients,
        config.airframe().geometry(),
        flightsim_fdm::aero_angles(relative),
        b.angular_velocity,
        controls,
        air.density,
    );
    let gravity =
        flightsim_fdm::gravity::acceleration_ecef(b.geodetic(), &LocalFrame::new(b.geodetic()));
    let nongravity = b.orientation.inverse()
        * (d.rigid_body.acceleration - gravity)
        * config.airframe().mass_properties().mass().get();
    let flow = flow(config, s, env);
    // Residual extraction uses accepted aero/gravity/propeller evaluations.
    // It avoids changing or copying the private contact implementation.
    let contact = nongravity - aero_force - DVec3::X * flow["thrust_n"].as_f64().unwrap();
    let disk = baseline::disk_clearance(s, env);
    json!({"time_s":time, "state":baseline::state_json(s), "flow":flow,
        "controls":baseline::controls_json(controls), "contact_force_body_n":contact.to_array(),
        "nongravity_force_body_n":nongravity.to_array(), "aero_force_body_n":aero_force.to_array(),
        "gear_clearances_m":model.gear_clearances(s, env).unwrap().map(|x| x.get()),
        "disk_clearance_m":disk,"static_blade_swept_clearance_m":swept_clearance(s, env, disk)})
}

pub fn run(
    config: &law2::TurbopropAircraftConfig,
    initial: law1::TurbopropState,
    env: &Environment,
    seconds: u32,
    hz: u32,
    stages: bool,
    controls: impl Fn(f64, &law1::TurbopropState) -> ControlInputs,
) -> Value {
    assert!([120, 240, 480, 960].contains(&hz));
    let dt = Seconds(1.0 / f64::from(hz));
    let mut model = law2::TurbopropFlightDynamics::new(config.clone(), initial).unwrap();
    let mut trace = vec![sample(&model, &initial, controls(0.0, &initial), env, 0.0)];
    let mut negatives = Vec::new();
    let mut completed = 0;
    let mut failure = None;
    let mut minimum_clearance = baseline::disk_clearance(&initial, env);
    let mut minimum_swept_clearance = swept_clearance(&initial, env, minimum_clearance);
    let mut maximum_compression = 0.0_f64;
    let mut last_braked_10s_peak_speed = 0.0_f64;
    let mut last_braked_10s_distance = 0.0;
    let mut braked_window_steps = 0;
    let mut near_static_seconds = 0.0;
    let mut max_substeps = 0;
    let mut observed_evaluations = 0;
    let mut positive_parity_steps = 0;
    for tick in 0..seconds * hz {
        let time = f64::from(tick) / f64::from(hz);
        let c = controls(time, model.state());
        let before = *model.state();
        let report = match model.step(dt, c, env) {
            Ok(r) => r,
            Err(error) => {
                assert_eq!(
                    baseline::state_bits(model.state()),
                    baseline::state_bits(&before)
                );
                let v = error.diagnostics.values();
                failure = Some(json!({"time_s":time, "error":format!("{error:?}"),
                    "reason":format!("{:?}",error.reason), "substep":error.substep,
                    "stage":format!("{:?}",error.stage), "rollback_all_16_words":true,
                    "last_committed_state":baseline::state_json(&before),
                    "signed_j":v.advance_ratio.map(|x|x.0),
                    "adverse_ratio":v.adverse_inflow_ratio,
                    "transverse_ratio":v.transverse_inflow_ratio,
                    "hover_velocity_mps":v.hover_velocity.map(|x|x.get())}));
                break;
            }
        };
        completed += 1;
        max_substeps = max_substeps.max(report.substeps);
        if stages {
            let prior_negative_count = negatives.len();
            let shadow = observe_stages(
                &model,
                before,
                dt,
                report.substeps,
                c,
                env,
                |s, substep, stage, offset| {
                    observed_evaluations += 1;
                    let v = s.rigid_body.orientation.inverse()
                        * (s.rigid_body.velocity - env.wind_ecef);
                    if v.x < 0.0 {
                        let values = flow(config, s, env);
                        assert!(values["signed_j"].as_f64().unwrap() >= -0.01);
                        assert!(values["signed_adverse_ratio"].as_f64().unwrap() <= 0.10);
                        assert!(values["transverse_ratio"].as_f64().unwrap() <= 0.10);
                        negatives.push(json!({"tick":tick,"substep":substep,"stage":stage,
                            "time_s":time+offset,"flow":values}));
                    }
                },
            );
            assert_eq!(
                baseline::state_bits(&shadow),
                baseline::state_bits(model.state()),
                "observer drift at {hz} Hz, tick {tick}"
            );
            if negatives.len() == prior_negative_count {
                let mut old =
                    law1::TurbopropFlightDynamics::new(config.forward_config().clone(), before)
                        .unwrap();
                let old_report = old.step(dt, c, env).unwrap();
                assert_eq!(old_report, report);
                assert_eq!(
                    baseline::state_bits(old.state()),
                    baseline::state_bits(model.state())
                );
                positive_parity_steps += 1;
            }
        }
        let s = model.state();
        let elapsed = f64::from(completed) / f64::from(hz);
        if c.brakes() >= 1.0 && (10.0..BRAKE_RELEASE_S).contains(&time) {
            braked_window_steps += 1;
            last_braked_10s_peak_speed =
                last_braked_10s_peak_speed.max(s.rigid_body.velocity.length());
            last_braked_10s_distance += s.rigid_body.velocity.length() * dt.get();
        }
        if c.brakes() >= 1.0 && time < BRAKE_RELEASE_S && s.rigid_body.velocity.length() < 0.1 {
            near_static_seconds += dt.get();
        }
        for c in report.gear_clearances {
            maximum_compression = maximum_compression.max(-c.get());
        }
        let disk = baseline::disk_clearance(s, env);
        minimum_clearance = minimum_clearance.min(disk);
        minimum_swept_clearance = minimum_swept_clearance.min(swept_clearance(s, env, disk));
        if completed % (hz / 20) == 0 {
            trace.push(sample(&model, s, c, env, elapsed));
        }
    }
    json!({"hz":hz,"requested_duration_s":seconds,"completed_s":f64::from(completed)/f64::from(hz),
        "failure":failure,"initial":baseline::state_json(&initial),"final":baseline::state_json(model.state()),
        "minimum_disk_clearance_m":minimum_clearance,"maximum_gear_compression_m":maximum_compression,
        "minimum_static_blade_swept_clearance_m":minimum_swept_clearance,
        "maximum_substeps":max_substeps,"near_static_braked_seconds_below_0_1mps":near_static_seconds,
        "last_braked_10s_peak_speed_mps":last_braked_10s_peak_speed,
        "last_braked_10s_traveled_distance_m":last_braked_10s_distance,
        "braked_10_to_20s_observed_seconds":f64::from(braked_window_steps)/f64::from(hz),
        "stationary_parking_pass":braked_window_steps==10*hz && last_braked_10s_peak_speed<0.001,
        "stage_observer_all_endpoints_bit_exact":stages,"observed_evaluations":observed_evaluations,
        "positive_flow_bit_exact_step_count":positive_parity_steps,
        "negative_stage_evaluations":negatives,"samples_20hz":trace})
}

pub fn critical() -> Value {
    let config = configuration();
    let boundary = boundary_witness(&config);
    let mut runs = Vec::new();
    for hz in [120, 240, 480, 960] {
        let r = run(
            &config,
            baseline::start(1.6, 25.0),
            &Environment::still_air(),
            DURATION_S,
            hz,
            true,
            braking_controls,
        );
        let failed = !r["failure"].is_null();
        eprintln!(
            "Cedar critical {hz} Hz: completed {} s, failure {}",
            r["completed_s"], r["failure"]
        );
        runs.push(r);
        if failed {
            break;
        }
    }
    json!({"schema":1,"source_law2_commit":"9f6a9449226bc6be5fee8d471afcbff2ad4c71f6",
        "original_candidate_commit":"be5094dd49a88d5a3709c7435dca7bfca287e7df",
        "forward_canonical_bytes_equal":true,"old_identity":"f83d082e814871c0",
        "law2_identity_allocation":null,"brake_release_s":BRAKE_RELEASE_S,
        "controls_note":"Neutral idle and maximum brakes through 20 s; brake release only thereafter",
        "sampling_note":"Every accepted step endpoint and every reconstructed Initial/K1/K2/K3/K4/Endpoint query; all shadow endpoints match production bits. Histories at 20 Hz; all negative-stage flow values retained.",
        "force_note":"Contact force is recovered from accepted nongravity force minus independently reevaluated aero and propeller loads; it is a diagnostic residual, not a new contact implementation.",
        "boundary_witness":boundary,"critical_runs":runs})
}

/// Original candidate flight/contact matrix, with its original scripted inputs.
/// Call only after all four unchanged critical trajectories complete.
pub fn matrix() -> Value {
    use flightsim_core::{Geodetic, Meters, Ned};
    use flightsim_fdm::{Atmosphere, GroundSlope};
    let config = configuration();
    let env = Environment::still_air();
    let ground = baseline::start(1.6, 0.0);
    let mut cases = Vec::new();
    for (speed, gamma, flaps) in [
        (38.0, 0.0, 0.5),
        (40.0, 0.0, 0.0),
        (50.0, 0.0, 0.0),
        (60.0, 0.0, 0.0),
        (45.0, 0.06, 0.5),
    ] {
        let t = baseline::trim(config.forward_config(), 500.0, speed, gamma, flaps, &env).unwrap();
        let held = run(&config, t.state, &env, 60, 120, true, |_, _| t.controls);
        let release = run(&config, t.state, &env, 10, 120, true, |_, _| {
            t.controls.with_aileron(0.0).with_rudder(0.0)
        });
        cases.push(json!({"case":format!("trim_{speed}_{gamma}_{flaps}"),"result":held}));
        cases.push(
            json!({"case":format!("roll_rudder_release_{speed}_{gamma}_{flaps}"),"result":release}),
        );
    }
    let idle = |_: f64, _: &law1::TurbopropState| ControlInputs::neutral().with_brakes(1.0);
    let wind = Environment::with_wind_ned(
        Atmosphere::standard(),
        Geodetic::from_degrees(35.0, 139.0, 1.6),
        Ned::new(-5.0, 2.0, 0.0),
    );
    let slope = wind.with_ground_plane(
        Geodetic::from_degrees(35.0, 139.0, 0.0),
        Meters(0.0),
        GroundSlope::new(0.02, 0.01),
    );
    for (name, environment) in [
        ("ground_idle", &env),
        ("ground_head_crosswind", &wind),
        ("sloped_ground", &slope),
    ] {
        cases.push(json!({"case":name,"result":run(&config,ground,environment,20,120,true,idle)}));
    }
    cases.push(
        json!({"case":"takeoff","result":run(&config,ground,&env,40,120,true,|time,_|{
            if time<17.0 {
                ControlInputs::new(0.055,if time>12.0{0.15}else{0.0},0.004,1.0,0.5)
            } else {ControlInputs::new(0.035,0.043,0.0026,0.70,0.5)}
        })}),
    );
    cases.push(
        json!({"case":"ground_power_cycle","result":run(&config,ground,&env,70,120,true,|time,_|
        ControlInputs::neutral().with_brakes(1.0).with_throttle(if time<30.0{1.0}else{0.0}))}),
    );
    cases.push(json!({"case":"braking_headwind","result":run(&config,baseline::start(1.6,25.0),&wind,10,120,true,idle)}));
    for sink in [1.0, 2.0, 3.0] {
        let t = baseline::trim(config.forward_config(), 2.5, 38.0, 0.0, 0.5, &env).unwrap();
        let mut s = t.state;
        s.rigid_body.velocity = s
            .rigid_body
            .local_frame()
            .ned_to_ecef_vector(Ned::new(38.0, 0.0, sink));
        cases.push(json!({"case":format!("touchdown_{sink}"),"result":run(&config,s,&env,8,120,true,|time,_|
            t.controls.with_throttle(0.0).with_elevator(0.0).with_brakes(if time>2.0{0.6}else{0.0}))}));
    }
    let t = baseline::trim(config.forward_config(), 500.0, 50.0, 0.0, 0.0, &env).unwrap();
    cases.push(json!({"case":"flight_power_cycle","result":run(&config,t.state,&env,60,120,true,|time,_|
        t.controls.with_throttle(if(10.0..30.0).contains(&time){t.controls.throttle()+0.10}else{t.controls.throttle()}))}));
    let stalled = baseline::state(500.0, 35.0, 0.36, 0.0, 0.0, [0.35, 180.0, 0.4]);
    cases.push(json!({"case":"stall_recovery","result":run(&config,stalled,&env,20,120,true,|time,_|
        ControlInputs::new(0.035,if time<0.5{-0.02}else if time<8.0{0.12}else{0.06},0.0026,0.6,0.5))}));
    json!({"schema":1,"cases":cases,"original_scripts_preserved":true,
        "qualification":"Original authored numerical approximation; no measured-aircraft or full-envelope claim"})
}
