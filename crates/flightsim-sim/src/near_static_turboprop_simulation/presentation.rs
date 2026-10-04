//! Read-only presentation at the committed physical clock. No renderer state
//! enters the simulation and no table is extrapolated for an unsupported query.
use super::NearStaticTurbopropSimulation;
use crate::{GroundPlane, InterpolatedState};
use flightsim_core::Meters;
use flightsim_fdm::subsonic::{JetConditions, MachNumber};
use flightsim_fdm::{AeroAngles, AeroCoefficients, AtmosphereSample};
use flightsim_world::ClimateSample;

/// Pose alone is interpolated. All instruments/contact diagnostics describe the
/// current committed state at its executed clock, using the sim-owned weather
/// bridge and held ground plane. Reading never samples terrain or steps physics.
#[derive(Debug, Clone, Copy)]
pub struct NearStaticTurbopropPresentationSnapshot {
    pub pose: InterpolatedState,
    pub atmosphere: AtmosphereSample,
    pub aero_angles: AeroAngles,
    pub mach: Option<MachNumber>,
    /// None when the current query is outside the authored operating envelope
    /// or schedule. This can occur at an unsupported frame-zero terminal.
    pub aero_coefficients: Option<AeroCoefficients>,
    pub climate: Option<ClimateSample>,
    pub ground: GroundPlane,
    pub agl: Meters,
    pub gear_clearances: [Meters; 3],
    /// Contact-history hysteresis, not a new geometric collision classifier.
    pub on_ground: bool,
    /// Zero below 5 m/s or without supported coefficients; otherwise absolute
    /// angle of attack divided by the Mach-scheduled stall angle, without a cap.
    pub stall_fraction: f64,
}

impl NearStaticTurbopropSimulation {
    #[must_use]
    pub fn climate_sample(&self) -> Option<ClimateSample> {
        self.climate
            .as_ref()
            .zip(self.environment.conditions.climate_date)
            .map(|(climate, date)| climate.sample(self.state().rigid_body.geodetic(), date))
    }

    #[must_use]
    pub fn atmosphere_sample(&self) -> AtmosphereSample {
        self.environment_at(self.state(), self.committed.ground, self.elapsed())
            .atmosphere
            .sample(self.state().rigid_body.altitude())
    }

    #[must_use]
    pub fn aero_angles(&self) -> AeroAngles {
        flightsim_fdm::aero_angles_of(
            &self.state().rigid_body,
            &self.environment_at(self.state(), self.committed.ground, self.elapsed()),
        )
    }

    #[must_use]
    pub fn stall_fraction(&self) -> f64 {
        self.presentation().stall_fraction
    }

    #[must_use]
    pub fn presentation(&self) -> NearStaticTurbopropPresentationSnapshot {
        self.presentation_with_pose(self.interpolated())
    }

    pub(crate) fn presentation_with_pose(
        &self,
        pose: InterpolatedState,
    ) -> NearStaticTurbopropPresentationSnapshot {
        let environment = self.environment_at(self.state(), self.committed.ground, self.elapsed());
        let atmosphere = environment
            .atmosphere
            .sample(self.state().rigid_body.altitude());
        let aero_angles = flightsim_fdm::aero_angles_of(&self.state().rigid_body, &environment);
        // Use the raw relative norm, just as the turboprop law does. The legacy angle
        // helper deliberately substitutes zeros for invalid relative velocity.
        let relative = self.state().rigid_body.velocity - environment.wind_ecef;
        let body_relative = self.state().rigid_body.orientation.inverse() * relative;
        let query = (relative.is_finite()
            && relative.length().is_finite()
            && body_relative.is_finite()
            && body_relative.length().is_finite())
        .then(|| {
            JetConditions::from_atmosphere(
                atmosphere,
                flightsim_core::MetersPerSecond(body_relative.length()),
            )
            .ok()
        })
        .flatten();
        let aero_coefficients = query
            .filter(|_| {
                // Full current propulsion support is evaluated read-only. A
                // presentation query never warms, clamps or commits the rotor.
                self.dynamics
                    .derivative(
                        self.state(),
                        flightsim_fdm::ControlInputs::neutral(),
                        &environment,
                    )
                    .is_ok()
            })
            .and_then(|query| self.config().aero().sample(query.mach).ok())
            .map(|sample| sample.coefficients);
        let stall_fraction = aero_coefficients.map_or(0.0, |coefficients| {
            if !aero_angles.is_finite() || aero_angles.true_airspeed.get() < 5.0 {
                0.0
            } else {
                aero_angles.angle_of_attack.get().abs() / coefficients.stall_angle.get().abs()
            }
        });
        NearStaticTurbopropPresentationSnapshot {
            pose,
            atmosphere,
            aero_angles,
            mach: query.map(|query| query.mach),
            aero_coefficients,
            climate: self.climate_sample(),
            ground: self.committed.ground,
            agl: Meters(
                self.state().rigid_body.altitude().get() - self.committed.ground.elevation.get(),
            ),
            gear_clearances: self.committed.gear_clearances,
            on_ground: !self.committed.airborne,
            stall_fraction,
        }
    }
}
