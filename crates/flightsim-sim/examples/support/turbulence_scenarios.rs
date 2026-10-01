//! Shared deterministic fixtures for issue #5, used by the example and tests.
//! Envelopes are game regression budgets, not aircraft certification or pilot ratings.

use flightsim_core::{Attitude, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, RECOMMENDED_FIXED_DT, RigidBodyState, Turbulence,
};
use flightsim_sim::{DirectorTargets, FlightDirector, GroundSampler, Simulation, VerticalTarget};
use flightsim_world::{MemoryTileSource, Terrain};

pub const SAMPLE_HZ: u32 = 120;
pub const WARMUP_SECONDS: u32 = 60;
pub const RUN_SECONDS: u32 = 120;
pub const SEEDS: [u64; 3] = [1, 7, 4242];
/// Standard acceleration of gravity; loads below are dimensionless multiples of g0.
const G0: f64 = 9.806_65;

#[derive(Debug, Clone, Copy)]
pub enum Scenario {
    Cruise,
    Approach,
}

impl Scenario {
    pub const ALL: [Self; 2] = [Self::Cruise, Self::Approach];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Cruise => "cruise",
            Self::Approach => "approach",
        }
    }

    pub fn targets(self) -> DirectorTargets {
        DirectorTargets {
            vertical: match self {
                Self::Cruise => VerticalTarget::AltitudeAgl(Meters(1200.0)),
                Self::Approach => VerticalTarget::DescentRate(MetersPerSecond(1.83)),
            },
            heading: Radians::ZERO,
            airspeed: MetersPerSecond(match self {
                Self::Cruise => 50.0,
                Self::Approach => 35.0,
            }),
            flaps: match self {
                Self::Cruise => 0.0,
                Self::Approach => 1.0,
            },
            brakes: 0.0,
            throttle_override: None,
            wings_level: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Severity {
    Calm,
    Light,
    Moderate,
    Severe,
}

impl Severity {
    pub const ALL: [Self; 4] = [Self::Calm, Self::Light, Self::Moderate, Self::Severe];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Light => "light",
            Self::Moderate => "moderate",
            Self::Severe => "severe",
        }
    }

    pub const fn turbulence(self, seed: u64) -> Turbulence {
        match self {
            Self::Calm => Turbulence::CALM,
            Self::Light => Turbulence::light(seed),
            Self::Moderate => Turbulence::moderate(seed),
            Self::Severe => Turbulence::severe(seed),
        }
    }
}

pub fn simulation(state: RigidBodyState) -> Simulation<MemoryTileSource> {
    Simulation::from_state(
        AircraftConfig::light_single(),
        state,
        Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
        GroundSampler::default(),
    )
}

/// Settle in calm air, then reconstruct at the settled state so turbulence time starts at zero.
/// This avoids conflating spawn/trim transients with gust response. The full gust-onset transient
/// is included in the measured interval; there is no unreported post-onset exclusion window.
pub fn settled_state(scenario: Scenario) -> RigidBodyState {
    let targets = scenario.targets();
    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1200.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(targets.airspeed.get(), 0.0, 0.0),
    );
    let mut sim = simulation(initial);
    let director = FlightDirector::default();
    for _ in 0..WARMUP_SECONDS * SAMPLE_HZ {
        let controls = director.control(sim.state(), sim.agl(), targets);
        let report = sim.advance(RECOMMENDED_FIXED_DT, controls);
        assert!(!report.diverged && !sim.crashed(), "calm warmup failed");
    }
    *sim.state()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    sum_squares: f64,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum_squares: 0.0,
        }
    }
}

impl Range {
    fn add(&mut self, value: f64) {
        assert!(value.is_finite(), "non-finite measurement: {value}");
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.sum_squares += value * value;
    }

    fn is_finite(self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.sum_squares.is_finite()
    }

