//! Original numerical fixtures. None are measured engine/aircraft data.
use super::*;
use crate::{
    ControlInputs, Environment, RigidBodyState,
    definition::{AerodynamicDefinition, GearDefinition},
    subsonic::{AirframeDefinition, MachAeroDefinition, MachAeroKnotDefinition, MachAeroSchedule},
};
use flightsim_core::{
    Attitude, Ecef, Geodetic, KilogramsPerCubicMeter, Ned, Radians, RadiansPerSecond, Seconds,
};
use glam::{DQuat, DVec3};
use std::f64::consts::PI;

fn close(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol,
        "{a:.17e} != {b:.17e}; error {}",
        (a - b).abs()
    );
}
fn power_definition() -> TurbineDefinition {
    TurbineDefinition {
        schema: 1,
        pressure_ratios: vec![0.1, 2.0],
        temperature_ratios: vec![0.25, 2.0],
        cells: vec![
            PowerCellDefinition {
                idle_w: 30_000.0,
                maximum_w: 250_000.0
            };
            4
        ],
        rise_seconds: 1.0,
        fall_seconds: 2.0,
        output_torque_limit_nm: 5000.0,
    }
}
fn propeller_definition() -> PropellerDefinition {
    PropellerDefinition {
        schema: 1,
        convention: PropellerConvention::IsolatedAxialPropeller,
        diameter_m: 2.0,
        rotor_axial_inertia_kg_m2: 20.0,
        rotation_sense: 1,
        advance_ratio: vec![0.0, 2.0],
        blade_pitch_rad: vec![0.1, 0.7],
        cells: vec![
            PropellerCellDefinition { ct: 0.01, cp: 0.03 },
            PropellerCellDefinition { ct: 0.01, cp: 0.3 },
            PropellerCellDefinition { ct: 0.01, cp: 0.03 },
            PropellerCellDefinition { ct: 0.01, cp: 0.3 },
        ],
    }
}
fn governor_definition() -> GovernorDefinition {
    GovernorDefinition {
        schema: 1,
        reference_rad_s: 180.0,
        gain: 0.001,
        fine_rate_rad_s: 0.05,
        coarse_rate_rad_s: 0.05,
        minimum_pitch_rad: 0.1,
        maximum_pitch_rad: 0.7,
    }
}
fn envelope_definition() -> TurbopropEnvelopeDefinition {
    TurbopropEnvelopeDefinition {
        pressure_ratio: [0.1, 2.0],
        temperature_ratio: [0.25, 2.0],
        mach: [0.0, 0.8],
        relative_shaft_rad_s: [40.0, 260.0],
        absolute_spin_rad_s: [40.0, 260.0],
        maximum_helical_tip_mach: 0.8,
        maximum_crossflow_tip_ratio: 0.1,
    }
}
fn airframe_definition() -> AirframeDefinition {
    let gear = |x, y| GearDefinition {
        contact_m: [x, y, 1.2],
        spring_n_per_m: 90_000.0,
        damping_ns_per_m: 9000.0,
        max_stroke_m: 0.3,
        bottom_stop_travel_m: 0.05,
        max_recoil_mps: 0.5,
    };
    AirframeDefinition {
        name: "Original turboprop numerical fixture".into(),
        mass_kg: 2000.0,
        inertia_kg_m2: [3000.0, 5000.0, 7000.0, 0.0],
        wing_area_m2: 20.0,
        wing_span_m: 12.0,
        mean_chord_m: 1.6,
        landing_gear: [gear(2.0, 0.0), gear(-1.0, -1.5), gear(-1.0, 1.5)],
        rolling_friction: 0.02,
        braking_friction: 0.7,
        lateral_friction: 0.8,
        friction_transition_mps: 0.3,
    }
}
fn schedule() -> MachAeroSchedule {
    let aero = AerodynamicDefinition {
        lift_zero: 0.0,
        lift_alpha: 0.0,
        lift_flaps: 0.0,
        stall_angle_rad: 0.3,
        stall_blend_rate: 20.0,
        drag_min: 0.01,
        oswald_efficiency: 0.8,
        drag_flaps: 0.0,
        side_beta: 0.0,
        side_rudder: 0.0,
        roll_beta: 0.0,
        roll_rate_p: 0.0,
        roll_rate_r: 0.0,
        roll_aileron: 0.0,
        roll_rudder: 0.0,
        pitch_zero: 0.0,
        pitch_alpha: 0.0,
        pitch_rate_q: 0.0,
        pitch_elevator: 0.0,
        pitch_flaps: 0.0,
        yaw_beta: 0.0,
        yaw_rate_p: 0.0,
        yaw_rate_r: 0.0,
        yaw_aileron: 0.0,
        yaw_rudder: 0.0,
    };
    MachAeroSchedule::from_definition(MachAeroDefinition {
        schema: 1,
        knots: vec![
            MachAeroKnotDefinition { mach: 0.0, aero },
            MachAeroKnotDefinition { mach: 0.8, aero },
        ],
    })
    .unwrap()
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "fixture builder consistently consumes its independent definitions"
)]
fn config_from(
    a: AirframeDefinition,
    p: TurbineDefinition,
    r: PropellerDefinition,
    g: GovernorDefinition,
    e: TurbopropEnvelopeDefinition,
) -> Result<TurbopropAircraftConfig, TurbopropConfigError> {
    TurbopropAircraftConfig::new(
        a.to_config().unwrap(),
        TurbinePowerTable::from_definition(p)?,
        PropellerMap::from_definition(r)?,
        SampledGovernor::from_definition(g)?,
        schedule(),
        TurbopropEnvelope::from_definition(e)?,
    )
}
fn config() -> TurbopropAircraftConfig {
    config_from(
        airframe_definition(),
        power_definition(),
        propeller_definition(),
        governor_definition(),
        envelope_definition(),
    )
    .unwrap()
}
fn level(height: f64, speed: f64) -> TurbopropState {
    TurbopropState {
        rigid_body: RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, height),
            Attitude::default(),
            Ned::new(speed, 0.0, 0.0),
        ),
        turbine_fraction: TurbineFraction::new(0.4).unwrap(),
        shaft_rad_s: RadiansPerSecond(180.0),
        blade_pitch_rad: Radians(0.3),
    }
}
fn state_bits(s: &TurbopropState) -> [u64; 16] {
    let b = s.rigid_body;
    let p = b.position.as_vec();
    let v = b.velocity;
    let q = b.orientation;
    let w = b.angular_velocity;
    [
        p.x,
        p.y,
        p.z,
        v.x,
        v.y,
        v.z,
        q.x,
        q.y,
        q.z,
        q.w,
        w.x,
        w.y,
        w.z,
        s.turbine_fraction.get(),
        s.shaft_rad_s.get(),
        s.blade_pitch_rad.get(),
    ]
    .map(f64::to_bits)
}
fn query(j: f64, pitch: f64) -> PropellerQuery {
    PropellerQuery {
        advance_ratio: AdvanceRatio(j),
        blade_pitch: Radians(pitch),
    }
}
fn condition() -> PowerConditions {
    PowerConditions {
        pressure_ratio: PressureRatio(1.0),
        temperature_ratio: TemperatureRatio(1.0),
    }
}
fn cells_at(ct: f64, cp: f64) -> PropellerDefinition {
    let mut d = propeller_definition();
    d.advance_ratio = vec![0.0, 0.5];
    d.cells = vec![PropellerCellDefinition { ct, cp }; 4];
    d
}

