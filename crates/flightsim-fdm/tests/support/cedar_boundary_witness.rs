//! Same-runtime Cedar law-1 boundary witness, shared only by tests/qualification.
//! Reconstruct the first two RK4 endpoints and third K2 through public force
//! queries, never through `step`, its private integrator, or law 2. This checks
//! integration/diagnostic plumbing independently, not the shared force laws.
use super::cedar_witness_fdm as fdm;
use fdm::turboprop::{self as law1, GovernorSample, TurbopropDerivative, TurbopropState};
use flightsim_core::{Ecef, RadiansPerSecond, RadiansPerSecondSquared, Seconds};
use glam::{DQuat, DVec4};

pub const SUBSTEPS: u32 = 6;

fn offset(
    config: &law1::TurbopropAircraftConfig,
    start: &TurbopropState,
    derivative: TurbopropDerivative,
    h: f64,
    controls: fdm::ControlInputs,
    governor: GovernorSample,
) -> TurbopropState {
    let body = &start.rigid_body;
    let d = derivative.rigid_body;
    let q = DVec4::from_array(body.orientation.to_array()) + d.orientation_rate * h;
    TurbopropState {
        rigid_body: fdm::RigidBodyState {
            position: Ecef(body.position.0 + d.velocity * h),
            velocity: body.velocity + d.acceleration * h,
            orientation: DQuat::from_array(q.to_array()).normalize(),
            angular_velocity: body.angular_velocity + d.angular_acceleration * h,
        },
        shaft_rad_s: RadiansPerSecond(
            start.shaft_rad_s.get() + derivative.shaft_acceleration.get() * h,
        ),
        turbine_fraction: config
            .turbine()
            .fraction_after(
                start.turbine_fraction,
                law1::TurbineFraction::new(controls.throttle()).unwrap(),
                Seconds(h),
            )
            .unwrap(),
        blade_pitch_rad: governor.pitch_after(Seconds(h)).unwrap(),
    }
}

fn diagnostic_bits(error: law1::TurbopropStepError) -> [Option<u64>; 9] {
    let d = error.diagnostics.values();
    [
        d.pressure_ratio.map(|v| v.0),
        d.temperature_ratio.map(|v| v.0),
        d.mach.map(|v| v.0),
        d.advance_ratio.map(|v| v.0),
        d.blade_pitch.map(|v| v.get()),
        d.relative_shaft.map(|v| v.get()),
        d.absolute_spin.map(|v| v.get()),
        d.tip_mach.map(|v| v.0),
        d.crossflow_ratio,
    ]
    .map(|v| v.map(f64::to_bits))
}

pub fn assert_rejection(
    config: &law1::TurbopropAircraftConfig,
    start: TurbopropState,
    controls: fdm::ControlInputs,
    environment: &fdm::Environment,
    actual: law1::TurbopropStepError,
    linux_reference_j: f64,
) {
    assert_eq!(
        (actual.stage, actual.substep),
        (law1::TurbopropStage::K2, 2)
    );
    assert!(matches!(actual.reason,
        law1::TurbopropFailureReason::OutsidePropellerDomain(d)
        if d.advance_ratio == law1::AxisStatus::Below && d.blade_pitch == law1::AxisStatus::Within));
    let model = law1::TurbopropFlightDynamics::new(config.clone(), start).unwrap();
    let h = (1.0 / 120.0) / f64::from(SUBSTEPS);
    let mut s = start;
    for _ in 0..2 {
        let governor = config
            .governor()
            .sample(s.shaft_rad_s, s.blade_pitch_rad)
            .unwrap();
        let k1 = model.derivative(&s, controls, environment).unwrap();
        let s2 = offset(config, &s, k1, h * 0.5, controls, governor);
        let k2 = model.derivative(&s2, controls, environment).unwrap();
        let s3 = offset(config, &s, k2, h * 0.5, controls, governor);
        let k3 = model.derivative(&s3, controls, environment).unwrap();
        let s4 = offset(config, &s, k3, h, controls, governor);
        let k4 = model.derivative(&s4, controls, environment).unwrap();
        let weighted = TurbopropDerivative {
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
        s = offset(config, &s, weighted, h, controls, governor);
        model.derivative(&s, controls, environment).unwrap();
    }
    let governor = config
        .governor()
        .sample(s.shaft_rad_s, s.blade_pitch_rad)
        .unwrap();
    let k1 = model.derivative(&s, controls, environment).unwrap();
    let rejected = offset(config, &s, k1, h * 0.5, controls, governor);
    let body = rejected.rigid_body;
    let velocity = body.orientation.inverse() * (body.velocity - environment.wind_ecef);
    let spin = rejected.shaft_rad_s.get()
        + config.propeller().rotation_sense().sign() * body.angular_velocity.x;
    let j = velocity.x / ((spin / std::f64::consts::TAU) * config.propeller().diameter().get());
    assert!(
        j.is_finite() && j < 0.0,
        "independently reconstructed signed J: {j}"
    );
    assert_eq!(
        actual
            .diagnostics
            .values()
            .advance_ratio
            .unwrap()
            .0
            .to_bits(),
        j.to_bits()
    );
    let direct = model
        .derivative(&rejected, controls, environment)
        .unwrap_err();
    assert_eq!(actual.reason, direct.reason);
    assert_eq!(diagnostic_bits(actual), diagnostic_bits(direct));
    // Historical derived-output regression for the platform that recorded it.
    // Every platform executes the independent bit-exact witness above; no
    // physical tolerance or negative-domain exception is introduced here.
    if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        assert_eq!(
            j.to_bits(),
            linux_reference_j.to_bits(),
            "Linux GNU reference J"
        );
    }
}
