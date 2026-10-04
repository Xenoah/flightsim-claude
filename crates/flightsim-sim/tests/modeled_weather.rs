//! Contract tests, not a claim of meteorological accuracy or rendered effects.

use core::f64::consts::{FRAC_PI_2, PI};
use flightsim_core::{Geodetic, Meters, Radians};
use flightsim_sim::weather::{
    CloudMorphology, MAX_PRECIPITATION_RATE, ModeledCloudLayer, ModeledFogLayer, PrecipitationKind,
    WEATHER_MODEL_REVISION, WEATHER_PARAMETER_SCHEMA, WaterEquivalentRate, WeatherError,
    WeatherParameters, WeatherPreset, WeatherScenario, WeatherSelection, WeatherSource,
};

const PRESETS: [WeatherPreset; 6] = [
    WeatherPreset::Clear,
    WeatherPreset::Cloud,
    WeatherPreset::Fog,
    WeatherPreset::Rain,
    WeatherPreset::Snow,
    WeatherPreset::Storm,
];

fn custom() -> WeatherParameters {
    WeatherParameters {
        parameter_schema: 1,
        source: WeatherSource::AuthoredModel,
        model_revision: 1,
        preset: WeatherPreset::Custom,
        seed: 42,
        departure_reference: Geodetic::from_degrees(35.0, 139.0, 0.0),
        ambient_visibility: Meters(50_000.0),
        precipitation_kind: PrecipitationKind::Rain,
        precipitation_rate: WaterEquivalentRate(0.005 / 3600.0),
        cloud: Some(ModeledCloudLayer {
            morphology: CloudMorphology::Layered,
            base: Meters(1000.0),
            top: Meters(2000.0),
            coverage: 0.5,
            visibility: Meters(500.0),
        }),
        fog: Some(ModeledFogLayer {
            bottom: Meters(0.0),
            top: Meters(1000.0),
            visibility: Meters(250.0),
        }),
    }
}

#[test]
fn default_is_legacy_and_clear_requires_an_explicit_departure_and_seed() {
    assert_eq!(WeatherSelection::default(), WeatherSelection::Legacy);
    assert_ne!(
        WeatherSelection::default(),
        WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                WeatherPreset::Clear,
                Geodetic::from_degrees(0.0, 0.0, 0.0),
                0,
            )
            .unwrap(),
        ),
    );
    assert_eq!(
        WeatherScenario::from_preset(WeatherPreset::Custom, custom().departure_reference, 42),
        Err(WeatherError::CustomRequiresParameters),
    );
}

#[test]
fn authored_presets_match_the_reviewable_contract() {
    // Independent literal fixtures from the authored design table. These values
    // are design choices, not an external observation or climatology reference.
    let reference = Geodetic::from_degrees(31.5, 35.5, -430.0);
    let fixtures = [
        (100_000.0, PrecipitationKind::None, 0.0, None, None),
        (
            40_000.0,
            PrecipitationKind::None,
            0.0,
            Some((CloudMorphology::Puffy, 1070.0, 2270.0, 0.65, 500.0)),
            None,
        ),
        (
            50_000.0,
            PrecipitationKind::None,
            0.0,
            None,
            Some((-430.0, -130.0, 250.0)),
        ),
        (
            10_000.0,
            PrecipitationKind::Rain,
            5.0,
            Some((CloudMorphology::Layered, 170.0, 2170.0, 1.0, 250.0)),
            None,
        ),
        (
            3000.0,
            PrecipitationKind::Snow,
            1.0,
            Some((CloudMorphology::Layered, -130.0, 1370.0, 1.0, 200.0)),
            None,
        ),
        (
            5000.0,
            PrecipitationKind::Rain,
            25.0,
            Some((CloudMorphology::Towering, 70.0, 7570.0, 1.0, 150.0)),
            None,
        ),
    ];
    for (preset, (ambient, kind, mm_per_hour, cloud, fog)) in PRESETS.into_iter().zip(fixtures) {
        let actual = WeatherScenario::from_preset(preset, reference, 0x1234)
            .unwrap()
            .parameters();
        assert_eq!(actual.parameter_schema, 1);
        assert_eq!(actual.model_revision, 1);
        assert_eq!(actual.source, WeatherSource::AuthoredModel);
        assert_eq!(actual.preset, preset);
        assert_eq!(actual.seed, 0x1234);
        assert_eq!(actual.departure_reference, reference);
        assert_eq!(actual.ambient_visibility, Meters(ambient));
        assert_eq!(actual.precipitation_kind, kind);
        // 1 mm = 0.001 m and 1 hour = 3600 s; do not interpret snow as its depth.
        assert!((actual.precipitation_rate.0 * 3_600_000.0 - mm_per_hour).abs() < 1e-12);
        assert_eq!(
            actual.cloud,
            cloud.map(
                |(morphology, base, top, coverage, visibility)| ModeledCloudLayer {
                    morphology,
                    base: Meters(base),
                    top: Meters(top),
                    coverage,
                    visibility: Meters(visibility),
                }
            )
        );
        assert_eq!(
            actual.fog,
            fog.map(|(bottom, top, visibility)| ModeledFogLayer {
                bottom: Meters(bottom),
                top: Meters(top),
                visibility: Meters(visibility),
            })
        );
    }
}

