use super::{
    DryThrottle, EvaluationError, JetConditions, JetDomainStatus, MAX_TABLE_MACH, MAX_THRUST_CELLS,
    TableError, axis_bytes, axis_status, bracket, lerp, range, scalar_bytes, validate_axis,
};
use flightsim_core::Newtons;
use serde::{Deserialize, Serialize};

/// Raw external-data boundary; never evaluate this representation directly.
///
/// Owners must bound bytes before JSON deserialization. Runtime construction
/// validates dimensions before copying or evaluating cells. Schema 1 is local
/// to this component and is not an aircraft profile or replay schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DryJetDefinition {
    pub schema: u16,
    /// p / 101325 Pa, static ambient pressure; first knot must be zero.
    pub pressure_ratios: Vec<f64>,
    /// T / 288.15 K, static absolute ambient temperature.
    pub temperature_ratios: Vec<f64>,
    /// V / local sound speed; first knot must be zero.
    pub mach: Vec<f64>,
    /// Flattened `[pressure][temperature][Mach]`, Mach varying fastest.
    pub cells: Vec<NetThrustCellDefinition>,
}

/// Installed aggregate net thrust (including inlet momentum loss) in newtons.
/// This is not gross exhaust thrust or shaft power. Idle may be negative;
/// maximum dry must be no less than idle. Either may be negative. No afterburner.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetThrustCellDefinition {
    pub idle_n: f64,
    pub maximum_dry_n: f64,
}

/// Validated immutable table. No automatic serde deserialization bypass exists.
#[derive(Debug, Clone)]
pub struct DryJetTable {
    definition: DryJetDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct ThrustSample {
    pub net_thrust: Newtons,
    pub domain: JetDomainStatus,
}

impl DryJetTable {
    /// Validate schema, dimensions, ordering, finite values and numerical bounds.
    ///
    /// Pressure ratio: 0..=2, temperature ratio: 0.25..=2, Mach: 0..=0.95.
    /// Both thrust channels: -1 MN..=1 MN. All zero-pressure cells
    /// must be zero, so vacuum cannot generate thrust. These are authored
    /// numerical-policy bounds; they are not a validated aircraft envelope.
    ///
    /// # Errors
    /// Unsupported schema, malformed axes/cells, or any failed bound.
    pub fn from_definition(definition: DryJetDefinition) -> Result<Self, TableError> {
        if definition.schema != 1 {
            return Err(TableError("unsupported dry-jet table schema".into()));
        }
        validate_axis("pressure_ratios", &definition.pressure_ratios, 0.0, 2.0)?;
        validate_axis(
            "temperature_ratios",
            &definition.temperature_ratios,
            0.25,
            2.0,
        )?;
        validate_axis("mach", &definition.mach, 0.0, MAX_TABLE_MACH)?;
        if definition.pressure_ratios[0].abs() > 0.0 || definition.mach[0].abs() > 0.0 {
            return Err(TableError(
                "pressure and Mach axes must start at zero".into(),
            ));
        }
        // Axes are bounded before multiplication. No unchecked attacker-sized
        // product or conversion is used for indexing/allocation.
        let count = definition.pressure_ratios.len()
            * definition.temperature_ratios.len()
            * definition.mach.len();
        if count > MAX_THRUST_CELLS || definition.cells.len() != count {
            return Err(TableError(format!(
                "dry-jet table needs exactly {count} cells, at most {MAX_THRUST_CELLS}"
            )));
        }
        let zero_pressure_count = definition.temperature_ratios.len() * definition.mach.len();
        for (index, cell) in definition.cells.iter().enumerate() {
            range("idle_n", cell.idle_n, -1e6, 1e6)?;
            range("maximum_dry_n", cell.maximum_dry_n, -1e6, 1e6)?;
            if cell.idle_n > cell.maximum_dry_n {
                return Err(TableError(
                    "maximum dry thrust must be at least idle thrust".into(),
                ));
            }
            if index < zero_pressure_count
                && (cell.idle_n.abs() > 0.0 || cell.maximum_dry_n.abs() > 0.0)
            {
                return Err(TableError(
                    "zero-pressure cells must contain zero thrust".into(),
                ));
            }
        }
        Ok(Self { definition })
    }

