//! Regressions uncovered while validating issue #5: gust-aware airspeed and frame-independent time.
use flightsim_core::{Attitude, Geodetic, LocalFrame, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState, Turbulence};
use flightsim_sim::{GroundSampler, Simulation, Wind};
use flightsim_world::{MemoryTileSource, Terrain};

fn initial() -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    )
}

fn simulation(turbulence: Turbulence) -> Simulation<MemoryTileSource> {
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        initial(),
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    sim.set_turbulence(turbulence);
    sim
}

fn bits(state: &RigidBodyState) -> [u64; 13] {
    let p = state.position.as_vec();
    let v = state.velocity;
    let q = state.orientation;
    let w = state.angular_velocity;
    [
        p.x, p.y, p.z, v.x, v.y, v.z, q.x, q.y, q.z, q.w, w.x, w.y, w.z,
    ]
    .map(f64::to_bits)
}

fn controls() -> ControlInputs {
    ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65)
}

#[test]
fn reported_airspeed_includes_the_same_gust_as_aerodynamic_forces() {
    for turbulence in [
        Turbulence::CALM,
        Turbulence::light(1),
        Turbulence::moderate(7),
        Turbulence::severe(4242),
    ] {
        let mut sim = simulation(turbulence);
        sim.set_wind(Wind {
            from: Radians(0.7),
            speed: MetersPerSecond(6.0),
        });
        for _ in 0..120 {
            let state = sim.state();
            let p = state.geodetic();
            let frame = LocalFrame::new(p);
            let wind = frame.ned_to_ecef_vector(sim.wind().to_ned());
            let gust = frame.ned_to_ecef_vector(turbulence.gust_at(sim.elapsed(), p));
            let expected = (state.velocity - (wind + gust)).length();
            assert!(
                (sim.airspeed().get() - expected).abs() < 1e-10,
                "reported {} vs gust-relative {} m/s",
                sim.airspeed().get(),
                expected
            );
            assert!((sim.airspeed().get() - sim.aero_angles().true_airspeed.get()).abs() < 1e-10);
            sim.advance(Seconds(1.0 / 120.0), controls());
        }
    }
}

#[test]
fn held_controls_and_gusts_ignore_render_frame_grouping() {
    for turbulence in [Turbulence::CALM, Turbulence::severe(1)] {
        for hz in [15, 30, 60, 240] {
            let mut reference = simulation(turbulence);
            let mut grouped = simulation(turbulence);
            for second in 0..60 {
                for _ in 0..120 {
                    reference.advance(Seconds(1.0 / 120.0), controls());
                }
                let mut steps = 0;
                for _ in 0..hz {
                    let report = grouped.advance(Seconds(1.0 / f64::from(hz)), controls());
                    assert!(!report.diverged && !grouped.crashed());
                    steps += report.steps;
                }
                assert_eq!(steps, 120);
                assert_eq!(
                    bits(grouped.state()),
                    bits(reference.state()),
                    "{turbulence:?}, {hz} Hz, second {second}"
                );
            }
        }
    }
}

#[test]
fn irregular_fractional_frames_match_the_same_fixed_steps() {
    let mut reference = simulation(Turbulence::severe(1));
    let mut grouped = simulation(Turbulence::severe(1));
    for cycle in 0..1200 {
        for _ in 0..6 {
            reference.advance(Seconds(1.0 / 120.0), controls());
        }
        let mut steps = 0;
        for multiplier in [0.5, 3.5, 2.0] {
            steps += grouped
                .advance(Seconds(multiplier / 120.0), controls())
                .steps;
        }
        assert_eq!(steps, 6);
        assert_eq!(
            bits(grouped.state()),
            bits(reference.state()),
            "cycle {cycle}"
        );
    }
}

#[test]
fn non_integral_frame_rates_use_each_executed_physics_step() {
    for hz in [59, 144, 165] {
        let mut reference = simulation(Turbulence::severe(7));
        let mut grouped = simulation(Turbulence::severe(7));
        for frame in 0..60 * hz {
            let report = grouped.advance(Seconds(1.0 / f64::from(hz)), controls());
            for _ in 0..report.steps {
                reference.advance(Seconds(1.0 / 120.0), controls());
            }
            assert_eq!(
                bits(grouped.state()),
                bits(reference.state()),
                "{hz} Hz frame {frame}"
            );
            assert_eq!(
                grouped.elapsed().get().to_bits(),
                reference.elapsed().get().to_bits()
            );
        }
    }
}

#[test]
fn restart_resets_the_gust_clock_and_zero_time_does_not_advance_it() {
    let mut sim = simulation(Turbulence::severe(1));
    for _ in 0..600 {
        sim.advance(Seconds(1.0 / 60.0), controls());
    }
    let first = bits(sim.state());
    let elapsed = sim.elapsed().get().to_bits();
    assert_eq!(sim.advance(Seconds::ZERO, controls()).steps, 0);
    assert_eq!(bits(sim.state()), first);
    assert_eq!(sim.elapsed().get().to_bits(), elapsed);
    sim.restart_at(initial());
    assert_eq!(sim.elapsed().get().to_bits(), 0.0_f64.to_bits());
    for _ in 0..600 {
        sim.advance(Seconds(1.0 / 60.0), controls());
    }
    assert_eq!(bits(sim.state()), first);
}