    pub fn peak(self) -> f64 {
        self.min.abs().max(self.max.abs())
    }
    pub fn rms(self, samples: u32) -> f64 {
        (self.sum_squares / f64::from(samples)).sqrt()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub elapsed: Seconds,
    pub state: RigidBodyState,
    pub controls: ControlInputs,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub heading_error_deg: f64,
    pub true_airspeed_mps: f64,
    pub reported_airspeed_mps: f64,
    pub altitude_m: f64,
    pub acceleration_mps2: f64,
    pub normal_load_g: f64,
    pub stall_fraction: f64,
    pub gust: Ned,
}

impl Sample {
    /// Exact trajectory/control comparison within the same binary, including signed-zero bits.
    #[allow(
        dead_code,
        reason = "Used by trajectory regression tests, not the CSV example"
    )]
    pub fn bits(self) -> [u64; 19] {
        let p = self.state.position.as_vec();
        let v = self.state.velocity;
        let q = self.state.orientation;
        let w = self.state.angular_velocity;
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
            self.controls.aileron(),
            self.controls.elevator(),
            self.controls.rudder(),
            self.controls.throttle(),
            self.controls.flaps(),
            self.elapsed.get(),
        ]
        .map(f64::to_bits)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub samples: u32,
    pub roll_deg: Range,
    pub pitch_deg: Range,
    pub heading_error_deg: Range,
    pub true_airspeed_mps: Range,
    pub airspeed_disagreement_mps: Range,
    pub altitude_m: Range,
    pub acceleration_mps2: Range,
    pub normal_load_g: Range,
    pub load_deviation_g: Range,
    pub stall_fraction: Range,
    /// Order: aileron, elevator, rudder, throttle. Surface margin = 1 - absolute peak.
    pub controls: [Range; 4],
    pub saturation_samples: [u32; 4],
    pub gust: [Range; 3],
    pub quaternion_error: f64,
    pub final_state: RigidBodyState,
}

impl Metrics {
    fn new(initial: RigidBodyState) -> Self {
        Self {
            samples: 0,
            roll_deg: Range::default(),
            pitch_deg: Range::default(),
            heading_error_deg: Range::default(),
            true_airspeed_mps: Range::default(),
            airspeed_disagreement_mps: Range::default(),
            altitude_m: Range::default(),
            acceleration_mps2: Range::default(),
            normal_load_g: Range::default(),
            load_deviation_g: Range::default(),
            stall_fraction: Range::default(),
            controls: [Range::default(); 4],
            saturation_samples: [0; 4],
            gust: [Range::default(); 3],
            quaternion_error: 0.0,
            final_state: initial,
        }
    }

    fn add(&mut self, sample: Sample) {
        self.samples += 1;
        self.roll_deg.add(sample.roll_deg);
        self.pitch_deg.add(sample.pitch_deg);
        self.heading_error_deg.add(sample.heading_error_deg);
        self.true_airspeed_mps.add(sample.true_airspeed_mps);
        self.airspeed_disagreement_mps
            .add(sample.reported_airspeed_mps - sample.true_airspeed_mps);
        self.altitude_m.add(sample.altitude_m);
        self.acceleration_mps2.add(sample.acceleration_mps2);
        self.normal_load_g.add(sample.normal_load_g);
        self.load_deviation_g.add(sample.normal_load_g - 1.0);
        self.stall_fraction.add(sample.stall_fraction);
        for (i, value) in [
            sample.controls.aileron(),
            sample.controls.elevator(),
            sample.controls.rudder(),
            sample.controls.throttle(),
        ]
        .into_iter()
        .enumerate()
        {
            self.controls[i].add(value);
            if value.abs() >= 0.98 {
                self.saturation_samples[i] += 1;
            }
        }
        for (range, value) in
            self.gust
                .iter_mut()
                .zip([sample.gust.north(), sample.gust.east(), sample.gust.down()])
        {
            range.add(value);
        }
        self.quaternion_error = self
            .quaternion_error
            .max((sample.state.orientation.length() - 1.0).abs());
        self.final_state = sample.state;
    }
}

/// ECEF finite-difference acceleration and body-normal specific load.
/// A freely falling object has about 9.8 m/s² acceleration and 0 g specific load.
pub fn acceleration_sample(before: RigidBodyState, after: RigidBodyState) -> (f64, f64) {
    let acceleration = (after.velocity - before.velocity) / RECOMMENDED_FIXED_DT.get();
    let gravity =
        flightsim_fdm::gravity::acceleration_ecef(before.geodetic(), &before.local_frame());
    let specific_force_body = before.orientation.inverse() * (acceleration - gravity);
    (acceleration.length(), -specific_force_body.z / G0)
}

