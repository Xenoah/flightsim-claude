//! Warning derivation must precede the actual nonlinear CL maximum, not its
//! configured sigmoid midpoint. This read-only helper changes no force law.
use flightsim_core::{MetersPerSecond, Radians};
use flightsim_fdm::{AircraftConfig, ControlInputs, aero};
use glam::DVec3;

fn configs() -> [AircraftConfig; 2] {
    [
        include_str!("../../../assets/aircraft/light_single.json"),
        include_str!("../../../assets/aircraft/swift_sport.json"),
    ]
    .map(|json| {
        let p: serde_json::Value = serde_json::from_str(json).unwrap();
        serde_json::from_value::<flightsim_fdm::definition::AircraftDefinition>(
            p["dynamics"].clone(),
        )
        .unwrap()
        .to_config()
        .unwrap()
    })
}
fn lift(c: &AircraftConfig, alpha: f64, flaps: f64) -> f64 {
    aero::coefficients(
        &c.aero,
        &c.geometry,
        aero::AeroAngles {
            angle_of_attack: Radians(alpha),
            sideslip: Radians::ZERO,
            true_airspeed: MetersPerSecond(40.0),
        },
        DVec3::ZERO,
        ControlInputs::neutral().with_flaps(flaps),
    )
    .lift
}

#[test]
fn both_aircraft_flap_peaks_match_an_independent_dense_scan() {
    for c in configs() {
        let mut previous = f64::INFINITY;
        for flaps in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let peak = aero::positive_stall_peak_angle(&c.aero, &c.geometry, flaps)
                .unwrap()
                .get();
            let step = c.aero.stall_angle.get() / 20_000.0;
            let dense = (0..=20_000)
                .map(|i| f64::from(i) * step)
                .max_by(|a, b| lift(&c, *a, flaps).total_cmp(&lift(&c, *b, flaps)))
                .unwrap();
            assert!((peak - dense).abs() <= step);
            assert!((12.0..14.0).contains(&peak.to_degrees()));
            assert!(peak <= previous);
            previous = peak;
            assert!(lift(&c, peak - 0.001, flaps) < lift(&c, peak, flaps));
            assert!(lift(&c, peak + 0.001, flaps) < lift(&c, peak, flaps));
            let warning = 0.85 * peak;
            assert!(warning < dense - step);
            assert!(
                lift(&c, warning + 0.001, flaps) > lift(&c, warning, flaps),
                "warning must start on the rising side"
            );
            // Old .85 * blend midpoint was already beyond the actual peak.
            assert!(0.85 * c.aero.stall_angle.get() > peak);
        }
    }
}

#[test]
fn invalid_inputs_and_missing_attached_flow_peaks_return_none() {
    let c = AircraftConfig::light_single();
    for flaps in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(aero::positive_stall_peak_angle(&c.aero, &c.geometry, flaps).is_none());
    }
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let mut a = c.aero;
        a.stall_blend_rate = invalid;
        assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
        let mut a = c.aero;
        a.stall_angle = Radians(invalid);
        assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
        let mut a = c.aero;
        a.lift_alpha = invalid;
        assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
    }
    let mut a = c.aero;
    a.stall_angle = Radians(core::f64::consts::FRAC_PI_2);
    assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
    let mut a = c.aero;
    a.lift_zero = f64::NAN;
    assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
    let mut a = c.aero;
    a.lift_flaps = f64::INFINITY;
    assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
    // An extremely broad transition has no attached-flow local maximum in
    // the interval. Do not pick the later flat-plate maximum or nominal16°.
    let mut a = c.aero;
    a.stall_blend_rate = 0.1;
    assert!(aero::positive_stall_peak_angle(&a, &c.geometry, 0.0).is_none());
}
