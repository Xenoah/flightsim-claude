//! Additive physical qualification, not profile/replay/app adoption.
#[path = "../examples/support/cedar_near_static.rs"]
mod cedar;
use serde_json::Value;

fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap()
}

#[test]
fn unchanged_forward_config_and_original_law1_failure_are_pinned() {
    let c = cedar::configuration();
    let witness = cedar::boundary_witness(&c);
    assert!(witness["law1_rollback_all_16_words"].as_bool().unwrap());
}

#[test]
fn calm_braking_near_static_interval_release_and_refinement() {
    let report = cedar::critical();
    let runs = report["critical_runs"].as_array().unwrap();
    assert_eq!(runs.len(), 4, "{report}");
    for run in runs {
        assert!(run["failure"].is_null(), "{}", run["failure"]);
        assert!((number(run, "completed_s") - f64::from(cedar::DURATION_S)).abs() < 1e-12);
        assert!(number(run, "near_static_braked_seconds_below_0_1mps") > 10.0);
        assert!(number(run, "minimum_disk_clearance_m") > 0.10);
        assert!(number(run, "maximum_gear_compression_m") < 0.18);
        assert!(number(run, "maximum_substeps") <= 8.0);
        assert!(number(run, "positive_flow_bit_exact_step_count") > 100.0);
        assert!(
            !run["negative_stage_evaluations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            run["stage_observer_all_endpoints_bit_exact"]
                .as_bool()
                .unwrap()
        );
        let creep = number(run, "last_braked_10s_peak_speed_mps");
        assert!(
            creep > 0.001 && creep < 0.05,
            "report braked idle creep honestly: {creep}"
        );
        assert!(!run["stationary_parking_pass"].as_bool().unwrap());
        assert!(number(&run["final"], "speed_mps") > 1.0);
    }
    // Fixed source tolerances, evaluated at the shared 20 Hz history.
    // These are numerical agreement limits, not aircraft performance tolerances.
    let finest = runs[3]["samples_20hz"].as_array().unwrap();
    for run in &runs[..3] {
        let samples = run["samples_20hz"].as_array().unwrap();
        assert_eq!(samples.len(), finest.len());
        for (coarse, fine) in samples.iter().zip(finest) {
            for (key, tolerance) in [
                ("speed_mps", 0.01),
                ("pitch_rad", 0.001),
                ("shaft_rad_s", 0.10),
                ("blade_pitch_rad", 0.001),
                ("altitude_m", 0.01),
            ] {
                assert!(
                    (number(&coarse["state"], key) - number(&fine["state"], key)).abs() < tolerance,
                    "{} Hz {key} at {}",
                    run["hz"],
                    coarse["time_s"]
                );
            }
            assert!(
                (number(coarse, "disk_clearance_m") - number(fine, "disk_clearance_m")).abs()
                    < 0.002
            );
            for key in ["gear_clearances_m", "contact_force_body_n"] {
                let tolerance = if key == "gear_clearances_m" {
                    0.002
                } else {
                    100.0
                };
                for axis in 0..3 {
                    assert!(
                        (coarse[key][axis].as_f64().unwrap() - fine[key][axis].as_f64().unwrap())
                            .abs()
                            < tolerance,
                        "{} Hz {key} axis {axis} at {}",
                        run["hz"],
                        coarse["time_s"]
                    );
                }
            }
        }
    }
}

#[test]
fn original_flight_and_contact_matrix_preserves_positive_flow_bits() {
    let matrix = cedar::matrix();
    let cases = matrix["cases"].as_array().unwrap();
    let expected_cases = [
        ("trim_38_0_0.5", 7200),
        ("roll_rudder_release_38_0_0.5", 1200),
        ("trim_40_0_0", 7200),
        ("roll_rudder_release_40_0_0", 1200),
        ("trim_50_0_0", 7200),
        ("roll_rudder_release_50_0_0", 1200),
        ("trim_60_0_0", 7200),
        ("roll_rudder_release_60_0_0", 1200),
        ("trim_45_0.06_0.5", 7200),
        ("roll_rudder_release_45_0.06_0.5", 1200),
        ("ground_idle", 2400),
        ("ground_head_crosswind", 2400),
        ("sloped_ground", 2400),
        ("takeoff", 4800),
        ("ground_power_cycle", 8400),
        ("braking_headwind", 1200),
        ("touchdown_1", 960),
        ("touchdown_2", 960),
        ("touchdown_3", 960),
        ("flight_power_cycle", 7200),
        ("stall_recovery", 2400),
    ];
    assert_eq!(cases.len(), expected_cases.len());
    let mut total_parity_steps = 0;
    for (case, (name, expected_steps)) in cases.iter().zip(expected_cases) {
        assert_eq!(case["case"].as_str().unwrap(), name);
        let r = &case["result"];
        assert_eq!(r["hz"].as_u64().unwrap(), 120);
        assert!(r["failure"].is_null(), "{}: {}", case["case"], r["failure"]);
        assert!((number(r, "completed_s") - number(r, "requested_duration_s")).abs() < 1e-12);
        assert!(
            number(r, "minimum_disk_clearance_m") > 0.10,
            "{}",
            case["case"]
        );
        assert!(
            number(r, "maximum_gear_compression_m") < 0.18,
            "{}",
            case["case"]
        );
        let parity_steps = r["positive_flow_bit_exact_step_count"].as_u64().unwrap();
        assert_eq!(
            parity_steps, expected_steps,
            "{name}: every step must match law 1"
        );
        assert!(
            r["negative_stage_evaluations"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{name}: the original supported matrix must remain entirely forward-flow"
        );
        total_parity_steps += parity_steps;
    }
    assert_eq!(total_parity_steps, 76_080);
    let takeoff = cases.iter().find(|c| c["case"] == "takeoff").unwrap();
    assert!(number(&takeoff["result"]["final"], "altitude_m") > 50.0);
    let recovery = cases
        .iter()
        .find(|c| c["case"] == "stall_recovery")
        .unwrap();
    assert!(number(&recovery["result"]["final"], "vertical_speed_mps") > 0.0);
}
