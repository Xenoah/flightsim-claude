//! Validated, authored weather scenarios, independent of simulation and rendering.
//!
//! This is a parameter contract only. It neither diagnoses current weather from
//! monthly climate nor changes atmosphere, wind, turbulence or contact. Replay v3
//! serializes this contract without enabling weather playback in the application.
//! [`WeatherSelection::Legacy`] preserves those existing environmental semantics.
//! Future effects must use executed simulation time, never solar or wall time.
//!
//! Heights are fixed WGS84 ellipsoidal metres. Visibility parameters describe
//! separate, nonnegative extinction contributions; cloud quality must never
//! disable the ambient or fog contributions. See `docs/modeled-weather.md` for
//! the exact wire contract, bounds, authored presets and limitations.

use core::f64::consts::{FRAC_PI_2, PI};
use core::fmt;
use flightsim_core::{Geodetic, Meters, MetersPerSecond};

/// Resolved parameter layout. Unknown schemas must be rejected, not reinterpreted.
pub const WEATHER_PARAMETER_SCHEMA: u16 = 1;
/// Authored parameter semantics/presets. Changing them requires a new revision.
pub const WEATHER_MODEL_REVISION: u32 = 1;
/// Maximum precipitation water-equivalent depth rate: 300 mm/hour in SI m/s.
pub const MAX_PRECIPITATION_RATE: WaterEquivalentRate = WaterEquivalentRate(0.3 / 3600.0);

/// Source of the resolved parameters, not evidence of observed weather.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WeatherSource {
    /// Simulator/user-authored values; no live or diagnosed meteorology.
    AuthoredModel = 1,
}

/// Authored preset identity. Any override must be marked [`Self::Custom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WeatherPreset {
    /// Explicit, validated parameters, without a canonical preset claim.
    Custom = 0,
    Clear = 1,
    Cloud = 2,
    Fog = 3,
    Rain = 4,
    Snow = 5,
    /// A towering-cloud/rain visual scenario, without storm flight dynamics.
    Storm = 6,
}

/// Authored visual shapes, not diagnosed WMO cloud classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CloudMorphology {
    Layered = 1,
    Puffy = 2,
    Towering = 3,
}

/// Explicit authored precipitation phase. No temperature inference is performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PrecipitationKind {
    None = 0,
    Rain = 1,
    Snow = 2,
}

macro_rules! checked_tag {
    ($name:ident, $($tag:literal => $variant:ident),+ $(,)?) => {
        impl TryFrom<u16> for $name {
            type Error = WeatherError;

            fn try_from(tag: u16) -> Result<Self, Self::Error> {
                match tag {
                    $($tag => Ok(Self::$variant),)+
                    _ => Err(WeatherError::UnknownTag { field: stringify!($name), tag }),
                }
            }
        }
    };
}

checked_tag!(WeatherSource, 1 => AuthoredModel);
checked_tag!(WeatherPreset, 0 => Custom, 1 => Clear, 2 => Cloud, 3 => Fog,
    4 => Rain, 5 => Snow, 6 => Storm);
checked_tag!(CloudMorphology, 1 => Layered, 2 => Puffy, 3 => Towering);
checked_tag!(PrecipitationKind, 0 => None, 1 => Rain, 2 => Snow);

/// Water-equivalent precipitation depth rate in **metres per second**.
///
/// For snow this is liquid-water-equivalent depth, not snowfall depth or particle
/// fall speed. Like core unit types, the raw value is validated at the scenario
/// boundary. The module does not model accumulation or infer a snow/water ratio.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct WaterEquivalentRate(pub f64);

impl WaterEquivalentRate {
    pub const ZERO: Self = Self(0.0);

    #[must_use]
    pub const fn as_meters_per_second(self) -> MetersPerSecond {
        MetersPerSecond(self.0)
    }
}

/// A single authored layer with fixed ellipsoidal bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeledCloudLayer {
    pub morphology: CloudMorphology,
    pub base: Meters,
    pub top: Meters,
    /// Nominal covered fraction, strictly positive and at most one.
    /// Absence is represented by `None`, not a zero-coverage layer.
    pub coverage: f64,
    /// Separate extinction contribution in occupied cloud, not total visibility.
    pub visibility: Meters,
}

/// Horizontally uniform authored fog with fixed ellipsoidal bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeledFogLayer {
    pub bottom: Meters,
    pub top: Meters,
    /// Separate extinction contribution inside fog, not total visibility.
    pub visibility: Meters,
}

