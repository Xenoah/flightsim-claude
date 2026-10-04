//! App acceptance is deliberately stricter than file-format validity. No current
//! configuration, display name or drift check recovers missing legacy evidence.
use flightsim_fdm::AircraftConfig;
use flightsim_sim::replay::{
    ReplayFile,
    identity::{AircraftCompatibility, AircraftIdentity, RecordedAircraftIdentity},
};

pub(crate) const LEGACY_NOTICE: &str =
    "LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified";
pub(crate) const MANUAL_CLOUD_NOTICE: &str = "F9 OFF: manual clouds";
pub(crate) const MANUAL_CLOUD_DIAGNOSTIC: &str = "Recording and F9 saving are disabled because manual cloud weather cannot yet be saved. Restart without --cloud-cover/--cloud-base/--cloud-top/--cloud-visibility to record a flight.";

/// Explicit opt-in permits only these independently pinned revision-2 baselines.
/// Matching the selected configuration does NOT prove the original recording
/// used that configuration: its missing yaw coefficient remains unknown.
fn supported_legacy_baseline(config: &AircraftConfig, fingerprint: u64) -> bool {
    let expected = match fingerprint {
        0x0505_e664_4bb2_9a53 => 0xb7fa_864d_c478_24f7,
        0x0606_8a31_d11a_75e0 => 0x1721_6717_4cf9_0012,
        _ => return false,
    };
    AircraftIdentity::for_config(config)
        == (AircraftIdentity {
            algorithm: 1,
            schema: 1,
            fdm_model_revision: 2,
            fingerprint: expected,
        })
}

fn identity_label(identity: RecordedAircraftIdentity) -> String {
    match identity {
        RecordedAircraftIdentity::LegacyPartial { fingerprint } => {
            format!("legacy partial fingerprint {fingerprint:016x}")
        }
        RecordedAircraftIdentity::Complete(identity) => format!(
            "complete algorithm {} schema {} FDM revision {} fingerprint {:016x}",
            identity.algorithm, identity.schema, identity.fdm_model_revision, identity.fingerprint,
        ),
    }
}

