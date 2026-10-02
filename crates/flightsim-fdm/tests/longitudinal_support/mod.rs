//! Independent steady-flight force balance for numerical tests, not a controller.
//! Wind-axis equations: T cos(alpha) - D = W sin(gamma),
//! L + T sin(alpha) = W cos(gamma), and Cm = 0.
use flightsim_core::{Attitude, Geodetic, Meters, MetersPerSecond, Ned, Radians};
use flightsim_fdm::{AircraftConfig, Atmosphere, ControlInputs, RigidBodyState, aero, gravity};
use glam::DVec3;

const PROFILES: [&str; 2] = [
    include_str!("../../../../assets/aircraft/light_single.json"),
    include_str!("../../../../assets/aircraft/swift_sport.json"),
];

pub fn default_trims() -> [f64; 2] {
    PROFILES.map(|json| {
        serde_json::from_str::<serde_json::Value>(json).unwrap()["controls"]["default_trim"]
            .as_f64()
            .unwrap()
    })
}

pub fn configs() -> [AircraftConfig; 2] {
    PROFILES.map(|json| {
        let p: serde_json::Value = serde_json::from_str(json).unwrap();
        serde_json::from_value::<flightsim_fdm::definition::AircraftDefinition>(
            p["dynamics"].clone(),
        )
        .unwrap()
        .to_config()
        .unwrap()
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Trim {
    pub alpha: f64,
    pub gamma: f64,
    pub tas: f64,
    pub controls: ControlInputs,
}
impl Trim {
    pub fn state(self, altitude: f64) -> RigidBodyState {
        RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.0, 139.0, altitude),
            Attitude::new(
                Radians::ZERO,
                Radians(self.alpha + self.gamma),
                Radians::ZERO,
            ),
            Ned::new(
                self.tas * self.gamma.cos(),
                0.0,
                -self.tas * self.gamma.sin(),
            ),
        )
    }
}

pub fn loads(
    c: &AircraftConfig,
    alpha: f64,
    tas: f64,
    atmosphere: Atmosphere,
    altitude: f64,
) -> (f64, f64) {
    let coeff = aero::coefficients(
        &c.aero,
        &c.geometry,
        aero::AeroAngles {
            angle_of_attack: Radians(alpha),
            sideslip: Radians::ZERO,
            true_airspeed: MetersPerSecond(tas),
        },
        DVec3::ZERO,
        ControlInputs::neutral(),
    );
    let q_s = 0.5
        * atmosphere.sample(Meters(altitude)).density.get()
        * tas
        * tas
        * c.geometry.wing_area.get();
    (q_s * coeff.lift, q_s * coeff.drag)
}

fn root(mut low: f64, mut high: f64, f: impl Fn(f64) -> f64) -> Option<f64> {
    let mut f_low = f(low);
    let f_high = f(high);
    if !f_low.is_finite() || !f_high.is_finite() || f_low * f_high > 0.0 {
        return None;
    }
    for _ in 0..60 {
        let mid = (low + high) * 0.5;
        let value = f(mid);
        if !value.is_finite() {
            return None;
        }
        if f_low * value <= 0.0 {
            high = mid;
        } else {
            low = mid;
            f_low = value;
        }
    }
    Some((low + high) * 0.5)
}

pub fn level(c: &AircraftConfig, ias: f64, altitude: f64, atmosphere: Atmosphere) -> Option<Trim> {
    let air = atmosphere.sample(Meters(altitude));
    let tas = ias / air.density_ratio().sqrt();
    let weight = c.mass_properties.mass().get()
        * gravity::magnitude(Geodetic::from_degrees(35.0, 139.0, altitude));
    let alpha = root(-0.1, 0.20, |alpha| {
        let (l, d) = loads(c, alpha, tas, atmosphere, altitude);
        l + d * alpha.tan() - weight
    })?;
    let (_, drag) = loads(c, alpha, tas, atmosphere, altitude);
    let thrust = drag / alpha.cos();
    let throttle = root(0.0, 1.0, |t| {
        c.engine.thrust(t, tas, air.density_ratio()).get() - thrust
    })?;
    let elevator = -(c.aero.pitch_zero + c.aero.pitch_alpha * alpha) / c.aero.pitch_elevator;
    if elevator.abs() > 1.0 {
        return None;
    }
    Some(Trim {
        alpha,
        gamma: 0.0,
        tas,
        controls: ControlInputs::neutral()
            .with_throttle(throttle)
            .with_elevator(elevator),
    })
}

pub fn climb(
    c: &AircraftConfig,
    ias: f64,
    throttle: f64,
    altitude: f64,
    atmosphere: Atmosphere,
) -> Option<Trim> {
    let air = atmosphere.sample(Meters(altitude));
    let tas = ias / air.density_ratio().sqrt();
    let weight = c.mass_properties.mass().get()
        * gravity::magnitude(Geodetic::from_degrees(35.0, 139.0, altitude));
    let thrust = c.engine.thrust(throttle, tas, air.density_ratio()).get();
    let residual = |alpha: f64| {
        let (l, d) = loads(c, alpha, tas, atmosphere, altitude);
        let sin_gamma = (thrust * alpha.cos() - d) / weight;
        l + thrust * alpha.sin() - weight * (1.0 - sin_gamma * sin_gamma).sqrt()
    };
    let alpha = root(-0.1, 0.20, residual)?;
    let (_, drag) = loads(c, alpha, tas, atmosphere, altitude);
    let gamma = ((thrust * alpha.cos() - drag) / weight).asin();
    let elevator = -(c.aero.pitch_zero + c.aero.pitch_alpha * alpha) / c.aero.pitch_elevator;
    if !gamma.is_finite() || elevator.abs() > 1.0 {
        return None;
    }
    Some(Trim {
        alpha,
        gamma,
        tas,
        controls: ControlInputs::neutral()
            .with_throttle(throttle)
            .with_elevator(elevator),
    })
}
