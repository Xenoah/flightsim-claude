//! Separate, bounded jet integration law; legacy FlightDynamics is untouched.

use super::{
    AxisStatus, DryThrottle, EvaluationError, JetAircraftConfig, JetConditions, JetDomainStatus,
};
use crate::{
    Atmosphere, AtmosphereSample, ControlInputs, Environment, RigidBodyState, StateDerivative,
    aero, gravity, landing_gear, state,
};
use flightsim_core::{Geodetic, LocalFrame, Meters, MetersPerSecond, Seconds};
use glam::{DVec3, DVec4};

/// Host model discriminator; independent of component/profile/replay schemas.
pub const DRY_JET_MODEL_KIND_ID: u16 = 2;
/// Jet law, including rejection, integration and numerical-bound semantics.
pub const JET_FDM_MODEL_REVISION: u32 = 1;
pub const MAX_JET_SUBSTEPS: u32 = 8;
pub const MAX_JET_SUBSTEP_DT: Seconds = Seconds(1.0 / 120.0);
pub const MAX_JET_STEP_DT: Seconds = Seconds(8.0 / 120.0);
const MAX_PHASE: f64 = 0.05;
// Permits roundoff only when checking an already-chosen substep phase, not
// domain endpoints or the caller's maximum dt. This is model-law arithmetic.
const PHASE_ROUNDOFF: f64 = 32.0 * f64::EPSILON;
const MIN_ALTITUDE: f64 = -5_000.0;
const MAX_ALTITUDE: f64 = 86_000.0;

/// Closed numerical-input diagnostic codes, stable under jet law revision 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum JetInvalidInput {
    TimeStep = 1,
    State = 2,
    Quaternion = 3,
    Position = 4,
    AtmosphereOffset = 5,
    Ground = 6,
    Wind = 7,
    RelativeVelocity = 8,
    AtmosphereTemperature = 9,
    AtmosphereDensity = 10,
    ComponentQuery = 11,
    Derivative = 12,
    IntermediateState = 13,
    AngularRate = 14,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JetStage {
    Initial = 0,
    K1 = 1,
    K2 = 2,
    K3 = 3,
    K4 = 4,
    Endpoint = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JetFailureReason {
    InvalidInput(JetInvalidInput),
    OutsideAtmosphereAltitude(AxisStatus),
    OutsideOperatingEnvelope(JetDomainStatus),
    OutsideJetDomain(JetDomainStatus),
    OutsideMachDomain(AxisStatus),
    SubstepBudgetExceeded,
}

/// A failure never advances the owned state. Query is present only after all
/// ambient quantities and the raw relative-speed norm were validated as finite.
/// Substep is zero-based; Initial failures occur before any attempted substep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JetStepError {
    pub reason: JetFailureReason,
    pub substep: u32,
    pub stage: JetStage,
    pub query: Option<JetConditions>,
}
impl std::fmt::Display for JetStepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "jet step rejected at substep {} {:?}: {:?}",
            self.substep, self.stage, self.reason
        )
    }
}
impl std::error::Error for JetStepError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JetStepReport {
    pub substeps: u32,
    /// Endpoint signed normal clearances for the same frozen ground plane used
    /// by this call's forces. Positive is above ground; negative is penetration.
    pub gear_clearances: [Meters; 3],
}

#[derive(Debug, Clone, Copy)]
struct Context {
    substep: u32,
    stage: JetStage,
    query: Option<JetConditions>,
}
impl Context {
    const INITIAL: Self = Self {
        substep: 0,
        stage: JetStage::Initial,
        query: None,
    };
    fn error(self, reason: JetFailureReason) -> JetStepError {
        JetStepError {
            reason,
            substep: self.substep,
            stage: self.stage,
            query: self.query,
        }
    }
    fn invalid(self, input: JetInvalidInput) -> JetStepError {
        self.error(JetFailureReason::InvalidInput(input))
    }
    fn component(self, error: EvaluationError) -> JetStepError {
        self.error(match error {
            EvaluationError::InvalidInput(_) => {
                JetFailureReason::InvalidInput(JetInvalidInput::ComponentQuery)
            }
            EvaluationError::OutsideJetDomain(domain) => JetFailureReason::OutsideJetDomain(domain),
            EvaluationError::OutsideMachDomain(domain) => {
                JetFailureReason::OutsideMachDomain(domain)
            }
        })
    }
}

