//! Original sport/jet spatial details. Real aircraft inform the broad seating,
//! canopy and control architecture; these are not branded avionics or replicas.
use super::*;

pub(super) fn add_stick(parts: &mut Vec<CockpitPart>, eye: Vec3, side: f32, layout: CockpitLayout) {
    let pivot = eye + Vec3::new(0.39, side * 0.25 - eye.y, 0.70);
    parts.push(part(
        "stick gaiter",
        Sphere::new(1.0)
            .mesh()
            .uv(20, 12)
            .scaled_by(Vec3::new(0.065, 0.060, 0.045)),
        pivot,
        BLACK,
    ));
    let bend = pivot + Vec3::new(-0.014, 0.0, -0.26);
    let grip = pivot + Vec3::new(-0.040, 0.0, -0.35);
    let mut shaft = bar("moving control stick shaft", pivot, bend, 0.012, METAL);
    shaft.motion = Motion::Stick { pivot };
    parts.push(shaft);
    let mut handle = bar(
        "working control stick grip",
        bend,
        grip,
        0.021,
        if layout == CockpitLayout::Sport {
            Color::srgb(0.13, 0.15, 0.17)
        } else {
            BLACK
        },
    );
    handle.motion = Motion::Stick { pivot };
    handle.control = Some((CockpitControl::Stick, Vec2::new(0.060, 0.115)));
    parts.push(handle);
    for n in 1..=4 {
        let mut ring = bar(
            "stick grip rib",
            bend.lerp(grip, n as f32 / 5.0) - Vec3::Z * 0.003,
            bend.lerp(grip, n as f32 / 5.0) + Vec3::Z * 0.003,
            0.022,
            BLACK,
        );
        ring.motion = Motion::Stick { pivot };
        parts.push(ring);
    }
}

pub(super) fn add_canopy(parts: &mut Vec<CockpitPart>, eye: Vec3, layout: CockpitLayout) {
    // Open sky remains transparent. Aft hoop sits behind the pilot, not across
    // the forward view as an opaque roof or exterior canopy shell would.
    let jet = layout == CockpitLayout::JetTrainer;
    for side in [-1.0, 1.0] {
        parts.push(bar(
            "low canopy rail",
            eye + Vec3::new(-0.44, side * 0.505 - eye.y, 0.17),
            eye + Vec3::new(0.80, side * 0.505 - eye.y, 0.17),
            if jet { 0.029 } else { 0.018 },
            METAL,
        ));
        parts.push(cuboid(
            "raised side console",
            eye + Vec3::new(0.12, side * 0.46 - eye.y, 0.60),
            Vec3::new(0.72, 0.085, if jet { 0.24 } else { 0.12 }),
            BLACK,
        ));
    }
    let point = |angle: f32| {
        Vec3::new(
            eye.x - 0.32,
            angle.cos() * 0.505,
            eye.z + 0.17 - angle.sin() * if jet { 0.55 } else { 0.51 },
        )
    };
    for n in 0..16 {
        parts.push(bar(
            "arched aft canopy frame",
            point(n as f32 * std::f32::consts::PI / 16.0),
            point((n + 1) as f32 * std::f32::consts::PI / 16.0),
            if jet { 0.021 } else { 0.013 },
            METAL,
        ));
    }
}

pub(super) fn add_primary_display(parts: &mut Vec<CockpitPart>, eye: Vec3) {
    let center = eye + Vec3::new(EYE_TO_PANEL as f32 - 0.036, 0.0, 0.237);
    parts.push(cuboid(
        "sport primary display bezel",
        center,
        Vec3::new(0.036, 0.354, 0.249),
        BLACK,
    ));
    parts.push(face(
        "live sport primary flight display",
        center - Vec3::X * 0.020,
        Vec2::new(0.330, 0.220),
        Display::Primary,
    ));
    for side in [-1.0, 1.0] {
        for vertical in [-1.0, 1.0] {
            parts.push(disk(
                "display retaining screw",
                center + Vec3::new(-0.020, side * 0.17, vertical * 0.117),
                0.002,
                0.003,
                METAL,
            ));
        }
    }
}

pub(super) fn add_jet_throttle(parts: &mut Vec<CockpitPart>, eye: Vec3) {
    let pivot = eye + Vec3::new(0.42, -0.43 - eye.y, 0.50);
    for side in [-1.0, 1.0] {
        parts.push(cuboid(
            "throttle quadrant slot rail",
            pivot + Vec3::new(0.0, side * 0.034, -0.033),
            Vec3::new(0.28, 0.012, 0.018),
            METAL,
        ));
    }
    let tip = pivot + Vec3::new(-0.07, 0.0, -0.16);
    let mut arm = bar("aggregate jet throttle lever", pivot, tip, 0.012, METAL);
    arm.motion = Motion::ThrottleLever { pivot };
    parts.push(arm);
    let mut grip = cuboid(
        "working aggregate jet throttle grip",
        tip,
        Vec3::new(0.043, 0.065, 0.035),
        BLACK,
    );
    grip.motion = Motion::ThrottleLever { pivot };
    grip.control = Some((CockpitControl::Throttle, Vec2::new(0.080, 0.065)));
    parts.push(grip);
    label(
        parts,
        eye + Vec3::new(EYE_TO_PANEL as f32 - 0.028, -0.43 - eye.y, 0.485),
        Vec2::new(0.085, 0.04),
        "THROTTLE\nAGGREGATE",
    );
}

