//! File schema compatibility does not imply compatibility with older physics.

use flightsim_core::{Attitude, Geodetic, Ned, Seconds};
use flightsim_fdm::{AircraftConfig, ControlInputs, FDM_MODEL_REVISION, RigidBodyState};
use flightsim_sim::replay::{Conditions, Recorder, Recording, ReplayError, aircraft_fingerprint};
use flightsim_world::ClimateDate;

// Pre-revision alpha.21 Light Single identity, captured from the existing native
// baseline recordings. The config-only algorithm and unchanged default config
// produce this value; neither the display name nor the file version selects FDM.
const ALPHA21_LIGHT_SINGLE: u64 = 0xe8ca_8e4c_ac33_cd04;

#[test]
fn identity_binds_the_current_model_revision_even_with_unchanged_config() {
    let mut expected = ALPHA21_LIGHT_SINGLE;
    for byte in b"flightsim-fdm-model"
        .iter()
        .copied()
        .chain(FDM_MODEL_REVISION.to_le_bytes())
    {
        expected ^= u64::from(byte);
        expected = expected.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(
        aircraft_fingerprint(&AircraftConfig::light_single()),
        expected
    );
    assert_ne!(expected, ALPHA21_LIGHT_SINGLE);
}

#[test]
fn old_v1_and_v2_bytes_remain_readable_and_writable_but_reject_current_physics() {
    let config = AircraftConfig::light_single();
    for extended in [false, true] {
        let mut conditions = Conditions::default()
            .with_aircraft(&config)
            .with_world_climate(
                extended,
                extended.then(|| ClimateDate::from_month(7).unwrap()),
            );
        conditions.aircraft_fingerprint = ALPHA21_LIGHT_SINGLE;
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 1000.0),
            Attitude::from_degrees(0.0, 2.0, 0.0),
            Ned::new(50.0, 0.0, 0.0),
        );
        let mut recorder = Recorder::new(conditions);
        // Preserve historical render-frame granularity, including valid zero dt.
        recorder.record(Seconds::ZERO, ControlInputs::neutral(), Some(&state));
        recorder.record(Seconds(1.0 / 60.0), ControlInputs::neutral(), Some(&state));
        let mut bytes = Vec::new();
        recorder.finish().write_to(&mut bytes).unwrap();
        assert_eq!(
            u16::from_le_bytes([bytes[8], bytes[9]]),
            if extended { 2 } else { 1 }
        );
        let restored = Recording::read_from(&mut &bytes[..]).unwrap();
        assert_eq!(
            restored.conditions().aircraft_fingerprint,
            ALPHA21_LIGHT_SINGLE
        );
        assert_eq!(restored.frames()[0].frame_time, Seconds::ZERO);
        assert_eq!(restored.frames()[1].frame_time, Seconds(1.0 / 60.0));
        match restored.check_reproducible_with(&config) {
            Err(ReplayError::ConditionsMismatch { detail }) => {
                assert!(detail.contains("aircraft/FDM model mismatch"));
                assert!(detail.contains(&format!("FDM model revision {FDM_MODEL_REVISION}")));
            }
            other => panic!("old physics must be refused, got {other:?}"),
        }
        let mut rewritten = Vec::new();
        restored.write_to(&mut rewritten).unwrap();
        assert_eq!(
            rewritten, bytes,
            "reading/writing must never relabel old physics"
        );
    }
}
