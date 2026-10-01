//! Offline WGS84 geoid undulation from a local GeographicLib PGM grid.
//!
//! Format and sign convention: <https://geographiclib.sourceforge.io/C++/doc/geoid.html>.
//! Samples are 16-bit big-endian, north to south from 90N, and eastwards from
//! 0E with a periodic longitude seam. `N = Offset + Scale * sample`, in metres.
//! Orthometric DEM height `H` becomes ellipsoidal height `h = H + N`.
//!
//! This reader uses bilinear interpolation, not GeographicLib's default cubic
//! interpolation. No grid is downloaded or bundled. The caller explicitly names
//! EGM2008 or EGM96; the grid's Description must agree. Mean sea level with no
//! identified model is never silently mapped to either of them.

use crate::vertical_datum::GeoidModel;
use flightsim_core::{Geodetic, Meters};
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

const MAX_HEADER_BYTES: u64 = 64 * 1024;
const MAX_GRID_BYTES: usize = 512 * 1024 * 1024;

/// Validated global geoid grid. Only the offline tool holds this data.
#[derive(Debug)]
pub struct GeoidGrid {
    model: GeoidModel,
    width: u32,
    height: u32,
    offset: f64,
    scale: f64,
    samples: Vec<u16>,
    description: String,
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl GeoidGrid {
    /// Read a user-supplied GeographicLib PGM grid. No network access occurs.
    ///
    /// # Errors
    /// File errors, malformed headers, model mismatch, oversized/truncated data,
    /// trailing bytes, or non-constant pole rows are rejected.
    pub fn open(path: &Path, model: GeoidModel) -> io::Result<Self> {
        Self::read(BufReader::new(std::fs::File::open(path)?), model)
    }

    /// Decode an already-open grid with the same checks as [`Self::open`].
    ///
    /// # Errors
    /// See [`Self::open`].
    pub fn read(mut reader: impl BufRead, model: GeoidModel) -> io::Result<Self> {
        if model == GeoidModel::UnspecifiedMeanSeaLevel {
            return Err(invalid(
                "a geoid grid needs an explicit EGM2008 or EGM96 model",
            ));
        }
        let mut header_bytes = 0_u64;
        let mut tokens = Vec::new();
        let mut offset = None;
        let mut scale = None;
        let mut description = None;
        while tokens.len() < 4 {
            let mut line = Vec::new();
            let length = reader
                .by_ref()
                .take(MAX_HEADER_BYTES - header_bytes + 1)
                .read_until(b'\n', &mut line)?;
            header_bytes += length as u64;
            if length == 0 || header_bytes > MAX_HEADER_BYTES || !line.ends_with(b"\n") {
                return Err(invalid("truncated or oversized GeographicLib PGM header"));
            }
            let text = std::str::from_utf8(&line)
                .map_err(|_| invalid("non-UTF-8 PGM header"))?
                .trim();
            if let Some(comment) = text.strip_prefix('#') {
                let mut fields = comment.split_whitespace();
                let key = fields.next().unwrap_or("");
                let value = fields.collect::<Vec<_>>().join(" ");
                match key {
                    "Offset" => set_number(&mut offset, &value, "Offset")?,
                    "Scale" => set_number(&mut scale, &value, "Scale")?,
                    "Description" => {
                        if description.replace(value).is_some() {
                            return Err(invalid("duplicate geoid Description"));
                        }
                    }
                    "Origin" if value != "90N 0E" => {
                        return Err(invalid("geoid origin must be 90N 0E"));
                    }
                    "AREA_OR_POINT" if value != "Point" => {
                        return Err(invalid("geoid grid must use point samples"));
                    }
                    "Vertical_Datum" if value != "WGS84" => {
                        return Err(invalid("geoid ellipsoid must be WGS84"));
                    }
                    _ => {}
                }
            } else {
                tokens.extend(text.split_whitespace().map(str::to_owned));
            }
        }
        if tokens.len() != 4 || tokens[0] != "P5" || tokens[3] != "65535" {
            return Err(invalid("expected GeographicLib P5/16-bit PGM header"));
        }
        let width = tokens[1]
            .parse::<u32>()
            .map_err(|_| invalid("invalid geoid width"))?;
        let height = tokens[2]
            .parse::<u32>()
            .map_err(|_| invalid("invalid geoid height"))?;
        if height < 3 || width != height.saturating_sub(1).saturating_mul(2) {
            return Err(invalid(
                "global geoid requires width = 2 * (height - 1), height >= 3",
            ));
        }
        let count = (width as usize)
            .checked_mul(height as usize)
            .filter(|count| *count <= MAX_GRID_BYTES / 2)
            .ok_or_else(|| invalid("geoid grid exceeds 512 MiB limit"))?;
        let offset = offset.ok_or_else(|| invalid("missing geoid Offset"))?;
        let scale = scale.ok_or_else(|| invalid("missing geoid Scale"))?;
        if scale <= 0.0 || !(offset + scale * f64::from(u16::MAX)).is_finite() {
            return Err(invalid(
                "geoid Scale must be positive with finite decoded heights",
            ));
        }
        let description = description.ok_or_else(|| invalid("missing geoid Description"))?;
        if !description.starts_with(&format!("WGS84 {},", model.name())) {
            return Err(invalid(format!(
                "geoid Description {description:?} does not identify WGS84 {}",
                model.name()
            )));
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(count)
            .map_err(|_| invalid("could not allocate geoid grid"))?;
        // Stream rows rather than retaining a second grid-sized byte buffer.
        let mut row = vec![0_u8; width as usize * 2];
        for _ in 0..height {
            reader.read_exact(&mut row)?;
            samples.extend(
                row.chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]])),
            );
        }
        if reader.read(&mut [0_u8; 1])? != 0 {
            return Err(invalid("trailing data after geoid grid"));
        }
        for row in [0, height as usize - 1] {
            let pole = &samples[row * width as usize..(row + 1) * width as usize];
            if !pole.iter().all(|sample| *sample == pole[0]) {
                return Err(invalid("geoid pole rows must be independent of longitude"));
            }
        }
        Ok(Self {
            model,
            width,
            height,
            offset,
            scale,
            samples,
            description,
        })
    }

    /// The explicit geoid model, checked against the file description.
    #[must_use]
    pub const fn model(&self) -> GeoidModel {
        self.model
    }

    /// Producer's model/resolution description for provenance.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Grid dimensions (longitude columns, latitude rows).
    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Bilinearly interpolate `N`, the geoid height above the WGS84 ellipsoid.
    /// Returns `None` for non-finite or out-of-range latitude/longitude input.
    #[must_use]
    pub fn undulation(&self, position: Geodetic) -> Option<Meters> {
        let latitude = position.latitude.to_degrees().get();
        let longitude = position.longitude.to_degrees().get();
        if !latitude.is_finite() || !longitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            return None;
        }
        let x = longitude.rem_euclid(360.0) / 360.0 * f64::from(self.width);
        let y = ((90.0 - latitude) / 180.0 * f64::from(self.height - 1))
            .clamp(0.0, f64::from(self.height - 1));
        // The finite, bounded grid coordinates above fit both u32 and usize.
        #[allow(clippy::cast_possible_truncation)]
        let (column, row) = (
            x.floor() as u32 % self.width,
            (y.floor() as u32).min(self.height - 2),
        );
        let (fx, fy) = (x - x.floor(), y - f64::from(row));
        let next = (column + 1) % self.width;
        let value = |column: u32, row: u32| {
            f64::from(self.samples[row as usize * self.width as usize + column as usize])
        };
        let north = (1.0 - fx) * value(column, row) + fx * value(next, row);
        let south = (1.0 - fx) * value(column, row + 1) + fx * value(next, row + 1);
        Some(Meters(
            self.offset + self.scale * ((1.0 - fy) * north + fy * south),
        ))
    }
}