#[test]
fn all_presets_resolve_at_departure_boundaries_poles_and_dateline() {
    for preset in PRESETS {
        for latitude in [-FRAC_PI_2, 0.0, FRAC_PI_2] {
            for longitude in [-PI, 0.0, PI] {
                for altitude in [-1000.0, 0.0, 10_000.0] {
                    for seed in [0, 1, u64::MAX] {
                        let departure =
                            Geodetic::new(Radians(latitude), Radians(longitude), Meters(altitude));
                        let a = WeatherScenario::from_preset(preset, departure, seed).unwrap();
                        let b = WeatherScenario::from_preset(preset, departure, seed).unwrap();
                        assert_eq!(a, b);
                        assert_eq!(a.parameters().departure_reference, departure);
                        assert_eq!(a.parameters().seed, seed);
                    }
                }
            }
        }
    }
}

#[test]
fn departure_reference_and_seed_are_fixed_values_not_mutable_state() {
    let original = WeatherScenario::from_preset(
        WeatherPreset::Cloud,
        Geodetic::from_degrees(0.0, 0.0, 1000.0),
        42,
    )
    .unwrap();
    let mut edited = original.parameters();
    edited.seed = 43;
    edited.departure_reference.altitude = Meters(2000.0);
    assert_eq!(original.parameters().seed, 42);
    assert_eq!(
        original.parameters().departure_reference.altitude,
        Meters(1000.0)
    );
    // A changed reference does not silently recompute already resolved heights.
    assert_eq!(
        WeatherScenario::try_from(edited),
        Err(WeatherError::PresetMismatch(WeatherPreset::Cloud))
    );
    edited.preset = WeatherPreset::Custom;
    assert!(WeatherScenario::try_from(edited).is_ok());
}

#[test]
fn named_presets_cannot_hide_overrides() {
    for preset in PRESETS {
        let mut p = WeatherScenario::from_preset(preset, custom().departure_reference, 0)
            .unwrap()
            .parameters();
        p.ambient_visibility = Meters(p.ambient_visibility.get() + 1.0);
        assert_eq!(
            WeatherScenario::try_from(p),
            Err(WeatherError::PresetMismatch(preset))
        );
        p.preset = WeatherPreset::Custom;
        assert!(WeatherScenario::try_from(p).is_ok());
    }
}

type EditScalar = fn(&mut WeatherParameters, f64);
const SCALARS: [EditScalar; 12] = [
    |p, v| p.departure_reference.latitude = Radians(v),
    |p, v| p.departure_reference.longitude = Radians(v),
    |p, v| p.departure_reference.altitude = Meters(v),
    |p, v| p.ambient_visibility = Meters(v),
    |p, v| p.precipitation_rate = WaterEquivalentRate(v),
    |p, v| p.cloud.as_mut().unwrap().base = Meters(v),
    |p, v| p.cloud.as_mut().unwrap().top = Meters(v),
    |p, v| p.cloud.as_mut().unwrap().coverage = v,
    |p, v| p.cloud.as_mut().unwrap().visibility = Meters(v),
    |p, v| p.fog.as_mut().unwrap().bottom = Meters(v),
    |p, v| p.fog.as_mut().unwrap().top = Meters(v),
    |p, v| p.fog.as_mut().unwrap().visibility = Meters(v),
];

