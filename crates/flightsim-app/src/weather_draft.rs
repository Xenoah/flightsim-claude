//! Presentation-only authored template. Relative heights stay unresolved until
//! the existing new-flight transaction has selected its actual terrain source.

use flightsim_core::{Geodetic, Meters};
use flightsim_sim::weather::{WeatherParameters, WeatherPreset, WeatherScenario};
use flightsim_ui::world_map::WorldMapWeatherEdit;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthoredDraft {
    // Canonical parameters at the zero-height reference, never UI-rounded.
    template: WeatherScenario,
    background_visibility: Option<Meters>,
    cloud_base: Option<Meters>,
}

impl AuthoredDraft {
    pub fn from_preset(preset: WeatherPreset, seed: u64) -> Result<Self, String> {
        Ok(Self {
            template: WeatherScenario::from_preset(
                preset,
                Geodetic::from_degrees(0.0, 0.0, 0.0),
                seed,
            )
            .map_err(|error| error.to_string())?,
            background_visibility: None,
            cloud_base: None,
        })
    }

    pub fn preset(self) -> WeatherPreset {
        if self.background_visibility.is_some() || self.cloud_base.is_some() {
            WeatherPreset::Custom
        } else {
            self.template.parameters().preset
        }
    }

    pub fn background_visibility(self) -> Meters {
        self.background_visibility
            .unwrap_or(self.template.parameters().ambient_visibility)
    }

    pub fn cloud_base(self) -> Option<Meters> {
        self.template
            .parameters()
            .cloud
            .map(|layer| self.cloud_base.unwrap_or(layer.base))
    }

    pub fn template(self) -> WeatherParameters {
        self.template.parameters()
    }

    /// Validate on a copy. No field, including a valid first field, is retained
    /// when another submitted field is invalid. Actual ellipsoidal limits are
    /// checked again at the selected departure, with no clamping.
    pub fn edited(mut self, edit: WorldMapWeatherEdit) -> Result<Self, String> {
        if let Some(value) = edit.background_visibility {
            if !value.get().is_finite() || !(10.0..=200_000.0).contains(&value.get()) {
                return Err("Background visibility must be 10 to 200000 m".into());
            }
            self.background_visibility = Some(value);
        }
        if let Some(value) = edit.cloud_base {
            let layer = self
                .template
                .parameters()
                .cloud
                .ok_or("This authored preset has no cloud layer")?;
            let thickness = (layer.top - layer.base).get();
            // A base above ground must fit somewhere in the existing permitted
            // departure [-1000,10000] and cloud [-1000,30000] height ranges.
            // The actual departure may impose a lower maximum at Start.
            if !value.get().is_finite() || !(0.0..=31_000.0 - thickness).contains(&value.get()) {
                return Err(format!(
                    "Cloud base must be 0 to {} m; thickness is retained",
                    31_000.0 - thickness
                ));
            }
            self.cloud_base = Some(value);
        }
        Ok(self)
    }

    pub fn resolve(self, reference: Geodetic) -> Result<WeatherScenario, String> {
        let template = self.template.parameters();
        // Reuse canonical construction for every unedited parameter. This
        // retains the named preset's exact arithmetic, fog and seed values.
        let mut parameters =
            WeatherScenario::from_preset(template.preset, reference, template.seed)
                .map_err(|error| error.to_string())?
                .parameters();
        parameters.preset = self.preset();
        if let Some(value) = self.background_visibility {
            parameters.ambient_visibility = value;
        }
        if let Some(value) = self.cloud_base {
            let original = template.cloud.expect("validated cloud draft");
            let layer = parameters.cloud.as_mut().expect("same authored template");
            layer.base = reference.altitude + value;
            layer.top = layer.base + (original.top - original.base);
        }
        WeatherScenario::try_from(parameters).map_err(|error| error.to_string())
    }
}

fn optional_bits(value: Option<Meters>) -> Option<u64> {
    value.map(|value| value.get().to_bits())
}

