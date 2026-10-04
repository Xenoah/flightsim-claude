use super::*;
use exact::BoundedVec;

#[test]
fn original_tokens_match_independent_binary64_patterns() {
    for (token, bits) in [
        ("-30.400000000000002", 0xc03e_6666_6666_6667),
        ("0.1", 0x3fb9_9999_9999_999a),
        ("1e0", 0x3ff0_0000_0000_0000),
        ("-0", 0x8000_0000_0000_0000),
        ("-0.000e100", 0x8000_0000_0000_0000),
        ("0e-99999", 0),
        ("5e-324", 1),
        ("-5e-324", 0x8000_0000_0000_0001),
        ("2.225073858507201e-308", 0x000f_ffff_ffff_ffff),
        ("2.2250738585072014e-308", 0x0010_0000_0000_0000),
        ("1.7976931348623157e308", 0x7fef_ffff_ffff_ffff),
        ("2.4703282292062328e-324", 1),
        (
            "1.00000000000000011102230246251565404236316680908203125",
            0x3ff0_0000_0000_0000,
        ),
        (
            "1.00000000000000011102230246251565404236316680908203126",
            0x3ff0_0000_0000_0001,
        ),
        ("9007199254740993", 0x4340_0000_0000_0000),
    ] {
        let value: ExactF64 = serde_json::from_str(token).unwrap();
        assert_eq!(value.get().to_bits(), bits, "{token}");
        let exported = serde_json::to_string(&value).unwrap();
        let again: ExactF64 = serde_json::from_str(&exported).unwrap();
        assert_eq!(again.get().to_bits(), bits, "export {token}");
    }
}

#[test]
fn malformed_nonfinite_underflow_and_long_numbers_reject() {
    for token in [
        "1e999",
        "-1e999",
        "1e-999",
        "-1e-999",
        "2.4703282292062327e-324",
        "NaN",
        "Infinity",
        "null",
        "true",
        "\"1\"",
        "[]",
        "{}",
        "+1",
        "01",
        "1.",
        ".1",
        "1e",
        "1 2",
    ] {
        assert!(serde_json::from_str::<ExactF64>(token).is_err(), "{token}");
    }
    assert!(ExactF64::new(f64::NAN).is_err());
    assert!(ExactF64::new(f64::INFINITY).is_err());
    assert_eq!(
        ExactF64::new(-0.0).unwrap().get().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(serde_json::from_str::<ExactF64>(&format!("0.{}", "0".repeat(126))).is_ok());
    assert!(serde_json::from_str::<ExactF64>(&format!("0.{}", "0".repeat(127))).is_err());
}

#[test]
fn sequence_limits_reject_before_parsing_an_excess_value() {
    for limit in [3, 32, 4096] {
        // The first excess token is deliberately incomplete. The resource-limit
        // error must win over its syntax error; no excess DTO is decoded.
        let json = format!("[{},{{\"unparsed", vec!["0"; limit].join(","));
        let error = match limit {
            3 => serde_json::from_str::<BoundedVec<ExactF64, 3>>(&json).unwrap_err(),
            32 => serde_json::from_str::<BoundedVec<ExactF64, 32>>(&json).unwrap_err(),
            _ => serde_json::from_str::<BoundedVec<ExactF64, 4096>>(&json).unwrap_err(),
        };
        assert!(error.to_string().contains("array exceeds"), "{error}");
    }
}

#[test]
fn default_legacy_decoder_is_unchanged_under_raw_value_feature_unification() {
    let legacy: f64 = serde_json::from_str("-30.400000000000002").unwrap();
    let exact: ExactF64 = serde_json::from_str("-30.400000000000002").unwrap();
    assert_eq!(legacy.to_bits(), 0xc03e_6666_6666_6666);
    assert_eq!(exact.get().to_bits(), 0xc03e_6666_6666_6667);
}

#[test]
fn preflight_depth_guard_handles_quotes_escapes_and_raw_value_subtrees() {
    assert!(exact::check_depth(&format!("{}0{}", "[".repeat(128), "]".repeat(128))).is_ok());
    assert!(exact::check_depth(&format!("{}0{}", "[".repeat(129), "]".repeat(129))).is_err());
    let string =
        serde_json::to_string(&format!("{}\\\"{}", "[".repeat(1024), "}".repeat(1024))).unwrap();
    assert!(exact::check_depth(&format!("{{\"text\":{string}}}")).is_ok());
}
