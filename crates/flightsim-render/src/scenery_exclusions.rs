//! Conservative visual precedence for known airport pavement. This removes
//! optional ground depiction near pavement; it never edits source geography or
//! physical collision heights. Small extra clearing at triangle edges is intentional.
use flightsim_core::{Ecef, Geodetic, Meters};
use glam::DVec3;

const MAX_SHAPES: usize = 16_384;

/// Short surface-projected chords for an authored horizontal line. A single
/// long ECEF chord passes below Earth and is not a pavement/tree exclusion.
/// Bounded500m pieces have less than6mm Earth-curvature sag at ground level.
#[must_use]
pub fn horizontal_surface_segments(
    a: Geodetic,
    b: Geodetic,
    limit: usize,
) -> Option<Vec<(Ecef, Ecef)>> {
    if !valid(a) || !valid(b) {
        return None;
    }
    let a = horizontal(a);
    let b = horizontal(b);
    let delta = b - a;
    let count = (delta.length() / 500.0).ceil().max(1.0);
    let limit = limit.min(MAX_SHAPES);
    if count > f64::from(u32::try_from(limit).ok()?) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite ceiled count checked against16384 before narrowing"
    )]
    let count = count as u32;
    let mut result = Vec::new();
    result.try_reserve_exact(count as usize).ok()?;
    let mut previous = Ecef::from_vec(a);
    for step in 1..=count {
        let p = Ecef::from_vec(a + delta * (f64::from(step) / f64::from(count))).to_geodetic();
        let next = Ecef::from_vec(horizontal(p));
        result.push((previous, next));
        previous = next;
    }
    Some(result)
}

#[derive(Debug)]
enum Shape {
    Segment(DVec3, DVec3, f64),
    Disc(DVec3, f64),
}