/// Editable or externally decoded values; use [`WeatherScenario::try_from`]
/// before passing them to any future consumer. No field is silently clamped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherParameters {
    pub parameter_schema: u16,
    pub source: WeatherSource,
    pub model_revision: u32,
    pub preset: WeatherPreset,
    pub seed: u64,
    /// Fixed departure reference, independent of moving aircraft/DEM samples.
    /// Its altitude is the resolved ground/reference height, not aircraft AGL.
    pub departure_reference: Geodetic,
    /// Background extinction contribution outside and inside cloud/fog layers.
    pub ambient_visibility: Meters,
    pub precipitation_kind: PrecipitationKind,
    pub precipitation_rate: WaterEquivalentRate,
    pub cloud: Option<ModeledCloudLayer>,
    pub fog: Option<ModeledFogLayer>,
}

/// Immutable scenario that has passed all revision, numeric and preset checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherScenario {
    parameters: WeatherParameters,
}

/// Explicit opt-in state. Default/absence means existing environmental behavior,
/// including its existing climate/manual cloud path; it does **not** mean Clear.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WeatherSelection {
    #[default]
    Legacy,
    Modeled(WeatherScenario),
}

impl WeatherScenario {
    /// Resolve one of the six fixed authored presets at a departure reference.
    ///
    /// # Errors
    /// Rejects `Custom` (which needs explicit parameters), unknown/out-of-range
    /// departure coordinates and all invalid resolved values.
    pub fn from_preset(
        preset: WeatherPreset,
        departure_reference: Geodetic,
        seed: u64,
    ) -> Result<Self, WeatherError> {
        Self::try_from(preset_parameters(preset, departure_reference, seed)?)
    }

    /// A copy is safe to edit; edits must pass validation to become a scenario.
    #[must_use]
    pub const fn parameters(self) -> WeatherParameters {
        self.parameters
    }
}

impl TryFrom<WeatherParameters> for WeatherScenario {
    type Error = WeatherError;

    fn try_from(parameters: WeatherParameters) -> Result<Self, Self::Error> {
        if parameters.parameter_schema != WEATHER_PARAMETER_SCHEMA {
            return Err(WeatherError::UnsupportedSchema(parameters.parameter_schema));
        }
        if parameters.model_revision != WEATHER_MODEL_REVISION {
            return Err(WeatherError::UnsupportedModelRevision(
                parameters.model_revision,
            ));
        }
        let departure = parameters.departure_reference;
        finite_range(
            "departure latitude",
            departure.latitude.get(),
            -FRAC_PI_2,
            FRAC_PI_2,
        )?;
        finite_range("departure longitude", departure.longitude.get(), -PI, PI)?;
        finite_range(
            "departure height",
            departure.altitude.get(),
            -1000.0,
            10_000.0,
        )?;
        visibility("ambient visibility", parameters.ambient_visibility)?;
        finite_range(
            "precipitation rate",
            parameters.precipitation_rate.0,
            0.0,
            MAX_PRECIPITATION_RATE.0,
        )?;
        // Finite, nonnegative rate was checked above, so <= 0 is exact zero.
        if (parameters.precipitation_kind == PrecipitationKind::None)
            != (parameters.precipitation_rate.0 <= 0.0)
        {
            return Err(WeatherError::PrecipitationKindRateMismatch);
        }
        if let Some(cloud) = parameters.cloud {
            layer_bounds("cloud", cloud.base, cloud.top, 30_000.0, 20_000.0)?;
            finite_range("cloud coverage", cloud.coverage, 0.0, 1.0)?;
            if cloud.coverage <= 0.0 {
                return Err(WeatherError::ZeroCloudCoverage);
            }
            visibility("cloud visibility", cloud.visibility)?;
        }
        if let Some(fog) = parameters.fog {
            layer_bounds("fog", fog.bottom, fog.top, 15_000.0, 5000.0)?;
            visibility("fog visibility", fog.visibility)?;
        }
        if parameters.preset != WeatherPreset::Custom
            && parameters != preset_parameters(parameters.preset, departure, parameters.seed)?
        {
            return Err(WeatherError::PresetMismatch(parameters.preset));
        }
        Ok(Self { parameters })
    }
}