struct Evaluated {
    derivative: StateDerivative,
    frame: LocalFrame,
    context: Context,
}

/// Six-DoF jet law with no spool, fuel, variable-gear or other hidden state.
/// Installed aggregate NET thrust acts along body +X through the center of mass:
/// no engine count multiplier, inlet/ram correction or thrust-offset moment.
#[derive(Debug, Clone)]
pub struct JetFlightDynamics {
    config: JetAircraftConfig,
    state: RigidBodyState,
}

impl JetFlightDynamics {
    /// Construction checks the state; atmosphere/envelope support is checked
    /// when an environment is provided to derivative/step.
    /// # Errors
    /// Nonfinite state, invalid quaternion, or unsupported geometric altitude.
    pub fn new(config: JetAircraftConfig, state: RigidBodyState) -> Result<Self, JetStepError> {
        validate_state(&state, Context::INITIAL)?;
        Ok(Self { config, state })
    }
    #[must_use]
    pub const fn config(&self) -> &JetAircraftConfig {
        &self.config
    }
    #[must_use]
    pub const fn state(&self) -> &RigidBodyState {
        &self.state
    }

    /// # Errors
    /// The same state checks as construction. A failed replacement is atomic.
    pub fn set_state(&mut self, state: RigidBodyState) -> Result<(), JetStepError> {
        validate_state(&state, Context::INITIAL)?;
        self.state = state;
        Ok(())
    }

    /// Reevaluate the complete force law at the supplied state and environment.
    /// No previous force/coefficient is retained. This does not change state.
    /// # Errors
    /// Invalid numerical inputs or unsupported atmosphere/envelope/component query.
    pub fn derivative(
        &self,
        state: &RigidBodyState,
        controls: ControlInputs,
        environment: &Environment,
    ) -> Result<StateDerivative, JetStepError> {
        self.evaluate(state, controls, environment, Context::INITIAL)
            .map(|value| value.derivative)
    }

    /// Signed normal clearance of each rotated gear contact from the supplied
    /// ground plane, in configured leg order. Positive is above the plane;
    /// zero/negative means geometric contact/penetration. This diagnostic does
    /// not evaluate force, clip state, or classify body collision or a crash.
    ///
    /// # Errors
    /// Invalid state/environment or nonfinite contact geometry. This validates
    /// numerical geometry, not the atmosphere/table operating envelope.
    pub fn gear_clearances(
        &self,
        state: &RigidBodyState,
        environment: &Environment,
    ) -> Result<[Meters; 3], JetStepError> {
        let position = validate_state(state, Context::INITIAL)?;
        validate_environment(environment, Context::INITIAL)?;
        self.checked_gear_clearances(
            state,
            environment,
            &LocalFrame::new(position),
            Context::INITIAL,
        )
    }

    fn checked_gear_clearances(
        &self,
        state: &RigidBodyState,
        environment: &Environment,
        frame: &LocalFrame,
        context: Context,
    ) -> Result<[Meters; 3], JetStepError> {
        let clearances = landing_gear::signed_normal_clearances(
            self.config.airframe().landing_gear(),
            state,
            environment,
            frame,
        );
        if clearances.iter().any(|clearance| !clearance.is_finite()) {
            return Err(context.invalid(JetInvalidInput::Ground));
        }
        Ok(clearances)
    }

