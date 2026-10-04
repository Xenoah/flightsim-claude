//! Explicit, authored near-static adverse-inflow approximation (FDM law 2).
//!
//! A separate component and runtime preserve law 1's closed errors and old
//! profile/replay gates. No empirical validation, dynamic inflow, engine-out,
//! reverse thrust, VRS, static friction or stationary parking is implied.
//! Negative rows and bounds are immutable; accepted state remains 16 scalars.

mod config;
mod propeller;
mod runtime;

pub use super::{
    AdvanceRatio, AxisStatus, GovernorSample, MachNumber, PowerConditions, PowerDomainStatus,
    PressureRatio, PropellerCellDefinition, PropellerCoefficients, PropellerDomainStatus,
    PropellerPowerBound, PropellerQuery, PropellerSample, RotationSense, SampledGovernor,
    TemperatureRatio, TurbineFraction, TurbinePowerTable, TurbopropConfigError,
    TurbopropDerivative, TurbopropDomainStatus, TurbopropEnvelope, TurbopropEvaluationError,
    TurbopropInvalidInput, TurbopropStage, TurbopropState, TurbopropStepReport,
};
use super::{bracket, finite, lerp, positive, range, status};
pub use config::TurbopropAircraftConfig;
pub use propeller::{
    EvaluationError, MAX_ADVERSE_INFLOW_RATIO, MAX_TRANSVERSE_INFLOW_RATIO,
    NEGATIVE_ADVANCE_RATIO_KNOTS, NearStaticDomainStatus, PropellerMap, StaticPowerBound,
};
pub use runtime::{
    MAX_TURBOPROP_STEP_DT, MAX_TURBOPROP_SUBSTEP_DT, MAX_TURBOPROP_SUBSTEPS,
    RUNNING_TURBOPROP_MODEL_KIND_ID, TURBOPROP_FDM_MODEL_REVISION, TurbopropDiagnosticValues,
    TurbopropDiagnostics, TurbopropFailureReason, TurbopropFlightDynamics, TurbopropStepError,
};

#[cfg(test)]
mod tests;
