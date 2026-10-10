//! Original, procedural analog light-aircraft cockpit. The supplied reference is
//! a collage of Cessna-style interiors, not an identified or licensed aircraft.
//! This is an authored layout, not a certified replica. Every dial and movable
//! control below has an explicit simulation or presentation binding. There are
//! deliberately no fictional fuel, mixture, electrical-bus or radio controls.
//!
//! Geometry uses aircraft body axes: +X forward, +Y right, +Z down. Instrument
//! faces and hit surfaces are aircraft-local, so looking around cannot detach
//! the gauges from the panel. No downloaded models, fonts or photo textures.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "bounded metre-scale presentation geometry and texture coordinates use f32"
)]

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
#[cfg(test)]
use flightsim_core::{Feet, FeetPerMinute, Knots};
use flightsim_core::{Meters, MetersPerSecond, Radians, RadiansPerSecond};
use flightsim_fdm::ControlInputs;

mod texture;
pub use texture::display_image;

pub const CABIN_WIDTH: f64 = 40.0 * 0.0254;
pub const CABIN_HEIGHT: f64 = 48.0 * 0.0254;
pub const INSTRUMENT_DIAMETER: f64 = 3.125 * 0.0254;
pub const EYE_TO_PANEL: f64 = 0.67;
pub const EYE_TO_FLOOR: f64 = 0.95;
pub const PILOT_SEAT_OFFSET: f64 = 0.25;
pub const SIX_PACK_DROP: f64 = 0.24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dial {
    Airspeed,
    Attitude,
    Altitude,
    Turn,
    Heading,
    VerticalSpeed,
    Power,
    Flaps,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Dial(Dial),
    Flight,
    Controls,
    Label(&'static str),
}
impl Display {
    #[must_use]
    pub const fn dynamic(self) -> bool {
        matches!(
            self,
            Self::Dial(Dial::Attitude | Dial::Heading) | Self::Flight | Self::Controls
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CockpitControl {
    Yoke,
    RudderLeft,
    RudderRight,
    Throttle,
    Flaps,
    Trim,
    Brake,
    Lighting,
    CenterView,
    Hud,
}
impl CockpitControl {
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Yoke => "YOKE: hold and drag left/right; down pulls, up pushes",
            Self::RudderLeft => "LEFT RUDDER: hold left mouse button",
            Self::RudderRight => "RIGHT RUDDER: hold left mouse button",
            Self::Throttle => "THROTTLE: hold and drag up to increase / down to reduce",
            Self::Flaps => "FLAPS: hold and drag down to extend / up to retract",
            Self::Trim => "ELEVATOR TRIM: hold and drag down for nose up / up for nose down",
            Self::Brake => "WHEEL BRAKES: hold left mouse button; release to let go",
            Self::Lighting => "INSTRUMENT LIGHT: click to switch illumination",
            Self::CenterView => "CENTER VIEW: click, or press Home",
            Self::Hud => "FLIGHT HUD: click, or V to show/hide readouts and help",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Motion {
    Fixed,
    Needle {
        dial: Dial,
        secondary: bool,
        pivot: Vec3,
    },
    Yoke {
        pivot: Vec3,
    },
    Pedal {
        right: bool,
    },
    Throttle,
    Flaps {
        pivot: Vec3,
    },
    Trim {
        pivot: Vec3,
    },
    Brake,
    Lighting {
        pivot: Vec3,
    },
    Hud {
        pivot: Vec3,
    },
}
#[derive(Debug, Clone)]
pub struct CockpitPart {
    pub name: &'static str,
    pub mesh: Mesh,
    pub transform: Transform,
    pub color: Color,
    pub metallic: f32,
    pub display: Option<Display>,
    pub motion: Motion,
    /// Dimensions of a local, front-facing rectangular hit surface. Passive
    /// cabin geometry has no action. Actual mouse gating belongs to the app.
    pub control: Option<(CockpitControl, Vec2)>,
}

#[derive(Debug, Clone, Copy)]
pub struct CockpitState {
    pub airspeed: MetersPerSecond,
    pub altitude: Meters,
    pub agl: Meters,
    pub vertical_speed: MetersPerSecond,
    pub heading: Radians,
    pub pitch: Radians,
    pub roll: Radians,
    /// Body-axis yaw angular rate, radians/second. This is not a gyro turn coordinator.
    pub yaw_rate: RadiansPerSecond,
    pub ground_speed: MetersPerSecond,
    pub wind_speed: MetersPerSecond,
    pub wind_from: Radians,
    pub controls: ControlInputs,
    pub trim: Option<f64>,
    pub replay: bool,
    pub stall: bool,
    pub stall_unavailable: bool,
    pub on_ground: bool,
    pub lighting: bool,
    pub show_hud: bool,
}
impl Default for CockpitState {
    fn default() -> Self {
        Self {
            airspeed: MetersPerSecond(0.0),
            altitude: Meters(0.0),
            agl: Meters(0.0),
            vertical_speed: MetersPerSecond(0.0),
            heading: Radians(0.0),
            pitch: Radians(0.0),
            roll: Radians(0.0),
            yaw_rate: RadiansPerSecond(0.0),
            ground_speed: MetersPerSecond(0.0),
            wind_speed: MetersPerSecond(0.0),
            wind_from: Radians(0.0),
            controls: ControlInputs::neutral(),
            trim: Some(0.0),
            replay: false,
            stall: false,
            stall_unavailable: false,
            on_ground: true,
            lighting: true,
            show_hud: false,
        }
    }
}
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
#[must_use]
pub fn dial_angle(dial: Dial, secondary: bool, state: &CockpitState) -> f32 {
    let degrees = match dial {
        Dial::Airspeed => -150.0 + finite(state.airspeed.to_knots().get()).clamp(0.0, 200.0) * 1.5,
        Dial::Altitude => {
            finite(state.altitude.to_feet().get()).rem_euclid(if secondary {
                10000.0
            } else {
                1000.0
            }) / if secondary { 10000.0 } else { 1000.0 }
                * 360.0
        }
        Dial::VerticalSpeed => {
            finite(state.vertical_speed.to_feet_per_minute().get()).clamp(-2000.0, 2000.0) / 2000.0
                * 135.0
                - 90.0
        }
        Dial::Turn => finite(state.yaw_rate.get()).to_degrees().clamp(-6.0, 6.0) * 7.5,
        Dial::Power => -135.0 + state.controls.throttle() * 270.0,
        Dial::Flaps => -135.0 + state.controls.flaps() * 270.0,
        Dial::Heading => -finite(state.heading.get()).to_degrees(),
        Dial::Attitude => -finite(state.roll.get()).to_degrees(),
    };
    degrees.to_radians() as f32
}
#[must_use]
pub fn animated_transform(base: Transform, motion: Motion, state: &CockpitState) -> Transform {
    let mut result = base;
    let mut around = |pivot: Vec3, rotation: Quat| {
        result.translation = pivot + rotation * (base.translation - pivot);
        result.rotation = rotation * base.rotation;
    };
    match motion {
        Motion::Fixed => {}
        Motion::Needle {
            dial,
            secondary,
            pivot,
        } => around(
            pivot,
            Quat::from_rotation_x(dial_angle(dial, secondary, state)),
        ),
        Motion::Yoke { pivot } => {
            around(
                pivot,
                Quat::from_rotation_x(state.controls.aileron() as f32 * 0.70),
            );
            result.translation.x -= state.controls.elevator() as f32 * 0.055;
        }
        Motion::Pedal { right } => {
            result.translation.x +=
                state.controls.rudder() as f32 * if right { 0.045 } else { -0.045 }
        }
        Motion::Throttle => result.translation.x += state.controls.throttle() as f32 * 0.045,
        Motion::Flaps { pivot } => around(
            pivot,
            Quat::from_rotation_y(state.controls.flaps() as f32 * 0.70),
        ),
        Motion::Trim { pivot } => around(
            pivot,
            Quat::from_rotation_y(state.trim.unwrap_or(0.0) as f32 * 6.0),
        ),
        Motion::Brake => result.translation.x -= state.controls.brakes() as f32 * 0.025,
        Motion::Hud { pivot } => around(
            pivot,
            Quat::from_rotation_y(if state.show_hud { -0.35 } else { 0.35 }),
        ),
        Motion::Lighting { pivot } => around(
            pivot,
            Quat::from_rotation_y(if state.lighting { -0.35 } else { 0.35 }),
        ),
    }
    result
}

const PANEL: Color = Color::srgb(0.36, 0.38, 0.39);
const BLACK: Color = Color::srgb(0.025, 0.033, 0.039);
const TRIM: Color = Color::srgb(0.39, 0.40, 0.37);
const METAL: Color = Color::srgb(0.44, 0.47, 0.48);
const WHITE: Color = Color::srgb(0.92, 0.91, 0.83);
fn part(name: &'static str, mesh: Mesh, at: Vec3, color: Color) -> CockpitPart {
    CockpitPart {
        name,
        mesh,
        transform: Transform::from_translation(at),
        color,
        metallic: if color == METAL { 0.65 } else { 0.0 },
        display: None,
        motion: Motion::Fixed,
        control: None,
    }
}
fn cuboid(name: &'static str, at: Vec3, size: Vec3, color: Color) -> CockpitPart {
    part(name, Cuboid::from_size(size).into(), at, color)
}
fn bar(name: &'static str, a: Vec3, b: Vec3, radius: f32, color: Color) -> CockpitPart {
    let mut p = part(
        name,
        Cylinder::new(radius, a.distance(b))
            .mesh()
            .resolution(12)
            .build(),
        (a + b) * 0.5,
        color,
    );
    p.transform.rotation = Quat::from_rotation_arc(Vec3::Y, (b - a).normalize());
    p
}
fn disk(name: &'static str, center: Vec3, radius: f32, depth: f32, color: Color) -> CockpitPart {
    let mut p = part(
        name,
        Cylinder::new(radius, depth).mesh().resolution(48).build(),
        center,
        color,
    );
    p.transform.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    p
}
fn face(name: &'static str, center: Vec3, size: Vec2, display: Display) -> CockpitPart {
    let mut p = part(name, front_quad(size), center, Color::WHITE);
    p.display = Some(display);
    p
}
fn label(parts: &mut Vec<CockpitPart>, center: Vec3, size: Vec2, text: &'static str) {
    let lines: Vec<_> = text.split('\n').collect();
    let longest = lines.iter().map(|line| line.len()).max().unwrap_or(1) as f32;
    let height = size
        .y
        .min(size.x * (lines.len() as f32 * 9.0 + 2.0) / (longest * 6.0 + 2.0));
    parts.push(face(
        "engraved placard",
        center,
        Vec2::new(size.x, height),
        Display::Label(text),
    ));
}
fn add_dial(parts: &mut Vec<CockpitPart>, center: Vec3, radius: f32, dial: Dial) {
    parts.push(disk(
        "instrument metal housing",
        center + Vec3::X * 0.006,
        radius * 1.16,
        0.025,
        METAL,
    ));
    parts.push(disk(
        "instrument raised bezel",
        center,
        radius * 1.09,
        0.027,
        BLACK,
    ));
    // Circular face geometry, not a rectangular overlay. Face's front normal is -X.
    let mut p = part(
        "recessed instrument face",
        front_disk(radius),
        center - Vec3::X * 0.014,
        Color::WHITE,
    );
    p.display = Some(Display::Dial(dial));
    parts.push(p);
    for y in [-1.0, 1.0] {
        for z in [-1.0, 1.0] {
            parts.push(disk(
                "instrument mounting screw",
                center + Vec3::new(-0.012, y * radius * 0.93, z * radius * 0.93),
                0.0017,
                0.003,
                METAL,
            ));
        }
    }
    if !matches!(dial, Dial::Attitude | Dial::Heading) {
        for secondary in [false, true] {
            if secondary && dial != Dial::Altitude {
                continue;
            }
            let length = radius * if secondary { 0.49 } else { 0.75 };
            let pivot = center - Vec3::X * 0.019;
            let mut p = cuboid(
                "live instrument needle",
                pivot - Vec3::Z * length * 0.43,
                Vec3::new(0.0015, if secondary { 0.0025 } else { 0.0016 }, length),
                WHITE,
            );
            p.motion = Motion::Needle {
                dial,
                secondary,
                pivot,
            };
            parts.push(p);
        }
        parts.push(disk(
            "needle spindle",
            center - Vec3::X * 0.022,
            radius * 0.07,
            0.004,
            METAL,
        ));
    }
}

/// The complete original cabin. Small hardware is passive structural detail;
/// everything presented as a switch, lever or instrument is functionally bound.
#[must_use]
pub fn interior_parts(eye: [Meters; 3]) -> Vec<CockpitPart> {
    let eye = Vec3::new(
        eye[0].get() as f32,
        eye[1].get() as f32,
        eye[2].get() as f32,
    );
    let x = eye.x + EYE_TO_PANEL as f32;
    let top = eye.z + 0.095;
    let mut parts = vec![];
    let outline = [
        (-0.505, 0.12),
        (-0.495, 0.055),
        (-0.45, 0.015),
        (-0.32, 0.0),
        (0.32, 0.0),
        (0.45, 0.015),
        (0.495, 0.055),
        (0.505, 0.12),
        (0.49, 0.43),
        (-0.49, 0.43),
    ];
    parts.push(part(
        "sculpted instrument panel",
        panel_mesh(&outline, 0.045),
        Vec3::new(x, 0.0, top),
        PANEL,
    ));
    // Curved padded brow, using short continuous rounded segments.
    for pair in outline[..8].windows(2) {
        parts.push(bar(
            "padded glare shield",
            Vec3::new(x - 0.055, pair[0].0, top + pair[0].1 - 0.014),
            Vec3::new(x - 0.055, pair[1].0, top + pair[1].1 - 0.014),
            0.027,
            BLACK,
        ));
    }
    parts.push(cuboid(
        "glare shield shelf",
        Vec3::new(x + 0.06, 0.0, top - 0.012),
        Vec3::new(0.22, 0.89, 0.022),
        BLACK,
    ));
    parts.push(cuboid(
        "carpet floor",
        Vec3::new(eye.x + 0.03, 0.0, eye.z + 0.95),
        Vec3::new(1.30, 0.99, 0.025),
        Color::srgb(0.055, 0.065, 0.071),
    ));
    for side in [-1.0, 1.0] {
        let y = side * 0.51;
        parts.push(cuboid(
            "upholstered sidewall",
            Vec3::new(eye.x + 0.10, y, eye.z + 0.57),
            Vec3::new(1.35, 0.034, 0.71),
            TRIM,
        ));
        parts.push(bar(
            "window sill",
            Vec3::new(eye.x - 0.48, y, eye.z + 0.16),
            Vec3::new(x + 0.14, y, eye.z + 0.16),
            0.026,
            BLACK,
        ));
        parts.push(bar(
            "windscreen pillar",
            Vec3::new(x + 0.14, y, eye.z + 0.15),
            Vec3::new(eye.x + 0.12, y * 0.9, eye.z - 0.34),
            0.025,
            TRIM,
        ));
        parts.push(bar(
            "door frame",
            Vec3::new(eye.x - 0.38, y, eye.z + 0.16),
            Vec3::new(eye.x - 0.40, y, eye.z - 0.31),
            0.023,
            TRIM,
        ));
        parts.push(bar(
            "ceiling rail",
            Vec3::new(eye.x - 0.40, y, eye.z - 0.31),
            Vec3::new(eye.x + 0.12, y * 0.9, eye.z - 0.34),
            0.024,
            TRIM,
        ));
        parts.push(cuboid(
            "stitched door insert",
            Vec3::new(eye.x + 0.13, y - side * 0.022, eye.z + 0.47),
            Vec3::new(0.70, 0.017, 0.26),
            Color::srgb(0.13, 0.16, 0.18),
        ));
        parts.push(bar(
            "armrest",
            Vec3::new(eye.x - 0.07, y - side * 0.05, eye.z + 0.51),
            Vec3::new(eye.x + 0.38, y - side * 0.05, eye.z + 0.51),
            0.026,
            BLACK,
        ));
        parts.push(cuboid(
            "seat cushion",
            Vec3::new(eye.x - 0.13, side * 0.25, eye.z + 0.67),
            Vec3::new(0.43, 0.41, 0.10),
            Color::srgb(0.16, 0.19, 0.20),
        ));
        let yoke = Vec3::new(x - 0.18, side * 0.25, eye.z + 0.40);
        let mut shaft = bar(
            "yoke steering shaft",
            yoke + Vec3::X * 0.01,
            yoke + Vec3::X * 0.23,
            0.013,
            METAL,
        );
        shaft.motion = Motion::Yoke { pivot: yoke };
        parts.push(shaft);
        let mut yoke_parts = vec![part(
            "contoured yoke boss",
            Sphere::new(1.0)
                .mesh()
                .uv(24, 12)
                .scaled_by(Vec3::new(0.027, 0.043, 0.030)),
            yoke,
            Color::srgb(0.055, 0.065, 0.070),
        )];
        // Smoothly joined U-shaped arms and slim grips, rather than a bar with blocks.
        for hand in [-1.0, 1.0] {
            let offsets = [
                Vec3::new(0.0, hand * 0.026, 0.005),
                Vec3::new(-0.009, hand * 0.063, 0.024),
                Vec3::new(-0.019, hand * 0.095, 0.025),
                Vec3::new(-0.025, hand * 0.112, 0.010),
                Vec3::new(-0.030, hand * 0.115, -0.020),
                Vec3::new(-0.034, hand * 0.110, -0.057),
            ];
            for (i, pair) in offsets.windows(2).enumerate() {
                let radius = if i >= 3 { 0.013 } else { 0.010 };
                yoke_parts.push(bar(
                    "curved yoke arm",
                    yoke + pair[0],
                    yoke + pair[1],
                    radius,
                    BLACK,
                ));
                let mut joint = part(
                    "rounded yoke joint",
                    Sphere::new(radius).mesh().uv(12, 8),
                    yoke + pair[1],
                    BLACK,
                );
                if i >= 3 {
                    joint.control = Some((CockpitControl::Yoke, Vec2::new(0.032, 0.040)));
                }
                yoke_parts.push(joint);
            }
        }
        for p in &mut yoke_parts {
            p.motion = Motion::Yoke { pivot: yoke };
        }
        yoke_parts[0].control = Some((CockpitControl::Yoke, Vec2::new(0.085, 0.065)));
        parts.extend(yoke_parts);
        for right in [false, true] {
            let p = Vec3::new(
                x - 0.04,
                side * 0.25 + if right { 0.068 } else { -0.068 },
                eye.z + 0.81,
            );
            let mut pedal = cuboid("rudder pedal", p, Vec3::new(0.045, 0.095, 0.09), METAL);
            pedal.motion = Motion::Pedal { right };
            pedal.control = Some((
                if right {
                    CockpitControl::RudderRight
                } else {
                    CockpitControl::RudderLeft
                },
                Vec2::new(0.10, 0.11),
            ));
            parts.push(pedal);
            for n in -2..=2 {
                let mut grip = cuboid(
                    "pedal grip",
                    p + Vec3::new(-0.026, 0.0, n as f32 * 0.015),
                    Vec3::new(0.007, 0.087, 0.004),
                    BLACK,
                );
                grip.motion = Motion::Pedal { right };
                parts.push(grip);
            }
        }
    }
    parts.push(cuboid(
        "ceiling headliner",
        Vec3::new(eye.x - 0.1, 0.0, eye.z - 0.36),
        Vec3::new(0.85, 0.98, 0.035),
        TRIM,
    ));
    // Standard six-pack layout; the lower-left instrument honestly labels body yaw.
    let r = INSTRUMENT_DIAMETER as f32 * 0.5;
    for (i, dial) in [
        Dial::Airspeed,
        Dial::Attitude,
        Dial::Altitude,
        Dial::Turn,
        Dial::Heading,
        Dial::VerticalSpeed,
    ]
    .into_iter()
    .enumerate()
    {
        add_dial(
            &mut parts,
            Vec3::new(
                x - 0.033,
                eye.y + (i % 3) as f32 * 0.102 - 0.102,
                eye.z + 0.18 + (i / 3) as f32 * 0.105,
            ),
            r,
            dial,
        );
    }
    for (z, dial) in [(0.19, Dial::Power), (0.295, Dial::Flaps)] {
        add_dial(
            &mut parts,
            Vec3::new(x - 0.033, 0.32, eye.z + z),
            r * 0.90,
            dial,
        );
    }
    // Central stack provides real simulator data rather than pretend radio tuning.
    parts.push(cuboid(
        "flight data stack surround",
        Vec3::new(x - 0.034, 0.065, eye.z + 0.24),
        Vec3::new(0.035, 0.19, 0.273),
        BLACK,
    ));
    parts.push(face(
        "live flight data screen",
        Vec3::new(x - 0.054, 0.065, eye.z + 0.18),
        Vec2::new(0.172, 0.104),
        Display::Flight,
    ));
    parts.push(face(
        "live control data screen",
        Vec3::new(x - 0.054, 0.065, eye.z + 0.303),
        Vec2::new(0.172, 0.114),
        Display::Controls,
    ));
    label(
        &mut parts,
        Vec3::new(x - 0.026, 0.32, eye.z + 0.382),
        Vec2::new(0.26, 0.038),
        "SIMULATED FLIGHT\nNO FUEL OR RADIO MODEL",
    );
    label(
        &mut parts,
        Vec3::new(x - 0.026, eye.y, eye.z + 0.366),
        Vec2::new(0.31, 0.025),
        "EAS KT  /  TRUE HDG  /  ELLIPSOID FT",
    );
    // Throttle has an actual axial stroke. White flap paddle, textured trim wheel.
    let throttle = Vec3::new(x - 0.14, -0.045, eye.z + 0.445);
    parts.push(bar(
        "throttle shaft",
        throttle,
        throttle + Vec3::X * 0.105,
        0.005,
        METAL,
    ));
    let mut knob = disk("working throttle knob", throttle, 0.020, 0.036, BLACK);
    knob.motion = Motion::Throttle;
    knob.control = Some((CockpitControl::Throttle, Vec2::splat(0.060)));
    parts.push(knob);
    label(
        &mut parts,
        Vec3::new(x - 0.028, -0.045, eye.z + 0.405),
        Vec2::new(0.080, 0.024),
        "THROTTLE",
    );
    let flap = Vec3::new(x - 0.07, 0.14, eye.z + 0.445);
    let mut lever = bar("flap lever", flap, flap - Vec3::X * 0.075, 0.007, METAL);
    lever.motion = Motion::Flaps { pivot: flap };
    parts.push(lever);
    let mut handle = cuboid(
        "working flap paddle",
        flap - Vec3::X * 0.076,
        Vec3::new(0.025, 0.048, 0.018),
        WHITE,
    );
    handle.motion = Motion::Flaps { pivot: flap };
    handle.control = Some((CockpitControl::Flaps, Vec2::new(0.068, 0.062)));
    parts.push(handle);
    label(
        &mut parts,
        Vec3::new(x - 0.028, 0.14, eye.z + 0.405),
        Vec2::new(0.060, 0.024),
        "FLAPS",
    );
    let trim = Vec3::new(x - 0.21, 0.0, eye.z + 0.575);
    parts.push(cuboid(
        "trim pedestal",
        trim + Vec3::new(0.03, 0.0, 0.15),
        Vec3::new(0.16, 0.095, 0.32),
        BLACK,
    ));
    let mut wheel = part(
        "working elevator trim wheel",
        Torus::new(0.045, 0.058)
            .mesh()
            .major_resolution(36)
            .minor_resolution(10)
            .build(),
        trim,
        BLACK,
    );
    wheel.motion = Motion::Trim { pivot: trim };
    wheel.control = Some((CockpitControl::Trim, Vec2::new(0.072, 0.14)));
    parts.push(wheel);
    for n in 0..16 {
        let a = n as f32 * std::f32::consts::TAU / 16.0;
        let mut notch = cuboid(
            "trim wheel tread",
            trim + Vec3::new(a.cos() * 0.056, 0.0, a.sin() * 0.056),
            Vec3::new(0.009, 0.022, 0.009),
            METAL,
        );
        notch.motion = Motion::Trim { pivot: trim };
        parts.push(notch);
    }
    label(
        &mut parts,
        Vec3::new(x - 0.305, 0.0, eye.z + 0.675),
        Vec2::new(0.072, 0.035),
        "ELEVATOR\nTRIM",
    );
    let brake = Vec3::new(x - 0.083, -0.145, eye.z + 0.46);
    let mut handle = cuboid(
        "working brake pull",
        brake,
        Vec3::new(0.025, 0.046, 0.017),
        Color::srgb(0.50, 0.12, 0.07),
    );
    handle.motion = Motion::Brake;
    handle.control = Some((CockpitControl::Brake, Vec2::new(0.062, 0.054)));
    parts.push(handle);
    label(
        &mut parts,
        Vec3::new(x - 0.03, -0.15, eye.z + 0.423),
        Vec2::new(0.074, 0.024),
        "BRAKE HOLD",
    );
    for (y, action, text) in [
        (-0.45, CockpitControl::Lighting, "PANEL LIGHT"),
        (-0.37, CockpitControl::CenterView, "CENTER VIEW"),
        (-0.29, CockpitControl::Hud, "FLIGHT HUD"),
    ] {
        let pivot = Vec3::new(x - 0.035, y, eye.z + 0.423);
        parts.push(disk("switch mounting nut", pivot, 0.008, 0.009, METAL));
        let mut lever = bar(
            "functional panel switch",
            pivot,
            pivot + Vec3::new(-0.023, 0.0, -0.013),
            0.0035,
            METAL,
        );
        if action == CockpitControl::Hud {
            lever.motion = Motion::Hud { pivot };
        }
        if action == CockpitControl::Lighting {
            lever.motion = Motion::Lighting { pivot };
        }
        lever.control = Some((action, Vec2::new(0.068, 0.06)));
        parts.push(lever);
        label(
            &mut parts,
            Vec3::new(x - 0.028, y, eye.z + 0.39),
            Vec2::new(0.080, 0.023),
            text,
        );
    }
    parts
}

fn front_quad(size: Vec2) -> Mesh {
    let y = size.x * 0.5;
    let z = size.y * 0.5;
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, -y, -z], [0.0, y, -z], [0.0, y, z], [0.0, -y, z]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[-1.0, 0.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}
fn front_disk(radius: f32) -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]];
    let mut uv = vec![[0.5, 0.5]];
    let mut indices = vec![];
    for n in 0..=64 {
        let a = n as f32 * std::f32::consts::TAU / 64.0;
        positions.push([0.0, a.sin() * radius, -a.cos() * radius]);
        uv.push([0.5 + a.sin() * 0.5, 0.5 - a.cos() * 0.5]);
        if n > 0 {
            indices.extend([0, n + 1, n]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[-1.0, 0.0, 0.0]; positions.len()],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices))
}
fn panel_mesh(outline: &[(f32, f32)], depth: f32) -> Mesh {
    let center = Vec3::new(0.0, 0.0, 0.21);
    let mut positions = vec![];
    let mut normals = vec![];
    for i in 0..outline.len() {
        let a = Vec3::new(0.0, outline[i].0, outline[i].1);
        let b = Vec3::new(
            0.0,
            outline[(i + 1) % outline.len()].0,
            outline[(i + 1) % outline.len()].1,
        );
        for p in [center, b, a] {
            positions.push(p.to_array());
            normals.push([-1.0, 0.0, 0.0]);
        }
        let normal = (b - a).cross(Vec3::X).normalize();
        for p in [
            a,
            b,
            b + Vec3::X * depth,
            a,
            b + Vec3::X * depth,
            a + Vec3::X * depth,
        ] {
            positions.push(p.to_array());
            normals.push(normal.to_array());
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; positions.len()])
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_dials_and_actions_are_real_and_finite() {
        let parts = interior_parts([Meters(0.6), Meters(-0.25), Meters(-0.9)]);
        assert!(parts.len() > 120);
        assert_eq!(
            parts
                .iter()
                .filter(|p| matches!(p.display, Some(Display::Dial(_))))
                .count(),
            8
        );
        for p in &parts {
            assert!(p.transform.is_finite());
            assert!(p.mesh.count_vertices() > 0);
        }
        for action in [
            CockpitControl::Yoke,
            CockpitControl::Throttle,
            CockpitControl::Flaps,
            CockpitControl::Trim,
            CockpitControl::Brake,
            CockpitControl::Lighting,
            CockpitControl::CenterView,
            CockpitControl::RudderLeft,
            CockpitControl::RudderRight,
        ] {
            assert!(
                parts
                    .iter()
                    .any(|p| p.control.is_some_and(|c| c.0 == action))
            );
        }
    }
    #[test]
    fn dial_scales_use_units_and_known_endpoints() {
        let mut s = CockpitState {
            airspeed: Knots(100.0).to_meters_per_second(),
            ..default()
        };
        assert!(dial_angle(Dial::Airspeed, false, &s).abs() < 1e-5);
        s.altitude = Feet(250.0).to_meters();
        assert!((dial_angle(Dial::Altitude, false, &s) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        s.vertical_speed = FeetPerMinute(0.0).to_meters_per_second();
        assert!(
            (dial_angle(Dial::VerticalSpeed, false, &s) + std::f32::consts::FRAC_PI_2).abs() < 1e-5
        );
    }
    #[test]
    fn extending_flaps_moves_the_physical_paddle_down() {
        let pivot = Vec3::X;
        let base = Transform::from_translation(pivot - Vec3::X * 0.076);
        let state = CockpitState {
            controls: ControlInputs::new(0.0, 0.0, 0.0, 0.0, 1.0),
            ..default()
        };
        let extended = animated_transform(base, Motion::Flaps { pivot }, &state);
        assert!(extended.translation.z > base.translation.z);
    }
    #[test]
    fn yokes_and_pedals_follow_effective_controls() {
        let s = CockpitState {
            controls: ControlInputs::new(1.0, 1.0, 1.0, 1.0, 1.0),
            ..default()
        };
        let b = Transform::from_xyz(1.0, 0.0, 0.0);
        let t = animated_transform(b, Motion::Yoke { pivot: Vec3::X }, &s);
        assert!(t.translation.x < b.translation.x);
        assert!(t.rotation.angle_between(Quat::IDENTITY) > 0.5);
        assert!(
            animated_transform(b, Motion::Pedal { right: true }, &s)
                .translation
                .x
                > b.translation.x
        );
        assert!(
            animated_transform(b, Motion::Pedal { right: false }, &s)
                .translation
                .x
                < b.translation.x
        );
    }
}