    /// Advance a requested fixed duration transactionally. ControlInputs are
    /// already sanitized effective commands at their existing construction boundary.
    ///
    /// At most eight equal substeps, each <=1/120 s, are selected from initial
    /// angular speed and imminent-contact gear frequency. Every RK4 stage and
    /// each accepted endpoint checks its domain and phase limit again. Initial
    /// contact prediction looks ahead dt; stage/endpoint gear checks use current
    /// contact (including the shared 1 mm margin), never another future interval.
    /// Later increased stiffness/rate rejects the call rather than capping it.
    /// Zero duration validates the current evaluation and returns zero substeps.
    ///
    /// # Errors
    /// Invalid input, unsupported intermediate/endpoint query, or budget failure.
    /// ALL failures preserve the original state, even after earlier substeps passed.
    pub fn step(
        &mut self,
        dt: Seconds,
        controls: ControlInputs,
        environment: &Environment,
    ) -> Result<JetStepReport, JetStepError> {
        let initial = Context::INITIAL;
        if !dt.is_finite() || dt.get() < 0.0 {
            return Err(initial.invalid(JetInvalidInput::TimeStep));
        }
        if dt.get() > MAX_JET_STEP_DT.get() {
            return Err(initial.error(JetFailureReason::SubstepBudgetExceeded));
        }
        let evaluated = self.evaluate(&self.state, controls, environment, initial)?;
        if dt.get() == 0.0 {
            let gear_clearances = self.checked_gear_clearances(
                &self.state,
                environment,
                &evaluated.frame,
                evaluated.context,
            )?;
            return Ok(JetStepReport {
                substeps: 0,
                gear_clearances,
            });
        }
        let required = self.required_ratio(
            &self.state,
            environment,
            &evaluated.frame,
            dt,
            dt,
            evaluated.context,
        )?;
        let count = required.ceil().max(1.0);
        if count > f64::from(MAX_JET_SUBSTEPS) {
            return Err(evaluated
                .context
                .error(JetFailureReason::SubstepBudgetExceeded));
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "finite integer bounded to 1..=8"
        )]
        let substeps = count as u32;
        let h = dt.get() / f64::from(substeps);
        let mut candidate = self.state;
        let mut gear_clearances = [Meters::ZERO; 3];
        for substep in 0..substeps {
            (candidate, gear_clearances) =
                self.integrate_rk4(&candidate, h, controls, environment, substep)?;
        }
        self.state = candidate;
        Ok(JetStepReport {
            substeps,
            gear_clearances,
        })
    }

    fn required_ratio(
        &self,
        state: &RigidBodyState,
        environment: &Environment,
        frame: &LocalFrame,
        dt: Seconds,
        contact_lookahead: Seconds,
        context: Context,
    ) -> Result<f64, JetStepError> {
        let angular_speed = state.angular_velocity.length();
        if !angular_speed.is_finite() {
            return Err(context.invalid(JetInvalidInput::AngularRate));
        }
        let airframe = self.config.airframe();
        let gear_ratio = if landing_gear::contact_is_active_or_imminent(
            airframe.landing_gear(),
            state,
            environment,
            frame,
            contact_lookahead,
        ) {
            let frequency = landing_gear::maximum_natural_frequency(
                airframe.landing_gear(),
                airframe.mass_properties(),
                state,
                environment,
                frame,
            );
            frequency * dt.get() / landing_gear::MAX_GEAR_PHASE_PER_SUBSTEP
        } else {
            0.0
        };
        let rotation_ratio = angular_speed * dt.get() / MAX_PHASE;
        if !gear_ratio.is_finite() || !rotation_ratio.is_finite() {
            return Err(context.error(JetFailureReason::SubstepBudgetExceeded));
        }
        Ok((dt.get() / MAX_JET_SUBSTEP_DT.get())
            .max(rotation_ratio)
            .max(gear_ratio))
    }

    fn stage(
        &self,
        state: &RigidBodyState,
        controls: ControlInputs,
        environment: &Environment,
        h: f64,
        context: Context,
    ) -> Result<Evaluated, JetStepError> {
        let evaluated = self.evaluate(state, controls, environment, context)?;
        let ratio = self.required_ratio(
            state,
            environment,
            &evaluated.frame,
            Seconds(h),
            // This stage already has its own advanced state. Another h of
            // lookahead would reject contact lying beyond the requested step.
            // Keep the shared predicate's 1 mm margin, but check current contact.
            Seconds::ZERO,
            evaluated.context,
        )?;
        if ratio > 1.0 + PHASE_ROUNDOFF {
            return Err(evaluated
                .context
                .error(JetFailureReason::SubstepBudgetExceeded));
        }
        Ok(evaluated)
    }

    fn integrate_rk4(
        &self,
        start: &RigidBodyState,
        h: f64,
        controls: ControlInputs,
        environment: &Environment,
        substep: u32,
    ) -> Result<(RigidBodyState, [Meters; 3]), JetStepError> {
        let at = |stage| Context {
            substep,
            stage,
            query: None,
        };
        let k1 = self
            .stage(start, controls, environment, h, at(JetStage::K1))?
            .derivative;
        let s2 = checked_offset(start, &k1, h * 0.5, at(JetStage::K2))?;
        let k2 = self
            .stage(&s2, controls, environment, h, at(JetStage::K2))?
            .derivative;
        let s3 = checked_offset(start, &k2, h * 0.5, at(JetStage::K3))?;
        let k3 = self
            .stage(&s3, controls, environment, h, at(JetStage::K3))?
            .derivative;
        let s4 = checked_offset(start, &k3, h, at(JetStage::K4))?;
        let k4 = self
            .stage(&s4, controls, environment, h, at(JetStage::K4))?
            .derivative;
        let weighted = (k1 + k2 * 2.0 + k3 * 2.0 + k4) * (1.0 / 6.0);
        let end = checked_offset(start, &weighted, h, at(JetStage::Endpoint))?;
        let endpoint = self.stage(&end, controls, environment, h, at(JetStage::Endpoint))?;
        let clearances =
            self.checked_gear_clearances(&end, environment, &endpoint.frame, endpoint.context)?;
        Ok((end, clearances))
    }

    fn evaluate(
        &self,
        state: &RigidBodyState,
        controls: ControlInputs,
        environment: &Environment,
        mut context: Context,
    ) -> Result<Evaluated, JetStepError> {
        let position = validate_state(state, context)?;
        validate_environment(environment, context)?;
        // Validate RAW relative components and both norms before aero_angles:
        // that legacy helper intentionally masks nonfinite speed as zero.
        let relative = state.velocity - environment.wind_ecef;
        let body_relative = state.orientation.inverse() * relative;
        let speed = body_relative.length();
        if !relative.is_finite()
            || !relative.length().is_finite()
            || !body_relative.is_finite()
            || !speed.is_finite()
        {
            return Err(context.invalid(JetInvalidInput::RelativeVelocity));
        }
        // sample() clips altitude and floors T at 1 K. Validate its raw inputs
        // first; standard ISA remains reused, without duplicating layer equations.
        let standard = Atmosphere::standard().sample(position.altitude);
        let raw_temperature =
            standard.temperature.get() + environment.atmosphere.temperature_offset();
        if !raw_temperature.is_finite() || raw_temperature < 1.0 {
            return Err(context.invalid(JetInvalidInput::AtmosphereTemperature));
        }
        let air = environment.atmosphere.sample(position.altitude);
        let query = checked_conditions(air, MetersPerSecond(speed), context)?;
        context.query = Some(query);
        let domain = self.config.envelope().status(query);
        if !domain.is_supported() {
            return Err(context.error(JetFailureReason::OutsideOperatingEnvelope(domain)));
        }
        let thrust = self
            .config
            .thrust()
            .sample(query, DryThrottle(controls.throttle()))
            .map_err(|error| context.component(error))?;
        let coefficients = self
            .config
            .aero()
            .sample(query.mach)
            .map_err(|error| context.component(error))?
            .coefficients;
        let angles = aero::aero_angles(body_relative);
        let airframe = self.config.airframe();
        let (aero_force, aero_moment) = aero::body_force_and_moment(
            &coefficients,
            airframe.geometry(),
            angles,
            state.angular_velocity,
            controls,
            air.density,
        );
        let frame = LocalFrame::new(position);
        let ground = landing_gear::loads(
            airframe.landing_gear(),
            state,
            controls,
            environment,
            &frame,
        );
        let force = aero_force + DVec3::X * thrust.net_thrust.get() + ground.force_body;
        let mass = airframe.mass_properties();
        let acceleration = state.orientation * (force / mass.mass().get())
            + gravity::acceleration_ecef(position, &frame);
        let momentum = mass.inertia() * state.angular_velocity;
        let angular_acceleration = mass.inverse_inertia()
            * (aero_moment + ground.moment_body - state.angular_velocity.cross(momentum));
        let derivative = StateDerivative {
            velocity: state.velocity,
            acceleration,
            orientation_rate: state::orientation_rate(state.orientation, state.angular_velocity),
            angular_acceleration,
        };
        if !derivative_is_finite(derivative) {
            return Err(context.invalid(JetInvalidInput::Derivative));
        }
        Ok(Evaluated {
            derivative,
            frame,
            context,
        })
    }
}