fn set_number(slot: &mut Option<f64>, text: &str, field: &str) -> io::Result<()> {
    let value = text
        .parse::<f64>()
        .map_err(|_| invalid(format!("invalid geoid {field}")))?;
    if !value.is_finite() || slot.replace(value).is_some() {
        return Err(invalid(format!("non-finite or duplicate geoid {field}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Radians;
    use std::io::Cursor;

    fn pgm(header: &str, pixels: &[u16]) -> Vec<u8> {
        [
            header.as_bytes().to_vec(),
            pixels
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect(),
        ]
        .concat()
    }

    fn fixture() -> Vec<u8> {
        pgm(
            "P5\n# Description WGS84 EGM2008, synthetic grid\n# Offset -10\n# Scale 0.5\n4 3\n65535\n",
            &[20, 20, 20, 20, 0, 20, 40, 60, 40, 40, 40, 40],
        )
    }

    fn grid() -> GeoidGrid {
        GeoidGrid::read(Cursor::new(fixture()), GeoidModel::Egm2008).expect("grid")
    }

    fn sample(grid: &GeoidGrid, lat: f64, lon: f64) -> f64 {
        grid.undulation(Geodetic::from_degrees(lat, lon, 0.0))
            .expect("valid coordinate")
            .get()
    }

    #[test]
    fn independently_known_values_verify_offset_scale_endian_and_sign() {
        let grid = grid();
        for (latitude, longitude, expected) in [
            (0.0, 0.0, -10.0),
            (0.0, 90.0, 0.0),
            (0.0, 180.0, 10.0),
            (0.0, -90.0, 20.0),
            (45.0, 45.0, -2.5),
        ] {
            assert!((sample(&grid, latitude, longitude) - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn longitude_is_periodic_and_poles_do_not_depend_on_it() {
        let grid = grid();
        assert!((sample(&grid, 0.0, 180.0) - sample(&grid, 0.0, -180.0)).abs() < 1e-12);
        assert!((sample(&grid, 0.0, -45.0) - 5.0).abs() < 1e-12);
        for lon in [-180.0, -70.0, 0.0, 135.0, 180.0] {
            assert!(sample(&grid, 90.0, lon).abs() < 1e-12);
            assert!((sample(&grid, -90.0, lon) - 10.0).abs() < 1e-12);
        }
    }

    #[test]
    fn malformed_headers_and_payloads_are_rejected() {
        let original = fixture();
        let boundary = original.len() - 24;
        let header = std::str::from_utf8(&original[..boundary]).expect("header");
        for (from, to) in [
            ("P5", "P2"),
            ("65535", "255"),
            ("4 3", "4294967295 4294967295"),
            ("4 3", "4 4"),
            ("Scale 0.5", "Scale NaN"),
            ("Scale 0.5", "Scale -1"),
            ("Offset -10", "Offset inf"),
            ("Offset -10", "Other -10"),
            ("Offset -10", "Offset -10\n# Offset -10"),
            ("EGM2008,", "EGM96,"),
        ] {
            let bytes = [
                header.replace(from, to).into_bytes(),
                original[boundary..].to_vec(),
            ]
            .concat();
            assert!(
                GeoidGrid::read(Cursor::new(bytes), GeoidModel::Egm2008).is_err(),
                "{to}"
            );
        }
        for length in [0, 1, boundary - 1, original.len() - 1] {
            assert!(
                GeoidGrid::read(Cursor::new(&original[..length]), GeoidModel::Egm2008).is_err()
            );
        }
        let mut extra = original.clone();
        extra.push(0);
        assert!(GeoidGrid::read(Cursor::new(extra), GeoidModel::Egm2008).is_err());
        let mut pole = original;
        pole[boundary] ^= 1;
        assert!(GeoidGrid::read(Cursor::new(pole), GeoidModel::Egm2008).is_err());
    }

    #[test]
    fn a_header_without_newlines_is_bounded() {
        assert!(GeoidGrid::read(Cursor::new(vec![b'X'; 70_000]), GeoidModel::Egm2008).is_err());
    }

    #[test]
    fn invalid_positions_return_no_sample() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut position = Geodetic::from_degrees(0.0, 0.0, 0.0);
            position.latitude = Radians(value);
            assert!(grid().undulation(position).is_none());
            position.latitude = Radians::ZERO;
            position.longitude = Radians(value);
            assert!(grid().undulation(position).is_none());
        }
    }
}
