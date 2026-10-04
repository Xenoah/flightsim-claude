//! Numerical revision 1: exact held-command turbine response and sampled pitch
//! ramps drive RK4 rigid-body/relative-shaft dynamics, with whole-call rollback.
use super::{
    AdvanceRatio, AxisStatus, GovernorSample, MachNumber, PowerConditions, PowerDomainStatus,
    PressureRatio, PropellerDomainStatus, PropellerPowerBound, PropellerQuery, TemperatureRatio,
    TurbineFraction, TurbopropAircraftConfig, TurbopropDomainStatus, TurbopropEvaluationError,
    status,
};
use crate::{
    Atmosphere, ControlInputs, Environment, RigidBodyState, StateDerivative, aero, gravity,
    landing_gear, state,
};
use flightsim_core::{
    Geodetic, LocalFrame, Meters, Radians, RadiansPerSecond, RadiansPerSecondSquared, Seconds,
};
use glam::{DVec3, DVec4};
use std::f64::consts::TAU;

pub const RUNNING_TURBOPROP_MODEL_KIND_ID: u16 = 3;
pub const TURBOPROP_FDM_MODEL_REVISION: u32 = 1;
pub const MAX_TURBOPROP_SUBSTEPS: u32 = 8;
pub const MAX_TURBOPROP_SUBSTEP_DT: Seconds = Seconds(1.0 / 120.0);
pub const MAX_TURBOPROP_STEP_DT: Seconds = Seconds(8.0 / 120.0);
const MAX_PHASE: f64 = 0.05;
const PHASE_ROUNDOFF: f64 = 32.0 * f64::EPSILON;
const MIN_ALTITUDE: f64 = -5000.0;
const MAX_ALTITUDE: f64 = 86000.0;

