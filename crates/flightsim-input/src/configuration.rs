//! Versioned, human-editable controller mappings and calibration.
//!
//! Values are dimensionless normalized device positions. Physical units never
//! enter this layer. Validation is fallible, including for user-authored files;
//! malformed configurations must not silently activate a different mapping.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::AxisCurve;

/// Maximum file size, checked before allocating/parsing an external file.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// A stable model selector. Runtime ECS entity IDs are deliberately not saved.
///
/// `First` is the conventional gamepad default. For a multi-controller cockpit,
/// use `Named`; `instance` distinguishes identical model names in connection
/// order. Bevy exposes no serial number, so identical physical units must be
/// connected in the same order after restarting the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeviceSelector {
    First,
    Named {
        name: String,
        #[serde(default)]
        vendor_id: Option<u16>,
        #[serde(default)]
        product_id: Option<u16>,
        #[serde(default)]
        instance: u16,
    },
}

/// Bevy's standard axes plus every backend-exposed nonstandard HOTAS axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisId {
    LeftStickX,
    LeftStickY,
    LeftZ,
    RightStickX,
    RightStickY,
    RightZ,
    Other(u8),
    /// Platform-specific native event code, available with --native-controllers.
    Native(u32),
}

/// Standard and backend-exposed nonstandard controller buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonId {
    South,
    East,
    North,
    West,
    C,
    Z,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Other(u8),
    /// Platform-specific native event code, available with --native-controllers.
    Native(u32),
}

/// An analog channel can also be a pressure-sensitive button/trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AnalogInput {
    Axis(AxisId),
    Button(ButtonId),
}

/// The unfiltered endpoints and neutral value reported by the input backend.
///
/// For centered flight controls, each side of `center` is independently mapped
/// to -1 or +1. For a unipolar lever (absolute throttle, flaps or brake), `min`
/// and `max` map to 0 and 1 and `center` is unused. Save measured values here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisCalibration {
    pub min: f64,
    pub center: f64,
    pub max: f64,
}

impl Default for AxisCalibration {
    fn default() -> Self {
        Self {
            min: -1.0,
            center: 0.0,
            max: 1.0,
        }
    }
}

impl AxisCalibration {
    /// Calibration for a pressure-sensitive button reporting 0..1.
    pub const BUTTON: Self = Self {
        min: 0.0,
        center: 0.5,
        max: 1.0,
    };

    pub(crate) fn validate(self) -> Result<(), InputConfigurationError> {
        if ![self.min, self.center, self.max]
            .iter()
            .all(|value| value.is_finite())
            || self.min >= self.center
            || self.center >= self.max
            || self.min < -1.0
            || self.max > 1.0
        {
            return Err(invalid("calibration needs -1 <= min < center < max <= 1"));
        }
        Ok(())
    }

    /// Normalize the two sides of a spring-centered stick independently.
    #[must_use]
    pub fn centered(self, raw: f64) -> f64 {
        if !raw.is_finite() || self.validate().is_err() {
            return 0.0;
        }
        let travel = if raw >= self.center {
            self.max - self.center
        } else {
            self.center - self.min
        };
        ((raw - self.center) / travel).clamp(-1.0, 1.0)
    }

    /// Normalize an absolute lever to 0..1, including the idle endpoint.
    #[must_use]
    pub fn unipolar(self, raw: f64) -> f64 {
        if !raw.is_finite() || self.validate().is_err() {
            return 0.0;
        }
        ((raw - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
}

/// A button reference; opposite directions may come from separate controllers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonSource {
    pub device: DeviceSelector,
    pub button: ButtonId,
}

/// A control can use any analog channel or a pair of digital/analog buttons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingSource {
    Axis {
        device: DeviceSelector,
        input: AnalogInput,
        #[serde(default)]
        calibration: AxisCalibration,
    },
    Buttons {
        positive: ButtonSource,
        #[serde(default)]
        negative: Option<ButtonSource>,
    },
}

/// Absolute levers set a position; rate controls move it and retain it on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMode {
    Absolute,
    Rate,
}

/// Mapping for one flight-control action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlBinding {
    pub source: BindingSource,
    pub mode: BindingMode,
    #[serde(default)]
    pub curve: AxisCurve,
}

