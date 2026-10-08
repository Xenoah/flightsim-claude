//! Validated visual distances. These values only configure render selection;
//! they never configure a physical `Terrain`, its DEM levels, or replay identity.

use crate::LodSelector;
use flightsim_core::Meters;
use std::{fmt, str::FromStr};

/// Explicit finite limits for a future distance control. Presets cannot expand
/// the existing terrain leaf/cache or scenery residency/upload budgets.
pub const MIN_REFINEMENT_RADIUS: Meters = Meters(10_000.0);
pub const MAX_REFINEMENT_RADIUS: Meters = Meters(200_000.0);
pub const MIN_SCENERY_RADIUS: Meters = Meters(1_000.0);
pub const MAX_SCENERY_RADIUS: Meters = Meters(9_000.0);
pub const MIN_DETAIL_MARGIN: Meters = Meters(1_000.0);
pub const MAX_TERRAIN_DETAIL_RADIUS: Meters = Meters(11_000.0);
pub const MIN_CAMERA_FAR: Meters = Meters(100_000.0);
pub const MAX_CAMERA_FAR: Meters = Meters(400_000.0);
pub const MIN_SCREEN_SPACE_ERROR: f64 = 12.0;
pub const MAX_SCREEN_SPACE_ERROR: f64 = 24.0;
/// Far terrain remains an SSE-selected, complete coarse globe under a cap.
pub const COARSE_TERRAIN_LEVEL: u8 = 6;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DrawDistancePreset {
    Short,
    #[default]
    Standard,
    Long,
}

impl DrawDistancePreset {
    pub const ALL: [Self; 3] = [Self::Short, Self::Standard, Self::Long];

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Short => Self::Standard,
            Self::Standard => Self::Long,
            Self::Long => Self::Short,
        }
    }

    #[must_use]
    pub const fn policy(self) -> DrawDistancePolicy {
        let (refinement_radius, scenery_radius, terrain_detail_radius, screen_space_error) =
            match self {
                Self::Short => (
                    Some(Meters(10_000.0)),
                    Meters(2_250.0),
                    Meters(3_250.0),
                    24.0,
                ),
                Self::Standard => (None, Meters(4_500.0), Meters(5_500.0), 16.0),
                Self::Long => (None, Meters(9_000.0), Meters(11_000.0), 12.0),
            };
        DrawDistancePolicy {
            refinement_radius,
            scenery_radius,
            terrain_detail_radius,
            // Preserve distant geography in every preset. A smaller projection
            // alone would not reduce terrain selection or scenery preparation.
            camera_far: MAX_CAMERA_FAR,
            screen_space_error,
        }
    }
}

impl fmt::Display for DrawDistancePreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Short => "short",
            Self::Standard => "standard",
            Self::Long => "long",
        })
    }
}

impl FromStr for DrawDistancePreset {
    type Err = DrawDistanceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "short" => Ok(Self::Short),
            "standard" => Ok(Self::Standard),
            "long" => Ok(Self::Long),
            _ => Err(DrawDistanceError(
                "draw distance must be short, standard, or long",
            )),
        }
    }
}

/// A validated visual-only configuration. `None` leaves planetary refinement
/// governed by SSE and the unchanged leaf limit; it does not imply unlimited
/// detail, residency, or work. Distances are horizontal ECEF footprint distances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawDistancePolicy {
    refinement_radius: Option<Meters>,
    scenery_radius: Meters,
    terrain_detail_radius: Meters,
    camera_far: Meters,
    screen_space_error: f64,
}

impl Default for DrawDistancePolicy {
    fn default() -> Self {
        DrawDistancePreset::Standard.policy()
    }
}

impl DrawDistancePolicy {
    /// Construct bounded numeric settings without silently clamping invalid input.
    ///
    /// # Errors
    /// Nonfinite/out-of-range values, insufficient terrain detail around scenery,
    /// or a detail region outside the optional refinement radius are rejected.
    pub fn new(
        refinement_radius: Option<Meters>,
        scenery_radius: Meters,
        terrain_detail_radius: Meters,
        camera_far: Meters,
        screen_space_error: f64,
    ) -> Result<Self, DrawDistanceError> {
        let in_range = |value: Meters, minimum: Meters, maximum: Meters| {
            value.is_finite() && (minimum.get()..=maximum.get()).contains(&value.get())
        };
        if refinement_radius
            .is_some_and(|radius| !in_range(radius, MIN_REFINEMENT_RADIUS, MAX_REFINEMENT_RADIUS))
        {
            return Err(DrawDistanceError(
                "terrain refinement radius must be 10..=200 km",
            ));
        }
        if !in_range(scenery_radius, MIN_SCENERY_RADIUS, MAX_SCENERY_RADIUS) {
            return Err(DrawDistanceError("scenery radius must be 1..=9 km"));
        }
        if !in_range(
            terrain_detail_radius,
            Meters(scenery_radius.get() + MIN_DETAIL_MARGIN.get()),
            MAX_TERRAIN_DETAIL_RADIUS,
        ) || refinement_radius.is_some_and(|radius| terrain_detail_radius.get() > radius.get())
        {
            return Err(DrawDistanceError(
                "terrain detail must include scenery plus 1 km, be at most 11 km, and fit the refinement radius",
            ));
        }
        if !in_range(camera_far, MIN_CAMERA_FAR, MAX_CAMERA_FAR)
            || refinement_radius.is_some_and(|radius| radius.get() > camera_far.get())
        {
            return Err(DrawDistanceError(
                "camera far plane must be 100..=400 km and contain refinement",
            ));
        }
        if !screen_space_error.is_finite()
            || !(MIN_SCREEN_SPACE_ERROR..=MAX_SCREEN_SPACE_ERROR).contains(&screen_space_error)
        {
            return Err(DrawDistanceError(
                "terrain screen-space error must be 12..=24 px",
            ));
        }
        Ok(Self {
            refinement_radius,
            scenery_radius,
            terrain_detail_radius,
            camera_far,
            screen_space_error,
        })
    }

    #[must_use]
    pub const fn refinement_radius(self) -> Option<Meters> {
        self.refinement_radius
    }
    #[must_use]
    pub const fn scenery_radius(self) -> Meters {
        self.scenery_radius
    }
    #[must_use]
    pub const fn terrain_detail_radius(self) -> Meters {
        self.terrain_detail_radius
    }
    #[must_use]
    pub const fn camera_far(self) -> Meters {
        self.camera_far
    }
    #[must_use]
    pub const fn screen_space_error(self) -> f64 {
        self.screen_space_error
    }

    /// Return a replacement selector with the chosen policy. Keep the caller's
    /// max level, viewport, error estimate and leaf budget. Reapply regional
    /// coverage's near-detail request after this call; stale floors are removed.
    /// The caller must replace its live selector, not just a settings resource.
    #[must_use]
    pub fn apply_to_selector(self, selector: LodSelector) -> LodSelector {
        let selector = selector
            .with_screen_space_error(self.screen_space_error)
            .with_primary_coverage_radius(self.terrain_detail_radius)
            .without_near_detail()
            .without_refinement_radius();
        self.refinement_radius.map_or(selector, |radius| {
            selector.with_refinement_radius(radius, COARSE_TERRAIN_LEVEL)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawDistanceError(&'static str);

impl fmt::Display for DrawDistanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for DrawDistanceError {}

#[cfg(test)]
#[path = "draw_distance_tests.rs"]
mod tests;
