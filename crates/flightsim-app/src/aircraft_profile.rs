//! Complete aircraft selection: validated dynamics, model, camera and controls.
use flightsim_core::{Attitude, Meters, MetersPerSecond, Radians};
use flightsim_fdm::{AircraftConfig, RigidBodyState, definition::AircraftDefinition};
use flightsim_input::{AxisState, ElevatorTrim, PilotControls, RampAxis};
use flightsim_render::{ModelAxis, ModelFit};
use flightsim_sim::aircraft_profile::{AircraftProfileV2, EngineSound};
use flightsim_sim::aircraft_profile_v3::AircraftProfileV3;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path};

const MAX_PROFILE_BYTES: u64 = 128 * 1024;

/// App selection keeps each dynamics family behind its own validated decoder.
/// Display metadata and sound categories never choose the physical model.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "bounded startup-only profiles retain their validated family values explicitly"
)]
pub(super) enum SelectedAircraftProfile {
    Legacy(AircraftProfile),
    Jet(AircraftProfileV2),
    Turboprop(AircraftProfileV3),
}

impl From<AircraftProfile> for SelectedAircraftProfile {
    fn from(profile: AircraftProfile) -> Self {
        Self::Legacy(profile)
    }
}

impl SelectedAircraftProfile {
    pub fn builtin(id: &str) -> Result<Self, String> {
        AircraftProfile::builtin(id).map(Self::Legacy)
    }

