//! Original procedural artwork, with a small independently authored bitmap font.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "bounded 256-pixel display rasterization"
)]
use super::*;
use bevy::{
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
const S: i32 = 256;
const INK: [u8; 4] = [221, 229, 213, 255];
const DARK: [u8; 4] = [12, 18, 21, 255];
const AMBER: [u8; 4] = [245, 191, 94, 255];
struct Canvas {
    data: Vec<u8>,
}
impl Canvas {
    fn new(color: [u8; 4]) -> Self {
        Self {
            data: color.repeat((S * S) as usize),
        }
    }
    fn dot(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if (0..S).contains(&x) && (0..S).contains(&y) {
            let p = ((y * S + x) * 4) as usize;
            self.data[p..p + 4].copy_from_slice(&color);
        }
    }
    fn line(&mut self, a: (f32, f32), b: (f32, f32), color: [u8; 4], width: i32) {
        let steps = ((a.0 - b.0).abs().max((a.1 - b.1).abs()).ceil() as i32).max(1);
        for n in 0..=steps {
            let t = n as f32 / steps as f32;
            let x = (a.0 + (b.0 - a.0) * t).round() as i32;
            let y = (a.1 + (b.1 - a.1) * t).round() as i32;
            for dy in -width / 2..=width / 2 {
                for dx in -width / 2..=width / 2 {
                    self.dot(x + dx, y + dy, color);
                }
            }
        }
    }
    fn text(&mut self, text: &str, x: i32, y: i32, scale: i32, color: [u8; 4]) {
        let mut x = x;
        let start = x;
        let mut y = y;
        for c in text.chars() {
            if c == '\n' {
                x = start;
                y += 9 * scale;
                continue;
            }
            for (r, bits) in glyph(c).into_iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.dot(x + col * scale + dx, y + r as i32 * scale + dy, color);
                            }
                        }
                    }
                }
            }
            x += 6 * scale;
        }
    }
    fn centered(&mut self, text: &str, y: i32, scale: i32, color: [u8; 4]) {
        self.text(
            text,
            (S - text.len() as i32 * 6 * scale) / 2,
            y,
            scale,
            color,
        );
    }
    fn tick(&mut self, angle: f32, inner: f32, outer: f32, color: [u8; 4], width: i32) {
        let a = angle.to_radians();
        self.line(
            (128.0 + a.sin() * inner, 128.0 - a.cos() * inner),
            (128.0 + a.sin() * outer, 128.0 - a.cos() * outer),
            color,
            width,
        );
    }
    fn number(&mut self, text: &str, angle: f32, radius: f32) {
        let a = angle.to_radians();
        self.text(
            text,
            (128.0 + a.sin() * radius) as i32 - text.len() as i32 * 9,
            (128.0 - a.cos() * radius) as i32 - 10,
            3,
            INK,
        );
    }
}
#[must_use]
pub fn display_image(display: Display, state: &CockpitState) -> Image {
    let mut c = Canvas::new(DARK);
    match display {
        Display::Dial(dial) => draw_dial(&mut c, dial, state),
        Display::Flight => {
            c.text("FLIGHT DATA", 10, 10, 3, AMBER);
            let lines = [
                format!(
                    "GS  {:03.0} KT",
                    finite(state.ground_speed.to_knots().get())
                ),
                format!("AGL {:05.0} FT", finite(state.agl.to_feet().get())),
                format!(
                    "WIND {:03.0}/{:02.0}",
                    finite(state.wind_from.get()).to_degrees().rem_euclid(360.0),
                    finite(state.wind_speed.to_knots().get())
                ),
                format!("ALT {:06.0} FT", finite(state.altitude.to_feet().get())),
            ];
            for (i, line) in lines.iter().enumerate() {
                c.text(line, 10, 57 + i as i32 * 39, 2, INK);
            }
            c.text(
                if state.stall {
                    "STALL WARNING"
                } else if state.stall_unavailable {
                    "STALL WARN N/A"
                } else if state.replay {
                    "REPLAY / READ ONLY"
                } else if state.on_ground {
                    "GROUND"
                } else {
                    "AIRBORNE"
                },
                10,
                226,
                2,
                if state.stall {
                    [255, 90, 55, 255]
                } else {
                    AMBER
                },
            );
        }
        Display::Controls => {
            c.text("CONTROL STATE", 10, 10, 3, AMBER);
            let lines = [
                format!("THROT {:03.0}%", state.controls.throttle() * 100.0),
                format!("FLAPS {:03.0}%", state.controls.flaps() * 100.0),
                state.trim.map_or_else(
                    || "TRIM  N/A REPLAY".to_owned(),
                    |t| format!("TRIM  {:+.0}%", t * 100.0),
                ),
                format!("BRAKE {:03.0}%", state.controls.brakes() * 100.0),
            ];
            for (i, line) in lines.iter().enumerate() {
                c.text(line, 10, 58 + i as i32 * 40, 2, INK);
            }
            c.text(
                if state.replay {
                    "YOKE = EFFECTIVE CMD"
                } else {
                    "DRAG KNOBS / YOKES"
                },
                10,
                226,
                2,
                AMBER,
            );
        }
        Display::Label(text) => {
            c = Canvas::new([20, 25, 28, 255]);
            let lines: Vec<_> = text.split('\n').collect();
            let longest = lines.iter().map(|l| l.len()).max().unwrap_or(1);
            let scale = (240 / (longest as i32 * 6)).clamp(1, 4);
            let top = (S - lines.len() as i32 * 9 * scale) / 2;
            for (i, line) in lines.iter().enumerate() {
                c.centered(line, top + i as i32 * 9 * scale, scale, INK);
            }
        }
    }
    let (width, height) = if let Display::Label(text) = display {
        let lines: Vec<_> = text.split('\n').collect();
        let longest = lines.iter().map(|line| line.len()).max().unwrap_or(1);
        let scale = (240 / (longest as i32 * 6)).clamp(1, 4);
        (
            longest as i32 * 6 * scale + 8,
            lines.len() as i32 * 9 * scale + 8,
        )
    } else {
        (S, S)
    };
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in (S - height) / 2..(S - height) / 2 + height {
        let start = (y * S * 4 + (S - width) / 2 * 4) as usize;
        data.extend_from_slice(&c.data[start..start + (width * 4) as usize]);
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}
fn draw_dial(c: &mut Canvas, dial: Dial, state: &CockpitState) {
    match dial {
        Dial::Attitude => {
            let roll = finite(state.roll.get()) as f32;
            let offset = finite(state.pitch.get()).to_degrees().clamp(-90.0, 90.0) as f32 * 2.3;
            for y in 0..S {
                for x in 0..S {
                    let h =
                        (x as f32 - 128.0) * roll.sin() + (y as f32 - 128.0) * roll.cos() - offset;
                    c.dot(
                        x,
                        y,
                        if h < 0.0 {
                            [56, 125, 169, 255]
                        } else {
                            [124, 77, 44, 255]
                        },
                    );
                    if h.abs() < 1.0 {
                        c.dot(x, y, INK);
                    }
                }
            }
            for pitch in [-30, -20, -10, 10, 20, 30] {
                let v = offset - pitch as f32 * 2.3;
                let w = if pitch % 20 == 0 { 32.0 } else { 19.0 };
                let map = |u: f32| {
                    (
                        128.0 + u * roll.cos() + v * roll.sin(),
                        128.0 - u * roll.sin() + v * roll.cos(),
                    )
                };
                c.line(map(-w), map(w), INK, 2);
            }
            c.line((54.0, 128.0), (107.0, 128.0), AMBER, 4);
            c.line((149.0, 128.0), (202.0, 128.0), AMBER, 4);
            c.line((107.0, 128.0), (118.0, 142.0), AMBER, 4);
            c.line((149.0, 128.0), (138.0, 142.0), AMBER, 4);
            for a in [-60.0, -30.0, 0.0, 30.0, 60.0] {
                c.tick(a, 105.0, 117.0, INK, 2);
            }
            c.centered("ATTITUDE", 209, 2, INK);
        }
        Dial::Heading => {
            let heading = finite(state.heading.get()).to_degrees() as f32;
            for i in 0..72 {
                let a = i as f32 * 5.0 - heading;
                c.tick(
                    a,
                    if i % 6 == 0 { 94.0 } else { 106.0 },
                    118.0,
                    INK,
                    if i % 6 == 0 { 3 } else { 1 },
                );
                if i % 6 == 0 {
                    let text = match i {
                        0 => "N".to_owned(),
                        18 => "E".to_owned(),
                        36 => "S".to_owned(),
                        54 => "W".to_owned(),
                        _ => format!("{}", i * 5 / 10),
                    };
                    c.number(&text, a, 80.0);
                }
            }
            c.line((128.0, 12.0), (120.0, 31.0), AMBER, 3);
            c.line((128.0, 12.0), (136.0, 31.0), AMBER, 3);
            c.centered("TRUE HDG", 112, 2, INK);
            c.centered(
                &format!("{:03.0}", heading.rem_euclid(360.0)),
                137,
                3,
                AMBER,
            );
        }
        Dial::Airspeed => {
            for n in 0..=40 {
                let a = -150.0 + n as f32 * 7.5;
                c.tick(
                    a,
                    if n % 4 == 0 { 94.0 } else { 104.0 },
                    117.0,
                    INK,
                    if n % 4 == 0 { 3 } else { 1 },
                );
                if n % 8 == 0 {
                    c.number(&format!("{}", n * 5), a, 78.0);
                }
            }
            c.centered("EAS", 78, 2, INK);
            c.centered("KNOTS", 161, 2, INK);
        }
        Dial::Altitude => {
            for n in 0..50 {
                let a = n as f32 * 7.2;
                c.tick(
                    a,
                    if n % 5 == 0 { 95.0 } else { 105.0 },
                    117.0,
                    INK,
                    if n % 5 == 0 { 3 } else { 1 },
                );
                if n % 5 == 0 {
                    c.number(&format!("{}", n / 5), a, 80.0);
                }
            }
            c.centered("ALT", 80, 2, INK);
            c.centered("100 / 1000 FT", 166, 1, INK);
            c.centered("ELLIPSOID", 186, 1, AMBER);
        }
        Dial::VerticalSpeed => {
            for n in -20i32..=20 {
                let a = -90.0 + n as f32 * 6.75;
                c.tick(
                    a,
                    if n % 5 == 0 { 95.0 } else { 106.0 },
                    117.0,
                    INK,
                    if n % 5 == 0 { 3 } else { 1 },
                );
                if n % 5 == 0 {
                    c.number(&format!("{}", n.abs()), a, 77.0);
                }
            }
            c.centered("VSI", 88, 2, INK);
            c.centered("X100 FPM", 161, 2, INK);
            c.text("UP", 169, 119, 1, AMBER);
            c.text("DN", 169, 139, 1, AMBER);
        }
        Dial::Turn => {
            for n in -6i32..=6 {
                c.tick(
                    n as f32 * 7.5,
                    if n % 3 == 0 { 83.0 } else { 94.0 },
                    107.0,
                    INK,
                    2,
                );
                if n % 3 == 0 {
                    c.number(&format!("{}", n.abs()), n as f32 * 7.5, 65.0);
                }
            }
            c.centered("BODY YAW", 154, 2, INK);
            c.centered("DEG/SEC", 181, 2, INK);
            c.text("L", 37, 104, 2, AMBER);
            c.text("R", 207, 104, 2, AMBER);
        }
        Dial::Power | Dial::Flaps => {
            for n in 0..=20 {
                let a = -135.0 + n as f32 * 13.5;
                c.tick(a, if n % 5 == 0 { 94.0 } else { 106.0 }, 117.0, INK, 2);
                if n % 5 == 0 {
                    c.number(&format!("{}", n * 5), a, 77.0);
                }
            }
            c.centered(
                if dial == Dial::Power {
                    "THROT"
                } else {
                    "FLAPS"
                },
                85,
                2,
                INK,
            );
            c.centered("PERCENT", 166, 2, INK);
        }
    }
}
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 27, 17],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        ':' => [0, 6, 6, 0, 6, 6, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        _ => [0; 7],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nose_up_puts_more_sky_in_the_aperture() {
        let image = display_image(
            Display::Dial(Dial::Attitude),
            &CockpitState {
                pitch: Radians(20f64.to_radians()),
                ..default()
            },
        );
        let p = ((160 * 256 + 128) * 4) as usize;
        assert_eq!(&image.data.unwrap()[p..p + 3], &[56, 125, 169]);
    }
    #[test]
    fn complete_display_images() {
        for d in [
            Display::Flight,
            Display::Controls,
            Display::Label("TRIM"),
            Display::Dial(Dial::Heading),
        ] {
            let image = display_image(d, &CockpitState::default());
            assert_eq!(
                image.data.as_ref().unwrap().len(),
                image.texture_descriptor.size.width as usize
                    * image.texture_descriptor.size.height as usize
                    * 4
            );
        }
    }
}
