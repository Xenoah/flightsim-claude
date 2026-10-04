//! Pending new-flight forces. Display rounding never becomes physical input.
//! The current flight, its recorder and the fixed-step laws remain authoritative
//! until the existing complete-aircraft transaction commits.

use bevy::prelude::*;
use flightsim_fdm::Turbulence;
use flightsim_sim::Wind;
use flightsim_ui::world_map::{
    WindSettingsView, WorldMapActions, WorldMapConditionsEdit, WorldMapState, WorldMapSystems,
    WorldMapTurbulence,
};

use crate::{FlightSimulation, ReplayPlayback, Startup, world_runtime};

#[derive(Debug, Clone, Copy)]
pub(super) struct PhysicalConditions {
    pub wind: Wind,
    pub turbulence: Turbulence,
    wind_was_given: bool,
    turbulence_was_given: bool,
}

impl PhysicalConditions {
    pub fn from_startup(startup: &Startup) -> Self {
        Self {
            wind: startup.wind,
            turbulence: startup.turbulence,
            wind_was_given: startup.wind_was_given,
            turbulence_was_given: startup.turbulence_was_given,
        }
    }

    pub fn apply(self, startup: &mut Startup) {
        startup.wind = self.wind;
        startup.turbulence = self.turbulence;
        startup.wind_was_given = self.wind_was_given;
        startup.turbulence_was_given = self.turbulence_was_given;
    }

    /// Validate the complete action before changing even the pending snapshot.
    /// Only explicitly edited components pass through the UI unit boundary.
    fn edited(mut self, edit: WorldMapConditionsEdit) -> Result<Self, String> {
        if let Some(from) = edit.wind_from {
            if !from.get().is_finite() || !(0.0..=360.0).contains(&from.get()) {
                return Err("Wind from must be 0 to 360 deg true".into());
            }
            self.wind.from = crate::parse_wind(&format!("{}/0", from.get()))?.from;
            self.wind_was_given = true;
        }
        if let Some(speed) = edit.wind_speed {
            self.wind.speed = crate::parse_wind(&format!("0/{}", speed.get()))?.speed;
            self.wind_was_given = true;
        }
        if let Some(level) = edit.turbulence {
            self.turbulence.intensity = match level {
                WorldMapTurbulence::Calm => Turbulence::CALM,
                WorldMapTurbulence::Light => Turbulence::light(self.turbulence.seed),
                WorldMapTurbulence::Moderate => Turbulence::moderate(self.turbulence.seed),
                WorldMapTurbulence::Severe => Turbulence::severe(self.turbulence.seed),
            }
            .intensity;
            self.turbulence_was_given = true;
        }
        Ok(self)
    }
}

// Candidate matching is exact, including signed zero and override provenance.
// Ordinary float PartialEq would lose that distinction at the transaction edge.
impl PartialEq for PhysicalConditions {
    fn eq(&self, other: &Self) -> bool {
        self.wind.from.get().to_bits() == other.wind.from.get().to_bits()
            && self.wind.speed.get().to_bits() == other.wind.speed.get().to_bits()
            && self.turbulence.intensity.get().to_bits()
                == other.turbulence.intensity.get().to_bits()
            && self.turbulence.seed == other.turbulence.seed
            && self.wind_was_given == other.wind_was_given
            && self.turbulence_was_given == other.turbulence_was_given
    }
}
impl Eq for PhysicalConditions {}

#[derive(Resource, Debug, Default)]
pub(super) struct PendingConditions {
    pub selection: Option<PhysicalConditions>,
    was_visible: bool,
    error: String,
}

fn allowed(startup: &Startup, replay: bool) -> bool {
    !replay
        && startup.replay.is_none()
        && startup.traffic.host.is_none()
        && startup.traffic.join.is_none()
}

/// Used both at admission and before regional/scene completion. No rounded UI
/// strings, weather presets or current difficulty are consulted here.
pub(super) fn snapshot(world: &World, startup: &Startup) -> PhysicalConditions {
    let replay = world.contains_resource::<ReplayPlayback>()
        || world
            .get_resource::<FlightSimulation>()
            .is_some_and(|simulation| simulation.0.is_replay());
    if allowed(startup, replay)
        && let Some(selection) = world
            .get_resource::<PendingConditions>()
            .and_then(|pending| pending.selection)
    {
        selection
    } else {
        PhysicalConditions::from_startup(startup)
    }
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<PendingConditions>().add_systems(
        Update,
        select_pending_conditions
            .after(WorldMapSystems::Input)
            .after(world_runtime::capture_map_input)
            .after(crate::region_runtime::update)
            .before(world_runtime::apply_world_map_start)
            .before(WorldMapSystems::Display),
    );
}

#[allow(clippy::too_many_arguments)]
fn select_pending_conditions(
    startup: Res<Startup>,
    mut map: ResMut<WorldMapState>,
    mut actions: ResMut<WorldMapActions>,
    playback: Option<Res<ReplayPlayback>>,
    simulation: Option<Res<FlightSimulation>>,
    mut pending: ResMut<PendingConditions>,
) {
    if !map.visible || !pending.was_visible {
        pending.selection = Some(PhysicalConditions::from_startup(&startup));
        pending.error.clear();
    }
    pending.was_visible = map.visible;
    let replay = playback.is_some()
        || simulation
            .as_ref()
            .is_some_and(|simulation| simulation.0.is_replay());
    let enabled = allowed(&startup, replay);
    if let Some(edit) = actions.conditions.take() {
        // Defense in depth for non-UI callers and same-frame replay/LAN changes.
        actions.invalidate_start();
        if enabled && map.new_flight_modal_ready() {
            let current = pending
                .selection
                .unwrap_or_else(|| PhysicalConditions::from_startup(&startup));
            match current.edited(edit) {
                Ok(selection) => {
                    pending.selection = Some(selection);
                    pending.error.clear();
                }
                Err(error) => pending.error = error,
            }
        }
    }
    let selected = pending
        .selection
        .unwrap_or_else(|| PhysicalConditions::from_startup(&startup));
    map.wind_settings = WindSettingsView {
        wind_from: format!("{:.3}", selected.wind.from.to_degrees().get()),
        wind_speed: format!("{:.3}", selected.wind.speed.to_knots().get()),
        turbulence: format!(
            "Horizontal bound: {} m/s\nDeterministic seed: {}",
            selected.turbulence.intensity.get(),
            selected.turbulence.seed
        ),
        turbulence_seed: selected.turbulence.seed,
        enabled,
        note: if enabled {
            "Visual presets do not change wind or turbulence.".into()
        } else {
            "Wind controls unavailable during replay or LAN".into()
        },
        error: pending.error.clone(),
    };
}

#[cfg(test)]
#[path = "conditions_runtime_tests.rs"]
mod tests;