fn preset_parameters(
    preset: WeatherPreset,
    departure_reference: Geodetic,
    seed: u64,
) -> Result<WeatherParameters, WeatherError> {
    let mut result = WeatherParameters {
        parameter_schema: WEATHER_PARAMETER_SCHEMA,
        source: WeatherSource::AuthoredModel,
        model_revision: WEATHER_MODEL_REVISION,
        preset,
        seed,
        departure_reference,
        ambient_visibility: Meters(100_000.0),
        precipitation_kind: PrecipitationKind::None,
        precipitation_rate: WaterEquivalentRate::ZERO,
        cloud: None,
        fog: None,
    };
    let reference = departure_reference.altitude;
    let cloud = |morphology, base, top, coverage, visibility| {
        Some(ModeledCloudLayer {
            morphology,
            base: reference + base,
            top: reference + top,
            coverage,
            visibility,
        })
    };
    match preset {
        WeatherPreset::Custom => return Err(WeatherError::CustomRequiresParameters),
        WeatherPreset::Clear => {}
        WeatherPreset::Cloud => {
            result.ambient_visibility = Meters(40_000.0);
            result.cloud = cloud(
                CloudMorphology::Puffy,
                Meters(1500.0),
                Meters(2700.0),
                0.65,
                Meters(500.0),
            );
        }
        WeatherPreset::Fog => {
            result.ambient_visibility = Meters(50_000.0);
            result.fog = Some(ModeledFogLayer {
                bottom: reference,
                top: reference + Meters(300.0),
                visibility: Meters(250.0),
            });
        }
        WeatherPreset::Rain => {
            result.ambient_visibility = Meters(10_000.0);
            result.cloud = cloud(
                CloudMorphology::Layered,
                Meters(600.0),
                Meters(2600.0),
                1.0,
                Meters(250.0),
            );
            result.precipitation_kind = PrecipitationKind::Rain;
            result.precipitation_rate = WaterEquivalentRate(0.005 / 3600.0);
        }
        WeatherPreset::Snow => {
            result.ambient_visibility = Meters(3000.0);
            result.cloud = cloud(
                CloudMorphology::Layered,
                Meters(300.0),
                Meters(1800.0),
                1.0,
                Meters(200.0),
            );
            result.precipitation_kind = PrecipitationKind::Snow;
            result.precipitation_rate = WaterEquivalentRate(0.001 / 3600.0);
        }
        WeatherPreset::Storm => {
            result.ambient_visibility = Meters(5000.0);
            result.cloud = cloud(
                CloudMorphology::Towering,
                Meters(500.0),
                Meters(8000.0),
                1.0,
                Meters(150.0),
            );
            result.precipitation_kind = PrecipitationKind::Rain;
            result.precipitation_rate = WaterEquivalentRate(0.025 / 3600.0);
        }
    }
    Ok(result)
}

fn finite_range(field: &'static str, value: f64, min: f64, max: f64) -> Result<(), WeatherError> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(WeatherError::OutOfRange { field });
    }
    Ok(())
}

fn visibility(field: &'static str, value: Meters) -> Result<(), WeatherError> {
    finite_range(field, value.get(), 10.0, 200_000.0)
}

fn layer_bounds(
    field: &'static str,
    base: Meters,
    top: Meters,
    maximum_height: f64,
    maximum_thickness: f64,
) -> Result<(), WeatherError> {
    finite_range(field, base.get(), -1000.0, maximum_height)?;
    finite_range(field, top.get(), -1000.0, maximum_height)?;
    finite_range(field, (top - base).get(), 1.0, maximum_thickness)
}

/// A strict validation failure, without clamping or rewriting external inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherError {
    UnsupportedSchema(u16),
    UnsupportedModelRevision(u32),
    UnknownTag { field: &'static str, tag: u16 },
    OutOfRange { field: &'static str },
    PrecipitationKindRateMismatch,
    ZeroCloudCoverage,
    CustomRequiresParameters,
    PresetMismatch(WeatherPreset),
}

impl fmt::Display for WeatherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(schema) => {
                write!(formatter, "unsupported weather schema {schema}")
            }
            Self::UnsupportedModelRevision(revision) => {
                write!(formatter, "unsupported weather model revision {revision}")
            }
            Self::UnknownTag { field, tag } => {
                write!(formatter, "unknown weather {field} tag {tag}")
            }
            Self::OutOfRange { field } => {
                write!(formatter, "weather {field} is nonfinite or out of range")
            }
            Self::PrecipitationKindRateMismatch => {
                formatter.write_str("weather precipitation kind and rate disagree")
            }
            Self::ZeroCloudCoverage => {
                formatter.write_str("zero cloud coverage must be represented by no layer")
            }
            Self::CustomRequiresParameters => {
                formatter.write_str("custom weather requires explicit parameters")
            }
            Self::PresetMismatch(preset) => write!(
                formatter,
                "weather parameters do not match {preset:?}; label overrides Custom"
            ),
        }
    }
}

impl std::error::Error for WeatherError {}