#[test]
fn every_numeric_field_rejects_nonfinite_values() {
    for edit in SCALARS {
        for bad in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
        ] {
            let mut p = custom();
            edit(&mut p, bad);
            assert!(matches!(
                WeatherScenario::try_from(p),
                Err(WeatherError::OutOfRange { .. })
            ));
        }
    }
}

#[test]
fn every_numeric_field_rejects_just_outside_its_documented_bounds() {
    let bounds = [
        (-FRAC_PI_2, FRAC_PI_2),
        (-PI, PI),
        (-1000.0, 10_000.0),
        (10.0, 200_000.0),
        (0.0, 0.3 / 3600.0),
        (-1000.0, 30_000.0),
        (-1000.0, 30_000.0),
        (0.0, 1.0),
        (10.0, 200_000.0),
        (-1000.0, 15_000.0),
        (-1000.0, 15_000.0),
        (10.0, 200_000.0),
    ];
    for (edit, (min, max)) in SCALARS.into_iter().zip(bounds) {
        for bad in [min.next_down(), max.next_up()] {
            let mut p = custom();
            edit(&mut p, bad);
            assert!(WeatherScenario::try_from(p).is_err());
        }
    }
}

#[test]
fn finite_boundary_scenarios_are_accepted_without_clamping() {
    for maximum in [false, true] {
        let mut p = custom();
        p.departure_reference = Geodetic::new(
            Radians(if maximum { FRAC_PI_2 } else { -FRAC_PI_2 }),
            Radians(if maximum { PI } else { -PI }),
            Meters(if maximum { 10_000.0 } else { -1000.0 }),
        );
        p.ambient_visibility = Meters(if maximum { 200_000.0 } else { 10.0 });
        p.precipitation_rate = if maximum {
            MAX_PRECIPITATION_RATE
        } else {
            WaterEquivalentRate(f64::from_bits(1))
        };
        p.cloud = Some(ModeledCloudLayer {
            morphology: CloudMorphology::Towering,
            base: Meters(if maximum { 10_000.0 } else { -1000.0 }),
            top: Meters(if maximum { 30_000.0 } else { -999.0 }),
            coverage: if maximum { 1.0 } else { f64::from_bits(1) },
            visibility: p.ambient_visibility,
        });
        p.fog = Some(ModeledFogLayer {
            bottom: Meters(if maximum { 10_000.0 } else { -1000.0 }),
            top: Meters(if maximum { 15_000.0 } else { -999.0 }),
            visibility: p.ambient_visibility,
        });
        assert_eq!(WeatherScenario::try_from(p).unwrap().parameters(), p);
    }
}

#[test]
fn inverted_thin_and_oversized_layers_are_rejected() {
    for (base, top) in [
        (0.0, -1.0),
        (0.0, 0.0),
        (0.0, 1.0_f64.next_down()),
        (0.0, 20_000.0_f64.next_up()),
    ] {
        let mut p = custom();
        p.cloud.as_mut().unwrap().base = Meters(base);
        p.cloud.as_mut().unwrap().top = Meters(top);
        assert!(WeatherScenario::try_from(p).is_err());
    }
    for (bottom, top) in [
        (0.0, -1.0),
        (0.0, 0.0),
        (0.0, 1.0_f64.next_down()),
        (0.0, 5000.0_f64.next_up()),
    ] {
        let mut p = custom();
        p.fog.as_mut().unwrap().bottom = Meters(bottom);
        p.fog.as_mut().unwrap().top = Meters(top);
        assert!(WeatherScenario::try_from(p).is_err());
    }
}

