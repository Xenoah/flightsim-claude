use super::{
    PropellerCellDefinition, PropellerMap, SampledGovernor, TurbinePowerTable,
    TurbopropConfigError, TurbopropEnvelope,
};
use crate::{
    MassProperties,
    subsonic::{AirframeConfig, MachAeroSchedule},
};

/// Complete immutable law-2 configuration. Construction retains the entire
/// validated forward model; only an explicit two-row negative extension is new.
#[derive(Debug, Clone)]
pub struct TurbopropAircraftConfig {
    forward: super::super::TurbopropAircraftConfig,
    propeller: PropellerMap,
}
impl TurbopropAircraftConfig {
    /// Rows are J=-0.01 then J=-0.005, pitch fastest on the forward pitch axis.
    /// No forward data, state, operating envelope or gear is changed.
    /// # Errors
    /// Invalid extension pairs, static floor, feedback or bounded table shape.
    pub fn from_forward(
        forward: super::super::TurbopropAircraftConfig,
        negative_rows: [Vec<PropellerCellDefinition>; 2],
    ) -> Result<Self, TurbopropConfigError> {
        let propeller = PropellerMap::from_forward(forward.propeller().clone(), negative_rows)?;
        Ok(Self { forward, propeller })
    }
    #[must_use]
    pub const fn forward_config(&self) -> &super::super::TurbopropAircraftConfig {
        &self.forward
    }
    #[must_use]
    pub const fn propeller(&self) -> &PropellerMap {
        &self.propeller
    }
    #[must_use]
    pub const fn airframe(&self) -> &AirframeConfig {
        self.forward.airframe()
    }
    #[must_use]
    pub const fn turbine(&self) -> &TurbinePowerTable {
        self.forward.turbine()
    }
    #[must_use]
    pub const fn governor(&self) -> &SampledGovernor {
        self.forward.governor()
    }
    #[must_use]
    pub const fn aero(&self) -> &MachAeroSchedule {
        self.forward.aero()
    }
    #[must_use]
    pub const fn envelope(&self) -> &TurbopropEnvelope {
        self.forward.envelope()
    }
    #[must_use]
    pub const fn effective_body_mass_properties(&self) -> &MassProperties {
        self.forward.effective_body_mass_properties()
    }
}
