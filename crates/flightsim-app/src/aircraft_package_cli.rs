//! Explicit offline validation/basic import only. No in-flight activation path.
use super::aircraft_profile::SelectedAircraftProfile;
use flightsim_content::{
    Error, Result,
    aircraft::{self, GeometrySummary, Manifest},
};
use serde::Deserialize;
use std::path::Path;

fn validate_profile(bytes: &[u8], manifest: &Manifest, geometry: &GeometrySummary) -> Result<()> {
    // Do not parse through serde_json::Value or reserialize: exact original number
    // tokens must reach the existing physical-family decoder once, unchanged.
    let profile = SelectedAircraftProfile::from_bytes(bytes).map_err(Error::Invalid)?;
    #[derive(Deserialize)]
    struct Version {
        version: u16,
    }
    let version: Version = serde_json::from_slice(bytes)?;
    if version.version != manifest.profile.version
        || manifest.model.path.strip_prefix("assets/") != Some(profile.model_path())
    {
        return Err(Error::Invalid(
            "aircraft package profile version/model path binding mismatch".into(),
        ));
    }
    let fit = profile.model_fit();
    #[allow(clippy::cast_possible_truncation)]
    let extents = bevy::math::Vec3::from_array(geometry.extents.map(|v| v.get() as f32));
    let length = (extents * fit.forward.to_vec3()).length();
    let scale = fit.scale_for(extents);
    if !length.is_finite() || length < 1e-6 || !scale.is_finite() || scale <= 0.0 {
        return Err(Error::Invalid(
            "aircraft package Scene0 cannot fit original profile axes/length".into(),
        ));
    }
    Ok(())
}

