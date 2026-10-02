//! Short real-global-terrain + monthly-climate integration regressions.
//!
//! These check finite numerical state, provenance, bounded ECEF motion and
//! continuity through geographic seams. They do not validate human handling,
//! local airport meteorology, navigation accuracy or long-duration flight.

use flightsim_core::{Attitude, Degrees, Geodetic, Meters, Ned, Radians, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState};
use flightsim_sim::{GroundSampler, Simulation, climate_atmosphere_sample};
use flightsim_world::global::{GlobalTerrain, GlobalTileSource};
use flightsim_world::{ClimateDate, MemoryTileSource, Terrain};

type GlobalSimulation = Simulation<GlobalTileSource<MemoryTileSource>>;

const DT: Seconds = Seconds(1.0 / 120.0);
const STEPS: u32 = 240; // Two seconds; safely above the surface throughout.

fn simulation(start: Geodetic, heading: Radians, date: ClimateDate) -> GlobalSimulation {
    let global = GlobalTerrain::bundled().expect("real bundled global atlas validates");
    let mut terrain = Terrain::new(
        GlobalTileSource::new(MemoryTileSource::new(), global),
        1024 * 1024,
        0..=13,
    );
    let ground = GroundSampler::default().sample(&mut terrain, start);
    assert!(
        ground.from_terrain,
        "global data must supply the initial ground"
    );
    let position = Geodetic {
        altitude: ground.elevation + Meters(1500.0),
        ..start
    };
    let air = climate_atmosphere_sample(position, Some(date)).expect("climate validates");
    // The same equivalent speed at each terrain height/month. Core owns all
    // coordinate transformations; this is a local heading/velocity definition.
    let speed = 55.0 / air.density_ratio().sqrt();
    let velocity = Ned::new(heading.cos() * speed, heading.sin() * speed, 0.0);
    let state = RigidBodyState::from_geodetic(
        position,
        Attitude::new(Radians::ZERO, Degrees(2.0).to_radians(), heading),
        velocity,
    );
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        terrain,
        GroundSampler::default(),
    );
    sim.set_climate(Some(date))
        .expect("same bundled climate snapshot validates");
    // RigidBodyState stores ECEF; its geodetic round trip can change the last
    // altitude bits, so compare physical tolerance rather than input bits.
    assert!(
        (sim.atmosphere_sample().temperature - air.temperature)
            .abs()
            .get()
            < 1e-7
    );
    assert!((sim.atmosphere_sample().density - air.density).abs().get() < 1e-9);
    sim
}

fn controls() -> ControlInputs {
    ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65)
}

fn checked_step(sim: &mut GlobalSimulation, atlas: &GlobalTerrain) {
    let before = sim.state().position;
    let previous_air = sim.atmosphere_sample();
    let report = sim.advance(DT, controls());
    assert_eq!(report.steps, 1);
    assert!(!report.diverged && !sim.diverged() && !sim.crashed());
    assert!(
        !report.terrain_missing,
        "global fallback disappeared during flight"
    );
    assert!(sim.state().is_finite());
    let movement = before.distance_to(sim.state().position).get();
    // At 120 Hz this generous upper bound is 600 m/s, well above this light
    // aircraft's initial speed but small enough to catch a geographic jump.
    assert!(
        (0.0..5.0).contains(&movement),
        "ECEF step moved {movement} m"
    );
    assert!(
        sim.agl().get() > 1000.0,
        "short probe unexpectedly approached terrain"
    );

    let ground = sim.ground();
    assert!(ground.from_terrain && ground.elevation.is_finite());
    assert!(ground.slope.north().is_finite() && ground.slope.east().is_finite());
    let direct = atlas
        .sample(ground.reference)
        .expect("finite ground reference");
    assert!(
        (ground.elevation - direct.surface_height).abs().get() < 1e-9,
        "physics ground must use the real global surface, including its geoid"
    );

    let air = sim.atmosphere_sample();
    let climate = sim.climate_sample().expect("climate remains enabled");
    assert!(air.is_finite() && climate.is_finite());
    assert!(air.temperature.get() > 100.0 && air.temperature.get() < 400.0);
    assert!(air.pressure.get() > 1000.0 && air.density.get() > 0.1);
    assert!(
        (air.temperature - previous_air.temperature).abs().get() < 0.1,
        "climate/altitude temperature jumped within a single fixed step"
    );
    assert!((air.pressure - previous_air.pressure).abs().get() < 200.0);
}