#[test]
fn absent_layers_are_explicit_and_do_not_have_placeholder_parameters() {
    for cloud_present in [false, true] {
        for fog_present in [false, true] {
            let mut p = custom();
            if !cloud_present {
                p.cloud = None;
            }
            if !fog_present {
                p.fog = None;
            }
            assert_eq!(WeatherScenario::try_from(p).unwrap().parameters(), p);
        }
    }
    for zero in [-0.0, 0.0] {
        let mut p = custom();
        p.cloud.as_mut().unwrap().coverage = zero;
        assert_eq!(
            WeatherScenario::try_from(p),
            Err(WeatherError::ZeroCloudCoverage)
        );
    }
}

#[test]
fn precipitation_kind_and_rate_agree_without_temperature_inference() {
    for kind in [
        PrecipitationKind::None,
        PrecipitationKind::Rain,
        PrecipitationKind::Snow,
    ] {
        for rate in [
            0.0,
            f64::from_bits(1),
            0.001 / 3600.0,
            MAX_PRECIPITATION_RATE.0,
        ] {
            let mut p = custom();
            p.precipitation_kind = kind;
            p.precipitation_rate = WaterEquivalentRate(rate);
            assert_eq!(
                WeatherScenario::try_from(p).is_ok(),
                (kind == PrecipitationKind::None) == (rate <= 0.0)
            );
        }
    }
    assert_eq!(
        WaterEquivalentRate(1.0).as_meters_per_second(),
        flightsim_core::MetersPerSecond(1.0)
    );
}

#[test]
fn signed_zero_bits_are_preserved_for_a_future_exact_codec() {
    let mut p = custom();
    p.departure_reference = Geodetic::new(Radians(-0.0), Radians(-0.0), Meters(-0.0));
    p.precipitation_kind = PrecipitationKind::None;
    p.precipitation_rate = WaterEquivalentRate(-0.0);
    let actual = WeatherScenario::try_from(p).unwrap().parameters();
    for value in [
        actual.departure_reference.latitude.get(),
        actual.departure_reference.longitude.get(),
        actual.departure_reference.altitude.get(),
        actual.precipitation_rate.0,
    ] {
        assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
    }
}

#[test]
fn unknown_revisions_and_all_unknown_tags_are_rejected() {
    assert_eq!(WEATHER_PARAMETER_SCHEMA, 1);
    assert_eq!(WEATHER_MODEL_REVISION, 1);
    for schema in [0, 2, u16::MAX] {
        let mut p = custom();
        p.parameter_schema = schema;
        assert_eq!(
            WeatherScenario::try_from(p),
            Err(WeatherError::UnsupportedSchema(schema))
        );
    }
    for revision in [0, 2, u32::MAX] {
        let mut p = custom();
        p.model_revision = revision;
        assert_eq!(
            WeatherScenario::try_from(p),
            Err(WeatherError::UnsupportedModelRevision(revision))
        );
    }
    for tag in 0..=u16::MAX {
        assert_eq!(WeatherSource::try_from(tag).is_ok(), tag == 1);
        assert_eq!(WeatherPreset::try_from(tag).is_ok(), tag <= 6);
        assert_eq!(
            CloudMorphology::try_from(tag).is_ok(),
            (1..=3).contains(&tag)
        );
        assert_eq!(PrecipitationKind::try_from(tag).is_ok(), tag <= 2);
    }
    for (tag, preset) in (1..=6).zip(PRESETS) {
        assert_eq!(WeatherPreset::try_from(tag), Ok(preset));
    }
    assert_eq!(WeatherSource::try_from(1), Ok(WeatherSource::AuthoredModel));
    for (tag, kind) in [
        PrecipitationKind::None,
        PrecipitationKind::Rain,
        PrecipitationKind::Snow,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(usize::from(kind as u16), tag);
    }
    for (tag, shape) in (1..=3).zip([
        CloudMorphology::Layered,
        CloudMorphology::Puffy,
        CloudMorphology::Towering,
    ]) {
        assert_eq!(CloudMorphology::try_from(tag), Ok(shape));
        assert_eq!(shape as u16, tag);
    }
}