fn derivative_is_finite(derivative: StateDerivative) -> bool {
    derivative.velocity.is_finite()
        && derivative.acceleration.is_finite()
        && derivative.orientation_rate.is_finite()
        && derivative.angular_acceleration.is_finite()
}

fn validate_state(state: &RigidBodyState, context: Context) -> Result<Geodetic, JetStepError> {
    if !state.is_finite() || !state.velocity.length().is_finite() {
        return Err(context.invalid(JetInvalidInput::State));
    }
    let norm = state.orientation.length();
    if !norm.is_finite() || (norm - 1.0).abs() > 1e-9 {
        return Err(context.invalid(JetInvalidInput::Quaternion));
    }
    if !state.position.as_vec().length().is_finite()
        || state.position.as_vec().length_squared() == 0.0
    {
        return Err(context.invalid(JetInvalidInput::Position));
    }
    if !state.angular_velocity.length().is_finite() {
        return Err(context.invalid(JetInvalidInput::AngularRate));
    }
    let position = state.position.to_geodetic();
    if !position.latitude.is_finite()
        || !position.longitude.is_finite()
        || !position.altitude.is_finite()
    {
        return Err(context.invalid(JetInvalidInput::Position));
    }
    if position.altitude.get() < MIN_ALTITUDE {
        return Err(context.error(JetFailureReason::OutsideAtmosphereAltitude(
            AxisStatus::Below,
        )));
    }
    if position.altitude.get() > MAX_ALTITUDE {
        return Err(context.error(JetFailureReason::OutsideAtmosphereAltitude(
            AxisStatus::Above,
        )));
    }
    Ok(position)
}

