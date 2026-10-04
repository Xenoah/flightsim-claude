//! Initialization only: the live jet law and replay frame-zero path are unchanged.
use super::{JetEnvironment, JetSimulation, JetSimulationError};
use crate::GroundPlane;
use flightsim_core::{Attitude, Geodetic, Meters, Ned, Radians};
use flightsim_fdm::subsonic::JetAircraftConfig;
use flightsim_fdm::{ControlInputs, RigidBodyState};
use glam::{DMat3, DQuat, DVec3};

/// Curved geodetic contact heights differ slightly from their local tangent
/// plane. Placement must still put every actual wheel within the FDM's 1 mm
/// contact margin. Large/degenerate gear layouts fail rather than being bent.
const PARKED_CLEARANCE_TOLERANCE: Meters = Meters(0.001);

impl JetSimulation {
    /// Place all three authored gear contacts along the sim-owned ground slope.
    /// `start` selects latitude/longitude; terrain determines the CG altitude.
    /// This is uncompressed geometric contact, not a static suspension solve or
    /// a parking lock. Idle thrust and gravity remain active on the first tick.
    ///
    /// # Errors
    /// Invalid coordinates/heading, unavailable environment, unsupported gear
    /// geometry, contact residual over 1 mm, or an unsupported initial jet query.
    pub fn parked(
        config: JetAircraftConfig,
        start: Geodetic,
        heading: Radians,
        environment: JetEnvironment,
    ) -> Result<Self, JetSimulationError> {
        if !start.latitude.is_finite()
            || !start.longitude.is_finite()
            || !start.altitude.is_finite()
            || !heading.is_finite()
            || start.latitude.get().abs() > std::f64::consts::FRAC_PI_2
            || start.longitude.get().abs() > std::f64::consts::PI
        {
            return Err(JetSimulationError::Parked(
                "parked position and heading must be finite, with geographic latitude/longitude",
            ));
        }
        // Only sim constructs the terrain/climate bridge. The provisional state
        // is never exposed, recorded or stepped and its altitude is not a guess
        // about the eventual terrain elevation.
        let position = Geodetic::new(start.latitude, start.longitude, Meters::ZERO);
        let initial = RigidBodyState::from_geodetic(
            position,
            Attitude::new(Radians::ZERO, Radians::ZERO, heading),
            Ned::new(0.0, 0.0, 0.0),
        );
        let probe = Self::from_state(config.clone(), initial, environment)?;
        // Terrain and the pose must share core's canonical local frame. At
        // exact poles, ECEF -> geodetic selects longitude zero regardless of
        // the caller's physically ambiguous longitude.
        let state = aligned_state(
            &config,
            probe.state().geodetic(),
            probe.committed.ground,
            heading,
        )?;
        // Use the same frame-zero constructor as replay, including the actual
        // round-tripped geodetic reference and its actual FDM gear clearances.
        let sim = Self::from_state(config, state, environment)?;
        if sim.committed.gear_clearances.iter().any(|clearance| {
            !clearance.is_finite() || clearance.get().abs() > PARKED_CLEARANCE_TOLERANCE.get()
        }) {
            return Err(JetSimulationError::Parked(
                "parked gear cannot align with the sampled ground within one millimetre",
            ));
        }
        sim.dynamics
            .derivative(
                sim.state(),
                ControlInputs::neutral(),
                &sim.environment_at(sim.state(), sim.committed.ground, sim.elapsed()),
            )
            .map_err(JetSimulationError::Initial)?;
        Ok(sim)
    }

    /// Transactionally replace the complete live flight with a pristine parked
    /// start. Failed placement preserves clocks, history and terminal state.
    /// # Errors
    /// The same placement/environment errors as [`Self::parked`].
    pub fn restart_parked_at(
        &mut self,
        start: Geodetic,
        heading: Radians,
    ) -> Result<(), JetSimulationError> {
        let replacement = Self::parked(self.config().clone(), start, heading, self.environment)?;
        *self = replacement;
        Ok(())
    }
}