    /// Immutable source values for versioned profile/replay owners. Changing any
    /// axis, idle cell or maximum-dry cell must change that model's identity.
    #[must_use]
    pub const fn definition(&self) -> &DryJetDefinition {
        &self.definition
    }

    /// Stateless trilinear net thrust followed by linear command interpolation.
    /// Arithmetic order is Mach, temperature, pressure, then idle-to-dry throttle.
    /// No density law, P/V, throttle clamping or implicit endpoint continuation.
    ///
    /// # Errors
    /// Invalid inputs or any out-of-table query. Outside-domain errors provide
    /// all axis statuses and contain no substitute force. A host must choose
    /// an explicit unsupported-flight policy before integrating this component.
    pub fn sample(
        &self,
        conditions: JetConditions,
        throttle: DryThrottle,
    ) -> Result<ThrustSample, EvaluationError> {
        conditions.validate()?;
        if !throttle.0.is_finite() || !(0.0..=1.0).contains(&throttle.0) {
            return Err(EvaluationError::InvalidInput("throttle"));
        }
        let def = &self.definition;
        let domain = JetDomainStatus {
            pressure: axis_status(&def.pressure_ratios, conditions.pressure_ratio.0),
            temperature: axis_status(&def.temperature_ratios, conditions.temperature_ratio.0),
            mach: axis_status(&def.mach, conditions.mach.0),
        };
        if !domain.is_supported() {
            return Err(EvaluationError::OutsideJetDomain(domain));
        }
        let (p, fp) = bracket(&def.pressure_ratios, conditions.pressure_ratio.0);
        let (t, ft) = bracket(&def.temperature_ratios, conditions.temperature_ratio.0);
        let (m, fm) = bracket(&def.mach, conditions.mach.0);
        let cell =
            |pi, ti, mi| def.cells[(pi * def.temperature_ratios.len() + ti) * def.mach.len() + mi];
        let interpolate = |pick: fn(NetThrustCellDefinition) -> f64| {
            let at_pressure = |pi| {
                let at_temperature =
                    |ti| lerp(pick(cell(pi, ti, m)), pick(cell(pi, ti, m + 1)), fm);
                lerp(at_temperature(t), at_temperature(t + 1), ft)
            };
            lerp(at_pressure(p), at_pressure(p + 1), fp)
        };
        let idle = interpolate(|value| value.idle_n);
        let dry = interpolate(|value| value.maximum_dry_n);
        Ok(ThrustSample {
            net_thrust: Newtons(lerp(idle, dry, throttle.0)),
            domain,
        })
    }

    /// Canonical component bytes for a future model-specific identity owner.
    ///
    /// This is not a hash or a replay compatibility gate. Domain tag and local
    /// schema identify these exact units, bounds, axis order, interpolation,
    /// throttle semantics and reject-outside policy. Append u32-LE axis lengths
    /// and exact f64-LE bits for pressure, temperature, Mach; then u32-LE cell
    /// count and each idle/max pair in flattening order. Signed zero is retained.
    /// Model dispatch/revision and remaining aircraft fields belong to the host.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let def = &self.definition;
        let mut bytes = b"flightsim/subsonic-dry-jet\0".to_vec();
        bytes.extend_from_slice(&def.schema.to_le_bytes());
        axis_bytes(&mut bytes, &def.pressure_ratios);
        axis_bytes(&mut bytes, &def.temperature_ratios);
        axis_bytes(&mut bytes, &def.mach);
        bytes.extend_from_slice(
            &u32::try_from(def.cells.len())
                .expect("validated bounded cells")
                .to_le_bytes(),
        );
        for cell in &def.cells {
            scalar_bytes(&mut bytes, cell.idle_n);
            scalar_bytes(&mut bytes, cell.maximum_dry_n);
        }
        bytes
    }
}