#[derive(Debug)]
pub struct GroundSceneryExclusions {
    shapes: Vec<Shape>,
    minimum: DVec3,
    maximum: DVec3,
    suppressed: bool,
}
impl Default for GroundSceneryExclusions {
    fn default() -> Self {
        Self {
            shapes: Vec::new(),
            minimum: DVec3::splat(f64::INFINITY),
            maximum: DVec3::splat(f64::NEG_INFINITY),
            suppressed: false,
        }
    }
}
impl GroundSceneryExclusions {
    /// Fail closed if malformed or excessive airport source data prevents a
    /// complete mask. Normal app inputs have already passed airport validation.
    pub fn add_segment(&mut self, a: Geodetic, b: Geodetic, half_width: Meters) {
        if !valid(a)
            || !valid(b)
            || !half_width.is_finite()
            || !(0.0..=1_000.0).contains(&half_width.get())
        {
            self.suppressed = true;
            return;
        }
        let radius = half_width.get() + 2.0;
        let Some(segments) =
            horizontal_surface_segments(a, b, MAX_SHAPES.saturating_sub(self.shapes.len()))
        else {
            self.suppressed = true;
            return;
        };
        for (a, b) in segments {
            let a = a.as_vec();
            let b = b.as_vec();
            self.add(
                Shape::Segment(a, b, radius),
                a.min(b) - DVec3::splat(radius),
                a.max(b) + DVec3::splat(radius),
            );
        }
    }
    /// A disc around a known pavement triangle is conservative, including
    /// triangle edges and holes. Optional ground may be cleared beyond its edge.
    pub fn add_polygon(&mut self, points: &[Geodetic]) {
        if points.len() < 3 || points.len() > 512 || points.iter().any(|&p| !valid(p)) {
            self.suppressed = true;
            return;
        }
        let count = u32::try_from(points.len()).expect("bounded polygon");
        let center = points.iter().map(|&p| horizontal(p)).sum::<DVec3>() / f64::from(count);
        let radius = points
            .iter()
            .map(|&p| horizontal(p).distance(center))
            .fold(0.0, f64::max)
            + 2.0;
        self.add(
            Shape::Disc(center, radius),
            center - DVec3::splat(radius),
            center + DVec3::splat(radius),
        );
    }
    fn add(&mut self, shape: Shape, minimum: DVec3, maximum: DVec3) {
        if self.shapes.len() >= MAX_SHAPES {
            self.suppressed = true;
            return;
        }
        self.minimum = self.minimum.min(minimum);
        self.maximum = self.maximum.max(maximum);
        self.shapes.push(shape);
    }
    #[must_use]
    pub const fn is_suppressed(&self) -> bool {
        self.suppressed
    }
    /// Full-triangle conservative test, not merely vertex/centroid containment.
    /// Each narrow predicate consumes the supplied shared batch work allowance.
    pub fn intersects_triangle(&self, points: [Ecef; 3], remaining: &mut usize) -> bool {
        if self.suppressed {
            return true;
        }
        if self.shapes.is_empty() {
            return false;
        }
        let p = points.map(|p| horizontal(p.to_geodetic()));
        if p.iter().any(|p| !p.is_finite()) {
            return true;
        }
        let minimum = p[0].min(p[1]).min(p[2]);
        let maximum = p[0].max(p[1]).max(p[2]);
        if maximum.cmplt(self.minimum).any() || minimum.cmpgt(self.maximum).any() {
            return false;
        }
        let center = (p[0] + p[1] + p[2]) / 3.0;
        let radius = p.iter().map(|p| p.distance(center)).fold(0.0, f64::max) + 0.05;
        for shape in &self.shapes {
            if *remaining == 0 {
                return true;
            }
            *remaining -= 1;
            let (distance, width) = match *shape {
                Shape::Disc(point, width) => (point.distance(center), width),
                Shape::Segment(a, b, width) => {
                    let d = b - a;
                    let t = if d.length_squared() > 0.0 {
                        ((center - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    ((a + d * t).distance(center), width)
                }
            };
            if distance <= radius + width {
                return true;
            }
        }
        false
    }
}
fn valid(p: Geodetic) -> bool {
    p.latitude.is_finite()
        && p.longitude.is_finite()
        && p.altitude.is_finite()
        && p.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && p.longitude.get().abs() <= core::f64::consts::PI
}
fn horizontal(p: Geodetic) -> DVec3 {
    Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
        .to_ecef()
        .as_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{LocalFrame, Ned};
    fn point(n: f64, e: f64) -> Geodetic {
        LocalFrame::new(Geodetic::from_degrees(47.1, 9.5, 0.0))
            .ned_to_ecef_position(Ned::new(n, e, -500.0))
            .to_geodetic()
    }
    #[test]
    fn crossing_triangle_is_excluded_even_when_no_vertex_is_on_the_runway() {
        let mut mask = GroundSceneryExclusions::default();
        mask.add_segment(point(-500.0, 0.0), point(500.0, 0.0), Meters(20.0));
        let p = [
            point(-20.0, -100.0),
            point(-20.0, 100.0),
            point(50.0, 100.0),
        ]
        .map(Geodetic::to_ecef);
        assert!(mask.intersects_triangle(p, &mut 100));
        assert!(!mask.intersects_triangle(
            [point(0.0, 200.0), point(20.0, 210.0), point(0.0, 220.0)].map(Geodetic::to_ecef),
            &mut 100
        ));
    }
    #[test]
    fn apron_mask_and_work_exhaustion_fail_closed_near_airport_only() {
        let mut mask = GroundSceneryExclusions::default();
        mask.add_polygon(&[point(0.0, 0.0), point(100.0, 0.0), point(0.0, 100.0)]);
        let near = [point(5.0, 5.0), point(10.0, 5.0), point(5.0, 10.0)].map(Geodetic::to_ecef);
        assert!(mask.intersects_triangle(near, &mut 100));
        assert!(mask.intersects_triangle(near, &mut 0));
        assert!(
            !mask.intersects_triangle(
                [
                    point(1000.0, 1000.0),
                    point(1010.0, 1000.0),
                    point(1000.0, 1010.0)
                ]
                .map(Geodetic::to_ecef),
                &mut 0
            )
        );
    }
}