pub(super) fn finish(parts: &mut Vec<CockpitPart>, eye: Vec3, layout: CockpitLayout) {
    let (panel, trim, seat, name) = match layout {
        CockpitLayout::Trainer => return,
        CockpitLayout::Meadow => (
            Color::srgb(0.58, 0.56, 0.48),
            Color::srgb(0.48, 0.46, 0.37),
            Color::srgb(0.26, 0.30, 0.23),
            "MEADOW TRAINER",
        ),
        CockpitLayout::Sport => (
            Color::srgb(0.29, 0.32, 0.35),
            Color::srgb(0.28, 0.32, 0.35),
            Color::srgb(0.15, 0.23, 0.29),
            "SWIFT SPORT",
        ),
        CockpitLayout::JetTrainer => (
            Color::srgb(0.12, 0.15, 0.17),
            Color::srgb(0.20, 0.23, 0.22),
            Color::srgb(0.20, 0.25, 0.19),
            "KESTREL JET TRAINER",
        ),
    };
    for p in parts.iter_mut() {
        if p.name == "sculpted instrument panel" {
            p.color = panel;
        }
        if p.color == TRIM {
            p.color = trim;
        }
        if p.name == "seat cushion" || p.name == "stitched door insert" {
            p.color = seat;
        }
        if layout == CockpitLayout::JetTrainer && p.display == Some(Display::Label("BRAKE HOLD")) {
            p.display = Some(Display::Label("SERVICE BRAKE"));
        }
    }
    label(
        parts,
        eye + Vec3::new(EYE_TO_PANEL as f32 - 0.030, 0.40 - eye.y, 0.45),
        Vec2::new(0.14, 0.022),
        name,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trainer_switch_row_center_rays_clear_the_neutral_yoke_shafts() {
        let eye = Vec3::new(0.6, -0.25, -0.9);
        for layout in [CockpitLayout::Trainer, CockpitLayout::Meadow] {
            let parts =
                interior_parts_for_layout(eye.to_array().map(|v| Meters(f64::from(v))), layout);
            for shaft in parts.iter().filter(|p| p.name == "yoke steering shaft") {
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                    shaft.mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("shaft vertices")
                };
                let mut lower = Vec3::splat(f32::INFINITY);
                let mut upper = Vec3::splat(f32::NEG_INFINITY);
                for point in vertices {
                    let point = shaft.transform.transform_point(Vec3::from_array(*point));
                    lower = lower.min(point);
                    upper = upper.max(point);
                }
                for control in parts.iter().filter(|p| p.name == "functional panel switch") {
                    let direction = control.transform.translation - eye;
                    let a = (lower - eye) / direction;
                    let b = (upper - eye) / direction;
                    let near = a.min(b).max_element().max(0.0);
                    let far = a.max(b).min_element().min(1.0);
                    assert!(
                        near > far,
                        "{layout:?} switch {:?} blocked by shaft",
                        control.control
                    );
                }
            }
        }
    }
    #[test]
    fn jet_throttle_and_label_have_eye_ray_clearance_above_passive_console() {
        let eye = Vec3::new(0.6, -0.25, -0.9);
        let parts = interior_parts_for_layout(
            eye.to_array().map(|v| Meters(f64::from(v))),
            CockpitLayout::JetTrainer,
        );
        let console = parts
            .iter()
            .find(|p| p.name == "raised side console" && p.transform.translation.y < 0.0)
            .unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
            console.mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("console vertices")
        };
        let mut lower = Vec3::splat(f32::INFINITY);
        let mut upper = Vec3::splat(f32::NEG_INFINITY);
        for point in vertices {
            let point = console.transform.transform_point(Vec3::from_array(*point));
            lower = lower.min(point);
            upper = upper.max(point);
        }
        for part in parts.iter().filter(|p| {
            p.control.is_some_and(|c| c.0 == CockpitControl::Throttle)
                || p.display == Some(Display::Label("THROTTLE\nAGGREGATE"))
        }) {
            for throttle in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let state = CockpitState {
                    controls: ControlInputs::new(0.0, 0.0, 0.0, throttle, 0.0),
                    ..default()
                };
                let target = animated_transform(part.transform, part.motion, &state).translation;
                let direction = target - eye;
                let a = (lower - eye) / direction;
                let b = (upper - eye) / direction;
                let near = a.min(b).max_element().max(0.0);
                let far = a.max(b).min_element().min(1.0);
                assert!(
                    near > far,
                    "{} at throttle {throttle} is hidden by console",
                    part.name
                );
            }
        }
    }
}