pub fn run(
    scenario: Scenario,
    turbulence: Turbulence,
    seconds: u32,
    mut observe: impl FnMut(Sample),
) -> Metrics {
    let initial = settled_state(scenario);
    let mut sim = simulation(initial);
    sim.set_turbulence(turbulence);
    let director = FlightDirector::default();
    let mut metrics = Metrics::new(initial);
    for _ in 0..seconds * SAMPLE_HZ {
        let before = *sim.state();
        let controls = director.control(&before, sim.agl(), scenario.targets());
        let report = sim.advance(RECOMMENDED_FIXED_DT, controls);
        assert_eq!(report.steps, 1, "the reference samples every physics step");
        assert!(
            !report.diverged && !sim.crashed() && sim.state().is_finite(),
            "scenario stopped or diverged"
        );
        let after = *sim.state();
        // Finite difference in the fixed ECEF frame, not differentiated body velocities.
        // Subtract modeled gravity before rotating to body axes. Positive nz supports the aircraft.
        let (acceleration_mps2, normal_load_g) = acceleration_sample(before, after);
        let attitude = after.attitude();
        let sample = Sample {
            elapsed: sim.elapsed(),
            state: after,
            controls,
            roll_deg: attitude.roll.to_degrees().get(),
            pitch_deg: attitude.pitch.to_degrees().get(),
            heading_error_deg: attitude
                .yaw
                .shortest_difference_to(Radians::ZERO)
                .to_degrees()
                .get(),
            true_airspeed_mps: sim.aero_angles().true_airspeed.get(),
            reported_airspeed_mps: sim.airspeed().get(),
            altitude_m: sim.agl().get(),
            acceleration_mps2,
            normal_load_g,
            stall_fraction: sim.stall_fraction(),
            gust: turbulence.gust_at(sim.elapsed(), after.geodetic()),
        };
        metrics.add(sample);
        observe(sample);
    }
    metrics
}

/// These deliberately broad budgets express a bounded, upright automated reference flight.
/// They must not be interpreted as FAA turbulence categories or structural aircraft limits.
#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    pub max_roll_deg: f64,
    pub max_pitch_deg: f64,
    pub max_acceleration_mps2: f64,
    pub min_load_g: f64,
    pub max_load_g: f64,
    pub min_surface_margin: f64,
}

impl Envelope {
    pub const fn for_severity(severity: Severity) -> Self {
        match severity {
            Severity::Calm => Self {
                max_roll_deg: 2.0,
                max_pitch_deg: 12.0,
                max_acceleration_mps2: 2.0,
                min_load_g: 0.8,
                max_load_g: 1.2,
                min_surface_margin: 0.65,
            },
            Severity::Light => Self {
                max_roll_deg: 10.0,
                max_pitch_deg: 15.0,
                max_acceleration_mps2: 8.0,
                min_load_g: 0.4,
                max_load_g: 1.7,
                min_surface_margin: 0.45,
            },
            Severity::Moderate => Self {
                max_roll_deg: 20.0,
                max_pitch_deg: 20.0,
                max_acceleration_mps2: 14.0,
                min_load_g: 0.0,
                max_load_g: 2.4,
                min_surface_margin: 0.25,
            },
            Severity::Severe => Self {
                max_roll_deg: 35.0,
                max_pitch_deg: 30.0,
                max_acceleration_mps2: 24.0,
                min_load_g: -0.5,
                max_load_g: 3.5,
                min_surface_margin: 0.05,
            },
        }
    }

    pub fn failures(self, metrics: &Metrics) -> Vec<&'static str> {
        let mut failures = Vec::new();
        let ranges = [
            metrics.roll_deg,
            metrics.pitch_deg,
            metrics.heading_error_deg,
            metrics.true_airspeed_mps,
            metrics.airspeed_disagreement_mps,
            metrics.altitude_m,
            metrics.acceleration_mps2,
            metrics.normal_load_g,
            metrics.load_deviation_g,
            metrics.stall_fraction,
        ];
        if metrics.samples == 0
            || !metrics.final_state.is_finite()
            || !metrics.quaternion_error.is_finite()
            || ranges
                .iter()
                .chain(metrics.controls.iter())
                .chain(metrics.gust.iter())
                .any(|range| !range.is_finite())
        {
            failures.push("invalid measurement");
        }
        if metrics.roll_deg.peak() > self.max_roll_deg {
            failures.push("roll");
        }
        if metrics.pitch_deg.peak() > self.max_pitch_deg {
            failures.push("pitch");
        }
        if metrics.acceleration_mps2.max > self.max_acceleration_mps2 {
            failures.push("acceleration");
        }
        if metrics.normal_load_g.min < self.min_load_g
            || metrics.normal_load_g.max > self.max_load_g
        {
            failures.push("normal load");
        }
        if metrics.controls[..3]
            .iter()
            .any(|axis| 1.0 - axis.peak() < self.min_surface_margin)
        {
            failures.push("surface margin");
        }
        if metrics.saturation_samples[..3]
            .iter()
            .any(|&samples| samples > 0)
        {
            failures.push("surface saturation");
        }
        if metrics.altitude_m.min < 100.0 {
            failures.push("terrain clearance");
        }
        if metrics.stall_fraction.max >= 1.0 {
            failures.push("stall");
        }
        if metrics.quaternion_error > 1e-12 {
            failures.push("quaternion norm");
        }
        failures
    }
}