pub(crate) fn validate_playback(
    recording: &ReplayFile,
    config: &AircraftConfig,
    legacy_opt_in: bool,
    manual_clouds: bool,
) -> Result<Option<&'static str>, String> {
    if manual_clouds {
        return Err("manual cloud overrides cannot be applied to a replay; remove --cloud-cover/--cloud-base/--cloud-top/--cloud-visibility".into());
    }
    match recording
        .check_compatibility_with(config)
        .map_err(|e| e.to_string())?
    {
        AircraftCompatibility::CompleteMatch => Ok(None),
        AircraftCompatibility::Mismatch => Err(format!(
            "aircraft/FDM model mismatch: recorded {}; selected {}",
            identity_label(recording.aircraft_identity()),
            identity_label(RecordedAircraftIdentity::Complete(
                AircraftIdentity::for_config(config)
            )),
        )),
        AircraftCompatibility::LegacyPartialMatch => {
            if !legacy_opt_in {
                return Err(format!(
                    "{LEGACY_NOTICE}; use --legacy-replay-compatibility only to explicitly assume the selected supported revision-2 baseline"
                ));
            }
            let RecordedAircraftIdentity::LegacyPartial { fingerprint } =
                recording.aircraft_identity()
            else {
                return Err("legacy compatibility requires legacy partial evidence".into());
            };
            if !supported_legacy_baseline(config, fingerprint) {
                return Err("legacy compatibility supports only the frozen revision-2 Light Single / Swift Sport complete configurations; custom legacy dynamics are unsupported".into());
            }
            Ok(Some(LEGACY_NOTICE))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_sim::{
        CurrentConditions, CurrentRecorder, EnvironmentConditions, Recorder, replay::Conditions,
    };

    fn legacy(config: &AircraftConfig, version: u16) -> ReplayFile {
        let recording = Recorder::new(Conditions::default().with_aircraft(config)).finish();
        match version {
            1 => ReplayFile::V1(recording),
            2 => ReplayFile::V2(recording),
            _ => unreachable!(),
        }
    }
    fn current(config: &AircraftConfig) -> ReplayFile {
        ReplayFile::V3(
            CurrentRecorder::new(CurrentConditions::for_aircraft(
                config,
                EnvironmentConditions::default(),
            ))
            .finish(),
        )
    }

    #[test]
    fn current_identity_requires_every_physical_parameter_including_yaw_rate_p() {
        for config in [
            AircraftConfig::light_single(),
            crate::aircraft_profile::AircraftProfile::builtin("swift-sport")
                .unwrap()
                .configuration(),
        ] {
            let recording = current(&config);
            assert_eq!(
                validate_playback(&recording, &config, false, false),
                Ok(None)
            );
            let mut changed = config.clone();
            changed.aero.yaw_rate_p += 0.01;
            for opt_in in [false, true] {
                assert!(
                    validate_playback(&recording, &changed, opt_in, false)
                        .unwrap_err()
                        .contains("mismatch")
                );
            }
            let mut renamed = config.clone();
            renamed.name = "Untrusted display label".into();
            assert_eq!(
                validate_playback(&recording, &renamed, false, false),
                Ok(None)
            );
        }
    }

    #[test]
    fn legacy_requires_explicit_choice_and_selected_complete_baseline() {
        for config in [
            AircraftConfig::light_single(),
            crate::aircraft_profile::AircraftProfile::builtin("swift-sport")
                .unwrap()
                .configuration(),
        ] {
            for version in [1, 2] {
                let recording = legacy(&config, version);
                let error = validate_playback(&recording, &config, false, false).unwrap_err();
                assert!(
                    error.contains("--legacy-replay-compatibility") && error.contains("yaw_rate_p")
                );
                assert_eq!(
                    validate_playback(&recording, &config, true, false),
                    Ok(Some(LEGACY_NOTICE))
                );
                let mut changed = config.clone();
                changed.aero.yaw_rate_p += 0.01;
                assert!(
                    validate_playback(&recording, &changed, true, false)
                        .unwrap_err()
                        .contains("custom legacy dynamics")
                );
                // The old file cannot distinguish this actual historical change.
                // Even successful opt-in still says the recorded evidence is partial.
                let ambiguous = legacy(&changed, version);
                assert_eq!(ambiguous.aircraft_identity(), recording.aircraft_identity());
                assert_eq!(
                    validate_playback(&ambiguous, &config, true, false),
                    Ok(Some(LEGACY_NOTICE))
                );
            }
        }
    }

    #[test]
    fn custom_and_old_physics_do_not_gain_compatibility_from_the_flag_or_name() {
        let mut custom = AircraftConfig::light_single();
        custom.aero.pitch_alpha += 0.01;
        assert!(validate_playback(&legacy(&custom, 1), &custom, true, false).is_err());
        let mut conditions = Conditions::default().with_aircraft(&AircraftConfig::light_single());
        conditions.aircraft_fingerprint = 0; // No current baseline identity.
        let old = ReplayFile::V1(Recorder::new(conditions).finish());
        assert!(validate_playback(&old, &AircraftConfig::light_single(), true, false).is_err());
        let light = legacy(&AircraftConfig::light_single(), 1);
        assert!(
            validate_playback(
                &light,
                &crate::aircraft_profile::AircraftProfile::builtin("swift-sport")
                    .unwrap()
                    .configuration(),
                true,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn mismatch_diagnostics_preserve_hex_fingerprints_and_label_partial_evidence() {
        let light = legacy(&AircraftConfig::light_single(), 1);
        let swift = crate::aircraft_profile::AircraftProfile::builtin("swift-sport")
            .unwrap()
            .configuration();
        let message = validate_playback(&light, &swift, false, false).unwrap_err();
        assert!(message.contains("aircraft/FDM model mismatch"));
        assert!(message.contains("legacy partial fingerprint 0505e6644bb29a53"));
        assert!(
            message.contains(
                "complete algorithm 1 schema 1 FDM revision 2 fingerprint 172167174cf90012"
            )
        );
    }

    #[test]
    fn supported_modeled_weather_keeps_complete_identity_gate() {
        for mut bytes in [
            include_bytes!("../../flightsim-sim/tests/fixtures/v3_clear.fsreplay").as_slice(),
            include_bytes!("../../flightsim-sim/tests/fixtures/v3_rain.fsreplay").as_slice(),
            include_bytes!("../../flightsim-sim/tests/fixtures/v3_fog.fsreplay").as_slice(),
            include_bytes!("../../flightsim-sim/tests/fixtures/v3_custom_both.fsreplay").as_slice(),
        ] {
            let file = ReplayFile::read_from(&mut bytes).unwrap();
            assert_eq!(
                validate_playback(&file, &AircraftConfig::light_single(), false, false),
                Ok(None)
            );
            assert!(matches!(
                file.weather(),
                flightsim_sim::weather::WeatherSelection::Modeled(_)
            ));
        }
    }

    #[test]
    fn manual_cloud_overrides_are_rejected_for_every_playback_format() {
        let config = AircraftConfig::light_single();
        for file in [legacy(&config, 1), legacy(&config, 2), current(&config)] {
            assert!(
                validate_playback(&file, &config, true, true)
                    .unwrap_err()
                    .contains("manual cloud overrides")
            );
        }
    }
}