#[test]
fn dimensional_rps_static_forward_and_signed_load_oracles() {
    let map = PropellerMap::from_definition(cells_at(0.1, 0.08)).unwrap();
    for j in [0.0, 0.5] {
        let sample = map
            .sample(
                query(j, 0.2),
                KilogramsPerCubicMeter(1.0),
                RadiansPerSecond(50.0 * PI),
            )
            .unwrap();
        close(sample.thrust.get(), 1000.0, 1e-10);
        close(sample.absorbed_power.get(), 40_000.0, 1e-9);
        close(sample.load_torque.get(), 254.64790894703253, 1e-11);
        close(
            j * sample.coefficients.ct / sample.coefficients.cp,
            if j == 0.0 { 0.0 } else { 0.625 },
            1e-14,
        );
    }
    let mut braking = cells_at(-0.1, 0.0);
    braking.advance_ratio = vec![0.0, 1.0];
    braking.cells[2].cp = -0.08;
    braking.cells[3].cp = -0.08;
    let load = PropellerMap::from_definition(braking)
        .unwrap()
        .sample(
            query(1.0, 0.3),
            KilogramsPerCubicMeter(1.0),
            RadiansPerSecond(50.0 * PI),
        )
        .unwrap();
    close(load.thrust.get(), -1000.0, 1e-10);
    close(load.absorbed_power.get(), -40_000.0, 1e-9);
    assert!(load.load_torque.get() < 0.0);
}
#[test]
fn paired_bilinear_asymmetric_map_and_exact_knots() {
    let mut d = propeller_definition();
    d.advance_ratio = vec![0.0, 0.2, 1.1];
    d.blade_pitch_rad = vec![0.1, 0.4, 0.7];
    // Independent simple surface with an interaction term; power has generous
    // induced-work margin. Different axis spacing detects index swaps.
    d.cells = d
        .advance_ratio
        .iter()
        .flat_map(|&j| {
            d.blade_pitch_rad
                .iter()
                .map(move |&b| PropellerCellDefinition {
                    ct: 0.01 + 0.002 * j + 0.003 * b + 0.001 * j * b,
                    cp: 0.1 + 0.05 * b + 0.01 * j,
                })
        })
        .collect();
    let map = PropellerMap::from_definition(d.clone()).unwrap();
    for j in [0.0, 0.15, 0.2, 0.65, 1.1] {
        for b in [0.1, 0.25, 0.4, 0.65, 0.7] {
            let c = map.coefficients(query(j, b)).unwrap();
            close(c.ct, 0.01 + 0.002 * j + 0.003 * b + 0.001 * j * b, 1e-16);
            close(c.cp, 0.1 + 0.05 * b + 0.01 * j, 1e-16);
        }
    }
    for (i, &j) in d.advance_ratio.iter().enumerate() {
        for (k, &b) in d.blade_pitch_rad.iter().enumerate() {
            let c = map.coefficients(query(j, b)).unwrap();
            let source = d.cells[i * 3 + k];
            assert_eq!(c.ct.to_bits(), source.ct.to_bits());
            assert_eq!(c.cp.to_bits(), source.cp.to_bits());
        }
    }
}
#[test]
fn continuous_weaker_inequality_catches_an_interior_only_violation() {
    // Endpoints g(0)=.15 and g(2)=0; midpoint g(1)=-.075.
    let mut d = propeller_definition();
    d.cells = vec![
        PropellerCellDefinition { ct: 0.3, cp: 0.15 },
        PropellerCellDefinition { ct: 0.3, cp: 0.15 },
        PropellerCellDefinition { ct: 0.0, cp: 0.0 },
        PropellerCellDefinition { ct: 0.0, cp: 0.0 },
    ];
    assert!(
        PropellerMap::from_definition(d)
            .unwrap_err()
            .0
            .contains("interior")
    );
}
#[test]
fn finite_disk_zero_power_tolerance_and_underflow_admission() {
    let ideal = 0.025231325220201606_f64; // independently derived static disk value
    for cp in [0.0, -0.0, ideal * 0.99] {
        assert!(PropellerMap::from_definition(cells_at(0.1, cp)).is_err());
    }
    // Use a tiny J range so the second node changes the ideal by < test margin.
    let at = |cp| {
        let mut d = cells_at(0.1, cp);
        d.advance_ratio = vec![0.0, 1e-9];
        d.cells[2].ct = 0.0;
        d.cells[3].ct = 0.0;
        d
    };
    for cp in [
        ideal,
        ideal * (1.0 + 32.0 * f64::EPSILON),
        ideal * (1.0 - 32.0 * f64::EPSILON),
    ] {
        assert!(PropellerMap::from_definition(at(cp)).is_ok(), "{cp}");
    }
    assert!(PropellerMap::from_definition(at(ideal * (1.0 - 128.0 * f64::EPSILON))).is_err());
    assert!(PropellerMap::from_definition(cells_at(f64::from_bits(1), f64::from_bits(1))).is_err());
    for cp in [0.0, -0.0] {
        assert!(PropellerMap::from_definition(cells_at(1e-100, cp)).is_err());
    }
}
fn interior_violation_map() -> PropellerDefinition {
    let mut d = propeller_definition();
    d.cells = vec![
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0.2, cp: 0.1 },
        PropellerCellDefinition { ct: 0.0, cp: 0.1 },
        PropellerCellDefinition { ct: 0.0, cp: 0.1 },
    ];
    d
}
#[test]
fn nodal_admission_does_not_remove_runtime_disk_bound() {
    let map = PropellerMap::from_definition(interior_violation_map()).unwrap();
    assert_eq!(
        map.coefficients(query(1.0, 0.3)).unwrap_err(),
        TurbopropEvaluationError::PropellerPowerBound(PropellerPowerBound::BelowIdealDisk)
    );
}
#[test]
fn authored_fixture_passes_required_17_by_17_disk_sampling() {
    let d = propeller_definition();
    let map = PropellerMap::from_definition(d.clone()).unwrap();
    let mut minimum = f64::INFINITY;
    let mut location = (0.0, 0.0);
    for i in 0..=16 {
        for k in 0..=16 {
            let j = 2.0 * f64::from(i) / 16.0;
            let b = 0.1 + 0.6 * f64::from(k) / 16.0;
            let c = map.coefficients(query(j, b)).unwrap();
            let ideal = c.ct * (j + (j * j + 8.0 * c.ct / PI).sqrt()) / 2.0;
            if c.cp - ideal < minimum {
                minimum = c.cp - ideal;
                location = (j, b);
            }
        }
    }
    assert!(minimum > 0.0099);
    println!("original fixture sampled disk minimum {minimum:.15e} at {location:?}");
    // Explicit near-zero and positive/negative transition fixture.
    let mut transition = cells_at(1e-30, 1e-10);
    transition.cells[2].ct = -1e-30;
    transition.cells[3].ct = -1e-30;
    let transition = PropellerMap::from_definition(transition).unwrap();
    for i in 0..=16 {
        for k in 0..=16 {
            transition
                .coefficients(query(
                    0.5 * f64::from(i) / 16.0,
                    0.1 + 0.6 * f64::from(k) / 16.0,
                ))
                .unwrap();
        }
    }
}
#[test]
fn map_rejects_bad_shapes_values_spacing_feedback_and_runtime_inputs() {
    let mut mutations = Vec::new();
    let d = propeller_definition();
    let mut x = d.clone();
    x.schema = 2;
    mutations.push(x);
    let mut x = d.clone();
    x.advance_ratio[0] = 0.1;
    mutations.push(x);
    let mut x = d.clone();
    x.advance_ratio[1] = 0.5e-9;
    mutations.push(x);
    let mut x = d.clone();
    x.blade_pitch_rad = vec![0.1; 33];
    mutations.push(x);
    let mut x = d.clone();
    x.cells.pop();
    mutations.push(x);
    let mut x = d.clone();
    x.cells[0].ct = f64::NAN;
    mutations.push(x);
    let mut x = d.clone();
    x.cells[0].cp = 0.5;
    mutations.push(x);
    let mut x = d.clone();
    x.rotation_sense = 0;
    mutations.push(x);
    for m in mutations {
        assert!(PropellerMap::from_definition(m).is_err());
    }
    let map = PropellerMap::from_definition(d).unwrap();
    for j in [
        f64::NAN,
        f64::INFINITY,
        -f64::from_bits(1),
        f64::from_bits(2.0_f64.to_bits() + 1),
    ] {
        assert!(map.coefficients(query(j, 0.3)).is_err());
    }
    for b in [
        f64::NAN,
        f64::from_bits(0.1_f64.to_bits() - 1),
        f64::from_bits(0.7_f64.to_bits() + 1),
    ] {
        assert!(map.coefficients(query(0.0, b)).is_err());
    }
    for s in [0.0, -1.0, f64::INFINITY] {
        assert!(
            map.sample(
                query(0.0, 0.3),
                KilogramsPerCubicMeter(1.0),
                RadiansPerSecond(s)
            )
            .is_err()
        );
    }
    for rho in [0.0, -1.0, f64::NAN] {
        assert!(
            map.sample(
                query(0.0, 0.3),
                KilogramsPerCubicMeter(rho),
                RadiansPerSecond(180.0)
            )
            .is_err()
        );
    }
}
#[test]
fn power_interpolation_cap_and_response_have_independent_oracles() {
    let mut d = power_definition();
    d.pressure_ratios = vec![0.1, 0.7, 2.0];
    d.temperature_ratios = vec![0.25, 1.0, 2.0];
    d.cells = d
        .pressure_ratios
        .iter()
        .flat_map(|&p| {
            d.temperature_ratios
                .iter()
                .map(move |&t| PowerCellDefinition {
                    idle_w: 1000.0 + 2000.0 * p + 3000.0 * t + 4000.0 * p * t,
                    maximum_w: 100_000.0 + 10_000.0 * p + 5000.0 * t + 2000.0 * p * t,
                })
        })
        .collect();
    d.output_torque_limit_nm = 500.0;
    let table = TurbinePowerTable::from_definition(d).unwrap();
    for (p, t) in [(0.1, 0.25), (0.4, 0.7), (0.7, 1.0), (1.3, 1.5), (2.0, 2.0)] {
        let fraction = TurbineFraction::new(0.4).unwrap();
        let c = PowerConditions {
            pressure_ratio: PressureRatio(p),
            temperature_ratio: TemperatureRatio(t),
        };
        let idle = 1000.0 + 2000.0 * p + 3000.0 * t + 4000.0 * p * t;
        let max = 100_000.0 + 10_000.0 * p + 5000.0 * t + 2000.0 * p * t;
        for rate in [60.0, 240.0] {
            let sample = table.sample(c, fraction, RadiansPerSecond(rate)).unwrap();
            close(sample.available.get(), 0.6 * idle + 0.4 * max, 1e-9);
            assert!(sample.drive_torque.get() <= 500.0);
            assert!(sample.delivered.get() <= sample.available.get() + 1e-10);
            close(
                sample.delivered.get(),
                sample.drive_torque.get() * rate,
                0.0,
            );
        }
    }
    let table = TurbinePowerTable::from_definition(power_definition()).unwrap();
    for (duration, expected) in [(1.0, 0.6321205588285577), (3.0, 0.950212931632136)] {
        close(
            table
                .fraction_after(
                    TurbineFraction::IDLE,
                    TurbineFraction::MAXIMUM,
                    Seconds(duration),
                )
                .unwrap()
                .get(),
            expected,
            1e-15,
        );
        close(
            table
                .fraction_after(
                    TurbineFraction::MAXIMUM,
                    TurbineFraction::IDLE,
                    Seconds(2.0 * duration),
                )
                .unwrap()
                .get(),
            1.0 - expected,
            1e-15,
        );
    }
    let negative_zero = TurbineFraction::new(-0.0).unwrap();
    assert_eq!(
        table
            .fraction_after(negative_zero, TurbineFraction::MAXIMUM, Seconds::ZERO)
            .unwrap()
            .get()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
    let mut x = TurbineFraction::IDLE;
    for _ in 0..120 {
        let next = table
            .fraction_after(x, TurbineFraction::MAXIMUM, Seconds(1.0 / 120.0))
            .unwrap();
        assert!(next.get() > x.get());
        x = next;
    }
    close(x.get(), 0.6321205588285577, 3e-14);
    for bad in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(TurbineFraction::new(bad).is_err());
    }
}
#[test]
fn governor_exact_rates_stops_release_and_zero_duration() {
    let g = SampledGovernor::from_definition(governor_definition()).unwrap();
    let coarse = g.sample(RadiansPerSecond(280.0), Radians(0.6)).unwrap();
    close(coarse.rate().get(), 0.05, 0.0);
    close(coarse.pitch_after(Seconds(1.0)).unwrap().get(), 0.65, 1e-15);
    close(coarse.pitch_after(Seconds(2.0)).unwrap().get(), 0.7, 0.0);
    close(coarse.pitch_after(Seconds(10.0)).unwrap().get(), 0.7, 0.0);
    let release = g.sample(RadiansPerSecond(170.0), Radians(0.7)).unwrap();
    close(release.rate().get(), -0.01, 1e-16);
    close(
        release.pitch_after(Seconds(1.0)).unwrap().get(),
        0.69,
        1e-15,
    );
    let fine = g.sample(RadiansPerSecond(80.0), Radians(0.2)).unwrap();
    close(fine.pitch_after(Seconds(2.0)).unwrap().get(), 0.1, 0.0);
    assert_eq!(
        fine.pitch_after(Seconds::ZERO).unwrap().get().to_bits(),
        0.2_f64.to_bits()
    );
    close(
        g.sample(RadiansPerSecond(180.0), Radians(0.3))
            .unwrap()
            .rate()
            .get(),
        0.0,
        0.0,
    );
}
#[test]
fn coupled_rotor_rational_oracle_and_energy_momentum_both_senses() {
    for sense in [-1, 1] {
        let mut a = airframe_definition();
        a.inertia_kg_m2 = [10.0, 12.0, 15.0, 2.0];
        let mut p = propeller_definition();
        p.rotor_axial_inertia_kg_m2 = 3.0;
        p.rotation_sense = sense;
        let config = config_from(
            a,
            power_definition(),
            p,
            governor_definition(),
            envelope_definition(),
        )
        .unwrap();
        let b = DVec3::new(0.2, -0.3, 0.4);
        let omega = 100.0;
        let moment = DVec3::new(1.0, 2.0, 3.0);
        let (alpha, wdot) = runtime::coupled_rotation(&config, b, omega, 50.0, 20.0, moment);
        let (expected, wexpected) = if sense == 1 {
            (
                DVec3::new(-22617.0 / 2525.0, -489.0 / 50.0, -704.0 / 101.0),
                47867.0 / 2525.0,
            )
        } else {
            (
                DVec3::new(23883.0 / 2525.0, 511.0 / 50.0, 756.0 / 101.0),
                49133.0 / 2525.0,
            )
        };
        assert!(alpha.distance(expected) < 1e-13);
        close(wdot, wexpected, 1e-13);
        let sign = f64::from(sense);
        let locked = config.airframe().mass_properties().inertia();
        let momentum = locked * b + DVec3::X * (sign * 3.0 * omega);
        let momentum_dot = locked * alpha + DVec3::X * (sign * 3.0 * wdot) + b.cross(momentum);
        assert!(momentum_dot.distance(moment - DVec3::X * (sign * 20.0)) < 1e-12);
        let energy_dot = b.dot(locked * alpha)
            + sign * 3.0 * (wdot * b.x + omega * alpha.x)
            + 3.0 * omega * wdot;
        close(energy_dot, if sense == 1 { 2996.8 } else { 3004.8 }, 1e-10);
        close(wdot + sign * alpha.x, 10.0, 1e-13);
        for (drive, load) in [(50.0, 0.0), (0.0, 20.0)] {
            let (alpha, wdot) =
                runtime::coupled_rotation(&config, DVec3::ZERO, 100.0, drive, load, DVec3::ZERO);
            let change = locked * alpha + DVec3::X * (sign * 3.0 * wdot);
            assert!(change.distance(-DVec3::X * (sign * load)) < 1e-12);
            if drive > 0.0 {
                assert!(sign * alpha.x < 0.0);
                assert!(wdot > 0.0);
            }
        }
    }
}
#[test]
fn rotor_subtraction_rechecks_full_tensor_and_tiny_determinant_without_panicking() {
    let mut a = airframe_definition();
    a.inertia_kg_m2 = [10.0, 12.0, 15.0, 10.0];
    let mut p = propeller_definition();
    p.rotor_axial_inertia_kg_m2 = 5.0;
    assert!(
        config_from(
            a,
            power_definition(),
            p,
            governor_definition(),
            envelope_definition()
        )
        .is_err()
    );
    let mut a = airframe_definition();
    a.inertia_kg_m2 = [1.0, 1.0, 1.0, 0.0];
    let mut p = propeller_definition();
    p.rotor_axial_inertia_kg_m2 = f64::from_bits(1.0_f64.to_bits() - 1);
    assert!(
        config_from(
            a,
            power_definition(),
            p,
            governor_definition(),
            envelope_definition()
        )
        .is_err()
    );
    let mut e = envelope_definition();
    e.maximum_helical_tip_mach = f64::from_bits(1);
    e.maximum_crossflow_tip_ratio = f64::from_bits(1);
    assert!(TurbopropEnvelope::from_definition(e).is_ok());
}
#[test]
fn zero_duration_preserves_every_bit_and_nonzero_step_changes_all_engine_states() {
    let mut s = level(1000.0, 40.0);
    s.shaft_rad_s = RadiansPerSecond(190.0);
    let mut model = TurbopropFlightDynamics::new(config(), s).unwrap();
    let controls = ControlInputs::neutral().with_throttle(0.8);
    assert_eq!(
        model
            .step(Seconds::ZERO, controls, &Environment::still_air())
            .unwrap()
            .substeps,
        0
    );
    assert_eq!(state_bits(model.state()), state_bits(&s));
    model
        .step(Seconds(1.0 / 120.0), controls, &Environment::still_air())
        .unwrap();
    let end = model.state();
    assert!(end.turbine_fraction.get() > s.turbine_fraction.get());
    assert!(end.blade_pitch_rad.get() > s.blade_pitch_rad.get());
    assert_ne!(
        end.shaft_rad_s.get().to_bits(),
        s.shaft_rad_s.get().to_bits()
    );
    close(end.blade_pitch_rad.get(), 0.3 + 0.01 / 120.0, 1e-15);
    close(
        end.turbine_fraction.get(),
        0.8 - 0.4 * (-1.0_f64 / 120.0).exp(),
        1e-15,
    );
}
#[test]
fn raw_engine_validation_precedes_altitude_and_state_replacement_is_atomic() {
    let original = level(1000.0, 40.0);
    let mut model = TurbopropFlightDynamics::new(config(), original).unwrap();
    for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut s = level(100_000.0, 40.0);
        s.shaft_rad_s = RadiansPerSecond(rate);
        assert_eq!(
            model.set_state(s).unwrap_err().reason,
            TurbopropFailureReason::InvalidInput(TurbopropInvalidInput::ShaftRate)
        );
        assert_eq!(state_bits(model.state()), state_bits(&original));
    }
    for pitch in [
        f64::NAN,
        f64::from_bits(0.1_f64.to_bits() - 1),
        f64::from_bits(0.7_f64.to_bits() + 1),
    ] {
        let mut s = original;
        s.blade_pitch_rad = Radians(pitch);
        assert!(model.set_state(s).is_err());
    }
}
#[test]
fn finite_out_of_profile_shaft_is_reported_before_j_division() {
    let mut s = level(1000.0, 40.0);
    s.shaft_rad_s = RadiansPerSecond(f64::from_bits(40.0_f64.to_bits() - 1));
    let mut model = TurbopropFlightDynamics::new(config(), s).unwrap();
    let e = model
        .step(
            Seconds::ZERO,
            ControlInputs::default(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert!(
        matches!(e.reason,TurbopropFailureReason::OutsideOperatingEnvelope(d) if d.relative_shaft==AxisStatus::Below)
    );
    assert!(e.diagnostics.values().advance_ratio.is_none());
    assert_eq!(state_bits(model.state()), state_bits(&s));
}
#[test]
fn runtime_interior_disk_failure_has_valid_diagnostics_and_complete_rollback() {
    let config = config_from(
        airframe_definition(),
        power_definition(),
        interior_violation_map(),
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    let s = level(1000.0, 180.0 / PI); // D=2, Omega=180 gives J=1 exactly up to transform roundoff
    let mut model = TurbopropFlightDynamics::new(config, s).unwrap();
    let e = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::default(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(
        e.reason,
        TurbopropFailureReason::PropellerPowerBound(PropellerPowerBound::BelowIdealDisk)
    );
    close(e.diagnostics.values().advance_ratio.unwrap().0, 1.0, 1e-14);
    assert!(e.diagnostics.values().blade_pitch.is_some());
    assert_eq!(state_bits(model.state()), state_bits(&s));
}
#[test]
fn crossflow_tip_absolute_spin_reverse_flow_and_duration_boundaries_reject_atomically() {
    let s = level(1000.0, 40.0);
    let mut model = TurbopropFlightDynamics::new(config(), s).unwrap();
    for dt in [
        Seconds(-1.0),
        Seconds(f64::NAN),
        Seconds(f64::from_bits(MAX_TURBOPROP_STEP_DT.get().to_bits() + 1)),
    ] {
        assert!(
            model
                .step(dt, ControlInputs::default(), &Environment::still_air())
                .is_err()
        );
        assert_eq!(state_bits(model.state()), state_bits(&s));
    }
    for (body_velocity, rate) in [
        (DVec3::new(40.0, 30.0, 0.0), 180.0),
        (DVec3::new(100.0, 0.0, 0.0), 260.0),
        (DVec3::new(-1.0, 0.0, 0.0), 180.0),
    ] {
        let mut candidate = s;
        candidate.rigid_body.velocity = candidate.rigid_body.orientation * body_velocity;
        candidate.shaft_rad_s = RadiansPerSecond(rate);
        let mut trial = TurbopropFlightDynamics::new(config(), candidate).unwrap();
        assert!(
            trial
                .step(
                    Seconds::ZERO,
                    ControlInputs::default(),
                    &Environment::still_air()
                )
                .is_err()
        );
        assert_eq!(state_bits(trial.state()), state_bits(&candidate));
    }
    let mut candidate = s;
    candidate.rigid_body.angular_velocity.x = -200.0;
    let mut trial = TurbopropFlightDynamics::new(config(), candidate).unwrap();
    assert!(
        matches!(trial.step(Seconds::ZERO,ControlInputs::default(),&Environment::still_air()).unwrap_err().reason,TurbopropFailureReason::OutsideOperatingEnvelope(d) if d.absolute_spin==AxisStatus::Below)
    );
}
#[test]
fn finite_guards_prevent_legacy_helpers_from_masking_bad_inputs() {
    let s = level(1000.0, 40.0);
    for wind in [DVec3::splat(f64::NAN), DVec3::splat(f64::MAX)] {
        let mut model = TurbopropFlightDynamics::new(config(), s).unwrap();
        let mut env = Environment::still_air();
        env.wind_ecef = wind;
        assert_eq!(
            model
                .step(Seconds::ZERO, ControlInputs::default(), &env)
                .unwrap_err()
                .reason,
            TurbopropFailureReason::InvalidInput(TurbopropInvalidInput::Wind)
        );
    }
    for mutate in [0, 1, 2, 3] {
        let mut bad = s;
        match mutate {
            0 => bad.rigid_body.velocity = DVec3::splat(f64::MAX),
            1 => bad.rigid_body.position = Ecef(DVec3::ZERO),
            2 => bad.rigid_body.orientation = DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0),
            _ => bad.rigid_body.angular_velocity = DVec3::splat(f64::MAX),
        }
        assert!(TurbopropFlightDynamics::new(config(), bad).is_err());
    }
}

#[test]
fn power_and_governor_invalid_boundaries_are_not_clamped() {
    let table = TurbinePowerTable::from_definition(power_definition()).unwrap();
    for r in [0.0, -1.0, f64::NAN] {
        assert!(
            table
                .sample(condition(), TurbineFraction::IDLE, RadiansPerSecond(r))
                .is_err()
        );
    }
    for p in [f64::NAN, 0.09, 2.01] {
        let mut c = condition();
        c.pressure_ratio = PressureRatio(p);
        assert!(
            table
                .sample(c, TurbineFraction::IDLE, RadiansPerSecond(180.0))
                .is_err()
        );
    }
    for t in [-1.0, f64::NAN] {
        assert!(
            table
                .fraction_after(TurbineFraction::IDLE, TurbineFraction::MAXIMUM, Seconds(t))
                .is_err()
        );
    }
    let mut d = power_definition();
    d.cells[0].idle_w = d.cells[0].maximum_w + 1.0;
    assert!(TurbinePowerTable::from_definition(d).is_err());
    let mut g = governor_definition();
    g.maximum_pitch_rad = g.minimum_pitch_rad;
    assert!(SampledGovernor::from_definition(g).is_err());
}

#[test]
fn stage_and_later_substep_failures_restore_the_entire_call() {
    let start = level(1000.0, 40.0);
    let controls = ControlInputs::neutral().with_throttle(1.0);
    for (maximum, dt, expected_stage, expected_substep) in [
        (180.01, 1.0 / 120.0, TurbopropStage::K2, 0),
        (180.0215, 1.0 / 120.0, TurbopropStage::K3, 0),
        (180.03, 1.0 / 120.0, TurbopropStage::K4, 0),
        (180.06, 1.0 / 60.0, TurbopropStage::K2, 1),
    ] {
        let mut e = envelope_definition();
        e.relative_shaft_rad_s[1] = maximum;
        let mut model = TurbopropFlightDynamics::new(
            config_from(
                airframe_definition(),
                power_definition(),
                propeller_definition(),
                governor_definition(),
                e,
            )
            .unwrap(),
            start,
        )
        .unwrap();
        let error = model
            .step(Seconds(dt), controls, &Environment::still_air())
            .unwrap_err();
        assert_eq!(error.stage, expected_stage);
        assert_eq!(error.substep, expected_substep);
        assert_eq!(state_bits(model.state()), state_bits(&start));
    }
    let mut model = TurbopropFlightDynamics::new(config(), start).unwrap();
    model
        .step(Seconds(1.0 / 120.0), controls, &Environment::still_air())
        .unwrap();
    println!("endpoint shaft {:.17}", model.state().shaft_rad_s.get());
}

// Independent scalar continuous-governor oracle for a restrained axial bench.
// It uses the authored fixture constants, not production propeller, power,
// governor, derivative or integration helpers. State order x,omega,beta,body-p.
fn bench_rhs(y: [f64; 4], command: f64, rho: f64, limit: f64) -> [f64; 4] {
    let [x, w, b, p] = y;
    let q = ((30_000.0 + 220_000.0 * x) / w).min(limit);
    let cp = 0.45 * b - 0.015;
    let n = (w + p) / (2.0 * PI);
    let load = cp * rho * n * n * 32.0 / (2.0 * PI);
    let pitch_rate = (0.001 * (w - 180.0)).clamp(-0.05, 0.05);
    [
        (command - x) / if command > x { 1.0 } else { 2.0 },
        (q - load) / 20.0 + q / 29980.0,
        if (b <= 0.1 && pitch_rate < 0.0) || (b >= 0.7 && pitch_rate > 0.0) {
            0.0
        } else {
            pitch_rate
        },
        -q / 29980.0,
    ]
}
fn oracle_bench(command: f64, rho: f64, limit: f64, steps: u32, dt: f64) -> [f64; 4] {
    let mut y = [0.4, 180.0, 0.3, 0.0];
    for _ in 0..steps {
        let shifted = |y: [f64; 4], k: [f64; 4], h: f64| std::array::from_fn(|i| y[i] + h * k[i]);
        let a = bench_rhs(y, command, rho, limit);
        let b = bench_rhs(shifted(y, a, dt / 2.0), command, rho, limit);
        let c = bench_rhs(shifted(y, b, dt / 2.0), command, rho, limit);
        let d = bench_rhs(shifted(y, c, dt), command, rho, limit);
        y = std::array::from_fn(|i| y[i] + dt * (a[i] + 2.0 * b[i] + 2.0 * c[i] + d[i]) / 6.0);
        y[2] = y[2].clamp(0.1, 0.7);
    }
    y
}
fn runtime_bench(
    command: f64,
    altitude: f64,
    speed: f64,
    limit: f64,
    hz: u32,
    seconds: u32,
) -> ([f64; 4], u32, Vec<f64>) {
    let mut p = power_definition();
    p.output_torque_limit_nm = limit;
    let mut frame = airframe_definition();
    frame.inertia_kg_m2[0] = 30_000.0;
    let cfg = config_from(
        frame,
        p,
        propeller_definition(),
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    let start = level(altitude, speed);
    let mut model = TurbopropFlightDynamics::new(cfg, start).unwrap();
    let control = ControlInputs::neutral().with_throttle(command);
    let dt = Seconds(1.0 / f64::from(hz));
    let mut maximum = 0;
    let mut samples = Vec::new();
    for i in 0..seconds * hz {
        // Restrain the test bench's translational operating point once per
        // sample. Engine states and roll dynamics are never reset. This is
        // numerical qualification, explicitly not an aircraft flight scenario.
        let mut state = *model.state();
        state.rigid_body.position = start.rigid_body.position;
        state.rigid_body.velocity = state.rigid_body.orientation * DVec3::new(speed, 0.0, 0.0);
        model.set_state(state).unwrap();
        let report = model
            .step(
                dt,
                control,
                &Environment::still_air().with_ground(flightsim_core::Meters(-100.0)),
            )
            .unwrap();
        maximum = maximum.max(report.substeps);
        if i % hz == hz - 1 {
            samples.push(model.state().shaft_rad_s.get());
        }
    }
    let s = model.state();
    (
        [
            s.turbine_fraction.get(),
            s.shaft_rad_s.get(),
            s.blade_pitch_rad.get(),
            s.rigid_body.angular_velocity.x,
        ],
        maximum,
        samples,
    )
}
#[test]
fn sampled_governor_converges_against_independent_finer_continuous_oracle() {
    for (command, altitude, speed, limit) in [
        (0.4, 0.0, 0.0, 5000.0),
        (0.7, 1000.0, 40.0, 5000.0),
        (1.0, 3000.0, 70.0, 1000.0),
        (0.0, 1000.0, 40.0, 5000.0),
    ] {
        // Independent ISA troposphere density, not Atmosphere::sample.
        let geopotential = 6_356_766.0 * altitude / (6_356_766.0 + altitude);
        let theta: f64 = 1.0 - 0.0065 * geopotential / 288.15;
        let rho =
            101325.0 / (287.052874 * 288.15) * theta.powf(9.80665 / (287.052874 * 0.0065) - 1.0);
        let reference = oracle_bench(command, rho, limit, 20 * 7680, 1.0 / 7680.0);
        let mut errors = Vec::new();
        for hz in [120, 240, 480] {
            let (actual, max_substeps, samples) =
                runtime_bench(command, altitude, speed, limit, hz, 20);
            let error = (actual[1] - reference[1]).abs() + 100.0 * (actual[2] - reference[2]).abs();
            errors.push(error);
            assert!(max_substeps <= 2);
            assert!(samples.iter().all(|s| (40.0..260.0).contains(s)));
            println!(
                "bench u={command} altitude={altitude} speed={speed} cap={limit} hz={hz}: shaft={:.12} pitch={:.12} shaft_error={:.6e} pitch_error={:.6e} max_substeps={max_substeps}",
                actual[1],
                actual[2],
                actual[1] - reference[1],
                actual[2] - reference[2]
            );
        }
        assert!(errors[0] < 0.3, "{errors:?}");
        assert!(errors[1] < errors[0] * 0.7 + 1e-5, "{errors:?}");
        assert!(errors[2] < errors[1] * 0.7 + 1e-5, "{errors:?}");
    }
}
#[test]
fn governed_load_response_settles_and_weak_output_reaches_fine_stop() {
    for command in [0.4, 0.7] {
        let (end, max_substeps, samples) = runtime_bench(command, 1000.0, 40.0, 5000.0, 120, 40);
        let earlier = samples[10..20]
            .iter()
            .map(|v| (v - 180.0).abs())
            .fold(0.0, f64::max);
        let later = samples[30..40]
            .iter()
            .map(|v| (v - 180.0).abs())
            .fold(0.0, f64::max);
        assert!(later < earlier);
        assert!((end[1] - 180.0).abs() < 1.0);
        assert!(max_substeps <= 3);
        println!(
            "settling u={command}: earlier peak={earlier}, later peak={later}, final shaft={}",
            end[1]
        );
    }
    let (end, _, _) = runtime_bench(0.0, 1000.0, 0.0, 100.0, 120, 40);
    close(end[2], 0.1, 1e-15);
    assert!(end[1] < 180.0);
}
#[test]
fn endpoint_only_failure_is_checked_before_commit() {
    // Independently selected interval lies between this case's K4 shaft
    // 180.11406012828962 and weighted endpoint 180.11406014776125.
    let mut start = level(1000.0, 40.0);
    start.turbine_fraction = TurbineFraction::new(0.2).unwrap();
    start.blade_pitch_rad = Radians(0.1);
    let mut envelope = envelope_definition();
    envelope.relative_shaft_rad_s[1] = 180.11406013802542;
    let mut model = TurbopropFlightDynamics::new(
        config_from(
            airframe_definition(),
            power_definition(),
            propeller_definition(),
            governor_definition(),
            envelope,
        )
        .unwrap(),
        start,
    )
    .unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(error.stage, TurbopropStage::Endpoint);
    assert_eq!(error.substep, 0);
    assert_eq!(state_bits(model.state()), state_bits(&start));
}
#[test]
fn maximum_work_and_contact_resolution_are_explicit() {
    let start = level(1000.0, 40.0);
    let mut model = TurbopropFlightDynamics::new(config(), start).unwrap();
    let report = model
        .step(
            MAX_TURBOPROP_STEP_DT,
            ControlInputs::neutral().with_throttle(0.7),
            &Environment::still_air(),
        )
        .unwrap();
    assert_eq!(report.substeps, 8); // initial + 5 per substep = 41 by call-graph audit
    let mut frame = airframe_definition();
    frame.inertia_kg_m2[0] = 1000.0;
    let mut prop = propeller_definition();
    prop.rotor_axial_inertia_kg_m2 = 999.0;
    let start = level(1.15, 0.0);
    let cfg = config_from(
        frame,
        power_definition(),
        prop,
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    close(
        cfg.effective_body_mass_properties().inertia().x_axis.x,
        1.0,
        0.0,
    );
    let mut model = TurbopropFlightDynamics::new(cfg, start).unwrap();
    let error = model
        .step(
            Seconds(1.0 / 120.0),
            ControlInputs::neutral(),
            &Environment::still_air(),
        )
        .unwrap_err();
    assert_eq!(error.reason, TurbopropFailureReason::SubstepBudgetExceeded);
    assert_eq!(error.stage, TurbopropStage::Initial);
    assert_eq!(state_bits(model.state()), state_bits(&start));
}
#[test]
fn compact_diagnostics_preserve_missing_values_and_signed_zero() {
    assert!(std::mem::size_of::<TurbopropStepError>() < 128);
    let values = TurbopropDiagnosticValues {
        advance_ratio: Some(AdvanceRatio(-0.0)),
        relative_shaft: Some(RadiansPerSecond(100.0)),
        ..TurbopropDiagnosticValues::default()
    };
    let actual = TurbopropDiagnostics::from_values(values).unwrap().values();
    assert!(actual.pressure_ratio.is_none());
    assert!(actual.blade_pitch.is_none());
    assert_eq!(
        actual.advance_ratio.unwrap().0.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(
        TurbopropDiagnostics::from_values(TurbopropDiagnosticValues {
            crossflow_ratio: Some(f64::NAN),
            ..values
        })
        .is_err()
    );
}

#[test]
fn rotor_inertia_changes_acceleration_without_changing_instantaneous_map_loads() {
    let original = propeller_definition();
    let mut heavier = original.clone();
    heavier.rotor_axial_inertia_kg_m2 = 40.0;
    let q = query(0.5, 0.3);
    let a = PropellerMap::from_definition(original.clone())
        .unwrap()
        .sample(q, KilogramsPerCubicMeter(1.0), RadiansPerSecond(180.0))
        .unwrap();
    let b = PropellerMap::from_definition(heavier.clone())
        .unwrap()
        .sample(q, KilogramsPerCubicMeter(1.0), RadiansPerSecond(180.0))
        .unwrap();
    assert_eq!(a, b);
    let first = config_from(
        airframe_definition(),
        power_definition(),
        original,
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    let second = config_from(
        airframe_definition(),
        power_definition(),
        heavier,
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    let first = runtime::coupled_rotation(&first, DVec3::ZERO, 180.0, 1000.0, 100.0, DVec3::ZERO);
    let second = runtime::coupled_rotation(&second, DVec3::ZERO, 180.0, 1000.0, 100.0, DVec3::ZERO);
    assert!(first.1 > second.1);
    assert!(first.0.x > second.0.x);
}
#[test]
fn reduced_tensor_exact_determinant_boundary_returns_error_without_panicking() {
    // Independent reviewer fixture: the algebraically regrouped scalar
    // determinant is >EPSILON, while glam's actual matrix determinant equals
    // EPSILON. The legacy assertion constructor must never see that matrix.
    let mut frame = airframe_definition();
    frame.inertia_kg_m2 = [
        1.0,
        5.763158480729852,
        6.247241073718394,
        2.5594038453272185e-8,
    ];
    let mut prop = propeller_definition();
    prop.rotor_axial_inertia_kg_m2 = 0.9999999999999999;
    let result = std::panic::catch_unwind(|| {
        config_from(
            frame,
            power_definition(),
            prop,
            governor_definition(),
            envelope_definition(),
        )
    });
    assert!(
        result.is_ok(),
        "invalid reduced inertia must be fallible, not a panic"
    );
    assert!(result.unwrap().is_err());
}
#[test]
fn contact_frequency_uses_the_independent_rotor_free_effective_mass_oracle() {
    let mut prop = propeller_definition();
    prop.rotor_axial_inertia_kg_m2 = 1000.0;
    let cfg = config_from(
        airframe_definition(),
        power_definition(),
        prop,
        governor_definition(),
        envelope_definition(),
    )
    .unwrap();
    let start = level(1.15, 0.0);
    let environment = Environment::still_air();
    let frame = start.rigid_body.local_frame();
    // Independent vertical main-leg lever is (±1.5,-1,0) m, I_B diag
    // (2000,5000,7000), m=2000 and maximum spring=6*90000 N/m.
    // Translation: omega²=810; main leg: omega²=985.5, which dominates.
    let frequency = crate::landing_gear::maximum_natural_frequency(
        cfg.airframe().landing_gear(),
        cfg.effective_body_mass_properties(),
        &start.rigid_body,
        &environment,
        &frame,
    );
    close(frequency, 985.5_f64.sqrt(), 1e-11);
    let incorrectly_locked = crate::landing_gear::maximum_natural_frequency(
        cfg.airframe().landing_gear(),
        cfg.airframe().mass_properties(),
        &start.rigid_body,
        &environment,
        &frame,
    );
    close(incorrectly_locked, 810.0_f64.sqrt(), 1e-11);
    let mut model = TurbopropFlightDynamics::new(cfg, start).unwrap();
    assert_eq!(
        model
            .step(Seconds(1.0 / 120.0), ControlInputs::neutral(), &environment)
            .unwrap()
            .substeps,
        6
    );
}