    pub fn load(choice: &str) -> Result<Self, String> {
        if !choice.ends_with(".json") {
            return AircraftProfile::load(choice).map(Self::Legacy);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(choice)
            .map_err(|error| error.to_string())?
            .take((flightsim_sim::aircraft_profile::MAX_PROFILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > flightsim_sim::aircraft_profile::MAX_PROFILE_BYTES {
            return Err("aircraft profile exceeds 1 MiB".into());
        }
        // Only the integer version is decoded here. Derived unknown-field
        // handling uses IgnoredAny, so physical/metadata number tokens are
        // skipped without f64 conversion, Value, or enum-content buffering.
        // Every actual decoder receives the same original bounded bytes.
        #[derive(Deserialize)]
        struct VersionProbe {
            version: u16,
        }
        let probe: VersionProbe =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        match probe.version {
            1 => {
                if bytes.len() as u64 > MAX_PROFILE_BYTES {
                    return Err("aircraft profile exceeds 128 KiB".into());
                }
                let json = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
                AircraftProfile::parse(json).map(Self::Legacy)
            }
            2 => AircraftProfileV2::from_bytes(bytes)
                .map(Self::Jet)
                .map_err(|error| error.to_string()),
            3 => AircraftProfileV3::from_bytes(bytes)
                .map(Self::Turboprop)
                .map_err(|error| error.to_string()),
            version => Err(format!("unsupported aircraft profile version {version}")),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Legacy(profile) => &profile.id,
            Self::Jet(profile) => profile.id(),
            Self::Turboprop(profile) => profile.id(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Legacy(profile) => &profile.dynamics.name,
            Self::Jet(profile) => profile.configuration().airframe().name(),
            Self::Turboprop(profile) => profile.configuration().airframe().name(),
        }
    }

    pub fn model_path(&self) -> &str {
        match self {
            Self::Legacy(profile) => &profile.model.path,
            Self::Jet(profile) => &profile.model().path,
            Self::Turboprop(profile) => &profile.model().path,
        }
    }

    pub fn model_fit(&self) -> ModelFit {
        match self {
            Self::Legacy(profile) => profile.model_fit(),
            Self::Jet(_) | Self::Turboprop(_) => {
                let model = self.bounded_model().expect("bounded profile");
                ModelFit::new(
                    ModelAxis::parse(&model.forward).expect("validated axis"),
                    ModelAxis::parse(&model.up).expect("validated axis"),
                    Meters(model.length_m.get()),
                )
                .expect("validated axes")
            }
        }
    }

    pub fn pilot_controls(&self, approach: bool) -> PilotControls {
        match self {
            Self::Legacy(profile) => profile.pilot_controls(approach),
            Self::Jet(_) | Self::Turboprop(_) => {
                let controls = self.bounded_controls().expect("bounded profile");
                let mut result = PilotControls::default();
                result.aileron =
                    AxisState::new(controls.surface_rate.get(), controls.centering_rate.get());
                result.elevator = AxisState::new(
                    controls
                        .elevator_rate
                        .unwrap_or(controls.surface_rate)
                        .get(),
                    controls
                        .elevator_centering_rate
                        .unwrap_or(controls.centering_rate)
                        .get(),
                );
                result.rudder =
                    AxisState::new(controls.surface_rate.get(), controls.centering_rate.get());
                result.throttle = RampAxis::new(
                    if approach {
                        controls.approach_throttle.get()
                    } else {
                        0.0
                    },
                    controls.throttle_rate.get(),
                );
                result.flaps = RampAxis::new(
                    if approach {
                        controls.approach_flaps.get()
                    } else {
                        0.0
                    },
                    controls.flap_rate.get(),
                );
                result.trim = ElevatorTrim::new(
                    if approach {
                        controls.approach_trim.get()
                    } else {
                        controls.default_trim.get()
                    },
                    controls.trim_rate.get(),
                );
                result
            }
        }
    }

    pub fn camera_eye(&self) -> [Meters; 3] {
        match self {
            Self::Legacy(profile) => profile.camera_eye_m.map(Meters),
            Self::Jet(profile) => profile.camera_eye(),
            Self::Turboprop(profile) => profile.camera_eye(),
        }
    }

    pub fn engine_kind(&self) -> flightsim_audio::EngineKind {
        use flightsim_audio::{EngineKind, EngineSpec, TurbineSpec};
        match self {
            Self::Legacy(profile) => {
                EngineKind::parse(&profile.engine_sound).expect("validated engine sound")
            }
            Self::Jet(_) | Self::Turboprop(_) => {
                match self.bounded_sound().expect("bounded profile") {
                    EngineSound::Piston => EngineKind::Piston(EngineSpec::default()),
                    EngineSound::Turbine => {
                        let base = TurbineSpec::default();
                        EngineKind::Turbine(TurbineSpec {
                            afterburner_threshold: 1.0,
                            afterburner_exhaust_speed: base.military_exhaust_speed,
                            ..base
                        })
                    }
                }
            }
        }
    }

    fn bounded_model(&self) -> Option<&flightsim_sim::aircraft_profile::ModelDefinition> {
        match self {
            Self::Legacy(_) => None,
            Self::Jet(profile) => Some(profile.model()),
            Self::Turboprop(profile) => Some(profile.model()),
        }
    }

    pub fn bounded_controls(&self) -> Option<&flightsim_sim::aircraft_profile::ControlDefinition> {
        match self {
            Self::Legacy(_) => None,
            Self::Jet(profile) => Some(profile.controls()),
            Self::Turboprop(profile) => Some(profile.controls()),
        }
    }

    fn bounded_sound(&self) -> Option<EngineSound> {
        match self {
            Self::Legacy(_) => None,
            Self::Jet(profile) => Some(profile.engine_sound()),
            Self::Turboprop(profile) => Some(profile.engine_sound()),
        }
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }

    pub fn uses_bounded_model(&self) -> bool {
        matches!(self, Self::Jet(_) | Self::Turboprop(_))
    }

    pub fn is_turboprop(&self) -> bool {
        matches!(self, Self::Turboprop(_))
    }

    pub fn is_jet(&self) -> bool {
        matches!(self, Self::Jet(_))
    }

    pub fn legacy(&self) -> Option<&AircraftProfile> {
        match self {
            Self::Legacy(profile) => Some(profile),
            Self::Jet(_) | Self::Turboprop(_) => None,
        }
    }

    pub fn jet(&self) -> Option<&AircraftProfileV2> {
        match self {
            Self::Legacy(_) | Self::Turboprop(_) => None,
            Self::Jet(profile) => Some(profile),
        }
    }

    pub fn turboprop(&self) -> Option<&AircraftProfileV3> {
        match self {
            Self::Legacy(_) | Self::Jet(_) => None,
            Self::Turboprop(profile) => Some(profile),
        }
    }

    /// Compatibility helper for legacy simulation/replay paths. Callers must
    /// dispatch bounded families first; neither fabricates a legacy config.
    pub fn configuration(&self) -> AircraftConfig {
        self.legacy()
            .expect("legacy configuration requires explicit profile dispatch")
            .configuration()
    }

    /// Initial approach pose only, using the selected profile's validated hints.
    /// No trim, stability, or real-aircraft performance claim is implied.
    pub fn approach_state(
        &self,
        runway: &flightsim_world::Runway,
        distance: Meters,
        glideslope: Radians,
    ) -> RigidBodyState {
        match self {
            Self::Legacy(profile) => profile.approach_state(runway, distance, glideslope),
            Self::Jet(_) | Self::Turboprop(_) => {
                let controls = self.bounded_controls().expect("bounded profile");
                let state = flightsim_sim::approach_state(
                    runway,
                    distance,
                    glideslope,
                    MetersPerSecond(controls.approach_speed_mps.get()),
                );
                RigidBodyState::from_geodetic(
                    state.geodetic(),
                    Attitude::new(
                        Radians::ZERO,
                        Radians(controls.approach_pitch_rad.get()),
                        state.attitude().yaw,
                    ),
                    state.velocity_ned(),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AircraftProfile {
    pub version: u16,
    pub id: String,
    pub dynamics: AircraftDefinition,
    pub model: ModelDefinition,
    pub controls: ControlDefinition,
    pub camera_eye_m: [f64; 3],
    pub engine_sound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelDefinition {
    pub path: String,
    pub forward: String,
    pub up: String,
    pub length_m: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlDefinition {
    pub surface_rate: f64,
    /// Optional pitch ramp rate for keyboard and rate-mode bindings.
    /// Old custom profiles retain surface_rate when this field is absent.
    /// Absolute analog positions are unaffected by this ramp rate.
    #[serde(default)]
    pub elevator_rate: Option<f64>,
    pub centering_rate: f64,
    /// Optional pitch-only spring return for keyboard/rate-mode input. Missing
    /// values preserve legacy centering; absolute analog positions stay direct.
    #[serde(default)]
    pub elevator_centering_rate: Option<f64>,
    pub throttle_rate: f64,
    pub flap_rate: f64,
    pub default_trim: f64,
    pub trim_rate: f64,
    pub approach_speed_mps: f64,
    /// Initial attitude and trim are distinct from the flaps-up takeoff trim.
    pub approach_pitch_rad: f64,
    pub approach_trim: f64,
    pub approach_throttle: f64,
    pub approach_flaps: f64,
}

impl AircraftProfile {
    pub fn builtin(id: &str) -> Result<Self, String> {
        let json = match id {
            "light-single" => include_str!("../../../assets/aircraft/light_single.json"),
            "swift-sport" => include_str!("../../../assets/aircraft/swift_sport.json"),
            _ => {
                return Err(format!(
                    "unknown aircraft `{id}`; choose light-single or swift-sport, or a JSON profile path"
                ));
            }
        };
        Self::parse(json)
    }

    pub fn load(choice: &str) -> Result<Self, String> {
        if !choice.ends_with(".json") {
            return Self::builtin(choice);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(choice)
            .map_err(|e| e.to_string())?
            .take(MAX_PROFILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PROFILE_BYTES {
            return Err("aircraft profile exceeds 128 KiB".into());
        }
        let json = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        Self::parse(json)
    }

    fn parse(json: &str) -> Result<Self, String> {
        let profile: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn configuration(&self) -> AircraftConfig {
        // All creation paths validate first. No untrusted object can bypass parse.
        self.dynamics
            .to_config()
            .expect("validated aircraft profile")
    }

    pub fn model_fit(&self) -> ModelFit {
        ModelFit::new(
            ModelAxis::parse(&self.model.forward).expect("validated axis"),
            ModelAxis::parse(&self.model.up).expect("validated axis"),
            Meters(self.model.length_m),
        )
        .expect("validated axes")
    }

    pub fn pilot_controls(&self, approach: bool) -> PilotControls {
        let c = self.controls;
        let mut result = PilotControls::default();
        result.aileron = AxisState::new(c.surface_rate, c.centering_rate);
        result.elevator = AxisState::new(
            c.elevator_rate.unwrap_or(c.surface_rate),
            c.elevator_centering_rate.unwrap_or(c.centering_rate),
        );
        result.rudder = AxisState::new(c.surface_rate, c.centering_rate);
        result.throttle = RampAxis::new(
            if approach { c.approach_throttle } else { 0.0 },
            c.throttle_rate,
        );
        result.flaps = RampAxis::new(if approach { c.approach_flaps } else { 0.0 }, c.flap_rate);
        result.trim = ElevatorTrim::new(
            if approach {
                c.approach_trim
            } else {
                c.default_trim
            },
            c.trim_rate,
        );
        result
    }

    /// Place the aircraft on the requested approach with its configured pitch.
    ///
    /// Bundled controls/pitch balance a 3-degree, still-air approach near sea
    /// level. This sets the initial pose only; no controller runs afterward.
    /// Other heights, wind, glideslopes and custom profiles need pilot input.
    pub fn approach_state(
        &self,
        runway: &flightsim_world::Runway,
        distance: Meters,
        glideslope: Radians,
    ) -> RigidBodyState {
        let state = flightsim_sim::approach_state(
            runway,
            distance,
            glideslope,
            MetersPerSecond(self.controls.approach_speed_mps),
        );
        RigidBodyState::from_geodetic(
            state.geodetic(),
            Attitude::new(
                Radians::ZERO,
                Radians(self.controls.approach_pitch_rad),
                state.attitude().yaw,
            ),
            state.velocity_ned(),
        )
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "unsupported aircraft profile version {}",
                self.version
            ));
        }
        if self.id.is_empty()
            || self.id.len() > 48
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err("aircraft id must be a short lowercase ASCII identifier".into());
        }
        self.dynamics.to_config().map_err(|e| e.to_string())?;
        let path = Path::new(&self.model.path);
        if self.model.path.is_empty()
            || self.model.path.len() > 256
            || path.is_absolute()
            || self.model.path.contains('\\')
            || self.model.path.contains(':')
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || !self.model.path.ends_with(".glb")
        {
            return Err(
                "model path must be a relative .glb asset path without parent traversal".into(),
            );
        }
        if !self.model.length_m.is_finite() || !(1.0..=100.0).contains(&self.model.length_m) {
            return Err("model length must be 1..=100 metres".into());
        }
        ModelFit::new(
            ModelAxis::parse(&self.model.forward).map_err(|e| e.to_string())?,
            ModelAxis::parse(&self.model.up).map_err(|e| e.to_string())?,
            Meters(self.model.length_m),
        )
        .map_err(|e| e.to_string())?;
        for value in self.camera_eye_m {
            if !value.is_finite() || value.abs() > 10.0 {
                return Err(
                    "camera eye must be finite and within 10 metres of the center of mass".into(),
                );
            }
        }
        if flightsim_audio::EngineKind::parse(&self.engine_sound).is_none() {
            return Err("engine_sound must be piston or turbine".into());
        }
        let c = self.controls;
        for value in [
            c.surface_rate,
            c.elevator_rate.unwrap_or(c.surface_rate),
            c.throttle_rate,
            c.flap_rate,
            c.trim_rate,
        ] {
            if !value.is_finite() || !(0.01..=10.0).contains(&value) {
                return Err("control rates must be finite and within 0.01..=10".into());
            }
        }
        if !c.centering_rate.is_finite()
            || !(0.0..=10.0).contains(&c.centering_rate)
            || !c
                .elevator_centering_rate
                .unwrap_or(c.centering_rate)
                .is_finite()
            || !(0.0..=10.0).contains(&c.elevator_centering_rate.unwrap_or(c.centering_rate))
            || !c.default_trim.is_finite()
            || !(-1.0..=1.0).contains(&c.default_trim)
            || !c.approach_speed_mps.is_finite()
            || !(10.0..=150.0).contains(&c.approach_speed_mps)
            || !c.approach_pitch_rad.is_finite()
            || c.approach_pitch_rad.abs() > core::f64::consts::FRAC_PI_4
            || !c.approach_trim.is_finite()
            || !(-1.0..=1.0).contains(&c.approach_trim)
            || !c.approach_throttle.is_finite()
            || !(0.0..=1.0).contains(&c.approach_throttle)
            || !c.approach_flaps.is_finite()
            || !(0.0..=1.0).contains(&c.approach_flaps)
        {
            return Err("invalid aircraft control or approach setting".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    const JET_JSON: &str =
        include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json");
    const TURBOPROP_JSON: &str =
        include_str!("../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json");
    const LEGACY_JSON: &str = include_str!("../../../assets/aircraft/light_single.json");

    #[test]
    fn selection_keeps_builtin_and_external_legacy_metadata_and_controls() {
        for id in ["light-single", "swift-sport"] {
            let selected = SelectedAircraftProfile::builtin(id).unwrap();
            let legacy = AircraftProfile::builtin(id).unwrap();
            assert!(!selected.is_jet());
            assert!(!selected.is_turboprop());
            assert!(selected.is_legacy());
            assert!(!selected.uses_bounded_model());
            assert!(selected.legacy().is_some());
            assert!(selected.jet().is_none());
            assert_eq!(selected.id(), legacy.id);
            assert_eq!(selected.name(), legacy.dynamics.name);
            assert_eq!(selected.model_path(), legacy.model.path);
            assert_eq!(selected.model_fit(), legacy.model_fit());
            assert_eq!(selected.camera_eye(), legacy.camera_eye_m.map(Meters));
            assert_eq!(
                selected.engine_kind(),
                flightsim_audio::EngineKind::parse(&legacy.engine_sound).unwrap()
            );
            for approach in [false, true] {
                let selected_controls = selected.pilot_controls(approach);
                let legacy_controls = legacy.pilot_controls(approach);
                assert_eq!(selected_controls.aileron, legacy_controls.aileron);
                assert_eq!(selected_controls.elevator, legacy_controls.elevator);
                assert_eq!(selected_controls.rudder, legacy_controls.rudder);
                assert_eq!(selected_controls.throttle, legacy_controls.throttle);
                assert_eq!(selected_controls.flaps, legacy_controls.flaps);
                assert_eq!(
                    selected_controls.trim.value().to_bits(),
                    legacy_controls.trim.value().to_bits()
                );
            }
        }
        let external = SelectedAircraftProfile::from_bytes(LEGACY_JSON.as_bytes()).unwrap();
        assert!(external.legacy().is_some());
        assert_eq!(external.id(), "light-single");
        assert!(SelectedAircraftProfile::load("unknown-jet").is_err());
    }

    #[test]
    fn external_jet_selection_needs_no_model_asset_and_exposes_validated_metadata() {
        let selected = SelectedAircraftProfile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
        ))
        .unwrap();
        assert!(selected.is_jet());
        assert!(selected.legacy().is_none());
        assert!(selected.jet().is_some());
        assert_eq!(selected.id(), "numerical-jet-fixture");
        assert_eq!(
            selected.name(),
            "Numerical jet profile fixture (not a real aircraft)"
        );
        assert_eq!(
            selected.model_path(),
            "aircraft/unprovided_numerical_jet_fixture.glb"
        );
        assert_eq!(selected.model_fit().forward, ModelAxis::NegativeX);
        assert_eq!(selected.model_fit().up, ModelAxis::PositiveY);
    }

    #[test]
    fn external_v3_selection_dispatches_exactly_without_a_preset_or_model_asset() {
        let selected = SelectedAircraftProfile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
        ))
        .unwrap();
        assert!(selected.is_turboprop());
        assert!(selected.uses_bounded_model());
        assert!(!selected.is_jet());
        assert!(!selected.is_legacy());
        assert!(selected.jet().is_none());
        assert!(selected.legacy().is_none());
        assert_eq!(selected.id(), "numerical-turboprop-fixture");
        assert_eq!(
            selected.name(),
            "Numerical turboprop fixture (original synthetic data)"
        );
        assert_eq!(
            selected.model_path(),
            "aircraft/unprovided_numerical_turboprop_fixture.glb"
        );
        assert_eq!(selected.model_fit().forward, ModelAxis::NegativeX);
        assert_eq!(selected.model_fit().up, ModelAxis::PositiveY);
        let profile = selected.turboprop().unwrap();
        let aero = profile.configuration().aero().definition();
        assert_eq!(
            aero.knots[0].aero.pitch_rate_q.to_bits(),
            0xc03e_6666_6666_6667
        );
        assert_eq!(
            aero.knots[0].aero.yaw_rate_p.to_bits(),
            (-0.0_f64).to_bits()
        );
        assert_eq!(
            profile.running_start().turbine_fraction().get().to_bits(),
            (-0.0_f64).to_bits()
        );
        assert_eq!(
            profile.running_start().shaft_speed().get().to_bits(),
            180.0_f64.to_bits()
        );
        assert_eq!(
            profile.running_start().blade_pitch().get().to_bits(),
            0.3_f64.to_bits()
        );
        assert!(SelectedAircraftProfile::builtin("numerical-turboprop-fixture").is_err());
        assert!(SelectedAircraftProfile::builtin("cedar-turboprop").is_err());
    }

    #[test]
    fn v3_dispatch_keeps_strict_schema_kind_revision_and_number_admission() {
        for (old, new) in [
            ("\"version\": 3", "\"version\": 3.0"),
            ("\"version\": 3", "\"version\": 3e0"),
            ("\"version\": 3", "\"version\": 2"),
            ("\"revision\": 1", "\"revision\": 2"),
            ("running_turboprop_table", "dry_jet_table"),
            ("\"shaft_rad_s\": 180.0", "\"shaft_rad_s\": 1e9999"),
            ("\"shaft_rad_s\": 180.0", "\"shaft_rad_s\": 1e-9999"),
            (
                "\"turbine_fraction\": -0.0",
                "\"turbine_fraction\": -0.0, \"turbine_fraction\": 0.0",
            ),
            (
                "\"blade_pitch_rad\": 0.3",
                "\"blade_pitch_rad\": 0.3, \"unexpected\": 1",
            ),
        ] {
            assert!(TURBOPROP_JSON.contains(old));
            let invalid = TURBOPROP_JSON.replacen(old, new, 1);
            assert!(
                SelectedAircraftProfile::from_bytes(invalid.as_bytes()).is_err(),
                "{new}"
            );
        }
        assert!(
            SelectedAircraftProfile::from_bytes(format!("{TURBOPROP_JSON} null").as_bytes())
                .is_err()
        );
        let mut bytes = TURBOPROP_JSON.as_bytes().to_vec();
        bytes.resize(flightsim_sim::aircraft_profile_v3::MAX_PROFILE_BYTES, b' ');
        assert!(
            SelectedAircraftProfile::from_bytes(&bytes)
                .unwrap()
                .is_turboprop()
        );
        bytes.push(b' ');
        assert!(
            SelectedAircraftProfile::from_bytes(&bytes)
                .unwrap_err()
                .contains("1 MiB")
        );
    }

    #[test]
    fn dispatch_keeps_distinct_exact_v2_and_ordinary_v1_numeric_bits() {
        let selected = SelectedAircraftProfile::from_bytes(JET_JSON.as_bytes()).unwrap();
        let aero = selected.jet().unwrap().configuration().aero().definition();
        assert_eq!(
            aero.knots[0].aero.pitch_rate_q.to_bits(),
            0xc03e_6666_6666_6667
        );
        assert_eq!(
            aero.knots[0].aero.yaw_rate_p.to_bits(),
            (-0.0_f64).to_bits()
        );

        let legacy = LEGACY_JSON.replace(
            "\"pitch_rate_q\": -12.4",
            "\"pitch_rate_q\": -30.400000000000002",
        );
        let selected = SelectedAircraftProfile::from_bytes(legacy.as_bytes()).unwrap();
        assert_eq!(
            selected.configuration().aero.pitch_rate_q.to_bits(),
            0xc03e_6666_6666_6666
        );
        assert_eq!(
            selected.configuration().aero.pitch_rate_q.to_bits(),
            AircraftProfile::parse(&legacy)
                .unwrap()
                .configuration()
                .aero
                .pitch_rate_q
                .to_bits()
        );
    }

    #[test]
    fn bounded_metadata_adapter_retains_exact_numbers_signed_zero_and_subnormals() {
        for fixture in [JET_JSON, TURBOPROP_JSON] {
            // Just above the halfway point between 1 and its next binary64 value.
            let halfway_above = "1.00000000000000011102230246251565404236316680908203126";
            // Exercise both checkout styles without parsing or reserializing numbers.
            for newline in ["\n", "\r\n"] {
                let mut json = fixture.replace("\r\n", "\n").replace('\n', newline);
                for (before, after) in [
                    (
                        "\"length_m\": 8.3",
                        format!("\"length_m\": {halfway_above}"),
                    ),
                    (
                        "\"surface_rate\": 2.5",
                        format!("\"surface_rate\": {halfway_above}"),
                    ),
                    (
                        "\"default_trim\": 0.09",
                        "\"default_trim\": -0.0".to_owned(),
                    ),
                    (
                        "    0.6,\n    -0.25,\n    -0.9",
                        "    -0.0,\n    4.9406564584124654e-324,\n    -0.9".to_owned(),
                    ),
                ] {
                    let before = before.replace('\n', newline);
                    let after = after.replace('\n', newline);
                    assert_eq!(
                        json.matches(&before).count(),
                        1,
                        "missing fixture: {before:?}"
                    );
                    json = json.replacen(&before, &after, 1);
                    assert!(!json.contains(&before));
                    assert!(json.contains(&after));
                }
                let selected = SelectedAircraftProfile::from_bytes(json.as_bytes()).unwrap();
                assert_eq!(
                    selected.model_fit().target_length.get().to_bits(),
                    0x3ff0_0000_0000_0001
                );
                assert_eq!(
                    selected.pilot_controls(false).aileron,
                    AxisState::new(f64::from_bits(0x3ff0_0000_0000_0001), 1.8)
                );
                assert_eq!(
                    selected.pilot_controls(false).trim.value().to_bits(),
                    (-0.0_f64).to_bits()
                );
                let eye = selected.camera_eye();
                assert_eq!(eye[0].get().to_bits(), (-0.0_f64).to_bits());
                assert_eq!(eye[1].get().to_bits(), 1);
            }
        }
    }

    #[test]
    fn version_dispatch_preserves_each_serialized_size_boundary() {
        let mut legacy = LEGACY_JSON.as_bytes().to_vec();
        legacy.resize(usize::try_from(MAX_PROFILE_BYTES).unwrap(), b' ');
        assert!(
            SelectedAircraftProfile::from_bytes(&legacy)
                .unwrap()
                .legacy()
                .is_some()
        );
        legacy.push(b' ');
        assert!(
            SelectedAircraftProfile::from_bytes(&legacy)
                .unwrap_err()
                .contains("128 KiB")
        );

        let mut jet = JET_JSON.as_bytes().to_vec();
        jet.resize(flightsim_sim::aircraft_profile::MAX_PROFILE_BYTES, b' ');
        assert!(SelectedAircraftProfile::from_bytes(&jet).unwrap().is_jet());
        jet.push(b' ');
        assert!(
            SelectedAircraftProfile::from_bytes(&jet)
                .unwrap_err()
                .contains("1 MiB")
        );
    }

    #[test]
    fn dispatch_requires_explicit_integer_version_and_keeps_strict_jet_errors() {
        for version in ["2.0", "2e0", "\"2\"", "null", "3", "-1"] {
            let invalid =
                JET_JSON.replacen("\"version\": 2", &format!("\"version\": {version}"), 1);
            assert!(
                SelectedAircraftProfile::from_bytes(invalid.as_bytes()).is_err(),
                "{version}"
            );
        }
        for (old, new) in [
            ("\"version\": 2,", "\"version\": 2, \"version\": 2,"),
            ("\"version\": 2,", ""),
            ("\"revision\": 1", "\"revision\": 2"),
            ("\"kind\": \"dry_jet_table\"", "\"kind\": \"piston\""),
            ("\"length_m\": 8.3", "\"length_m\": 8.3, \"length_m\": 8.3"),
            ("\"length_m\": 8.3", "\"length_m\": 8.3, \"unknown\": 1"),
            ("\"length_m\": 8.3", "\"length_m\": 1e9999"),
            ("\"length_m\": 8.3", "\"length_m\": 1e-9999"),
        ] {
            let invalid = JET_JSON.replacen(old, new, 1);
            assert!(
                SelectedAircraftProfile::from_bytes(invalid.as_bytes()).is_err(),
                "{new}"
            );
        }
        assert!(
            SelectedAircraftProfile::from_bytes(format!("{JET_JSON} null").as_bytes()).is_err()
        );
        assert!(SelectedAircraftProfile::from_bytes(&[0xff]).is_err());
        let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
        let invalid = JET_JSON.replace("\"length_m\": 8.3", &format!("\"length_m\": {nested}"));
        assert!(
            SelectedAircraftProfile::from_bytes(invalid.as_bytes())
                .unwrap_err()
                .contains("nesting")
        );

        // Version is a root key, independent of order, escaped spelling, or a
        // similarly named key nested inside dynamics.
        let reordered = JET_JSON.replacen("\"version\": 2,", "", 1);
        let reordered = format!(
            "{}, \"\\u0076ersion\": 2 }}",
            reordered.trim_end().strip_suffix('}').unwrap()
        );
        assert!(
            SelectedAircraftProfile::from_bytes(reordered.as_bytes())
                .unwrap()
                .is_jet()
        );
    }

    #[test]
    fn bounded_input_defaults_optional_pitch_rates_and_approach_pose_are_applied() {
        for fixture in [JET_JSON, TURBOPROP_JSON] {
            use flightsim_core::{Degrees, NauticalMiles};
            for newline in ["\n", "\r\n"] {
                let mut json = fixture.replace("\r\n", "\n").replace('\n', newline);
                for optional_field in [
                    "    \"elevator_rate\": 0.25,\n",
                    "    \"elevator_centering_rate\": 5.0,\n",
                ] {
                    let optional_field = optional_field.replace('\n', newline);
                    assert_eq!(json.matches(&optional_field).count(), 1);
                    json = json.replacen(&optional_field, "", 1);
                    assert!(!json.contains(&optional_field));
                }
                let selected = SelectedAircraftProfile::from_bytes(json.as_bytes()).unwrap();
                let parked = selected.pilot_controls(false);
                assert_eq!(parked.elevator, AxisState::new(2.5, 1.8));
                assert_eq!(parked.throttle.value().to_bits(), 0.0_f64.to_bits());
                assert_eq!(parked.flaps.value().to_bits(), 0.0_f64.to_bits());
                assert_eq!(parked.trim.value().to_bits(), 0.09_f64.to_bits());
                let approach = selected.pilot_controls(true);
                assert_eq!(
                    approach.throttle.value().to_bits(),
                    0.39155148_f64.to_bits()
                );
                assert_eq!(approach.flaps.value().to_bits(), 1.0_f64.to_bits());
                assert_eq!(approach.trim.value().to_bits(), 0.15570889_f64.to_bits());
                let runway = flightsim_world::Runway::synthetic();
                let distance = NauticalMiles(1.5).to_meters();
                let glideslope = Degrees(3.0).to_radians();
                let state = selected.approach_state(&runway, distance, glideslope);
                let expected = flightsim_sim::approach_state(
                    &runway,
                    distance,
                    glideslope,
                    MetersPerSecond(35.0),
                );
                assert!((state.position.0 - expected.position.0).length() < 1e-6);
                assert!((state.velocity - expected.velocity).length() < 1e-9);
                assert!((state.attitude().pitch.get() - -0.07269918).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn bounded_turbine_sound_is_dry_at_every_accepted_throttle() {
        for fixture in [JET_JSON, TURBOPROP_JSON] {
            let selected = SelectedAircraftProfile::from_bytes(fixture.as_bytes()).unwrap();
            let flightsim_audio::EngineKind::Turbine(spec) = selected.engine_kind() else {
                panic!("fixture declares turbine sound");
            };
            for throttle in [0.0, 0.5, 0.9, 0.99, 1.0] {
                assert_eq!(
                    spec.afterburner_fraction(throttle).to_bits(),
                    0.0_f64.to_bits()
                );
                assert!(spec.exhaust_speed(throttle) <= spec.military_exhaust_speed);
            }
            let piston = fixture.replace(
                "\"engine_sound\": \"turbine\"",
                "\"engine_sound\": \"piston\"",
            );
            let selected = SelectedAircraftProfile::from_bytes(piston.as_bytes()).unwrap();
            assert!(selected.uses_bounded_model());
            assert!(matches!(
                selected.engine_kind(),
                flightsim_audio::EngineKind::Piston(_)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_definitions_select_distinct_dynamics_models_and_controls() {
        let light = AircraftProfile::builtin("light-single").unwrap();
        let sport = AircraftProfile::builtin("swift-sport").unwrap();
        assert_ne!(
            light.dynamics.mass_kg.to_bits(),
            sport.dynamics.mass_kg.to_bits()
        );
        assert_ne!(
            light.dynamics.max_shaft_power_w.to_bits(),
            sport.dynamics.max_shaft_power_w.to_bits()
        );
        assert_ne!(light.model.path, sport.model.path);
        assert_ne!(
            light.controls.surface_rate.to_bits(),
            sport.controls.surface_rate.to_bits()
        );
        assert_eq!(
            flightsim_sim::replay::aircraft_fingerprint(&light.configuration()),
            flightsim_sim::replay::aircraft_fingerprint(&AircraftConfig::light_single())
        );
    }
    #[test]
    fn both_bundled_aircraft_take_off_and_remain_numerically_stable() {
        use flightsim_core::{Degrees, Geodetic, MetersPerSecond, Radians, Seconds};
        use flightsim_sim::{
            DirectorTargets, FlightDirector, GroundSampler, Simulation, VerticalTarget,
        };
        use flightsim_world::{MemoryTileSource, Terrain};
        for id in ["light-single", "swift-sport"] {
            let profile = AircraftProfile::builtin(id).unwrap();
            let mut sim = Simulation::parked(
                profile.configuration(),
                Geodetic::from_degrees(35.55, 139.78, 0.0),
                Radians::ZERO,
                Terrain::new(MemoryTileSource::new(), 1024, 8..=12),
                GroundSampler::default(),
            );
            let director = FlightDirector::default();
            let mut liftoff = None;
            for step in 0..14_400 {
                let target_pitch = if sim.airspeed().get() < 30.0 {
                    0.0
                } else {
                    5.0
                };
                let controls = director.control(
                    sim.state(),
                    sim.agl(),
                    DirectorTargets {
                        vertical: VerticalTarget::Pitch(Degrees(target_pitch).to_radians()),
                        heading: Radians::ZERO,
                        airspeed: MetersPerSecond(45.0),
                        flaps: 0.0,
                        brakes: 0.0,
                        throttle_override: Some(1.0),
                        wings_level: true,
                    },
                );
                let report = sim.advance(Seconds(1.0 / 120.0), controls);
                assert!(
                    !report.diverged && !sim.crashed(),
                    "{id}: takeoff failed at {step}"
                );
                if sim.agl().get() > 3.0 && liftoff.is_none() {
                    liftoff = Some(f64::from(step) / 120.0);
                }
            }
            eprintln!(
                "{id}: liftoff={liftoff:?}s final AGL={:.1}m TAS={:.1}m/s",
                sim.agl().get(),
                sim.airspeed().get()
            );
            assert!(liftoff.is_some_and(|seconds| seconds < 60.0));
            assert!(sim.agl().get() > 100.0);
        }
    }

    #[test]
    fn bundled_approaches_hold_speed_and_three_degree_descent_without_a_controller() {
        use flightsim_core::{Degrees, NauticalMiles, Seconds};
        use flightsim_sim::{GroundSampler, Simulation};
        use flightsim_world::{MemoryTileSource, Runway, Terrain};
        for id in ["light-single", "swift-sport"] {
            let profile = AircraftProfile::builtin(id).unwrap();
            let state = profile.approach_state(
                &Runway::synthetic(),
                NauticalMiles(1.5).to_meters(),
                Degrees(3.0).to_radians(),
            );
            let initial_pitch = state.attitude().pitch.to_degrees().get();
            let mut sim = Simulation::from_state(
                profile.configuration(),
                state,
                Terrain::new(MemoryTileSource::new(), 1024, 8..=12),
                GroundSampler::default(),
            );
            // Exactly the startup controls, held unchanged. No flight director,
            // autopilot, feedback or stabilisation is involved in this test.
            let input = profile.pilot_controls(true).to_control_inputs();
            for step in 1..=3600 {
                let report = sim.advance(Seconds(1.0 / 120.0), input);
                let pitch = sim.state().attitude().pitch.to_degrees().get();
                let descent_angle = (-sim.state().vertical_speed().get())
                    .atan2(sim.state().ground_speed().get())
                    .to_degrees();
                assert!(!report.diverged && !sim.crashed(), "{id}, step {step}");
                // Scenario requirements, independent of the configured trim:
                // selected speed +/- 1 m/s and a 3 +/- 0.5 degree descent at
                // every step, including the transient immediately after spawn.
                assert!(
                    (sim.airspeed().get() - profile.controls.approach_speed_mps).abs() < 1.0,
                    "{id}, step {step}: TAS={}",
                    sim.airspeed().get()
                );
                assert!(
                    (descent_angle - 3.0).abs() < 0.5,
                    "{id}, step {step}: descent angle={descent_angle}"
                );
                assert!(
                    (pitch - initial_pitch).abs() < 1.0,
                    "{id}, step {step}: pitch={pitch}"
                );
                if step == 1200 || step == 3600 {
                    eprintln!(
                        "{id} approach{:.0}s: alt={:.3} TAS={:.3} pitch={:.3} VS={:.3} descent={:.3}",
                        f64::from(step) / 120.0,
                        sim.agl().get(),
                        sim.airspeed().get(),
                        pitch,
                        sim.state().vertical_speed().get(),
                        descent_angle,
                    );
                }
            }
        }
    }

    #[test]
    fn approach_defaults_balance_existing_fdm_forces_and_pitch_moment() {
        use bevy::math::DVec3;
        use flightsim_core::{Degrees, NauticalMiles};
        use flightsim_fdm::{Atmosphere, aero, gravity};
        for id in ["light-single", "swift-sport"] {
            let profile = AircraftProfile::builtin(id).unwrap();
            let state = profile.approach_state(
                &flightsim_world::Runway::synthetic(),
                NauticalMiles(1.5).to_meters(),
                Degrees(3.0).to_radians(),
            );
            let config = profile.configuration();
            let controls = profile.pilot_controls(true).to_control_inputs();
            let air = Atmosphere::standard().sample(state.altitude());
            let angles = aero::aero_angles(state.body_velocity());
            let (force, moment) = aero::body_force_and_moment(
                &config.aero,
                &config.geometry,
                angles,
                state.angular_velocity,
                controls,
                air.density,
            );
            let thrust = config.engine.thrust(
                controls.throttle(),
                angles.true_airspeed.get(),
                air.density_ratio(),
            );
            let acceleration = state.orientation
                * ((force + DVec3::X * thrust.get()) / config.mass_properties.mass().get())
                + gravity::acceleration_ecef(state.geodetic(), &state.local_frame());
            let angular_acceleration = config.mass_properties.inverse_inertia() * moment;
            assert!(acceleration.length() < 0.01, "{id}: {acceleration:?}");
            assert!(
                angular_acceleration.length() < 0.0001,
                "{id}: {angular_acceleration:?}"
            );
        }
    }

    #[test]
    fn approach_pose_preserves_runway_position_velocity_and_heading() {
        use flightsim_core::{Degrees, NauticalMiles};
        for id in ["light-single", "swift-sport"] {
            let profile = AircraftProfile::builtin(id).unwrap();
            for heading in [0.0, 50.0, 180.0, 270.0] {
                let mut runway = flightsim_world::Runway::synthetic();
                runway.heading = Degrees(heading).to_radians();
                let distance = NauticalMiles(1.5).to_meters();
                let glideslope = Degrees(3.0).to_radians();
                let base = flightsim_sim::approach_state(
                    &runway,
                    distance,
                    glideslope,
                    MetersPerSecond(profile.controls.approach_speed_mps),
                );
                let state = profile.approach_state(&runway, distance, glideslope);
                assert!((base.position.0 - state.position.0).length() < 1e-6);
                assert!((base.velocity - state.velocity).length() < 1e-9);
                assert!(
                    (base.attitude().yaw.get() - state.attitude().yaw.get()).cos() > 1.0 - 1e-12
                );
                assert!(state.attitude().roll.get().abs() < 1e-9);
                assert!(
                    (state.attitude().pitch.get() - profile.controls.approach_pitch_rad).abs()
                        < 1e-9
                );
            }
        }
    }

    #[test]
    fn parked_controls_keep_their_flaps_up_takeoff_trim() {
        for (id, trim) in [("light-single", 0.09), ("swift-sport", 0.08)] {
            let profile = AircraftProfile::builtin(id).unwrap();
            let parked = profile.pilot_controls(false).to_control_inputs();
            assert!((parked.elevator() - trim).abs() < 1e-12);
            assert!(parked.throttle().abs() < 1e-12);
            assert!(parked.flaps().abs() < 1e-12);
            assert!((profile.pilot_controls(true).trim.value() - trim).abs() > 0.01);
        }
    }

    #[test]
    fn approach_trim_and_pitch_reject_nonfinite_and_out_of_range_values() {
        let profile = AircraftProfile::builtin("light-single").unwrap();
        for trim in [-1.0, 1.0] {
            for pitch in [-core::f64::consts::FRAC_PI_4, core::f64::consts::FRAC_PI_4] {
                let mut boundary = profile.clone();
                boundary.controls.approach_trim = trim;
                boundary.controls.approach_pitch_rad = pitch;
                assert!(boundary.validate().is_ok());
            }
        }
        for trim in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.01, 1.01] {
            let mut invalid = profile.clone();
            invalid.controls.approach_trim = trim;
            assert!(invalid.validate().is_err());
        }
        for pitch in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.79, 0.79] {
            let mut invalid = profile.clone();
            invalid.controls.approach_pitch_rad = pitch;
            assert!(invalid.validate().is_err());
        }
        for (field, value) in [("approach_trim", 1.01), ("approach_pitch_rad", 0.79)] {
            let mut invalid = serde_json::to_value(&profile).unwrap();
            invalid["controls"][field] = serde_json::json!(value);
            assert!(AircraftProfile::parse(&invalid.to_string()).is_err());
            invalid["controls"].as_object_mut().unwrap().remove(field);
            assert!(AircraftProfile::parse(&invalid.to_string()).is_err());
        }
    }

    #[test]
    fn unknown_missing_and_unsafe_profiles_fail_instead_of_selecting_a_fallback() {
        assert!(AircraftProfile::load("jet").is_err());
        assert!(AircraftProfile::load("does-not-exist.json").is_err());
        let profile = AircraftProfile::builtin("swift-sport").unwrap();
        let mut json = serde_json::to_value(profile).unwrap();
        json["model"]["path"] = serde_json::json!("../other.glb");
        assert!(AircraftProfile::parse(&json.to_string()).is_err());
        json["model"]["path"] = serde_json::json!("aircraft/swift_sport.glb");
        json["version"] = serde_json::json!(999);
        assert!(AircraftProfile::parse(&json.to_string()).is_err());
    }
    #[test]
    fn optional_elevator_rate_preserves_old_custom_profiles_and_validates_bounds() {
        let profile = AircraftProfile::builtin("light-single").unwrap();
        let mut value = serde_json::to_value(&profile).unwrap();
        value["controls"]
            .as_object_mut()
            .unwrap()
            .remove("elevator_rate");
        value["controls"]
            .as_object_mut()
            .unwrap()
            .remove("elevator_centering_rate");
        let old = AircraftProfile::parse(&value.to_string()).unwrap();
        assert_eq!(old.controls.elevator_rate, None);
        assert_eq!(old.controls.elevator_centering_rate, None);
        let mut controls = old.pilot_controls(false);
        controls.update(
            flightsim_core::Seconds(0.1),
            flightsim_input::PilotKeys {
                pitch_up: true,
                ..Default::default()
            },
        );
        assert!((controls.elevator.value() - old.controls.surface_rate * 0.1).abs() < 1e-12);
        controls.elevator.set_absolute(1.0);
        controls.update(flightsim_core::Seconds(0.1), Default::default());
        assert!(
            (controls.elevator.value() - (1.0 - old.controls.centering_rate * 0.1)).abs() < 1e-12
        );
        for rate in [0.0, -0.1, 10.01, f64::NAN, f64::INFINITY] {
            let mut invalid = profile.clone();
            invalid.controls.elevator_rate = Some(rate);
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn optional_pitch_centering_is_validated_and_does_not_change_other_axes() {
        let mut profile = AircraftProfile::builtin("light-single").unwrap();
        profile.controls.elevator_centering_rate = Some(3.0);
        profile.validate().unwrap();
        let mut controls = profile.pilot_controls(false);
        controls.elevator.set_absolute(1.0);
        controls.aileron.set_absolute(1.0);
        controls.rudder.set_absolute(-1.0);
        controls.update(flightsim_core::Seconds(0.1), Default::default());
        assert!((controls.elevator.value() - 0.7).abs() < 1e-12);
        let ordinary = 1.0 - profile.controls.centering_rate * 0.1;
        assert!((controls.aileron.value() - ordinary).abs() < 1e-12);
        assert!((controls.rudder.value() + ordinary).abs() < 1e-12);
        for rate in [-0.01, 10.01, f64::NAN, f64::INFINITY] {
            profile.controls.elevator_centering_rate = Some(rate);
            assert!(profile.validate().is_err());
        }
        for rate in [0.0, 10.0] {
            profile.controls.elevator_centering_rate = Some(rate);
            assert!(profile.validate().is_ok());
        }
    }
}
