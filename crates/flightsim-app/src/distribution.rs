//! Explicit, opt-in local distribution policy. Never a legal-clearance claim.
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "commercial-staging")]
pub(super) const DEFAULT_AIRCRAFT: &str = "swift-sport";
#[cfg(not(feature = "commercial-staging"))]
pub(super) const DEFAULT_AIRCRAFT: &str = "light-single";

/// Machine-readable feature identity, before graphics, audio or asset loading.
pub(super) fn info() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "package": env!("CARGO_PKG_NAME"),
        "package_version": env!("CARGO_PKG_VERSION"),
        "profile": if cfg!(feature = "commercial-staging") {
            "commercial-staging"
        } else {
            "development"
        },
        "region_downloads": cfg!(feature = "region-downloads"),
        "default_aircraft": DEFAULT_AIRCRAFT,
        "default_model": super::BUNDLED_MODEL,
        "bundled_aircraft": if cfg!(feature = "commercial-staging") {
            vec!["swift-sport"]
        } else {
            vec!["light-single", "swift-sport"]
        },
        "target_os": std::env::consts::OS,
        "target_arch": std::env::consts::ARCH,
        "target_env": if cfg!(target_env = "msvc") {
            "msvc"
        } else if cfg!(target_env = "gnu") {
            "gnu"
        } else {
            "unsupported"
        },
        "release_authorized": false,
    })
}

pub(super) fn aircraft_listing() -> &'static str {
    if cfg!(feature = "commercial-staging") {
        "swift-sport   Swift Sport (generic), 750 kg, 134 kW, low wing (default)\n\
         Light Single's model is excluded from the commercial candidate.\n\
         Legacy profiles and replay fingerprints are unchanged; selecting an absent model fails.\n\
         Select with --aircraft ID, or pass a versioned JSON profile path."
    } else {
        "light-single  Light Single (generic), 1043 kg, 119 kW, high wing\n\
         swift-sport   Swift Sport (generic), 750 kg, 134 kW, low wing\n\
         Select with --aircraft ID, or pass a versioned JSON profile path."
    }
}

pub(super) fn adjacent_assets(executable: &Path) -> Option<PathBuf> {
    let candidate = executable.parent()?.join("assets");
    candidate.is_dir().then(|| candidate.canonicalize().ok())?
}

/// Preserve developer fallback, but never hide a broken commercial candidate.
/// `--no-model` remains an explicit choice, including for old replay inspection.
pub(super) fn validate_model(
    model: Option<&str>,
    assets: Option<&Path>,
    explicitly_selected: bool,
) -> Result<(), String> {
    if cfg!(feature = "commercial-staging")
        && let Some(model) = model
    {
        let path = Path::new(model);
        if model.is_empty()
            || model.contains(['\\', ':'])
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(
                "commercial candidate models must be relative paths inside adjacent assets/"
                    .to_owned(),
            );
        }
        if let Some(root) = assets
            && let (Ok(root), Ok(model)) = (root.canonicalize(), root.join(path).canonicalize())
            && !model.starts_with(root)
        {
            return Err("commercial candidate model escapes adjacent assets/".to_owned());
        }
    }
    if let Some(model) = model
        && (cfg!(feature = "commercial-staging") || explicitly_selected)
        && !assets.is_some_and(|root| root.join(model).is_file())
    {
        return Err(format!("selected aircraft model is missing: {model}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft_profile::AircraftProfile;

    #[test]
    fn machine_identity_matches_the_actual_default_profile() {
        let (startup, diagnostics) = crate::parse_arguments_from(Vec::new());
        assert!(diagnostics.0.is_empty());
        let metadata = info();
        assert_eq!(metadata["schema_version"], 1);
        assert_eq!(
            metadata["region_downloads"].as_bool(),
            Some(cfg!(feature = "region-downloads"))
        );
        assert_eq!(metadata["default_aircraft"], startup.aircraft.id());
        assert_eq!(metadata["default_model"], startup.model.unwrap());
        assert_eq!(metadata["release_authorized"], false);
        assert_eq!(startup.model_fit, startup.aircraft.model_fit());
        let expected = if cfg!(feature = "commercial-staging") {
            "swift-sport"
        } else {
            "light-single"
        };
        assert_eq!(startup.aircraft.id(), expected);
        assert_eq!(info().to_string(), info().to_string());
    }

    #[test]
    fn missing_selected_models_fail_and_explicit_placeholder_still_works() {
        for id in ["light-single", "swift-sport"] {
            let profile = AircraftProfile::builtin(id).unwrap();
            let error = validate_model(Some(&profile.model.path), None, true).unwrap_err();
            assert!(error.contains(&profile.model.path));
        }
        assert!(validate_model(None, None, true).is_ok());
        assert_eq!(
            validate_model(Some("absent.glb"), None, false).is_err(),
            cfg!(feature = "commercial-staging")
        );
    }

    #[test]
    fn commercial_asset_search_does_not_walk_up_to_developer_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(adjacent_assets(&root.join("flightsim-app")).is_some());
        assert!(adjacent_assets(&root.join("crates/flightsim-app/flightsim-app")).is_none());
    }

    #[test]
    fn commercial_model_overrides_cannot_escape_the_adjacent_asset_root() {
        if cfg!(feature = "commercial-staging") {
            for model in [
                "/tmp/model.glb",
                "../model.glb",
                "C:/model.glb",
                "aircraft\\model.glb",
            ] {
                let error = validate_model(Some(model), None, true).unwrap_err();
                assert!(error.contains("relative paths"), "{model}: {error}");
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        assert!(validate_model(Some("aircraft/swift_sport.glb"), Some(&root), true).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn commercial_model_symlink_cannot_escape_adjacent_assets() {
        let directory = std::env::temp_dir().join(format!(
            "flightsim-distribution-symlink-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let assets = directory.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let outside = directory.join("outside.glb");
        std::fs::write(&outside, b"test fixture").unwrap();
        std::os::unix::fs::symlink(&outside, assets.join("escaped.glb")).unwrap();
        let result = validate_model(Some("escaped.glb"), Some(&assets), true);
        assert_eq!(result.is_err(), cfg!(feature = "commercial-staging"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn explicit_legacy_aircraft_retains_fingerprint_and_is_not_remapped() {
        let (startup, _) = crate::parse_arguments_from([
            "--aircraft".to_owned(),
            "light-single".to_owned(),
            "--no-model".to_owned(),
        ]);
        assert_eq!(startup.aircraft.id(), "light-single");
        assert!(startup.model.is_none());
        let light = flightsim_fdm::AircraftConfig::light_single();
        assert_eq!(
            flightsim_sim::replay::aircraft_fingerprint(&startup.aircraft.configuration()),
            flightsim_sim::replay::aircraft_fingerprint(&light)
        );
        let recording = flightsim_sim::Recorder::new(
            flightsim_sim::replay::Conditions::default().with_aircraft(&light),
        )
        .finish();
        assert!(
            recording
                .check_reproducible_with(&startup.aircraft.configuration())
                .is_ok()
        );
        let sport = AircraftProfile::builtin("swift-sport").unwrap();
        assert!(
            recording
                .check_reproducible_with(&sport.configuration())
                .is_err()
        );
    }
}
