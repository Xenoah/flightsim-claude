//! Settled, six-minute flights over the real bundled world and monthly climate.
//!
//! The unchanged default FlightDirector is a deterministic QA fixture controller
//! (ADR-0006), not a product autopilot. Calm-wind numerical traversal and exact
//! same-build replay do not validate human handling, real aircraft performance,
//! airport weather, rendered seams, or arbitrary terrain clearance.

use flightsim_core::{Attitude, Degrees, Geodetic, Meters, MetersPerSecond, Ned, Seconds};
use flightsim_fdm::{AircraftConfig, Atmosphere, ControlInputs, RigidBodyState};
use flightsim_sim::replay::{Conditions, FORMAT_VERSION, KEYFRAME_INTERVAL, Recording};
use flightsim_sim::{
    DirectorTargets, FlightDirector, GroundSampler, Recorder, Simulation, VerticalTarget,
};
use flightsim_world::global::{GlobalTerrain, GlobalTileSource};
use flightsim_world::{ClimateDate, GlobalClimate, MemoryTileSource, Terrain};

type GlobalSimulation = Simulation<GlobalTileSource<MemoryTileSource>>;

const HZ: u32 = 120;
const DT: Seconds = Seconds(1.0 / 120.0);
const WARMUP_STEPS: u32 = 120 * HZ;
const RECORDED_STEPS: u32 = 360 * HZ;

fn simulation(state: RigidBodyState, date: ClimateDate) -> GlobalSimulation {
    let terrain = Terrain::new(
        GlobalTileSource::new(MemoryTileSource::new(), GlobalTerrain::bundled().unwrap()),
        1024 * 1024,
        0..=13,
    );
    let mut sim = Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        terrain,
        GroundSampler::default(),
    );
    sim.set_climate(Some(date)).unwrap();
    sim
}

fn controls(sim: &GlobalSimulation, target_altitude: Meters) -> ControlInputs {
    FlightDirector::default().control(
        sim.state(),
        sim.agl(),
        DirectorTargets {
            // Hold ellipsoidal altitude, not the distant ground's undulations.
            // Subtract the same ground sample used by Simulation::agl().
            vertical: VerticalTarget::AltitudeAgl(target_altitude - sim.ground().elevation),
            heading: Degrees(90.0).to_radians(),
            airspeed: MetersPerSecond(50.0),
            flaps: 0.0,
            brakes: 0.0,
            throttle_override: None,
            wings_level: false,
        },
    )
}

fn checked_step(
    sim: &mut GlobalSimulation,
    atlas: &GlobalTerrain,
    frame_time: Seconds,
    input: ControlInputs,
    ocean: bool,
) {
    let before = *sim.state();
    let previous_ground = sim.ground();
    let previous_air = sim.atmosphere_sample();
    let previous_climate = sim.climate_sample().unwrap();
    let report = sim.advance(frame_time, input);
    assert_eq!(report.steps, 1);
    assert!(!report.diverged && !sim.diverged() && !sim.crashed());
    assert!(!report.terrain_missing && sim.ground().from_terrain);
    assert!(sim.terrain().load_failures().is_empty());
    assert!(sim.state().is_finite());
    // 240 m/s is far above this light aircraft's cruise, but rules out jumps.
    assert!((0.0..2.0).contains(&before.position.distance_to(sim.state().position).get()));
    assert!((0.0..12_000.0).contains(&sim.state().altitude().get()));
    assert!(sim.agl().get() > 300.0);
    assert!((20.0..100.0).contains(&sim.airspeed().get()));
    assert!((sim.state().orientation.length_squared() - 1.0).abs() < 1e-12);

    let ground = sim.ground();
    assert!(ground.elevation.is_finite());
    assert!(ground.slope.north().is_finite() && ground.slope.east().is_finite());
    // Ground is the start-of-step sample used by the physical environment.
    let direct = atlas.sample(ground.reference).unwrap();
    assert!((ground.elevation - direct.surface_height).abs().get() < 1e-9);
    if ocean {
        assert!(!direct.is_land && !direct.is_inland_water);
        assert!(direct.elevation_msl.get().abs() < f64::EPSILON);
    } else {
        assert!(direct.is_land);
    }
    // A metre in one 120 Hz cruise step would be an implausible seam for these
    // smooth open-ocean/desert routes, rather than a global slope guarantee.
    assert!((ground.elevation - previous_ground.elevation).abs().get() < 1.0);

    let air = sim.atmosphere_sample();
    let climate = sim.climate_sample().unwrap();
    assert!(air.is_finite() && climate.is_finite());
    // Physical ISA uses geopotential altitude; the climate preview's lapse is
    // approximate. The contract is to apply the regional ISA offset once while
    // retaining ISA pressure, not equate the two temperature representations.
    let standard = Atmosphere::standard().sample(sim.state().altitude());
    assert!(
        (air.temperature - standard.temperature - climate.isa_temperature_offset)
            .abs()
            .get()
            < 1e-9
    );
    assert_eq!(air.pressure, standard.pressure);
    assert!((150.0..350.0).contains(&air.temperature.get()));
    assert!(air.pressure.get() > 15_000.0 && air.density.get() > 0.2);
    assert!((air.temperature - previous_air.temperature).abs().get() < 0.05);
    assert!((air.pressure - previous_air.pressure).abs().get() < 20.0);
    assert!((climate.cloud_cover - previous_climate.cloud_cover).abs() < 0.001);
    assert!(
        (climate.precipitation_rate - previous_climate.precipitation_rate)
            .abs()
            .get()
            < 1e-8
    );
}

