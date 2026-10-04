//! Numerical jet scenario helpers, independently tuned for authored test data.
//! These are regression drivers, not certified performance or an autopilot.
use flightsim_core::{Attitude, Geodetic, MetersPerSecond, Ned, Radians};
use flightsim_fdm::subsonic::{JetAircraftConfig, JetFlightDynamics};
use flightsim_fdm::{Atmosphere, ControlInputs, Environment, RigidBodyState};
use glam::{DMat3, DVec3};

#[derive(Debug, Clone, Copy)]
pub struct JetTrim {
    pub state: RigidBodyState,
    pub controls: ControlInputs,
    pub residual: DVec3,
}

/// Solve zero north/down acceleration and pitch moment using a finite-difference
/// Newton iteration over pitch, elevator and dry-thrust command. Flaps and path
/// descent are explicit inputs; unsupported or untrimmable points are errors.
/// # Errors
/// Invalid targets, unsupported model query, singular Jacobian or failed convergence.
pub fn solve_jet_trim(
    config: &JetAircraftConfig,
    position: Geodetic,
    speed: MetersPerSecond,
    descent: MetersPerSecond,
    flaps: f64,
) -> Result<JetTrim, String> {
    if !speed.is_finite()
        || speed.get() <= 0.0
        || !descent.is_finite()
        || descent.get().abs() >= speed.get()
        || !(0.0..=1.0).contains(&flaps)
    {
        return Err("invalid trim target".into());
    }
    let north = (speed.get() * speed.get() - descent.get() * descent.get()).sqrt();
    let make_state = |pitch| {
        RigidBodyState::from_geodetic(
            position,
            Attitude::new(Radians::ZERO, Radians(pitch), Radians::ZERO),
            Ned::new(north, 0.0, descent.get()),
        )
    };
    let environment =
        Environment::with_wind_ned(Atmosphere::standard(), position, Ned::new(0.0, 0.0, 0.0));
    let model =
        JetFlightDynamics::new(config.clone(), make_state(0.0)).map_err(|e| e.to_string())?;
    let evaluate = |x: DVec3| -> Result<DVec3, String> {
        if x.y.abs() > 1.0 || !(0.0..=1.0).contains(&x.z) {
            return Err("trim controls outside supported range".into());
        }
        let state = make_state(x.x);
        let controls = ControlInputs::neutral()
            .with_elevator(x.y)
            .with_throttle(x.z)
            .with_flaps(flaps);
        let derivative = model
            .derivative(&state, controls, &environment)
            .map_err(|e| e.to_string())?;
        let acceleration = state
            .local_frame()
            .ecef_to_ned_vector(derivative.acceleration)
            .0;
        Ok(DVec3::new(
            acceleration.x,
            acceleration.z,
            derivative.angular_acceleration.y,
        ))
    };
    let mut x = DVec3::new(0.03, 0.03, 0.5);
    for _ in 0..24 {
        let residual = evaluate(x)?;
        if residual.abs().max_element() < 1e-9 {
            return Ok(JetTrim {
                state: make_state(x.x),
                controls: ControlInputs::neutral()
                    .with_elevator(x.y)
                    .with_throttle(x.z)
                    .with_flaps(flaps),
                residual,
            });
        }
        let h = 1e-5;
        let mut columns = [DVec3::ZERO; 3];
        for (i, column) in columns.iter_mut().enumerate() {
            let mut shifted = x;
            shifted[i] += h;
            *column = (evaluate(shifted)? - residual) / h;
        }
        let matrix = DMat3::from_cols(columns[0], columns[1], columns[2]);
        if !matrix.determinant().is_finite() || matrix.determinant().abs() < 1e-12 {
            return Err("singular trim Jacobian".into());
        }
        let update = matrix.inverse() * residual;
        let mut scale = 1.0;
        let mut improved = None;
        for _ in 0..16 {
            let candidate = x - update * scale;
            if let Ok(next) = evaluate(candidate) {
                if next.length_squared() < residual.length_squared() {
                    improved = Some(candidate);
                    break;
                }
            }
            scale *= 0.5;
        }
        x = improved.ok_or_else(|| "trim line search did not converge".to_owned())?;
    }
    Err("trim did not converge in 24 iterations".into())
}

/// Dedicated takeoff driver for the numerical profile. Target pitch ramps after
/// 35 m/s; body-rate damping and roll/yaw leveling are explicit test gains.
#[derive(Debug, Clone, Copy, Default)]
pub struct JetTakeoffDriver {
    target_pitch: f64,
}
impl JetTakeoffDriver {
    #[must_use]
    pub fn controls(
        &mut self,
        dt: flightsim_core::Seconds,
        state: &RigidBodyState,
    ) -> ControlInputs {
        if state.ground_speed().get() > 35.0 {
            self.target_pitch = (self.target_pitch + 0.04 * dt.get()).min(0.12);
        }
        let attitude = state.attitude();
        ControlInputs::neutral()
            .with_throttle(1.0)
            .with_flaps(0.25)
            .with_elevator(
                0.06 + 1.8 * (self.target_pitch - attitude.pitch.get())
                    - 0.8 * state.angular_velocity.y,
            )
            .with_aileron(-1.5 * attitude.roll.get() - 0.5 * state.angular_velocity.x)
            .with_rudder(-0.8 * attitude.yaw.get() - 0.3 * state.angular_velocity.z)
    }
}
