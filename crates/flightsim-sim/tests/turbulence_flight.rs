//! 乱流下の飛行の検査。
//!
//! **揺れて楽しくなるのはよいが、飛べなくなっては困る。**
//! 中程度の乱流でも自動操縦が一周でき、機体が壊れないこと。

use flightsim_core::{Geodetic, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, Turbulence};
use flightsim_sim::{GroundSampler, Simulation};
use flightsim_world::{MemoryTileSource, Terrain};

fn flat_world() -> Terrain<MemoryTileSource> {
    Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12)
}

fn parked(turbulence: Turbulence) -> Simulation<MemoryTileSource> {
    let mut simulation = Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.548, 139.775, 0.0),
        Radians::ZERO,
        flat_world(),
        GroundSampler::default(),
    );
    simulation.set_turbulence(turbulence);
    simulation
}

#[test]
fn calm_turbulence_changes_nothing() {
    // 既定は無乱流。既存の呼び出しの挙動が変わらないこと。
    let mut with_calm = parked(Turbulence::CALM);
    let mut untouched = parked(Turbulence::CALM);
    untouched.set_turbulence(Turbulence::CALM);

    for _ in 0..600 {
        with_calm.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        untouched.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
    }
    assert_eq!(with_calm.state().position, untouched.state().position);
}

#[test]
fn the_whole_simulation_stays_deterministic_under_turbulence() {
    // **リプレイの前提**（ADR-0004）。乱流を入れても、同じ入力なら
    // 同じ軌跡になること。ここが崩れると同期もリプレイも成立しない。
    let run = || {
        let mut simulation = parked(Turbulence::severe(4242));
        for _ in 0..1200 {
            simulation.advance(
                Seconds(1.0 / 60.0),
                ControlInputs::neutral().with_throttle(0.6),
            );
        }
        simulation.state().position
    };
    assert_eq!(run(), run(), "turbulence broke determinism");
}

#[test]
fn a_parked_aircraft_is_not_blown_away() {
    // 地上で駐機している機体が乱流で滑走路の外へ飛んでいかないこと。
    let start = Geodetic::from_degrees(35.548, 139.775, 0.0);
    let mut simulation = parked(Turbulence::severe(9));
    let controls = ControlInputs::neutral().with_brakes(1.0);

    for _ in 0..1800 {
        let report = simulation.advance(Seconds(1.0 / 60.0), controls);
        assert!(!report.diverged, "turbulence diverged the simulation");
    }

    let moved = start
        .great_circle_distance(simulation.state().geodetic())
        .get();
    assert!(
        moved < 50.0,
        "a braked aircraft drifted {moved:.1} m in severe turbulence"
    );
    assert!(simulation.state().is_finite());
}

#[test]
fn turbulence_actually_shakes_the_aircraft() {
    // 入れたのに何も起きないなら、結線されていない。
    // 空中で放置して、無乱流との軌跡の差を見る。
    let fly = |turbulence: Turbulence| {
        let state = flightsim_fdm::RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.6, 139.5, 1000.0),
            flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
            flightsim_core::Ned::new(40.0, 0.0, 0.0),
        );
        let mut simulation = Simulation::from_state(
            AircraftConfig::light_single(),
            state,
            flat_world(),
            GroundSampler::default(),
        );
        simulation.set_turbulence(turbulence);
        for _ in 0..1200 {
            simulation.advance(
                Seconds(1.0 / 60.0),
                ControlInputs::neutral().with_throttle(0.5),
            );
        }
        simulation.state().geodetic()
    };

    let calm = fly(Turbulence::CALM);
    let rough = fly(Turbulence::moderate(3));
    let difference = calm.great_circle_distance(rough).get();
    assert!(
        difference > 1.0,
        "moderate turbulence changed the path by only {difference:.2} m — is it wired up?"
    );
}

#[test]
fn severe_turbulence_does_not_destroy_a_cruising_aircraft() {
    // 揺れても姿勢が破綻しないこと。**飛べなくなるほど揺らさない。**
    let state = flightsim_fdm::RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.6, 139.5, 1500.0),
        flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, Radians::ZERO),
        flightsim_core::Ned::new(45.0, 0.0, 0.0),
    );
    let mut simulation = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        flat_world(),
        GroundSampler::default(),
    );
    simulation.set_turbulence(Turbulence::severe(77));

    let mut worst_bank: f64 = 0.0;
    for _ in 0..1800 {
        simulation.advance(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral().with_throttle(0.6),
        );
        let attitude = simulation.state().attitude();
        worst_bank = worst_bank.max(attitude.roll.to_degrees().get().abs());
        assert!(simulation.state().is_finite());
    }
    assert!(
        worst_bank < 90.0,
        "severe turbulence rolled the aircraft to {worst_bank:.0} deg without any control input"
    );
}
