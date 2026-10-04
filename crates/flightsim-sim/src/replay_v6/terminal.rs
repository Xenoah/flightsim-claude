// Explicit v6 terminal wire codes and law-2 diagnostic evaluation order.
use super::*;
use flightsim_core::MetersPerSecond;

fn axis_code(axis: AxisStatus) -> u16 {
    match axis {
        AxisStatus::Below => 0,
        AxisStatus::Within => 1,
        AxisStatus::Above => 2,
    }
}
fn decode_axis(code: u16) -> Result<AxisStatus, ReplayError> {
    match code {
        0 => Ok(AxisStatus::Below),
        1 => Ok(AxisStatus::Within),
        2 => Ok(AxisStatus::Above),
        _ => Err(invalid("terminal axis", "requires0/1/2")),
    }
}
fn axes_code(axes: &[AxisStatus]) -> u16 {
    axes.iter()
        .enumerate()
        .fold(0, |bits, (i, axis)| bits | (axis_code(*axis) << (2 * i)))
}
fn decode_axes<const N: usize>(code: u16) -> Result<[AxisStatus; N], ReplayError> {
    require_valid(
        code >> (2 * N) == 0,
        "terminal domain",
        None,
        "reserved bits must be zero",
    )?;
    let mut axes = [AxisStatus::Within; N];
    for (i, axis) in axes.iter_mut().enumerate() {
        *axis = decode_axis((code >> (2 * i)) & 3)?;
    }
    require_valid(
        axes.iter().any(|v| *v != AxisStatus::Within),
        "terminal domain",
        None,
        "requires an outside axis",
    )?;
    Ok(axes)
}
fn invalid_code(code: u16) -> Result<TurbopropInvalidInput, ReplayError> {
    use TurbopropInvalidInput as I;
    Ok(match code {
        1 => I::TimeStep,
        2 => I::State,
        3 => I::Quaternion,
        4 => I::Position,
        5 => I::AtmosphereOffset,
        6 => I::Ground,
        7 => I::Wind,
        8 => I::RelativeVelocity,
        9 => I::AtmosphereTemperature,
        10 => I::AtmosphereDensity,
        11 => I::TurbineFraction,
        12 => I::ShaftRate,
        13 => I::BladePitch,
        14 => I::ComponentQuery,
        15 => I::Derivative,
        16 => I::IntermediateState,
        _ => {
            return Err(invalid(
                "terminal invalid-input code",
                "requires a defined1..16 code",
            ));
        }
    })
}
fn reason_codes(reason: TurbopropFailureReason) -> (u8, u16) {
    use TurbopropFailureReason as R;
    match reason {
        R::InvalidInput(code) => (1, code as u16),
        R::OutsideAtmosphereAltitude(axis) => (2, axis_code(axis)),
        R::OutsideOperatingEnvelope(d) => (
            3,
            axes_code(&[
                d.pressure,
                d.temperature,
                d.mach,
                d.relative_shaft,
                d.absolute_spin,
                d.tip_mach,
                d.crossflow,
            ]),
        ),
        R::OutsidePowerDomain(d) => (4, axes_code(&[d.pressure, d.temperature])),
        R::OutsidePropellerDomain(d) => (5, axes_code(&[d.advance_ratio, d.blade_pitch])),
        R::OutsideMachDomain(axis) => (6, axis_code(axis)),
        R::SubstepBudgetExceeded => (7, 0),
        R::PropellerPowerBound(detail) => (8, detail as u16),
        R::NearStaticPowerBound(detail) => (
            9,
            match detail {
                StaticPowerBound::NonpositiveThrust => 1,
                StaticPowerBound::NonpositivePower => 2,
                StaticPowerBound::BelowStaticFloor => 3,
                StaticPowerBound::InvalidDerivedBound => 4,
            },
        ),
        R::OutsideNearStaticDomain(d) => (10, axes_code(&[d.adverse_inflow, d.transverse_inflow])),
        R::InvalidNearStaticScale => (11, 0),
    }
}
fn decode_reason(tag: u8, detail: u16) -> Result<TurbopropFailureReason, ReplayError> {
    use TurbopropFailureReason as R;
    Ok(match tag {
        1 => R::InvalidInput(invalid_code(detail)?),
        2 | 6 => {
            let [axis] = decode_axes(detail)?;
            if tag == 2 {
                R::OutsideAtmosphereAltitude(axis)
            } else {
                R::OutsideMachDomain(axis)
            }
        }
        3 => {
            let [
                pressure,
                temperature,
                mach,
                relative_shaft,
                absolute_spin,
                tip_mach,
                crossflow,
            ] = decode_axes(detail)?;
            require_valid(
                tip_mach != AxisStatus::Below && crossflow != AxisStatus::Below,
                "terminal upper-only domain",
                None,
                "tip/crossflow lower bound is inclusive zero",
            )?;
            R::OutsideOperatingEnvelope(TurbopropDomainStatus {
                pressure,
                temperature,
                mach,
                relative_shaft,
                absolute_spin,
                tip_mach,
                crossflow,
            })
        }
        4 => {
            let [pressure, temperature] = decode_axes(detail)?;
            R::OutsidePowerDomain(PowerDomainStatus {
                pressure,
                temperature,
            })
        }
        5 => {
            let [advance_ratio, blade_pitch] = decode_axes(detail)?;
            R::OutsidePropellerDomain(PropellerDomainStatus {
                advance_ratio,
                blade_pitch,
            })
        }
        7 if detail == 0 => R::SubstepBudgetExceeded,
        8 => R::PropellerPowerBound(match detail {
            1 => PropellerPowerBound::NonpositivePower,
            2 => PropellerPowerBound::BelowIdealDisk,
            3 => PropellerPowerBound::InvalidDerivedBound,
            _ => return Err(invalid("terminal power-bound detail", "requires1..3")),
        }),
        9 => R::NearStaticPowerBound(match detail {
            1 => StaticPowerBound::NonpositiveThrust,
            2 => StaticPowerBound::NonpositivePower,
            3 => StaticPowerBound::BelowStaticFloor,
            4 => StaticPowerBound::InvalidDerivedBound,
            _ => return Err(invalid("terminal near-static power detail", "requires1..4")),
        }),
        10 => {
            let [adverse_inflow, transverse_inflow] = decode_axes(detail)?;
            require_valid(
                adverse_inflow != AxisStatus::Below && transverse_inflow != AxisStatus::Below,
                "terminal near-static domain",
                None,
                "requires one Above axis and the other Within or Above",
            )?;
            R::OutsideNearStaticDomain(NearStaticDomainStatus {
                adverse_inflow,
                transverse_inflow,
            })
        }
        11 if detail == 0 => R::InvalidNearStaticScale,
        _ => {
            return Err(invalid(
                "terminal reason/detail",
                "unknown or inconsistent reason/detail",
            ));
        }
    })
}
fn diagnostic_values(d: TurbopropDiagnostics) -> [Option<f64>; 12] {
    let d = d.values();
    [
        d.pressure_ratio.map(|v| v.0),
        d.temperature_ratio.map(|v| v.0),
        d.mach.map(|v| v.0),
        d.advance_ratio.map(|v| v.0),
        d.blade_pitch.map(Radians::get),
        d.relative_shaft.map(RadiansPerSecond::get),
        d.absolute_spin.map(RadiansPerSecond::get),
        d.tip_mach.map(|v| v.0),
        d.crossflow_ratio,
        d.hover_velocity.map(MetersPerSecond::get),
        d.adverse_inflow_ratio,
        d.transverse_inflow_ratio,
    ]
}
fn diagnostic_mask(d: TurbopropDiagnostics) -> u16 {
    diagnostic_values(d)
        .iter()
        .enumerate()
        .fold(0, |mask, (i, v)| mask | (u16::from(v.is_some()) << i))
}
pub(super) fn terminal_length(error: TurbopropStepError) -> u32 {
    62 + 8 * diagnostic_mask(error.diagnostics).count_ones()
}
pub(super) fn validate_failure(error: TurbopropStepError) -> Result<(), ReplayError> {
    use TurbopropFailureReason as R;
    use TurbopropInvalidInput as I;
    require_valid(
        error.substep < 8 && (error.stage != TurbopropStage::Initial || error.substep == 0),
        "terminal substep",
        None,
        "requires0..7 and Initial substep0",
    )?;
    let (tag, detail) = reason_codes(error.reason);
    decode_reason(tag, detail)?;
    let mask = diagnostic_mask(error.diagnostics);
    // The only complete groups established by law 2 are 0, 0x30, 0x77,
    // 0x1f7, 0x1ff and 0xfff. Near-static fields are inserted atomically.
    let valid_mask = match error.reason {
        R::OutsideAtmosphereAltitude(_) => mask == 0,
        R::OutsideOperatingEnvelope(d) => {
            let base_outside = [
                d.pressure,
                d.temperature,
                d.mach,
                d.relative_shaft,
                d.absolute_spin,
            ]
            .iter()
            .any(|a| *a != AxisStatus::Within);
            if base_outside {
                mask == 0x77
                    && d.tip_mach == AxisStatus::Within
                    && d.crossflow == AxisStatus::Within
            } else {
                mask == 0x1f7
            }
        }
        R::OutsidePowerDomain(_) => mask == 0x1f7,
        R::OutsidePropellerDomain(_) | R::PropellerPowerBound(_) => mask == 0x1ff,
        R::OutsideMachDomain(_) => matches!(mask, 0x1ff | 0xfff),
        R::NearStaticPowerBound(_) => mask == 0x1ff,
        R::OutsideNearStaticDomain(_) => mask == 0xfff,
        R::InvalidNearStaticScale => matches!(mask, 0x1f7 | 0x1ff),
        R::SubstepBudgetExceeded => {
            matches!(mask, 0x1ff | 0xfff) || (mask == 0 && error.stage == TurbopropStage::Initial)
        }
        R::InvalidInput(code) => match code {
            I::TimeStep
            | I::State
            | I::Quaternion
            | I::Position
            | I::TurbineFraction
            | I::ShaftRate
            | I::BladePitch
            | I::IntermediateState => mask == 0,
            I::AtmosphereOffset
            | I::Wind
            | I::RelativeVelocity
            | I::AtmosphereTemperature
            | I::AtmosphereDensity => mask == 0x30,
            I::Ground => {
                mask == 0x30
                    || (matches!(mask, 0x1ff | 0xfff) && error.stage == TurbopropStage::Endpoint)
            }
            I::ComponentQuery => matches!(mask, 0 | 0x30 | 0x77 | 0x1f7 | 0x1ff | 0xfff),
            I::Derivative => matches!(mask, 0x1ff | 0xfff),
        },
    };
    require_valid(
        valid_mask,
        "terminal diagnostic mask",
        None,
        "must match the reason and law2 evaluation order",
    )?;
    for (i, value) in diagnostic_values(error.diagnostics).into_iter().enumerate() {
        if let Some(value) = value {
            let sign_valid = match i {
                0 | 2 | 7 | 8 | 11 => value >= 0.0,
                1 | 5 | 9 | 10 => value > 0.0,
                4 => (0.0..=std::f64::consts::FRAC_PI_2).contains(&value),
                _ => true, // J may be reverse-flow; absolute spin may be nonpositive.
            };
            require_valid(
                value.is_finite() && sign_valid,
                "terminal diagnostic",
                None,
                "requires finite validated physical values",
            )?;
        }
    }
    let values = error.diagnostics.values();
    let negative_j = values.advance_ratio.is_some_and(|j| j.0 < 0.0);
    // The old power bound is evaluated only by the retained forward map.
    // Post-flow failures have the full group exactly when the branch was J<0.
    let needs_forward_j = mask == 0x1ff
        && matches!(
            error.reason,
            R::PropellerPowerBound(_)
                | R::OutsideMachDomain(_)
                | R::SubstepBudgetExceeded
                | R::InvalidInput(I::Derivative | I::Ground)
        );
    let needs_negative_j = mask == 0xfff
        || matches!(error.reason, R::NearStaticPowerBound(_))
        || (mask == 0x1ff && error.reason == R::InvalidNearStaticScale);
    require_valid(
        (!needs_negative_j || negative_j) && (!needs_forward_j || !negative_j),
        "terminal advance-ratio branch",
        None,
        "diagnostic group and reason must agree with the forward or negative branch",
    )?;
    if mask == 0xfff {
        // Fixed law-2 bounds are codec semantics, independent of the aircraft.
        let axis = |ratio: f64| {
            if ratio > 0.1 {
                AxisStatus::Above
            } else {
                AxisStatus::Within
            }
        };
        let domain = NearStaticDomainStatus {
            adverse_inflow: axis(values.adverse_inflow_ratio.expect("full group")),
            transverse_inflow: axis(values.transverse_inflow_ratio.expect("full group")),
        };
        let matches_domain = match error.reason {
            R::OutsideNearStaticDomain(encoded) => encoded == domain,
            _ => domain.is_supported(),
        };
        require_valid(
            matches_domain,
            "terminal near-static ratios",
            None,
            "status must agree with the fixed inclusive0.1 bounds",
        )?;
    }
    Ok(())
}
pub(super) fn write_failure<W: Write>(w: &mut W, e: TurbopropStepError) -> Result<(), ReplayError> {
    let (tag, detail) = reason_codes(e.reason);
    w.write_all(&[tag])?;
    w.write_all(&detail.to_le_bytes())?;
    w.write_all(&[e.stage as u8])?;
    w.write_all(&e.substep.to_le_bytes())?;
    w.write_all(&diagnostic_mask(e.diagnostics).to_le_bytes())?;
    for value in diagnostic_values(e.diagnostics).into_iter().flatten() {
        write_f64(w, value)?;
    }
    Ok(())
}
pub(super) fn read_failure<R: Read>(
    r: &mut R,
    length: u32,
) -> Result<TurbopropStepError, ReplayError> {
    let tag = read_u8(r)?;
    let detail = read_u16(r)?;
    let reason = decode_reason(tag, detail)?;
    let stage = match read_u8(r)? {
        0 => TurbopropStage::Initial,
        1 => TurbopropStage::K1,
        2 => TurbopropStage::K2,
        3 => TurbopropStage::K3,
        4 => TurbopropStage::K4,
        5 => TurbopropStage::Endpoint,
        _ => return Err(invalid("terminal stage", "unknown stage")),
    };
    let substep = read_u32(r)?;
    let mask = read_u16(r)?;
    require_valid(
        mask & !0xfff == 0 && length == 62 + 8 * mask.count_ones(),
        "terminal length/mask",
        None,
        "length must equal62+8*popcount with only bits0..11",
    )?;
    let mut v = [None; 12];
    for (i, slot) in v.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *slot = Some(read_f64(r)?);
        }
    }
    let diagnostics = TurbopropDiagnostics::from_values(TurbopropDiagnosticValues {
        pressure_ratio: v[0].map(PressureRatio),
        temperature_ratio: v[1].map(TemperatureRatio),
        mach: v[2].map(MachNumber),
        advance_ratio: v[3].map(AdvanceRatio),
        blade_pitch: v[4].map(Radians),
        relative_shaft: v[5].map(RadiansPerSecond),
        absolute_spin: v[6].map(RadiansPerSecond),
        tip_mach: v[7].map(MachNumber),
        crossflow_ratio: v[8],
        hover_velocity: v[9].map(MetersPerSecond),
        adverse_inflow_ratio: v[10],
        transverse_inflow_ratio: v[11],
    })
    .map_err(|_| invalid("terminal diagnostic", "requires finite values"))?;
    let error = TurbopropStepError {
        reason,
        substep,
        stage,
        diagnostics,
    };
    validate_failure(error)?;
    Ok(error)
}
pub(super) fn failure_bits_equal(a: TurbopropStepError, b: TurbopropStepError) -> bool {
    reason_codes(a.reason) == reason_codes(b.reason)
        && a.stage == b.stage
        && a.substep == b.substep
        && diagnostic_values(a.diagnostics).map(|v| v.map(f64::to_bits))
            == diagnostic_values(b.diagnostics).map(|v| v.map(f64::to_bits))
}