pub(super) fn run_cli(args: &[String]) -> Option<Result<()>> {
    let commands = [
        "--validate-aircraft-package",
        "--import-aircraft-package",
        "--inspect-aircraft-package",
    ];
    if !args
        .iter()
        .any(|a| commands.contains(&a.as_str()) || a == "--aircraft-store")
    {
        return None;
    }
    Some((|| {
        if cfg!(feature = "commercial-staging") {
            return Err(Error::Invalid("aircraft package commands require an ordinary development build; commercial-staging has a fixed adjacent asset inventory".into()));
        }
        if args.len() < 2 || !commands.contains(&args[0].as_str()) || args[1].starts_with("--") {
            return Err(Error::Invalid("use --validate-aircraft-package ZIP, --inspect-aircraft-package DIR, or --import-aircraft-package ZIP --aircraft-store DIR".into()));
        }
        match args[0].as_str() {
            "--validate-aircraft-package" if args.len() == 2 => {
                let id = aircraft::validate_zip(Path::new(&args[1]), validate_profile)?;
                println!(
                    "aircraft package and original profile validated: {}@{} manifest-sha256={}",
                    id.id, id.version, id.manifest_sha256
                );
            }
            "--inspect-aircraft-package" if args.len() == 2 => {
                let package = aircraft::inspect_installed(Path::new(&args[1]))?;
                validate_profile(
                    package.profile_bytes(),
                    package.manifest(),
                    package.geometry(),
                )?;
                println!(
                    "installed aircraft package and original profile validated: {}@{} manifest-sha256={}",
                    package.identity().id,
                    package.identity().version,
                    package.identity().manifest_sha256
                );
            }
            "--import-aircraft-package"
                if args.len() == 4
                    && args[2] == "--aircraft-store"
                    && !args[3].starts_with("--") =>
            {
                let stage = aircraft::stage_zip_with_progress(
                    Path::new(&args[1]),
                    Path::new(&args[3]),
                    |_| true,
                )?;
                let installed = stage.validate_profile(validate_profile)?.commit()?;
                println!(
                    "aircraft package installed (not activated): {}",
                    installed.directory().display()
                );
                println!("manifest-sha256={}", installed.identity().manifest_sha256);
            }
            _ => {
                return Err(Error::Invalid(
                    "package commands are exclusive; import requires exactly --aircraft-store DIR"
                        .into(),
                ));
            }
        }
        println!(
            "Static untextured Scene0 subset only. Runtime loading, flight qualification, replay support and distribution rights remain separate checks."
        );
        Ok(())
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn package_cli_is_explicit_and_exclusive() {
        assert!(run_cli(&args(&["--aircraft", "swift-sport"])).is_none());
        for values in [
            vec!["--aircraft-store", "x"],
            vec!["--validate-aircraft-package"],
            vec!["--validate-aircraft-package", "x", "--no-model"],
            vec!["--import-aircraft-package", "x"],
        ] {
            assert!(run_cli(&args(&values)).unwrap().is_err());
        }
    }
    #[test]
    fn package_profile_uses_original_loader_and_binds_model_and_version() {
        let manifest = aircraft::parse_manifest(include_bytes!(
            "../../../docs/examples/aircraft-packages/swift/manifest.json"
        ))
        .unwrap();
        let profile = include_bytes!("../../../assets/aircraft/swift_sport.json");
        let geometry = GeometrySummary {
            extents: [
                flightsim_core::Meters(7.),
                flightsim_core::Meters(3.),
                flightsim_core::Meters(10.),
            ],
            nodes: 31,
            primitives: 31,
            vertices: 100,
            indices: 300,
        };
        validate_profile(profile, &manifest, &geometry).unwrap();
        let mut wrong = manifest.clone();
        wrong.profile.version = 2;
        assert!(validate_profile(profile, &wrong, &geometry).is_err());
        let mut wrong = manifest.clone();
        wrong.model.path = "assets/other.glb".into();
        assert!(validate_profile(profile, &wrong, &geometry).is_err());
        let broken = String::from_utf8(profile.to_vec())
            .unwrap()
            .replace("750.0", "-1.0");
        assert!(validate_profile(broken.as_bytes(), &manifest, &geometry).is_err());
        let mut flat = geometry;
        flat.extents[0] = flightsim_core::Meters(0.);
        assert!(validate_profile(profile, &manifest, &flat).is_err());
    }
    #[test]
    fn commercial_package_commands_reject_before_file_access() {
        if !cfg!(feature = "commercial-staging") {
            return;
        }
        for values in [
            vec!["--validate-aircraft-package", "/does-not-exist.zip"],
            vec!["--inspect-aircraft-package", "/does-not-exist"],
            vec![
                "--import-aircraft-package",
                "/does-not-exist.zip",
                "--aircraft-store",
                "/does-not-exist",
            ],
        ] {
            let error = run_cli(&args(&values)).unwrap().unwrap_err();
            assert!(error.to_string().contains("commercial-staging"));
            assert!(matches!(error, Error::Invalid(_)));
        }
    }
    #[test]
    fn package_semantics_dispatch_all_original_profile_families() {
        let manifest = aircraft::parse_manifest(include_bytes!(
            "../../../docs/examples/aircraft-packages/swift/manifest.json"
        ))
        .unwrap();
        let geometry = GeometrySummary {
            extents: [flightsim_core::Meters(10.); 3],
            nodes: 1,
            primitives: 1,
            vertices: 3,
            indices: 3,
        };
        let fixtures: [&[u8]; 4] = [
            include_bytes!("../../../assets/aircraft/swift_sport.json"),
            include_bytes!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"),
            include_bytes!("../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"),
            include_bytes!(
                "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
            ),
        ];
        for (i, bytes) in fixtures.into_iter().enumerate() {
            let parsed = SelectedAircraftProfile::from_bytes(bytes).unwrap();
            let mut matched = manifest.clone();
            matched.profile.version = u16::try_from(i + 1).unwrap();
            matched.model.path = format!("assets/{}", parsed.model_path());
            validate_profile(bytes, &matched, &geometry).unwrap();
            let text = std::str::from_utf8(bytes).unwrap();
            let duplicate = text.replacen("{", &format!("{{\"version\":{},", i + 1), 1);
            assert!(validate_profile(duplicate.as_bytes(), &matched, &geometry).is_err());
            // Version tags must retain integer-token spelling, never Value/coercion.
            let token = format!("\"version\": {}", i + 1);
            let fractional = text.replacen(&token, &format!("\"version\": {}.0", i + 1), 1);
            assert_ne!(
                fractional, text,
                "fixture version spelling unexpectedly changed"
            );
            assert!(validate_profile(fractional.as_bytes(), &matched, &geometry).is_err());
        }
    }
}