// No hash or float equality at a launch boundary: compare every parameter bit.
impl PartialEq for AuthoredDraft {
    fn eq(&self, other: &Self) -> bool {
        let left = self.template.parameters();
        let right = other.template.parameters();
        let bits = |p: WeatherParameters| {
            [
                p.departure_reference.latitude.get().to_bits(),
                p.departure_reference.longitude.get().to_bits(),
                p.departure_reference.altitude.get().to_bits(),
                p.ambient_visibility.get().to_bits(),
                p.precipitation_rate.0.to_bits(),
            ]
        };
        let cloud = |p: WeatherParameters| {
            p.cloud.map(|c| {
                (
                    c.morphology,
                    [
                        c.base.get().to_bits(),
                        c.top.get().to_bits(),
                        c.coverage.to_bits(),
                        c.visibility.get().to_bits(),
                    ],
                )
            })
        };
        let fog = |p: WeatherParameters| {
            p.fog.map(|f| {
                [
                    f.bottom.get().to_bits(),
                    f.top.get().to_bits(),
                    f.visibility.get().to_bits(),
                ]
            })
        };
        left.parameter_schema == right.parameter_schema
            && left.source == right.source
            && left.model_revision == right.model_revision
            && left.preset == right.preset
            && left.seed == right.seed
            && left.precipitation_kind == right.precipitation_kind
            && bits(left) == bits(right)
            && cloud(left) == cloud(right)
            && fog(left) == fog(right)
            && optional_bits(self.background_visibility)
                == optional_bits(other.background_visibility)
            && optional_bits(self.cloud_base) == optional_bits(other.cloud_base)
    }
}
impl Eq for AuthoredDraft {}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit() -> WorldMapWeatherEdit {
        WorldMapWeatherEdit {
            background_visibility: None,
            cloud_base: None,
        }
    }

    #[test]
    fn untouched_apply_retains_all_six_canonical_presets_at_global_boundaries() {
        for preset in [
            WeatherPreset::Clear,
            WeatherPreset::Cloud,
            WeatherPreset::Fog,
            WeatherPreset::Rain,
            WeatherPreset::Snow,
            WeatherPreset::Storm,
        ] {
            for (lat, lon, ground) in [
                (31.5, 35.5, -430.0),
                (0.0, 180.0, -20.0),
                (0.0, -180.0, -0.0),
                (90.0, 180.0, 10000.0),
                (-90.0, -180.0, -1000.0),
            ] {
                let reference = Geodetic::from_degrees(lat, lon, ground);
                let original = AuthoredDraft::from_preset(preset, u64::MAX - 29).unwrap();
                let applied = original.edited(edit()).unwrap();
                assert_eq!(applied, original);
                assert_eq!(applied.preset(), preset);
                let actual = applied.resolve(reference).unwrap().parameters();
                let canonical = WeatherScenario::from_preset(preset, reference, u64::MAX - 29)
                    .unwrap()
                    .parameters();
                assert_eq!(actual, canonical);
                assert_eq!(
                    actual.departure_reference.altitude.get().to_bits(),
                    ground.to_bits()
                );
                assert_eq!(
                    actual.precipitation_rate.0.to_bits(),
                    canonical.precipitation_rate.0.to_bits()
                );
            }
        }
    }

    #[test]
    fn visual_edits_preserve_untouched_bits_thickness_seed_and_fog() {
        for preset in [
            WeatherPreset::Clear,
            WeatherPreset::Cloud,
            WeatherPreset::Fog,
            WeatherPreset::Rain,
            WeatherPreset::Snow,
            WeatherPreset::Storm,
        ] {
            let draft = AuthoredDraft::from_preset(preset, u64::MAX - 17).unwrap();
            let reference = Geodetic::from_degrees(31.5, 35.5, -430.123_456_789);
            let before = draft.resolve(reference).unwrap().parameters();
            let visibility = Meters(2_000.123_456_789_123);
            let changed = draft
                .edited(WorldMapWeatherEdit {
                    background_visibility: Some(visibility),
                    ..edit()
                })
                .unwrap();
            let mut expected = before;
            expected.preset = WeatherPreset::Custom;
            expected.ambient_visibility = visibility;
            assert_eq!(changed.resolve(reference).unwrap().parameters(), expected);
            assert_eq!(
                changed.background_visibility().get().to_bits(),
                visibility.get().to_bits()
            );
            if let Some(layer) = before.cloud {
                let offset = Meters(200.123_456_789_123);
                let actual = changed
                    .edited(WorldMapWeatherEdit {
                        cloud_base: Some(offset),
                        ..edit()
                    })
                    .unwrap()
                    .resolve(reference)
                    .unwrap()
                    .parameters();
                let mut expected = expected;
                let original = draft.template().cloud.unwrap();
                expected.cloud.as_mut().unwrap().base = reference.altitude + offset;
                expected.cloud.as_mut().unwrap().top =
                    reference.altitude + offset + (original.top - original.base);
                assert_eq!(actual, expected);
                assert_eq!(
                    actual.cloud.unwrap().coverage.to_bits(),
                    layer.coverage.to_bits()
                );
                assert_eq!(
                    actual.cloud.unwrap().visibility.get().to_bits(),
                    layer.visibility.get().to_bits()
                );
            } else {
                assert!(
                    changed
                        .edited(WorldMapWeatherEdit {
                            cloud_base: Some(Meters(200.0)),
                            ..edit()
                        })
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn complete_exact_snapshot_distinguishes_seed_and_signed_zero_even_when_resolved_equal() {
        let a = AuthoredDraft::from_preset(WeatherPreset::Rain, 1).unwrap();
        let b = AuthoredDraft::from_preset(WeatherPreset::Rain, 2).unwrap();
        assert_ne!(a, b);
        let plus = a
            .edited(WorldMapWeatherEdit {
                cloud_base: Some(Meters(0.0)),
                ..edit()
            })
            .unwrap();
        let minus = a
            .edited(WorldMapWeatherEdit {
                cloud_base: Some(Meters(-0.0)),
                ..edit()
            })
            .unwrap();
        assert_ne!(plus, minus);
        assert_eq!(
            minus.cloud_base().unwrap().get().to_bits(),
            (-0.0_f64).to_bits()
        );
        assert_eq!(minus.edited(edit()).unwrap(), minus);
    }

    #[test]
    fn strict_visibility_edges_and_atomic_invalid_multifield_apply() {
        let original = AuthoredDraft::from_preset(WeatherPreset::Rain, 123).unwrap();
        for visibility in [10.0, 200_000.0] {
            let changed = original
                .edited(WorldMapWeatherEdit {
                    background_visibility: Some(Meters(visibility)),
                    ..edit()
                })
                .unwrap();
            assert_eq!(
                changed.background_visibility().get().to_bits(),
                visibility.to_bits()
            );
        }
        for visibility in [
            9.999,
            200_000.001,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(
                original
                    .edited(WorldMapWeatherEdit {
                        background_visibility: Some(Meters(visibility)),
                        cloud_base: Some(Meters(200.0))
                    })
                    .is_err()
            );
        }
        for base in [-1.0, 29_000.001, 30_000.0, f64::NAN, f64::INFINITY] {
            assert!(
                original
                    .edited(WorldMapWeatherEdit {
                        background_visibility: Some(Meters(2000.0)),
                        cloud_base: Some(Meters(base))
                    })
                    .is_err()
            );
            assert_eq!(original.preset(), WeatherPreset::Rain);
            assert_eq!(
                original.background_visibility().get().to_bits(),
                10000.0_f64.to_bits()
            );
        }
    }

    #[test]
    fn actual_departure_bounds_are_strict_and_resolved_only_once() {
        let draft = AuthoredDraft::from_preset(WeatherPreset::Rain, 7)
            .unwrap()
            .edited(WorldMapWeatherEdit {
                cloud_base: Some(Meters(29_000.0)),
                ..edit()
            })
            .unwrap();
        let scenario = draft
            .resolve(Geodetic::from_degrees(0.0, 0.0, -1000.0))
            .unwrap();
        let layer = scenario.parameters().cloud.unwrap();
        assert_eq!(layer.base.get().to_bits(), 28000.0_f64.to_bits());
        assert_eq!(layer.top.get().to_bits(), 30000.0_f64.to_bits());
        assert!(
            draft
                .resolve(Geodetic::from_degrees(0.0, 0.0, -999.999))
                .is_err()
        );
        assert!(
            draft
                .resolve(Geodetic::from_degrees(0.0, 0.0, f64::NAN))
                .is_err()
        );
        // Reading the resolved scenario never follows later aircraft/ground heights.
        assert_eq!(scenario.parameters().cloud, Some(layer));
    }

    #[test]
    fn matching_named_template_reuses_all_bits_and_name_or_seed_changes_rebuild() {
        let canonical = AuthoredDraft::from_preset(WeatherPreset::Rain, 7).unwrap();
        let mut cached = canonical;
        let mut parameters = cached.template.parameters();
        // Signed zero is a valid zero reference but identifies an exact copy:
        // reconstructing the preset at a fresh +0 reference would lose this bit.
        parameters.departure_reference.latitude = flightsim_core::Radians(-0.0);
        cached.template = WeatherScenario::try_from(parameters).unwrap();
        assert_ne!(cached, canonical);
        assert_eq!(
            super::super::authored_draft(Some(WeatherPreset::Rain), 7, Some(cached)),
            Some(cached)
        );
        for (preset, seed) in [(WeatherPreset::Rain, 8), (WeatherPreset::Snow, 7)] {
            assert_eq!(
                super::super::authored_draft(Some(preset), seed, Some(cached)),
                Some(AuthoredDraft::from_preset(preset, seed).unwrap())
            );
        }
        let edited = cached
            .edited(WorldMapWeatherEdit {
                background_visibility: Some(Meters(2000.0)),
                ..edit()
            })
            .unwrap();
        assert_eq!(
            super::super::authored_draft(Some(WeatherPreset::Custom), 999, Some(edited)),
            Some(edited)
        );
        assert_eq!(
            super::super::authored_draft(Some(WeatherPreset::Rain), 7, Some(edited)),
            Some(canonical)
        );
        assert!(super::super::authored_draft(None, 7, Some(cached)).is_none());
    }
}
