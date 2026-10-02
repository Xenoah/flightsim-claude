//! Climate-to-FDM ownership, density, replay and deterministic stepping.

use flightsim_core::{Attitude, Geodetic, Ned, Seconds};
use flightsim_fdm::{AircraftConfig, Atmosphere, ControlInputs, RigidBodyState};
use flightsim_sim::replay::{Conditions, Recorder, Recording};
use flightsim_sim::{GroundSampler, Simulation, climate_atmosphere_sample};
use flightsim_world::{ClimateDate, MemoryTileSource, Terrain};

fn initial() -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    )
}

fn simulation(month: Option<u8>) -> Simulation<MemoryTileSource> {
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        initial(),
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    sim.set_climate(month.map(|month| ClimateDate::from_month(month).unwrap()))
        .unwrap();
    sim
}

fn controls() -> ControlInputs {
    ControlInputs::neutral()
        .with_elevator(0.02)
        .with_throttle(0.65)
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

#[test]
fn disabled_climate_retains_exact_isa_air_and_can_be_restored() {
    let mut sim = simulation(None);
    let expected = Atmosphere::standard().sample(sim.state().altitude());
    assert_eq!(sim.atmosphere_sample(), expected);
    assert!(sim.climate().is_none());
    assert!(sim.climate_sample().is_none());
    sim.set_climate(ClimateDate::from_month(7)).unwrap();
    assert!(sim.climate_sample().is_some());
    assert_ne!(sim.atmosphere_sample(), expected);
    sim.set_climate(None).unwrap();
    assert_eq!(sim.atmosphere_sample(), expected);
}

#[test]
fn hot_month_lowers_density_while_pressure_remains_isa() {
    let winter = simulation(Some(1));
    let summer = simulation(Some(7));
    let cold = winter.atmosphere_sample();
    let hot = summer.atmosphere_sample();
    assert!(hot.temperature > cold.temperature);
    assert!(hot.density < cold.density);
    assert_eq!(hot.pressure, cold.pressure);
    let climate = summer.climate_sample().unwrap();
    let expected = Atmosphere::with_temperature_offset(climate.isa_temperature_offset.get())
        .sample(summer.state().altitude());
    assert_eq!(hot, expected);
    assert_eq!(
        hot,
        climate_atmosphere_sample(summer.state().geodetic(), summer.climate()).unwrap()
    );
    // Climate's simple geometric lapse and ISA's geopotential lapse differ
    // slightly; they must agree at ordinary aircraft altitudes to <0.01 K.
    assert!((hot.temperature - climate.temperature).abs().get() < 0.01);
}

#[test]
fn climate_changes_actual_aircraft_forces_and_not_only_display_values() {
    let mut standard = simulation(None);
    let mut summer = simulation(Some(7));
    for _ in 0..120 {
        standard.advance(Seconds(1.0 / 120.0), controls());
        summer.advance(Seconds(1.0 / 120.0), controls());
    }
    assert!(!standard.diverged() && !summer.diverged());
    assert_ne!(bits(standard.state()), bits(summer.state()));
    assert!(
        (standard.state().position.as_vec() - summer.state().position.as_vec()).length() > 0.001
    );
}

#[test]
fn fixed_date_climate_ignores_render_frame_grouping() {
    for hz in [15, 30, 60, 144, 240] {
        let mut reference = simulation(Some(1));
        let mut grouped = simulation(Some(1));
        for _ in 0..hz * 3 {
            let report = grouped.advance(Seconds(1.0 / f64::from(hz)), controls());
            for _ in 0..report.steps {
                reference.advance(Seconds(1.0 / 120.0), controls());
            }
            assert_eq!(bits(grouped.state()), bits(reference.state()), "{hz} Hz");
        }
    }
}

#[test]
fn recorded_climate_restores_exactly_and_restart_preserves_its_date() {
    let config = AircraftConfig::light_single();
    let mut live = simulation(Some(7));
    let conditions = Conditions {
        start: initial().geodetic(),
        ..Conditions::default()
    }
    .with_aircraft(&config)
    .with_world_climate(false, live.climate());
    let mut recorder = Recorder::new(conditions);
    for i in 0..240 {
        let dt = Seconds(if i % 2 == 0 { 1.0 / 40.0 } else { 1.0 / 120.0 });
        recorder.record(dt, controls(), Some(live.state()));
        live.advance(dt, controls());
    }
    let mut bytes = Vec::new();
    recorder.finish().write_to(&mut bytes).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 2);
    let recording = Recording::read_from(&mut &bytes[..]).unwrap();
    recording.check_reproducible_with(&config).unwrap();
    let mut replay = simulation(None);
    replay
        .set_climate(recording.conditions().climate_date)
        .unwrap();
    for frame in recording.frames() {
        replay.advance(frame.frame_time, frame.controls);
    }
    assert_eq!(bits(live.state()), bits(replay.state()));
    let original = recording.conditions().climate_date;
    replay.restart_at(initial());
    assert_eq!(replay.climate(), original);
    for frame in recording.frames() {
        replay.advance(frame.frame_time, frame.controls);
    }
    assert_eq!(bits(live.state()), bits(replay.state()));
}