/// Complete physical state. There is no hidden controller, engine or actuator
/// history; sim/replay owners must preserve all 16 scalars, not just rigid body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropState {
    pub rigid_body: RigidBodyState,
    pub turbine_fraction: TurbineFraction,
    /// Positive rotation relative to body, never RPM or absolute spin.
    pub shaft_rad_s: RadiansPerSecond,
    pub blade_pitch_rad: Radians,
}
/// Differential part advanced by RK4. Turbine fraction and pitch instead follow
/// the explicitly sampled analytic programs during each substep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropDerivative {
    pub rigid_body: StateDerivative,
    pub shaft_acceleration: RadiansPerSecondSquared,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TurbopropInvalidInput {
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
    TurbineFraction = 11,
    ShaftRate = 12,
    BladePitch = 13,
    ComponentQuery = 14,
    Derivative = 15,
    IntermediateState = 16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TurbopropStage {
    Initial = 0,
    K1 = 1,
    K2 = 2,
    K3 = 3,
    K4 = 4,
    Endpoint = 5,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurbopropFailureReason {
    InvalidInput(TurbopropInvalidInput),
    OutsideAtmosphereAltitude(AxisStatus),
    OutsideOperatingEnvelope(TurbopropDomainStatus),
    OutsidePowerDomain(PowerDomainStatus),
    OutsidePropellerDomain(PropellerDomainStatus),
    OutsideMachDomain(AxisStatus),
    SubstepBudgetExceeded,
    PropellerPowerBound(PropellerPowerBound),
}
/// Only already validated finite values are present. An absent field has not
/// been established; it is not a fabricated zero. Fixed-size, no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TurbopropDiagnosticValues {
    pub pressure_ratio: Option<PressureRatio>,
    pub temperature_ratio: Option<TemperatureRatio>,
    pub mach: Option<MachNumber>,
    pub advance_ratio: Option<AdvanceRatio>,
    pub blade_pitch: Option<Radians>,
    pub relative_shaft: Option<RadiansPerSecond>,
    pub absolute_spin: Option<RadiansPerSecond>,
    pub tip_mach: Option<MachNumber>,
    /// Crossflow velocity / propeller tangential tip speed, dimensionless.
    pub crossflow_ratio: Option<f64>,
}
/// Compact fixed-size optional diagnostics. Presence is stored separately so
/// Result errors stay small without heap allocation. Unset storage is private.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TurbopropDiagnostics {
    values: [f64; 9],
    present: u16,
}
impl TurbopropDiagnostics {
    /// Build diagnostics from a codec's individually optional SI values.
    /// # Errors
    /// Any present value is nonfinite. Domain support is not implied.
    pub fn from_values(input: TurbopropDiagnosticValues) -> Result<Self, TurbopropEvaluationError> {
        let values = [
            input.pressure_ratio.map(|v| v.0),
            input.temperature_ratio.map(|v| v.0),
            input.mach.map(|v| v.0),
            input.advance_ratio.map(|v| v.0),
            input.blade_pitch.map(|v| v.get()),
            input.relative_shaft.map(|v| v.get()),
            input.absolute_spin.map(|v| v.get()),
            input.tip_mach.map(|v| v.0),
            input.crossflow_ratio,
        ];
        let mut diagnostics = Self::default();
        for (index, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                if !value.is_finite() {
                    return Err(TurbopropEvaluationError::InvalidInput("diagnostic"));
                }
                diagnostics.insert(index, value);
            }
        }
        Ok(diagnostics)
    }
    /// Fixed-size expanded view, with unit types and explicit missing values.
    #[must_use]
    pub fn values(self) -> TurbopropDiagnosticValues {
        let at = |index| {
            if self.present & (1_u16 << index) != 0 {
                Some(self.values[index])
            } else {
                None
            }
        };
        TurbopropDiagnosticValues {
            pressure_ratio: at(0).map(PressureRatio),
            temperature_ratio: at(1).map(TemperatureRatio),
            mach: at(2).map(MachNumber),
            advance_ratio: at(3).map(AdvanceRatio),
            blade_pitch: at(4).map(Radians),
            relative_shaft: at(5).map(RadiansPerSecond),
            absolute_spin: at(6).map(RadiansPerSecond),
            tip_mach: at(7).map(MachNumber),
            crossflow_ratio: at(8),
        }
    }
    fn insert(&mut self, index: usize, value: f64) {
        debug_assert!(value.is_finite());
        self.values[index] = value;
        self.present |= 1_u16 << index;
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropStepError {
    pub reason: TurbopropFailureReason,
    pub substep: u32,
    pub stage: TurbopropStage,
    pub diagnostics: TurbopropDiagnostics,
}
impl std::fmt::Display for TurbopropStepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "turboprop step rejected at substep {} {:?}: {:?}",
            self.substep, self.stage, self.reason
        )
    }
}
impl std::error::Error for TurbopropStepError {}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropStepReport {
    pub substeps: u32,
    pub gear_clearances: [Meters; 3],
}
#[derive(Debug, Clone, Copy)]
struct Context {
    substep: u32,
    stage: TurbopropStage,
    diagnostics: TurbopropDiagnostics,
}
impl Context {
    fn initial() -> Self {
        Self {
            substep: 0,
            stage: TurbopropStage::Initial,
            diagnostics: TurbopropDiagnostics::default(),
        }
    }
    fn error(self, reason: TurbopropFailureReason) -> TurbopropStepError {
        TurbopropStepError {
            reason,
            substep: self.substep,
            stage: self.stage,
            diagnostics: self.diagnostics,
        }
    }
    fn invalid(self, input: TurbopropInvalidInput) -> TurbopropStepError {
        self.error(TurbopropFailureReason::InvalidInput(input))
    }
    fn component(self, error: TurbopropEvaluationError) -> TurbopropStepError {
        self.error(match error {
            TurbopropEvaluationError::InvalidInput(_) => {
                TurbopropFailureReason::InvalidInput(TurbopropInvalidInput::ComponentQuery)
            }
            TurbopropEvaluationError::OutsidePowerDomain(d) => {
                TurbopropFailureReason::OutsidePowerDomain(d)
            }
            TurbopropEvaluationError::OutsidePropellerDomain(d) => {
                TurbopropFailureReason::OutsidePropellerDomain(d)
            }
            TurbopropEvaluationError::PropellerPowerBound(e) => {
                TurbopropFailureReason::PropellerPowerBound(e)
            }
        })
    }
}
struct Evaluated {
    derivative: TurbopropDerivative,
    frame: LocalFrame,
    context: Context,
}
#[derive(Debug, Clone)]
pub struct TurbopropFlightDynamics {
    config: TurbopropAircraftConfig,
    state: TurbopropState,
}
impl TurbopropFlightDynamics {
    /// Structural state/altitude validation only. Supply the actual environment
    /// to step(0) before host staging commits an initial flight.
    /// # Errors
    /// Invalid full state, pitch outside stops or unsupported geometric altitude.
    pub fn new(
        config: TurbopropAircraftConfig,
        state: TurbopropState,
    ) -> Result<Self, TurbopropStepError> {
        validate_state(&state, &config, Context::initial())?;
        Ok(Self { config, state })
    }
    #[must_use]
    pub const fn config(&self) -> &TurbopropAircraftConfig {
        &self.config
    }
    #[must_use]
    pub const fn state(&self) -> &TurbopropState {
        &self.state
    }
    /// # Errors
    /// Same structural checks as construction; failure preserves every bit.
    pub fn set_state(&mut self, state: TurbopropState) -> Result<(), TurbopropStepError> {
        validate_state(&state, &self.config, Context::initial())?;
        self.state = state;
        Ok(())
    }
    /// Reevaluate rigid/shaft dynamics with this state's actual x and pitch.
    /// # Errors
    /// Invalid state/environment or any unsupported physical query.
    pub fn derivative(
        &self,
        state: &TurbopropState,
        controls: ControlInputs,
        environment: &Environment,
    ) -> Result<TurbopropDerivative, TurbopropStepError> {
        self.evaluate(state, controls, environment, Context::initial())
            .map(|e| e.derivative)
    }
    /// Geometric diagnostic only; does not establish engine envelope support.
    /// # Errors
    /// Invalid full state, ground, wind, or nonfinite contact geometry.
    pub fn gear_clearances(
        &self,
        state: &TurbopropState,
        environment: &Environment,
    ) -> Result<[Meters; 3], TurbopropStepError> {
        let context = Context::initial();
        let position = validate_state(state, &self.config, context)?;
        validate_environment(environment, context)?;
        self.checked_clearances(state, environment, &LocalFrame::new(position), context)
    }
    fn checked_clearances(
        &self,
        state: &TurbopropState,
        environment: &Environment,
        frame: &LocalFrame,
        context: Context,
    ) -> Result<[Meters; 3], TurbopropStepError> {
        let clearances = landing_gear::signed_normal_clearances(
            self.config.airframe().landing_gear(),
            &state.rigid_body,
            environment,
            frame,
        );
        if clearances.iter().any(|v| !v.is_finite()) {
            return Err(context.invalid(TurbopropInvalidInput::Ground));
        }
        Ok(clearances)
    }
    /// One initial + five evaluations per substep, at most 41 total. One fixed
    /// subdivision decision, no retries or heap allocation, frozen environment.
    /// Every failure rolls back the ENTIRE call, including earlier substeps.
    /// Zero dt evaluates/validates and preserves every state bit.
    /// # Errors
    /// Invalid input, unsupported stage/endpoint or exhausted resolution budget.
    pub fn step(
        &mut self,
        dt: Seconds,
        controls: ControlInputs,
        environment: &Environment,
    ) -> Result<TurbopropStepReport, TurbopropStepError> {
        let initial = Context::initial();
        if !dt.is_finite() || dt.get() < 0.0 {
            return Err(initial.invalid(TurbopropInvalidInput::TimeStep));
        }
        if dt.get() > MAX_TURBOPROP_STEP_DT.get() {
            return Err(initial.error(TurbopropFailureReason::SubstepBudgetExceeded));
        }
        let evaluated = self.evaluate(&self.state, controls, environment, initial)?;
        if dt.get() == 0.0 {
            return Ok(TurbopropStepReport {
                substeps: 0,
                gear_clearances: self.checked_clearances(
                    &self.state,
                    environment,
                    &evaluated.frame,
                    evaluated.context,
                )?,
            });
        }
        let count = self
            .required_ratio(&self.state, environment, &evaluated, dt, dt)?
            .ceil()
            .max(1.0);
        if count > f64::from(MAX_TURBOPROP_SUBSTEPS) {
            return Err(evaluated
                .context
                .error(TurbopropFailureReason::SubstepBudgetExceeded));
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
        Ok(TurbopropStepReport {
            substeps,
            gear_clearances,
        })
    }
    fn required_ratio(
        &self,
        state: &TurbopropState,
        environment: &Environment,
        evaluated: &Evaluated,
        dt: Seconds,
        lookahead: Seconds,
    ) -> Result<f64, TurbopropStepError> {
        let body = &state.rigid_body;
        let airframe = self.config.airframe();
        let gear = if landing_gear::contact_is_active_or_imminent(
            airframe.landing_gear(),
            body,
            environment,
            &evaluated.frame,
            lookahead,
        ) {
            landing_gear::maximum_natural_frequency(
                airframe.landing_gear(),
                self.config.effective_body_mass_properties(),
                body,
                environment,
                &evaluated.frame,
            ) * dt.get()
                / landing_gear::MAX_GEAR_PHASE_PER_SUBSTEP
        } else {
            0.0
        };
        let d = self.config.turbine().definition();
        let g = self.config.governor().definition();
        let e = self.config.envelope().definition();
        let shaft = evaluated.derivative.shaft_acceleration.get();
        let absolute = shaft
            + self.config.propeller().rotation_sense().sign()
                * evaluated.derivative.rigid_body.angular_acceleration.x;
        let estimates = [
            dt.get() / MAX_TURBOPROP_SUBSTEP_DT.get(),
            gear,
            body.angular_velocity.length() * dt.get() / MAX_PHASE,
            dt.get() / d.rise_seconds.min(d.fall_seconds) / MAX_PHASE,
            dt.get() * shaft.abs() / e.relative_shaft_rad_s[0] / MAX_PHASE,
            dt.get() * absolute.abs() / e.absolute_spin_rad_s[0] / MAX_PHASE,
            dt.get() * g.fine_rate_rad_s.max(g.coarse_rate_rad_s)
                / (g.maximum_pitch_rad - g.minimum_pitch_rad)
                / MAX_PHASE,
        ];
        if estimates.iter().any(|v| !v.is_finite()) {
            return Err(evaluated
                .context
                .error(TurbopropFailureReason::SubstepBudgetExceeded));
        }
        Ok(estimates.into_iter().fold(0.0, f64::max))
    }
    fn stage(
        &self,
        state: &TurbopropState,
        controls: ControlInputs,
        environment: &Environment,
        h: f64,
        context: Context,
    ) -> Result<Evaluated, TurbopropStepError> {
        let evaluated = self.evaluate(state, controls, environment, context)?;
        if self.required_ratio(state, environment, &evaluated, Seconds(h), Seconds::ZERO)?
            > 1.0 + PHASE_ROUNDOFF
        {
            return Err(evaluated
                .context
                .error(TurbopropFailureReason::SubstepBudgetExceeded));
        }
        Ok(evaluated)
    }
    fn integrate_rk4(
        &self,
        start: &TurbopropState,
        h: f64,
        controls: ControlInputs,
        environment: &Environment,
        substep: u32,
    ) -> Result<(TurbopropState, [Meters; 3]), TurbopropStepError> {
        let at = |stage| Context {
            substep,
            stage,
            diagnostics: TurbopropDiagnostics::default(),
        };
        // No governor update at RK stages. Exactly one held rate per substep.
        let governor = self
            .config
            .governor()
            .sample(start.shaft_rad_s, start.blade_pitch_rad)
            .map_err(|e| at(TurbopropStage::K1).component(e))?;
        let k1 = self
            .stage(start, controls, environment, h, at(TurbopropStage::K1))?
            .derivative;
        let s2 = self.offset(
            start,
            k1,
            h * 0.5,
            controls,
            governor,
            at(TurbopropStage::K2),
        )?;
        let k2 = self
            .stage(&s2, controls, environment, h, at(TurbopropStage::K2))?
            .derivative;
        let s3 = self.offset(
            start,
            k2,
            h * 0.5,
            controls,
            governor,
            at(TurbopropStage::K3),
        )?;
        let k3 = self
            .stage(&s3, controls, environment, h, at(TurbopropStage::K3))?
            .derivative;
        let s4 = self.offset(start, k3, h, controls, governor, at(TurbopropStage::K4))?;
        let k4 = self
            .stage(&s4, controls, environment, h, at(TurbopropStage::K4))?
            .derivative;
        let weighted = TurbopropDerivative {
            rigid_body: (k1.rigid_body + k2.rigid_body * 2.0 + k3.rigid_body * 2.0 + k4.rigid_body)
                * (1.0 / 6.0),
            shaft_acceleration: RadiansPerSecondSquared(
                (k1.shaft_acceleration.get()
                    + 2.0 * k2.shaft_acceleration.get()
                    + 2.0 * k3.shaft_acceleration.get()
                    + k4.shaft_acceleration.get())
                    * (1.0 / 6.0),
            ),
        };
        let end = self.offset(
            start,
            weighted,
            h,
            controls,
            governor,
            at(TurbopropStage::Endpoint),
        )?;
        let evaluated = self.stage(&end, controls, environment, h, at(TurbopropStage::Endpoint))?;
        let clearances =
            self.checked_clearances(&end, environment, &evaluated.frame, evaluated.context)?;
        Ok((end, clearances))
    }
    fn offset(
        &self,
        start: &TurbopropState,
        derivative: TurbopropDerivative,
        h: f64,
        controls: ControlInputs,
        governor: GovernorSample,
        context: Context,
    ) -> Result<TurbopropState, TurbopropStepError> {
        let q = start.rigid_body.orientation;
        let raw = DVec4::new(q.x, q.y, q.z, q.w) + derivative.rigid_body.orientation_rate * h;
        if !derivative_is_finite(derivative) || !raw.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::IntermediateState));
        }
        let norm = raw.length();
        if !norm.is_finite() || norm <= 0.0 {
            return Err(context.invalid(TurbopropInvalidInput::IntermediateState));
        }
        let command =
            TurbineFraction::new(controls.throttle()).map_err(|e| context.component(e))?;
        let end = TurbopropState {
            rigid_body: state::offset(&start.rigid_body, &derivative.rigid_body, h),
            shaft_rad_s: RadiansPerSecond(
                start.shaft_rad_s.get() + derivative.shaft_acceleration.get() * h,
            ),
            turbine_fraction: self
                .config
                .turbine()
                .fraction_after(start.turbine_fraction, command, Seconds(h))
                .map_err(|e| context.component(e))?,
            blade_pitch_rad: governor
                .pitch_after(Seconds(h))
                .map_err(|e| context.component(e))?,
        };
        if !end.rigid_body.is_finite() || !end.shaft_rad_s.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::IntermediateState));
        }
        Ok(end)
    }
    fn evaluate(
        &self,
        state: &TurbopropState,
        controls: ControlInputs,
        environment: &Environment,
        mut context: Context,
    ) -> Result<Evaluated, TurbopropStepError> {
        let position = validate_state(state, &self.config, context)?;
        context.diagnostics.insert(4, state.blade_pitch_rad.get());
        context.diagnostics.insert(5, state.shaft_rad_s.get());
        validate_environment(environment, context)?;
        let body = &state.rigid_body;
        let relative = body.velocity - environment.wind_ecef;
        if !relative.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::RelativeVelocity));
        }
        let body_relative = body.orientation.inverse() * relative;
        if !body_relative.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::RelativeVelocity));
        }
        let speed = body_relative.length();
        if !relative.length().is_finite() || !speed.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::RelativeVelocity));
        }
        let standard = Atmosphere::standard().sample(position.altitude);
        let raw_temperature =
            standard.temperature.get() + environment.atmosphere.temperature_offset();
        if !raw_temperature.is_finite() || raw_temperature < 1.0 {
            return Err(context.invalid(TurbopropInvalidInput::AtmosphereTemperature));
        }
        let air = environment.atmosphere.sample(position.altitude);
        if !air.density.is_finite() || air.density.get() <= 0.0 {
            return Err(context.invalid(TurbopropInvalidInput::AtmosphereDensity));
        }
        if !air.speed_of_sound.is_finite()
            || air.speed_of_sound.get() <= 0.0
            || !air.pressure.is_finite()
            || air.pressure.get() < 0.0
        {
            return Err(context.invalid(TurbopropInvalidInput::ComponentQuery));
        }
        let pressure = PressureRatio(air.pressure.get() / crate::atmosphere::SEA_LEVEL_PRESSURE);
        let temperature =
            TemperatureRatio(air.temperature.get() / crate::atmosphere::SEA_LEVEL_TEMPERATURE);
        let mach = MachNumber(speed / air.speed_of_sound.get());
        let sense = self.config.propeller().rotation_sense().sign();
        let absolute = RadiansPerSecond(state.shaft_rad_s.get() + sense * body.angular_velocity.x);
        if [pressure.0, temperature.0, mach.0, absolute.get()]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(context.invalid(TurbopropInvalidInput::ComponentQuery));
        }
        context.diagnostics.insert(0, pressure.0);
        context.diagnostics.insert(1, temperature.0);
        context.diagnostics.insert(2, mach.0);
        context.diagnostics.insert(6, absolute.get());
        // An out-of-profile shaft rate is reported BEFORE any J division.
        let mut domain = self.config.envelope().base_status(
            pressure,
            temperature,
            mach,
            state.shaft_rad_s,
            absolute,
        );
        if !domain.is_supported() {
            return Err(context.error(TurbopropFailureReason::OutsideOperatingEnvelope(domain)));
        }
        let tangential = absolute.get() * self.config.propeller().diameter().get() * 0.5;
        let tip = body_relative.x.hypot(tangential) / air.speed_of_sound.get();
        let crossflow = body_relative.y.hypot(body_relative.z) / tangential;
        if !tangential.is_finite()
            || tangential <= 0.0
            || !tip.is_finite()
            || !crossflow.is_finite()
        {
            return Err(context.invalid(TurbopropInvalidInput::ComponentQuery));
        }
        context.diagnostics.insert(7, tip);
        context.diagnostics.insert(8, crossflow);
        let envelope = self.config.envelope().definition();
        domain.tip_mach = status(&[0.0, envelope.maximum_helical_tip_mach], tip);
        domain.crossflow = status(&[0.0, envelope.maximum_crossflow_tip_ratio], crossflow);
        if !domain.is_supported() {
            return Err(context.error(TurbopropFailureReason::OutsideOperatingEnvelope(domain)));
        }
        let power = self
            .config
            .turbine()
            .sample(
                PowerConditions {
                    pressure_ratio: pressure,
                    temperature_ratio: temperature,
                },
                state.turbine_fraction,
                state.shaft_rad_s,
            )
            .map_err(|e| context.component(e))?;
        let j =
            body_relative.x / ((absolute.get() / TAU) * self.config.propeller().diameter().get());
        if !j.is_finite() {
            return Err(context.invalid(TurbopropInvalidInput::ComponentQuery));
        }
        context.diagnostics.insert(3, j);
        let propeller = self
            .config
            .propeller()
            .sample(
                PropellerQuery {
                    advance_ratio: AdvanceRatio(j),
                    blade_pitch: state.blade_pitch_rad,
                },
                air.density,
                absolute,
            )
            .map_err(|e| context.component(e))?;
        let coefficients = self
            .config
            .aero()
            .sample(mach)
            .map_err(|e| {
                context.error(match e {
                    crate::subsonic::EvaluationError::OutsideMachDomain(s) => {
                        TurbopropFailureReason::OutsideMachDomain(s)
                    }
                    _ => {
                        TurbopropFailureReason::InvalidInput(TurbopropInvalidInput::ComponentQuery)
                    }
                })
            })?
            .coefficients;
        let airframe = self.config.airframe();
        let (aero_force, aero_moment) = aero::body_force_and_moment(
            &coefficients,
            airframe.geometry(),
            aero::aero_angles(body_relative),
            body.angular_velocity,
            controls,
            air.density,
        );
        let frame = LocalFrame::new(position);
        let ground =
            landing_gear::loads(airframe.landing_gear(), body, controls, environment, &frame);
        let force = aero_force + DVec3::X * propeller.thrust.get() + ground.force_body;
        let acceleration = body.orientation * (force / airframe.mass_properties().mass().get())
            + gravity::acceleration_ecef(position, &frame);
        let (angular_acceleration, shaft_acceleration) = coupled_rotation(
            &self.config,
            body.angular_velocity,
            state.shaft_rad_s.get(),
            power.drive_torque.get(),
            propeller.load_torque.get(),
            aero_moment + ground.moment_body,
        );
        let derivative = TurbopropDerivative {
            rigid_body: StateDerivative {
                velocity: body.velocity,
                acceleration,
                orientation_rate: state::orientation_rate(body.orientation, body.angular_velocity),
                angular_acceleration,
            },
            shaft_acceleration: RadiansPerSecondSquared(shaft_acceleration),
        };
        if !derivative_is_finite(derivative) {
            return Err(context.invalid(TurbopropInvalidInput::Derivative));
        }
        Ok(Evaluated {
            derivative,
            frame,
            context,
        })
    }
}