#[test]
fn representative_regions_fly_over_real_ground_in_both_hemisphere_seasons() {
    let atlas = GlobalTerrain::bundled().unwrap();
    // Coast, desert, rainforest, Southern Hemisphere, high mountains, ice and
    // open ocean exercise very different heights/climates without long flights.
    for (latitude, longitude) in [
        (35.55, 139.78),
        (25.0, 10.0),
        (-3.0, -60.0),
        (-33.9, 151.2),
        (28.0, 86.8),
        (72.0, -42.0),
        (0.0, -140.0),
    ] {
        for month in [1, 7] {
            let start = Geodetic::from_degrees(latitude, longitude, 0.0);
            let date = ClimateDate::from_month(month).unwrap();
            let mut sim = simulation(start, Radians::ZERO, date);
            let initial_position = sim.state().position;
            for _ in 0..STEPS {
                checked_step(&mut sim, &atlas);
            }
            assert!(initial_position.distance_to(sim.state().position).get() < 500.0);
            assert_eq!(sim.climate(), Some(date));
        }
    }
}

#[test]
fn short_dateline_crossing_keeps_world_air_and_ecef_motion_continuous() {
    let atlas = GlobalTerrain::bundled().unwrap();
    for month in [1, 7] {
        let mut sim = simulation(
            Geodetic::from_degrees(0.0, 179.9998, 0.0),
            Degrees(90.0).to_radians(),
            ClimateDate::from_month(month).unwrap(),
        );
        assert!(sim.state().geodetic().longitude.get() > 0.0);
        for _ in 0..STEPS {
            checked_step(&mut sim, &atlas);
        }
        assert!(
            sim.state().geodetic().longitude.get() < 0.0,
            "the scenario must actually cross the dateline rather than only approach it"
        );
    }
}

#[test]
fn short_north_pole_crossing_uses_finite_ground_and_climate_caps() {
    let atlas = GlobalTerrain::bundled().unwrap();
    for month in [1, 7] {
        let start = Geodetic::from_degrees(89.9996, 30.0, 0.0);
        let mut sim = simulation(
            start,
            Radians::ZERO,
            ClimateDate::from_month(month).unwrap(),
        );
        let mut closest_latitude = start.latitude.get();
        for _ in 0..STEPS {
            checked_step(&mut sim, &atlas);
            closest_latitude = closest_latitude.max(sim.state().geodetic().latitude.get());
        }
        assert!(closest_latitude > Degrees(89.9999).to_radians().get());
        let longitude_change = start
            .longitude
            .shortest_difference_to(sim.state().geodetic().longitude);
        assert!(
            longitude_change.abs() > Degrees(90.0).to_radians(),
            "the pole probe must cross onto the opposite meridian"
        );
    }
}

#[test]
fn explicit_global_only_source_preserves_every_fixed_step_and_interpolation() {
    let atlas = GlobalTerrain::bundled().unwrap();
    let date = ClimateDate::from_month(7).unwrap();
    for (lat, lon) in [
        (90.0, 0.0),
        (-90.0, 0.0),
        (0.0, 180.0),
        (31.5, 35.5),
        (27.9881, 86.925),
        (35.55, 139.78),
    ] {
        let mut before = simulation(Geodetic::from_degrees(lat, lon, 0.0), Radians::ZERO, date);
        let mut after = Simulation::from_state(
            AircraftConfig::light_single(),
            *before.state(),
            Terrain::new(
                GlobalTileSource::new(flightsim_world::EmptyTileSource, atlas.clone()),
                1024 * 1024,
                0..=13,
            ),
            GroundSampler::default(),
        );
        after.set_climate(Some(date)).unwrap();
        for _ in 0..STEPS {
            before.advance(DT, controls());
            after.advance(DT, controls());
            assert_eq!(before.state(), after.state());
            assert_eq!(before.ground(), after.ground());
            assert_eq!(before.atmosphere_sample(), after.atmosphere_sample());
            assert_eq!(before.interpolated(), after.interpolated());
        }
    }
}
