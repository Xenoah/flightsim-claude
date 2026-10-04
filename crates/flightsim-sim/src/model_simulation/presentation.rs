//! Read-only presentation at the committed physical clock. No renderer state
//! enters the simulation and no table is extrapolated for an unsupported query.
use super::JetSimulation;
use crate::{GroundPlane, InterpolatedState};
use flightsim_core::{Attitude, Ecef, LocalFrame, Meters};
use flightsim_fdm::subsonic::{JetConditions, MachNumber};
use flightsim_fdm::{AeroAngles, AeroCoefficients, AtmosphereSample};
use flightsim_world::ClimateSample;

/// Pose alone is interpolated. All instruments/contact diagnostics describe the
/// current committed state at its executed clock, using the sim-owned weather
/// bridge and held ground plane. Reading never samples terrain or steps physics.
#[derive(Debug, Clone, Copy)]
pub struct JetPresentationSnapshot {
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

impl JetSimulation {
    #[must_use]
    pub fn interpolated(&self) -> InterpolatedState {
        let alpha = if self.terminal.is_some() {
            1.0
        } else {
            self.fixed.interpolation_alpha()
        };
        self.interpolated_with_alpha(alpha)
    }

    pub(crate) fn interpolated_with_alpha(&self, alpha: f64) -> InterpolatedState {
        let previous = &self.committed.previous;
        let current = self.state();
        let position = Ecef::from_vec(
            previous
                .position
                .as_vec()
                .lerp(current.position.as_vec(), alpha),
        );
        let orientation = previous
            .orientation
            .slerp(current.orientation, alpha)
            .normalize();
        let geodetic = position.to_geodetic();
        let attitude = Attitude::from_quaternion(
            LocalFrame::new(geodetic).ned_to_ecef_rotation().inverse() * orientation,
        );
        InterpolatedState {
            position,
            orientation,
            geodetic,
            attitude,
        }
    }

    #[must_use]
    pub fn climate_sample(&self) -> Option<ClimateSample> {
        self.climate
            .as_ref()
            .zip(self.environment.conditions.climate_date)
            .map(|(climate, date)| climate.sample(self.state().geodetic(), date))
    }

    #[must_use]
    pub fn atmosphere_sample(&self) -> AtmosphereSample {
        self.environment_at(self.state(), self.committed.ground, self.elapsed())
            .atmosphere
            .sample(self.state().altitude())
    }

    #[must_use]
    pub fn aero_angles(&self) -> AeroAngles {
        flightsim_fdm::aero_angles_of(
            self.state(),
            &self.environment_at(self.state(), self.committed.ground, self.elapsed()),
        )
    }

    #[must_use]
    pub fn stall_fraction(&self) -> f64 {
        self.presentation().stall_fraction
    }

    #[must_use]
    pub fn presentation(&self) -> JetPresentationSnapshot {
        self.presentation_with_pose(self.interpolated())
    }

    pub(crate) fn presentation_with_pose(
        &self,
        pose: InterpolatedState,
    ) -> JetPresentationSnapshot {
        let environment = self.environment_at(self.state(), self.committed.ground, self.elapsed());
        let atmosphere = environment.atmosphere.sample(self.state().altitude());
        let aero_angles = flightsim_fdm::aero_angles_of(self.state(), &environment);
        // Use the raw relative norm, just as the jet law does. The legacy angle
        // helper deliberately substitutes zeros for invalid relative velocity.
        let relative = self.state().velocity - environment.wind_ecef;
        let body_relative = self.state().orientation.inverse() * relative;
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
            .filter(|query| {
                let envelope = self.config().envelope().definition();
                [
                    (envelope.pressure_ratio, query.pressure_ratio.0),
                    (envelope.temperature_ratio, query.temperature_ratio.0),
                    (envelope.mach, query.mach.0),
                ]
                .into_iter()
                .all(|([min, max], value)| (min..=max).contains(&value))
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
        JetPresentationSnapshot {
            pose,
            atmosphere,
            aero_angles,
            mach: query.map(|query| query.mach),
            aero_coefficients,
            climate: self.climate_sample(),
            ground: self.committed.ground,
            agl: Meters(self.state().altitude().get() - self.committed.ground.elevation.get()),
            gear_clearances: self.committed.gear_clearances,
            on_ground: !self.committed.airborne,
            stall_fraction,
        }
    }
}
