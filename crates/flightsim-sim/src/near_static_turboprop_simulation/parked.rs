//! Initialization only: the live turboprop law and replay frame-zero path are unchanged.
use super::{
    NearStaticTurbopropEnvironment, NearStaticTurbopropSimulation,
    NearStaticTurbopropSimulationError,
};
use crate::GroundPlane;
use crate::aircraft_profile_v4::RunningTurbopropStart;
use flightsim_core::{Attitude, Geodetic, Meters, Ned, Radians};
use flightsim_fdm::turboprop::near_static::{TurbopropAircraftConfig, TurbopropState};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use glam::{DMat3, DQuat, DVec3};

/// Curved geodetic contact heights differ slightly from their local tangent
/// plane. Placement must still put every actual wheel within the FDM's 1 mm
/// contact margin. Large/degenerate gear layouts fail rather than being bent.
const PARKED_CLEARANCE_TOLERANCE: Meters = Meters(0.001);

impl NearStaticTurbopropSimulation {
    /// Place all three authored gear contacts along the sim-owned ground slope.
    /// `start` selects latitude/longitude; terrain determines the CG altitude.
    /// This is uncompressed geometric contact, not a static suspension solve or
    /// a parking lock. Idle thrust and gravity remain active on the first tick.
    ///
    /// # Errors
    /// Invalid coordinates/heading, unavailable environment, unsupported gear
    /// geometry, contact residual over 1 mm, or an unsupported initial turboprop query.
    pub fn parked(
        config: TurbopropAircraftConfig,
        start: Geodetic,
        heading: Radians,
        environment: NearStaticTurbopropEnvironment,
        engine: RunningTurbopropStart,
    ) -> Result<Self, NearStaticTurbopropSimulationError> {
        if !start.latitude.is_finite()
            || !start.longitude.is_finite()
            || !start.altitude.is_finite()
            || !heading.is_finite()
            || start.latitude.get().abs() > std::f64::consts::FRAC_PI_2
            || start.longitude.get().abs() > std::f64::consts::PI
        {
            return Err(NearStaticTurbopropSimulationError::Parked(
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
        let complete = |rigid_body| TurbopropState {
            rigid_body,
            turbine_fraction: engine.turbine_fraction(),
            shaft_rad_s: engine.shaft_speed(),
            blade_pitch_rad: engine.blade_pitch(),
        };
        let probe = Self::from_state(config.clone(), complete(initial), environment)?;
        // Terrain and the pose must share core's canonical local frame. At
        // exact poles, ECEF -> geodetic selects longitude zero regardless of
        // the caller's physically ambiguous longitude.
        let state = aligned_state(
            &config,
            probe.state().rigid_body.geodetic(),
            probe.committed.ground,
            heading,
        )?;
        // Use the same frame-zero constructor as replay, including the actual
        // round-tripped geodetic reference and its actual FDM gear clearances.
        let sim = Self::from_supported_state(
            config,
            complete(state),
            environment,
            ControlInputs::neutral(),
        )?;
        if sim.committed.gear_clearances.iter().any(|clearance| {
            !clearance.is_finite() || clearance.get().abs() > PARKED_CLEARANCE_TOLERANCE.get()
        }) {
            return Err(NearStaticTurbopropSimulationError::Parked(
                "parked gear cannot align with the sampled ground within one millimetre",
            ));
        }
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
        engine: RunningTurbopropStart,
    ) -> Result<(), NearStaticTurbopropSimulationError> {
        let replacement = Self::parked(
            self.config().clone(),
            start,
            heading,
            self.environment,
            engine,
        )?;
        *self = replacement;
        Ok(())
    }
}

fn aligned_state(
    config: &TurbopropAircraftConfig,
    position: Geodetic,
    ground: GroundPlane,
    heading: Radians,
) -> Result<RigidBodyState, NearStaticTurbopropSimulationError> {
    let contacts = config
        .airframe()
        .landing_gear()
        .legs()
        .map(|leg| leg.contact_point().as_vec());
    let a = contacts[1] - contacts[0];
    let b = contacts[2] - contacts[0];
    let mut gear_down = a.cross(b);
    if gear_down.length() <= 1e-9 * a.length() * b.length() || gear_down.length() <= 1e-9 {
        return Err(NearStaticTurbopropSimulationError::Parked(
            "parked gear contacts must form a nondegenerate triangle",
        ));
    }
    gear_down = gear_down.normalize();
    if gear_down.z < 0.0 {
        gear_down = -gear_down;
    }
    let support_height = gear_down.dot(contacts[0]);
    if gear_down.z <= 1e-6 || support_height <= 0.0 {
        return Err(NearStaticTurbopropSimulationError::Parked(
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