impl ControlBinding {
    fn validate(&self) -> Result<(), InputConfigurationError> {
        if !self.curve.deadzone.is_finite() || !(0.0..1.0).contains(&self.curve.deadzone) {
            return Err(invalid("deadzone must be finite and in [0, 1)"));
        }
        if !self.curve.response.is_finite() || !(0.1..=10.0).contains(&self.curve.response) {
            return Err(invalid("response must be finite and in [0.1, 10]"));
        }
        match &self.source {
            BindingSource::Axis {
                device,
                calibration,
                ..
            } => {
                validate_device(device)?;
                calibration.validate()?;
            }
            BindingSource::Buttons { positive, negative } => {
                validate_device(&positive.device)?;
                if let Some(negative) = negative {
                    validate_device(&negative.device)?;
                    if positive == negative {
                        return Err(invalid("positive and negative buttons must differ"));
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_device(device: &DeviceSelector) -> Result<(), InputConfigurationError> {
    if let DeviceSelector::Named { name, .. } = device
        && (name.is_empty() || name.len() > 256 || name.chars().any(char::is_control))
    {
        return Err(invalid(
            "device name must contain 1..256 bytes without control characters",
        ));
    }
    Ok(())
}

/// Complete version-1 input configuration. `null` disables a controller mapping
/// for that action and leaves keyboard control available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputConfiguration {
    pub version: u32,
    pub pitch: Option<ControlBinding>,
    pub roll: Option<ControlBinding>,
    pub yaw: Option<ControlBinding>,
    pub throttle: Option<ControlBinding>,
    pub brake: Option<ControlBinding>,
    pub flaps: Option<ControlBinding>,
    /// Optional controller view shortcut; set null to free its button for flight.
    #[serde(default)]
    pub view_cycle: Option<ButtonSource>,
}

impl Default for InputConfiguration {
    fn default() -> Self {
        let axis = |id: AxisId, invert: bool| {
            Some(ControlBinding {
                source: BindingSource::Axis {
                    device: DeviceSelector::First,
                    input: AnalogInput::Axis(id),
                    calibration: AxisCalibration::default(),
                },
                mode: BindingMode::Absolute,
                curve: AxisCurve {
                    invert,
                    ..AxisCurve::default()
                },
            })
        };
        let buttons = |positive: ButtonId, negative: Option<ButtonId>, mode: BindingMode| {
            Some(ControlBinding {
                source: BindingSource::Buttons {
                    positive: ButtonSource {
                        device: DeviceSelector::First,
                        button: positive,
                    },
                    negative: negative.map(|button| ButtonSource {
                        device: DeviceSelector::First,
                        button,
                    }),
                },
                mode,
                curve: AxisCurve::default(),
            })
        };
        Self {
            version: 1,
            pitch: axis(AxisId::LeftStickY, true),
            roll: axis(AxisId::LeftStickX, false),
            yaw: axis(AxisId::RightStickX, false),
            throttle: buttons(
                ButtonId::RightTrigger2,
                Some(ButtonId::LeftTrigger2),
                BindingMode::Rate,
            ),
            brake: buttons(ButtonId::South, None, BindingMode::Absolute),
            flaps: buttons(
                ButtonId::DPadDown,
                Some(ButtonId::DPadUp),
                BindingMode::Rate,
            ),
            view_cycle: Some(ButtonSource {
                device: DeviceSelector::First,
                button: ButtonId::North,
            }),
        }
    }
}

impl InputConfiguration {
    /// Validate every setting without panicking, before it can reach controls.
    pub fn validate(&self) -> Result<(), InputConfigurationError> {
        if self.version != 1 {
            return Err(invalid(
                "unsupported input configuration version (expected 1)",
            ));
        }
        for (name, binding) in self.bindings() {
            if let Some(binding) = binding {
                binding
                    .validate()
                    .map_err(|error| invalid(format!("{name}: {error}")))?;
            }
        }
        if self
            .brake
            .as_ref()
            .is_some_and(|binding| binding.mode == BindingMode::Rate)
        {
            return Err(invalid(
                "brake must use absolute mode (released brakes must not latch)",
            ));
        }
        if let Some(button) = &self.view_cycle {
            validate_device(&button.device)?;
        }
        Ok(())
    }

    /// The fixed action order used by diagnostics and validation.
    #[must_use]
    pub fn bindings(&self) -> [(&'static str, Option<&ControlBinding>); 6] {
        [
            ("pitch", self.pitch.as_ref()),
            ("roll", self.roll.as_ref()),
            ("yaw", self.yaw.as_ref()),
            ("throttle", self.throttle.as_ref()),
            ("brake", self.brake.as_ref()),
            ("flaps", self.flaps.as_ref()),
        ]
    }

    /// Whether this configuration needs the optional native-channel backend.
    #[must_use]
    pub fn requires_native_backend(&self) -> bool {
        let native_button = |button: &ButtonSource| matches!(button.button, ButtonId::Native(_));
        self.bindings().iter().any(|(_, binding)| {
            binding.is_some_and(|binding| match &binding.source {
                BindingSource::Axis { input, .. } => matches!(
                    input,
                    AnalogInput::Axis(AxisId::Native(_)) | AnalogInput::Button(ButtonId::Native(_))
                ),
                BindingSource::Buttons { positive, negative } => {
                    native_button(positive) || negative.as_ref().is_some_and(native_button)
                }
            })
        }) || self.view_cycle.as_ref().is_some_and(native_button)
    }

    /// Parse and strictly validate a bounded JSON configuration.
    pub fn from_json(text: &str) -> Result<Self, InputConfigurationError> {
        if text.len() as u64 > MAX_CONFIG_BYTES {
            return Err(invalid("input configuration exceeds 1 MiB"));
        }
        let configuration: Self =
            serde_json::from_str(text).map_err(InputConfigurationError::Json)?;
        configuration.validate()?;
        Ok(configuration)
    }

    /// Serialize a validated configuration, including calibration and curves.
    pub fn to_json(&self) -> Result<String, InputConfigurationError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(InputConfigurationError::Json)
    }

    /// Load the requested file. Errors never turn into a default configuration.
    pub fn load(path: &Path) -> Result<Self, InputConfigurationError> {
        let file = fs::File::open(path).map_err(InputConfigurationError::Io)?;
        let mut text = String::new();
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(InputConfigurationError::Io)?;
        Self::from_json(&text)
    }

    /// Save to a new file without replacing an existing user configuration.
    /// All serialization and validation finish before the file is created.
    pub fn write_new(&self, path: &Path) -> Result<(), InputConfigurationError> {
        let mut text = self.to_json()?;
        text.push('\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(InputConfigurationError::Io)?;
        file.write_all(text.as_bytes())
            .map_err(InputConfigurationError::Io)?;
        file.sync_all().map_err(InputConfigurationError::Io)
    }
}

/// A configuration failed I/O, JSON parsing, or semantic validation.
#[derive(Debug)]
pub enum InputConfigurationError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> InputConfigurationError {
    InputConfigurationError::Invalid(message.into())
}

impl fmt::Display for InputConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for InputConfigurationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_maps_asymmetric_endpoints_and_neutral() {
        let calibration = AxisCalibration {
            min: -0.8,
            center: 0.1,
            max: 0.9,
        };
        for (raw, expected) in [
            (-0.8, -1.0),
            (-0.35, -0.5),
            (0.1, 0.0),
            (0.5, 0.5),
            (0.9, 1.0),
        ] {
            assert!((calibration.centered(raw) - expected).abs() < 1e-12);
        }
        assert!((calibration.unipolar(-0.8) - 0.0).abs() < 1e-12);
        assert!((calibration.unipolar(0.05) - 0.5).abs() < 1e-12);
        assert!((calibration.unipolar(0.9) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn non_finite_raw_values_do_not_reach_controls() {
        for raw in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(AxisCalibration::default().centered(raw).abs() < 1e-12);
            assert!(AxisCalibration::default().unipolar(raw).abs() < 1e-12);
        }
    }

    #[test]
    fn configuration_round_trip_preserves_every_setting() {
        let configuration = InputConfiguration {
            pitch: Some(ControlBinding {
                source: BindingSource::Axis {
                    device: DeviceSelector::Named {
                        name: "Flight Stick".into(),
                        vendor_id: Some(0x1234),
                        product_id: Some(0xabcd),
                        instance: 1,
                    },
                    input: AnalogInput::Axis(AxisId::Other(17)),
                    calibration: AxisCalibration {
                        min: -0.9,
                        center: 0.03,
                        max: 0.8,
                    },
                },
                mode: BindingMode::Absolute,
                curve: AxisCurve::new(0.07, 1.8, true),
            }),
            ..InputConfiguration::default()
        };
        let text = configuration.to_json().expect("valid configuration");
        assert_eq!(
            InputConfiguration::from_json(&text).expect("parse round trip"),
            configuration
        );
    }

    #[test]
    fn independently_authored_minimal_configuration_is_accepted() {
        let text = r#"{"version":1,"pitch":null,"roll":null,"yaw":null,"throttle":null,"brake":null,"flaps":null}"#;
        let configuration = InputConfiguration::from_json(text).expect("keyboard only");
        assert!(
            configuration
                .bindings()
                .iter()
                .all(|(_, binding)| binding.is_none())
        );
    }

    #[test]
    fn unknown_fields_and_schema_versions_are_rejected() {
        let text = InputConfiguration::default().to_json().expect("serialize");
        assert!(
            InputConfiguration::from_json(&text.replacen("\"version\": 1", "\"version\": 2", 1))
                .is_err()
        );
        assert!(
            InputConfiguration::from_json(&text.replacen(
                "\"version\": 1",
                "\"version\": 1, \"typo\": true",
                1
            ))
            .is_err()
        );
        assert!(
            InputConfiguration::from_json(&text.replacen(
                "\"response\": 1.0",
                "\"response\": 0.0",
                1
            ))
            .is_err()
        );
    }

    #[test]
    fn invalid_calibration_and_curve_are_errors_instead_of_panics() {
        let mut configuration = InputConfiguration::default();
        let pitch = configuration.pitch.as_mut().expect("pitch");
        pitch.curve.deadzone = 1.0;
        assert!(configuration.validate().is_err());
        configuration.pitch.as_mut().expect("pitch").curve = AxisCurve::default();
        if let BindingSource::Axis { calibration, .. } =
            &mut configuration.pitch.as_mut().expect("pitch").source
        {
            calibration.center = calibration.max;
        }
        assert!(configuration.validate().is_err());
        assert!(configuration.to_json().is_err());
    }

    #[test]
    fn brake_rate_mode_is_rejected_because_it_would_latch() {
        let mut configuration = InputConfiguration::default();
        configuration.brake.as_mut().expect("brake").mode = BindingMode::Rate;
        assert!(configuration.validate().is_err());
    }

    #[test]
    fn save_reload_preserves_calibration_and_never_overwrites_existing_file() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suffix = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flightsim-input-{}-{suffix}.json",
            std::process::id()
        ));
        let mut configuration = InputConfiguration::default();
        configuration.pitch.as_mut().expect("pitch").curve = AxisCurve::new(0.08, 2.0, true);
        configuration
            .write_new(&path)
            .expect("save new configuration");
        let before = fs::read(&path).expect("saved bytes");
        assert_eq!(
            InputConfiguration::load(&path).expect("reload"),
            configuration
        );
        assert!(InputConfiguration::default().write_new(&path).is_err());
        assert_eq!(fs::read(&path).expect("unchanged bytes"), before);
        fs::remove_file(path).expect("clean up test file");
    }

    #[test]
    fn oversized_or_truncated_configuration_is_rejected() {
        assert!(InputConfiguration::from_json(&" ".repeat(1024 * 1024 + 1)).is_err());
        let text = InputConfiguration::default().to_json().expect("serialize");
        assert!(InputConfiguration::from_json(&text[..text.len() - 1]).is_err());
    }

    #[test]
    fn native_channel_codes_round_trip_and_require_the_native_backend() {
        let mut configuration = InputConfiguration::default();
        assert!(!configuration.requires_native_backend());
        if let BindingSource::Axis { input, .. } =
            &mut configuration.roll.as_mut().expect("roll").source
        {
            *input = AnalogInput::Axis(AxisId::Native(0xffff_fffe));
        }
        assert!(configuration.requires_native_backend());
        let json = configuration.to_json().expect("serialize native code");
        assert!(json.contains("4294967294"));
        assert_eq!(
            InputConfiguration::from_json(&json).expect("native round trip"),
            configuration
        );
    }
}