pub(super) fn coupled_rotation(
    config: &TurbopropAircraftConfig,
    b: DVec3,
    omega: f64,
    drive: f64,
    load: f64,
    moment: DVec3,
) -> (DVec3, f64) {
    let inertia = config.propeller().rotor_inertia().get();
    let sense = config.propeller().rotation_sense().sign();
    let momentum =
        config.airframe().mass_properties().inertia() * b + DVec3::X * (sense * inertia * omega);
    let alpha = config.effective_body_mass_properties().inverse_inertia()
        * (moment - DVec3::X * (sense * drive) - b.cross(momentum));
    (alpha, (drive - load) / inertia - sense * alpha.x)
}
fn derivative_is_finite(d: TurbopropDerivative) -> bool {
    d.rigid_body.velocity.is_finite()
        && d.rigid_body.acceleration.is_finite()
        && d.rigid_body.orientation_rate.is_finite()
        && d.rigid_body.angular_acceleration.is_finite()
        && d.shaft_acceleration.is_finite()
}
fn validate_state(
    state: &TurbopropState,
    config: &TurbopropAircraftConfig,
    context: Context,
) -> Result<Geodetic, TurbopropStepError> {
    let b = &state.rigid_body;
    if !b.is_finite() {
        return Err(context.invalid(TurbopropInvalidInput::State));
    }
    if !b.velocity.length().is_finite() || !b.angular_velocity.length().is_finite() {
        return Err(context.invalid(TurbopropInvalidInput::State));
    }
    let norm = b.orientation.length();
    if !norm.is_finite() || (norm - 1.0).abs() > 1e-9 {
        return Err(context.invalid(TurbopropInvalidInput::Quaternion));
    }
    if !b.position.as_vec().length().is_finite() || b.position.as_vec().length_squared() == 0.0 {
        return Err(context.invalid(TurbopropInvalidInput::Position));
    }
    if !state.turbine_fraction.get().is_finite()
        || !(0.0..=1.0).contains(&state.turbine_fraction.get())
    {
        return Err(context.invalid(TurbopropInvalidInput::TurbineFraction));
    }
    if !state.shaft_rad_s.is_finite() || state.shaft_rad_s.get() <= 0.0 {
        return Err(context.invalid(TurbopropInvalidInput::ShaftRate));
    }
    let g = config.governor().definition();
    if !state.blade_pitch_rad.is_finite()
        || !(g.minimum_pitch_rad..=g.maximum_pitch_rad).contains(&state.blade_pitch_rad.get())
    {
        return Err(context.invalid(TurbopropInvalidInput::BladePitch));
    }
    // All raw rigid + engine quantities are checked before geodetic conversion.
    let position = b.position.to_geodetic();
    if !position.latitude.is_finite()
        || !position.longitude.is_finite()
        || !position.altitude.is_finite()
    {
        return Err(context.invalid(TurbopropInvalidInput::Position));
    }
    if position.altitude.get() < MIN_ALTITUDE {
        return Err(
            context.error(TurbopropFailureReason::OutsideAtmosphereAltitude(
                AxisStatus::Below,
            )),
        );
    }
    if position.altitude.get() > MAX_ALTITUDE {
        return Err(
            context.error(TurbopropFailureReason::OutsideAtmosphereAltitude(
                AxisStatus::Above,
            )),
        );
    }
    Ok(position)
}
fn validate_environment(
    environment: &Environment,
    context: Context,
) -> Result<(), TurbopropStepError> {
    if !environment.atmosphere.temperature_offset().is_finite() {
        return Err(context.invalid(TurbopropInvalidInput::AtmosphereOffset));
    }
    let ground = environment.ground_elevation.get();
    let slope = environment.ground_slope();
    let reference_valid = environment.ground_reference().is_none_or(|p| {
        p.latitude.is_finite()
            && p.longitude.is_finite()
            && p.altitude.is_finite()
            && p.latitude.get().abs() <= std::f64::consts::FRAC_PI_2
            && p.longitude.get().abs() <= std::f64::consts::PI
            && (MIN_ALTITUDE..=MAX_ALTITUDE).contains(&p.altitude.get())
    });
    if !ground.is_finite()
        || !(MIN_ALTITUDE..=MAX_ALTITUDE).contains(&ground)
        || !slope.is_finite()
        || slope.north().abs() > 1000.0
        || slope.east().abs() > 1000.0
        || !reference_valid
    {
        return Err(context.invalid(TurbopropInvalidInput::Ground));
    }
    if !environment.wind_ecef.is_finite() {
        return Err(context.invalid(TurbopropInvalidInput::Wind));
    }
    if !environment.wind_ecef.length().is_finite() {
        return Err(context.invalid(TurbopropInvalidInput::Wind));
    }
    Ok(())
}
