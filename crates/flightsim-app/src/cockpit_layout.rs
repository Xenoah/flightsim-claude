//! Presentation-only cockpit selection. Aircraft identity, admission, model fit,
//! controls and numerical dynamics remain owned by their existing modules.
use super::{Startup, aircraft_profile::SelectedAircraftProfile};
use flightsim_render::cockpit::CockpitLayout;

pub(super) fn selected(startup: &Startup) -> Option<CockpitLayout> {
    select(&startup.aircraft, startup.model.as_deref())
}

fn select(profile: &SelectedAircraftProfile, model: Option<&str>) -> Option<CockpitLayout> {
    let binding = match (profile.id(), profile.is_legacy(), profile.is_jet()) {
        ("light-single", true, _) => Some(("aircraft/light_single.glb", CockpitLayout::Trainer)),
        ("meadow-trainer", true, _) => Some(("aircraft/meadow_trainer.glb", CockpitLayout::Meadow)),
        ("swift-sport", true, _) => Some(("aircraft/swift_sport.glb", CockpitLayout::Sport)),
        ("kestrel-jet-trainer", false, true) => Some((
            "aircraft/kestrel_jet_trainer.glb",
            CockpitLayout::JetTrainer,
        )),
        _ => None,
    };
    if let Some((path, layout)) = binding
        && profile.model_path() == path
        && (model == Some(path) || (model.is_none() && profile.is_legacy()))
    {
        return Some(layout);
    }
    // Preserve the pre-existing generic legacy interior. Unknown bounded
    // profiles, and bounded-profile model replacements, keep their GLB/2D fallback.
    profile.is_legacy().then_some(CockpitLayout::Trainer)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(bytes: &[u8]) -> SelectedAircraftProfile {
        SelectedAircraftProfile::from_bytes(bytes).unwrap()
    }
    #[test]
    fn named_layouts_require_both_family_and_model_binding() {
        for (bytes, expected) in [
            (
                include_bytes!("../../../assets/aircraft/light_single.json").as_slice(),
                CockpitLayout::Trainer,
            ),
            (
                include_bytes!("../../../assets/aircraft/meadow_trainer.json").as_slice(),
                CockpitLayout::Meadow,
            ),
            (
                include_bytes!("../../../assets/aircraft/swift_sport.json").as_slice(),
                CockpitLayout::Sport,
            ),
            (
                include_bytes!("../../../assets/aircraft/kestrel_jet_trainer.json").as_slice(),
                CockpitLayout::JetTrainer,
            ),
        ] {
            let p = profile(bytes);
            assert_eq!(select(&p, Some(p.model_path())), Some(expected));
            assert_eq!(select(&p, None), p.is_legacy().then_some(expected));
            assert_eq!(
                select(&p, Some("external/custom.glb")),
                p.is_legacy().then_some(CockpitLayout::Trainer)
            );
        }
    }
    #[test]
    fn unrelated_bounded_profile_keeps_authored_interior() {
        let text = include_str!("../../../assets/aircraft/kestrel_jet_trainer.json")
            .replace("kestrel-jet-trainer", "external-jet");
        let p = profile(text.as_bytes());
        assert_eq!(select(&p, Some(p.model_path())), None);
    }
}