fn validate_environment(environment: &Environment, context: Context) -> Result<(), JetStepError> {
    if !environment.atmosphere.temperature_offset().is_finite() {
        return Err(context.invalid(JetInvalidInput::AtmosphereOffset));
    }
    if !environment.wind_ecef.is_finite() || !environment.wind_ecef.length().is_finite() {
        return Err(context.invalid(JetInvalidInput::Wind));
    }
    let ground = environment.ground_elevation.get();
    let slope = environment.ground_slope();
    let reference_valid = environment.ground_reference().is_none_or(|position| {
        position.latitude.is_finite()
            && position.longitude.is_finite()
            && position.altitude.is_finite()
            && position.latitude.get().abs() <= std::f64::consts::FRAC_PI_2
            && position.longitude.get().abs() <= std::f64::consts::PI
            && (MIN_ALTITUDE..=MAX_ALTITUDE).contains(&position.altitude.get())
    });
    // Explicit numerical input policy prevents the reused legacy gear's defensive
    // nonfinite-intermediate fallbacks from silently discarding extreme inputs.
    if !ground.is_finite()
        || !(MIN_ALTITUDE..=MAX_ALTITUDE).contains(&ground)
        || !slope.is_finite()
        || slope.north().abs() > 1000.0
        || slope.east().abs() > 1000.0
        || !reference_valid
    {
        return Err(context.invalid(JetInvalidInput::Ground));
    }
    Ok(())
}

fn checked_offset(
    start: &RigidBodyState,
    derivative: &StateDerivative,
    h: f64,
    context: Context,
) -> Result<RigidBodyState, JetStepError> {
    let raw = DVec4::new(
        start.orientation.x,
        start.orientation.y,
        start.orientation.z,
        start.orientation.w,
    ) + derivative.orientation_rate * h;
    let norm = raw.length();
    if !derivative_is_finite(*derivative) || !raw.is_finite() || !norm.is_finite() || norm <= 0.0 {
        return Err(context.invalid(JetInvalidInput::IntermediateState));
    }
    let result = state::offset(start, derivative, h);
    if !result.is_finite() {
        return Err(context.invalid(JetInvalidInput::IntermediateState));
    }
    Ok(result)
}

fn checked_conditions(
    air: AtmosphereSample,
    speed: MetersPerSecond,
    context: Context,
) -> Result<JetConditions, JetStepError> {
    if !air.density.is_finite() || air.density.get() < 0.0 {
        return Err(context.invalid(JetInvalidInput::AtmosphereDensity));
    }
    JetConditions::from_atmosphere(air, speed).map_err(|error| context.component(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{KilogramsPerCubicMeter, Meters};

    #[test]
    fn density_is_validated_before_aero_can_mask_bad_data() {
        let mut air = Atmosphere::standard().sample(Meters(0.0));
        for density in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            air.density = KilogramsPerCubicMeter(density);
            assert_eq!(
                checked_conditions(air, MetersPerSecond(100.0), Context::INITIAL)
                    .unwrap_err()
                    .reason,
                JetFailureReason::InvalidInput(JetInvalidInput::AtmosphereDensity)
            );
        }
        air.density = KilogramsPerCubicMeter(0.0);
        assert!(checked_conditions(air, MetersPerSecond(100.0), Context::INITIAL).is_ok());
    }
}