fn check_settled(sim: &GlobalSimulation) {
    let state = sim.state();
    let attitude = state.attitude();
    assert!(state.vertical_speed().get().abs() < 0.25);
    assert!(attitude.roll.to_degrees().get().abs() < 1.0);
    assert!(attitude.pitch.to_degrees().get().abs() < 8.0);
    assert!((40.0..65.0).contains(&sim.airspeed().get()));
    for rate in state.angular_velocity.to_array() {
        assert!(rate.to_degrees().abs() < 0.1);
    }
}

// PartialEq considers +0 and -0 equal; compare every stored IEEE-754 bit here.
fn state_bits(state: &RigidBodyState) -> [u64; 13] {
    [
        state.position.0.x,
        state.position.0.y,
        state.position.0.z,
        state.velocity.x,
        state.velocity.y,
        state.velocity.z,
        state.orientation.x,
        state.orientation.y,
        state.orientation.z,
        state.orientation.w,
        state.angular_velocity.x,
        state.angular_velocity.y,
        state.angular_velocity.z,
    ]
    .map(f64::to_bits)
}

fn reject_changed_dataset_identities(conditions: &Conditions, initial: &RigidBodyState) {
    for terrain_identity in [true, false] {
        let mut changed = conditions.clone();
        if terrain_identity {
            changed.terrain_fingerprint ^= 1;
        } else {
            changed.climate_fingerprint ^= 1;
        }
        let mut recorder = Recorder::new(changed);
        recorder.record(DT, ControlInputs::neutral(), Some(initial));
        assert!(
            recorder
                .finish()
                .check_reproducible_with(&AircraftConfig::light_single())
                .is_err()
        );
    }
}

