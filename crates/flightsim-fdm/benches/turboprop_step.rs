//! Original numerical fixtures only; not an aircraft performance benchmark.
use criterion::{Criterion, criterion_group, criterion_main};
use flightsim_core::{Attitude, Geodetic, Meters, Ned, Radians, RadiansPerSecond, Seconds};
use flightsim_fdm::{
    ControlInputs, Environment, RigidBodyState,
    subsonic::{AirframeDefinition, MachAeroDefinition, MachAeroSchedule},
    turboprop::*,
};
use serde::Deserialize;
use std::hint::black_box;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    airframe: AirframeDefinition,
    turbine: TurbineDefinition,
    propeller: PropellerDefinition,
    governor: GovernorDefinition,
    envelope: TurbopropEnvelopeDefinition,
    aero: MachAeroDefinition,
}
fn config() -> TurbopropAircraftConfig {
    let f: Fixture =
        serde_json::from_str(include_str!("../tests/fixtures/turboprop-numerical.json")).unwrap();
    TurbopropAircraftConfig::new(
        f.airframe.to_config().unwrap(),
        TurbinePowerTable::from_definition(f.turbine).unwrap(),
        PropellerMap::from_definition(f.propeller).unwrap(),
        SampledGovernor::from_definition(f.governor).unwrap(),
        MachAeroSchedule::from_definition(f.aero).unwrap(),
        TurbopropEnvelope::from_definition(f.envelope).unwrap(),
    )
    .unwrap()
}
fn start(altitude: f64, speed: f64) -> TurbopropState {
    TurbopropState {
        rigid_body: RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, altitude),
            Attitude::default(),
            Ned::new(speed, 0.0, 0.0),
        ),
        turbine_fraction: TurbineFraction::new(0.4).unwrap(),
        shaft_rad_s: RadiansPerSecond(180.0),
        blade_pitch_rad: Radians(0.3),
    }
}
fn benchmarks(c: &mut Criterion) {
    let config = config();
    let controls = ControlInputs::neutral().with_throttle(0.7);
    let mut group = c.benchmark_group("turboprop_step");
    group.sample_size(30);
    for (name, state, dt, environment) in [
        (
            "airborne_1_120",
            start(1000.0, 40.0),
            Seconds(1.0 / 120.0),
            Environment::still_air(),
        ),
        (
            "contact_1_120",
            start(1.15, 0.0),
            Seconds(1.0 / 120.0),
            Environment::still_air().with_ground(Meters::ZERO),
        ),
        (
            "maximum_duration_8_120",
            start(1000.0, 40.0),
            MAX_TURBOPROP_STEP_DT,
            Environment::still_air(),
        ),
    ] {
        let mut probe = TurbopropFlightDynamics::new(config.clone(), state).unwrap();
        let report = probe.step(dt, controls, &environment).unwrap();
        println!(
            "{name}: {} accepted substeps, {} force evaluations",
            report.substeps,
            1 + 5 * report.substeps
        );
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || TurbopropFlightDynamics::new(config.clone(), state).unwrap(),
                |fdm| {
                    black_box(
                        fdm.step(black_box(dt), black_box(controls), black_box(&environment))
                            .unwrap(),
                    )
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}
criterion_group!(benches, benchmarks);
criterion_main!(benches);