fn aligned_state(
    config: &JetAircraftConfig,
    position: Geodetic,
    ground: GroundPlane,
    heading: Radians,
) -> Result<RigidBodyState, JetSimulationError> {
    let contacts = config
        .airframe()
        .landing_gear()
        .legs()
        .map(|leg| leg.contact_point().as_vec());
    let a = contacts[1] - contacts[0];
    let b = contacts[2] - contacts[0];
    let mut gear_down = a.cross(b);
    if gear_down.length() <= 1e-9 * a.length() * b.length() || gear_down.length() <= 1e-9 {
        return Err(JetSimulationError::Parked(
            "parked gear contacts must form a nondegenerate triangle",
        ));
    }
    gear_down = gear_down.normalize();
    if gear_down.z < 0.0 {
        gear_down = -gear_down;
    }
    let support_height = gear_down.dot(contacts[0]);
    if gear_down.z <= 1e-6 || support_height <= 0.0 {
        return Err(JetSimulationError::Parked(
            "parked gear support plane must lie below the centre of gravity",
        ));
    }
    let gear_forward = (DVec3::X - gear_down * gear_down.x).normalize();
    let gear_right = gear_down.cross(gear_forward);
    let ground_down = DVec3::new(ground.slope.north(), ground.slope.east(), 1.0).normalize();
    let (sin, cos) = heading.get().sin_cos();
    let level_forward = DVec3::new(cos, sin, 0.0);
    let ground_forward = (level_forward - ground_down * level_forward.dot(ground_down)).normalize();
    let ground_right = ground_down.cross(ground_forward);
    let rotation = DQuat::from_mat3(
        &(DMat3::from_cols(ground_forward, ground_right, ground_down)
            * DMat3::from_cols(gear_forward, gear_right, gear_down).transpose()),
    )
    .normalize();
    let attitude = Attitude::from_quaternion(rotation);
    // Preserve the existing sim convention: heading is projected along the
    // local slope, and altitude is vertical height above sampled elevation.
    let height = support_height / ground_down.z;
    Ok(RigidBodyState::from_geodetic(
        Geodetic::new(
            position.latitude,
            position.longitude,
            Meters(ground.elevation.get() + height),
        ),
        attitude,
        Ned::new(0.0, 0.0, 0.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_fdm::subsonic::JetFlightDynamics;
    use flightsim_fdm::{Atmosphere, Environment, GroundSlope};

    #[test]
    fn actual_fdm_contacts_align_at_every_heading_on_positive_and_negative_slopes() {
        let config = crate::aircraft_profile::AircraftProfileV2::parse(include_str!(
            "../../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
        ))
        .unwrap()
        .configuration()
        .clone();
        let position = Geodetic::from_degrees(35.55, 139.33, 0.);
        let tangent = 15_f64.to_radians().tan();
        for heading in [0_f64, 45., 90., 135., 180., 225., 270., 315.] {
            for (north, east) in [
                (tangent, 0.),
                (-tangent, 0.),
                (0., tangent),
                (tangent, -tangent),
            ] {
                let ground = GroundPlane {
                    reference: position,
                    elevation: Meters(1400.),
                    slope: GroundSlope::new(north, east),
                    from_terrain: true,
                };
                let state = aligned_state(&config, position, ground, Radians(heading.to_radians()))
                    .unwrap();
                let environment = Environment::with_wind_ned(
                    Atmosphere::standard(),
                    state.geodetic(),
                    Ned::new(0., 0., 0.),
                )
                .with_ground_plane(
                    ground.reference,
                    ground.elevation,
                    ground.slope,
                );
                let dynamics = JetFlightDynamics::new(config.clone(), state).unwrap();
                let clearances = dynamics.gear_clearances(&state, &environment).unwrap();
                assert!(
                    clearances.iter().all(|c| c.get().abs() < 1e-6),
                    "heading {heading}, slope {north},{east}: {clearances:?}"
                );
            }
        }
    }
}