fn fly_and_replay(latitude: f64, longitude: f64, month: u8, ocean: bool) {
    eprintln!("global duration fixture: lat={latitude}, lon={longitude}, month={month}");
    let atlas = GlobalTerrain::bundled().unwrap();
    let climate = GlobalClimate::bundled().unwrap();
    let date = ClimateDate::from_month(month).unwrap();
    let start = Geodetic::from_degrees(latitude, longitude, 0.0);
    let target_altitude = atlas.sample(start).unwrap().surface_height + Meters(1500.0);
    let initial = RigidBodyState::from_geodetic(
        Geodetic {
            altitude: target_altitude,
            ..start
        },
        Attitude::from_degrees(0.0, 2.0, 90.0),
        Ned::new(0.0, 50.0, 0.0),
    );
    let mut warmup = simulation(initial, date);
    for _ in 0..WARMUP_STEPS {
        let input = controls(&warmup, target_altitude);
        checked_step(&mut warmup, &atlas, DT, input, ocean);
    }
    check_settled(&warmup);

    // Do not carry hidden warmup clock, accumulator, interpolation or history
    // into recording. Reconstruction is exactly the replay frame-zero path.
    let zero = *warmup.state();
    let mut sim = simulation(zero, date);
    let config = AircraftConfig::light_single();
    let conditions = Conditions {
        start: zero.geodetic(),
        heading: zero.attitude().yaw,
        ..Conditions::default()
    }
    .with_aircraft(&config)
    .with_world_climate(true, Some(date));
    assert_eq!(conditions.terrain_fingerprint, atlas.fingerprint());
    assert_eq!(conditions.climate_fingerprint, climate.fingerprint());
    let mut recorder = Recorder::new(conditions.clone());
    let mut minimum_altitude = zero.altitude().get();
    let mut maximum_altitude = minimum_altitude;
    let mut minimum_ground = sim.ground().elevation.get();
    let mut maximum_ground = minimum_ground;
    let mut crossings = 0;
    for _ in 0..RECORDED_STEPS {
        let input = controls(&sim, target_altitude);
        let previous_longitude = sim.state().geodetic().longitude;
        recorder.record(DT, input, Some(sim.state()));
        checked_step(&mut sim, &atlas, DT, input, ocean);
        check_settled(&sim);
        minimum_altitude = minimum_altitude.min(sim.state().altitude().get());
        maximum_altitude = maximum_altitude.max(sim.state().altitude().get());
        minimum_ground = minimum_ground.min(sim.ground().elevation.get());
        maximum_ground = maximum_ground.max(sim.ground().elevation.get());
        if previous_longitude.get() > 0.0 && sim.state().geodetic().longitude.get() < 0.0 {
            crossings += 1;
        }
    }
    assert!(maximum_altitude - minimum_altitude < 5.0);
    assert!(sim.log().distance.get() > 10_000.0);
    assert!(zero.position.distance_to(sim.state().position).get() > 10_000.0);
    assert!((sim.elapsed().get() - 360.0).abs() < 1e-7);
    assert_eq!(sim.touchdown_count(), 0);
    assert!(!sim.on_ground());
    assert_eq!(sim.climate(), Some(date));
    if ocean {
        assert!(zero.geodetic().longitude.get() > 0.0);
        assert!(sim.state().geodetic().longitude.get() < 0.0);
        assert_eq!(crossings, 1, "cross during recording, not only warmup");
    } else {
        // Ensure the contrasting route actually exercises varying land relief.
        assert!(maximum_ground - minimum_ground > 50.0);
    }

    let recording = recorder.finish();
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), FORMAT_VERSION);
    let restored = Recording::read_from(&mut bytes.as_slice()).unwrap();
    assert_eq!(restored, recording);
    let mut reserialized = Vec::new();
    restored.write_to(&mut reserialized).unwrap();
    assert_eq!(bytes, reserialized, "serialized fields retain all bits");
    restored.check_reproducible_with(&config).unwrap();
    assert_eq!(restored.conditions(), &conditions);
    reject_changed_dataset_identities(restored.conditions(), &zero);

    let key_zero = restored.keyframe_exactly_at(0).unwrap();
    let mut replay = simulation(key_zero.state, restored.conditions().climate_date.unwrap());
    replay.set_wind(restored.conditions().wind);
    replay.set_turbulence(restored.conditions().turbulence);
    let mut checkpoints = 0;
    for (index, frame) in restored.frames().iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        if let Some(key) = restored.keyframe_exactly_at(index) {
            assert_eq!(state_bits(&key.state), state_bits(replay.state()));
            checkpoints += 1;
        }
        checked_step(&mut replay, &atlas, frame.frame_time, frame.controls, ocean);
    }
    assert_eq!(restored.frames().len(), RECORDED_STEPS as usize);
    assert_eq!(checkpoints, RECORDED_STEPS / KEYFRAME_INTERVAL);
    assert_eq!(state_bits(replay.state()), state_bits(sim.state()));
    assert_eq!(replay.elapsed(), sim.elapsed());
    assert_eq!(replay.log(), sim.log());
    assert_eq!(replay.ground(), sim.ground());
    assert_eq!(replay.interpolated(), sim.interpolated());
    assert_eq!(replay.atmosphere_sample(), sim.atmosphere_sample());
    assert_eq!(replay.climate_sample(), sim.climate_sample());
}

#[test]
fn six_minute_ocean_dateline_flights_replay_exactly_in_january_and_july() {
    for month in [1, 7] {
        fly_and_replay(0.0, 179.9, month, true);
    }
}

#[test]
fn six_minute_desert_flights_replay_exactly_in_january_and_july() {
    for month in [1, 7] {
        fly_and_replay(25.0, 10.0, month, false);
    }
}
