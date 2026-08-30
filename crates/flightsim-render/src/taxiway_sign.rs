//! 待機位置に置く物理標識。
//!
//! 外部フォントへ依存せず、3x5 の ASCII ビットマップを小さな矩形へ展開する。
//! OSM の任意 Unicode を代替文字へ化けさせると標識を読み違えるため、未対応文字は
//! 標識全体を省略する。

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, Meters, Radians};

pub const MAX_SIGN_REF_CHARS: usize = 8;

const BOARD_BOTTOM: f64 = 0.48;
const BOARD_HEIGHT: f64 = 0.92;
const PANEL_PADDING: f64 = 0.14;
const CELL: f64 = 0.12;
const PIXEL: f64 = 0.095;
const GLYPH_COLUMNS: f64 = 3.0;
const GLYPH_ROWS: f64 = 5.0;
const PANEL_GAP: f64 = 0.05;
const GLYPH_LIFT: f64 = 0.018;
const SIGN_GROUND_LIFT: f64 = 0.04;

const TAXI_BACKGROUND: [f32; 3] = [0.015, 0.015, 0.012];
const TAXI_TEXT: [f32; 3] = [1.0, 0.72, 0.03];
const HOLD_BACKGROUND: [f32; 3] = [0.66, 0.015, 0.02];
const HOLD_TEXT: [f32; 3] = [1.0, 0.98, 0.93];
const POST_COLOR: [f32; 3] = [0.22, 0.22, 0.20];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxiwaySignMeshError {
    InvalidPosition,
    InvalidFacing,
    AllocationFailed,
    TooComplex,
}

impl core::fmt::Display for TaxiwaySignMeshError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPosition => "taxiway sign position is invalid",
            Self::InvalidFacing => "taxiway sign facing is invalid",
            Self::AllocationFailed => "could not allocate taxiway sign mesh",
            Self::TooComplex => "taxiway sign exceeds the mesh limit",
        })
    }
}

impl std::error::Error for TaxiwaySignMeshError {}

/// 黒地／黄文字の誘導路 ref と、赤地／白文字の待機 ref を一枚の標識にする。
///
/// `facing` は標識正面が向く真方位。空文字、8文字超、非 ASCII、未収録文字を含む
/// ref は、誤読可能な代替表示をせず `Ok(None)` とする。
pub fn holding_position_sign_mesh(
    position: Geodetic,
    facing: Radians,
    taxiway_ref: &str,
    holding_ref: &str,
) -> Result<Option<(Mesh, Ecef)>, TaxiwaySignMeshError> {
    if !valid_point(position) {
        return Err(TaxiwaySignMeshError::InvalidPosition);
    }
    if !facing.is_finite() {
        return Err(TaxiwaySignMeshError::InvalidFacing);
    }
    let Some(taxiway) = validated_ref(taxiway_ref) else {
        return Ok(None);
    };
    let Some(holding) = validated_ref(holding_ref) else {
        return Ok(None);
    };

    let taxi_width = panel_width(taxiway.len());
    let hold_width = panel_width(holding.len());
    let total_width = taxi_width + PANEL_GAP + hold_width;
    let taxi_left = -total_width * 0.5;
    let hold_left = taxi_left + taxi_width + PANEL_GAP;

    let lit_pixels = taxiway
        .iter()
        .chain(holding.iter())
        .map(|byte| {
            glyph(*byte)
                .expect("validated glyph")
                .map(u8::count_ones)
                .iter()
                .sum::<u32>()
        })
        .try_fold(0_usize, |total, pixels| {
            total.checked_add(usize::try_from(pixels).ok()?)
        })
        .ok_or(TaxiwaySignMeshError::TooComplex)?;
    let quad_count = 4_usize
        .checked_add(lit_pixels)
        .ok_or(TaxiwaySignMeshError::TooComplex)?;
    let origin = lifted(position, SIGN_GROUND_LIFT).to_ecef();
    let mut builder = SignMeshBuilder::new(origin, facing, quad_count)?;

    // 2 本の脚。盤面より奥へ置き、文字との z-fight を避ける。
    for x in [-total_width * 0.32, total_width * 0.32] {
        builder.rect(
            position,
            x - 0.035,
            x + 0.035,
            SIGN_GROUND_LIFT,
            BOARD_BOTTOM + 0.08,
            -0.012,
            POST_COLOR,
        )?;
    }
    builder.rect(
        position,
        taxi_left,
        taxi_left + taxi_width,
        BOARD_BOTTOM,
        BOARD_BOTTOM + BOARD_HEIGHT,
        0.0,
        TAXI_BACKGROUND,
    )?;
    builder.rect(
        position,
        hold_left,
        hold_left + hold_width,
        BOARD_BOTTOM,
        BOARD_BOTTOM + BOARD_HEIGHT,
        0.0,
        HOLD_BACKGROUND,
    )?;
    builder.text(position, taxi_left, taxiway, TAXI_TEXT)?;
    builder.text(position, hold_left, holding, HOLD_TEXT)?;

    Ok(Some((builder.build(), origin)))
}

fn validated_ref(value: &str) -> Option<&[u8]> {
    if value.is_empty() || !value.is_ascii() || value.len() > MAX_SIGN_REF_CHARS {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes.iter().all(|byte| glyph(*byte).is_some()) {
        Some(bytes)
    } else {
        None
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "標識 ref は MAX_SIGN_REF_CHARS (8) 以下へ検査済み"
)]
fn panel_width(characters: usize) -> f64 {
    characters as f64 * GLYPH_COLUMNS * CELL
        + characters.saturating_sub(1) as f64 * CELL
        + PANEL_PADDING * 2.0
}

struct SignMeshBuilder {
    origin: Ecef,
    facing: Radians,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl SignMeshBuilder {
    fn new(origin: Ecef, facing: Radians, quads: usize) -> Result<Self, TaxiwaySignMeshError> {
        let vertices = quads
            .checked_mul(4)
            .ok_or(TaxiwaySignMeshError::TooComplex)?;
        let indices = quads
            .checked_mul(6)
            .ok_or(TaxiwaySignMeshError::TooComplex)?;
        let mut result = Self {
            origin,
            facing,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        };
        result
            .positions
            .try_reserve_exact(vertices)
            .map_err(|_| TaxiwaySignMeshError::AllocationFailed)?;
        result
            .normals
            .try_reserve_exact(vertices)
            .map_err(|_| TaxiwaySignMeshError::AllocationFailed)?;
        result
            .colors
            .try_reserve_exact(vertices)
            .map_err(|_| TaxiwaySignMeshError::AllocationFailed)?;
        result
            .indices
            .try_reserve_exact(indices)
            .map_err(|_| TaxiwaySignMeshError::AllocationFailed)?;
        Ok(result)
    }

    fn text(
        &mut self,
        position: Geodetic,
        panel_left: f64,
        text: &[u8],
        color: [f32; 3],
    ) -> Result<(), TaxiwaySignMeshError> {
        let glyph_height = GLYPH_ROWS * CELL;
        let bottom = BOARD_BOTTOM + (BOARD_HEIGHT - glyph_height) * 0.5;
        let mut left = panel_left + PANEL_PADDING;
        for byte in text {
            let rows = glyph(*byte).expect("text was validated");
            for (row, bits) in rows.into_iter().enumerate() {
                for column in 0..3 {
                    if bits & (1 << (2 - column)) == 0 {
                        continue;
                    }
                    let x = left + f64::from(column) * CELL;
                    let row_from_bottom =
                        u32::try_from(4 - row).expect("3x5 glyph row is at most four");
                    let y = bottom + f64::from(row_from_bottom) * CELL;
                    self.rect(position, x, x + PIXEL, y, y + PIXEL, GLYPH_LIFT, color)?;
                }
            }
            left += (GLYPH_COLUMNS + 1.0) * CELL;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "矩形の面内境界と色を明示する")]
    fn rect(
        &mut self,
        position: Geodetic,
        left: f64,
        right: f64,
        bottom: f64,
        top: f64,
        depth: f64,
        color: [f32; 3],
    ) -> Result<(), TaxiwaySignMeshError> {
        let points = [
            sign_point(position, self.facing, left, bottom, depth),
            sign_point(position, self.facing, right, bottom, depth),
            sign_point(position, self.facing, right, top, depth),
            sign_point(position, self.facing, left, top, depth),
        ];
        let worlds = points.map(Geodetic::to_ecef);
        let normal = (worlds[1].as_vec() - worlds[0].as_vec())
            .cross(worlds[2].as_vec() - worlds[0].as_vec())
            .normalize();
        if !normal.is_finite() {
            return Err(TaxiwaySignMeshError::InvalidPosition);
        }
        let base =
            u32::try_from(self.positions.len()).map_err(|_| TaxiwaySignMeshError::TooComplex)?;
        let linear = Color::srgb(color[0], color[1], color[2]).to_linear();
        for world in worlds {
            let relative = world.as_vec() - self.origin.as_vec();
            if !relative.is_finite() {
                return Err(TaxiwaySignMeshError::InvalidPosition);
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "標識の原点相対位置と単位法線は f32 で十分"
            )]
            {
                self.positions
                    .push([relative.x as f32, relative.y as f32, relative.z as f32]);
                self.normals
                    .push([normal.x as f32, normal.y as f32, normal.z as f32]);
            }
            self.colors
                .push([linear.red, linear.green, linear.blue, 1.0]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        Ok(())
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn sign_point(position: Geodetic, facing: Radians, right: f64, up: f64, forward: f64) -> Geodetic {
    let (sin, cos) = facing.get().sin_cos();
    let north = forward * cos - right * sin;
    let east = forward * sin + right * cos;
    let horizontal = position.offset_by(Meters(north), Meters(east));
    Geodetic::new(
        horizontal.latitude,
        horizontal.longitude,
        Meters(position.altitude.get() + up),
    )
}

fn lifted(point: Geodetic, amount: f64) -> Geodetic {
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(point.altitude.get() + amount),
    )
}

fn valid_point(point: Geodetic) -> bool {
    point.latitude.is_finite()
        && point.longitude.is_finite()
        && point.altitude.is_finite()
        && point.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && point.longitude.get().abs() <= core::f64::consts::PI
}

/// 3x5 glyph。各行の下位 3 bit が左から右の画素。
fn glyph(byte: u8) -> Option<[u8; 5]> {
    Some(match byte {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        b'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        b'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        b'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        b'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        b'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        b'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        b'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        b'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        b'J' => [0b001, 0b001, 0b001, 0b101, 0b111],
        b'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        b'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        b'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        b'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        b'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        b'Q' => [0b111, 0b101, 0b101, 0b111, 0b001],
        b'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        b'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        b'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        b'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        b'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        b'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        b'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        b'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        b'-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        b'/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        b' ' => [0; 5],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use flightsim_core::Degrees;

    fn sign(taxiway: &str, holding: &str) -> Option<(Mesh, Ecef)> {
        holding_position_sign_mesh(
            Geodetic::from_degrees(35.0, 139.0, 8.0),
            Degrees(180.0).to_radians(),
            taxiway,
            holding,
        )
        .expect("valid geometry")
    }

    #[test]
    fn ascii_refs_build_black_yellow_and_red_white_panels() {
        let (mesh, _) = sign("A1", "34R-16L").expect("supported refs");
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(values)) => values,
            _ => panic!("missing colors"),
        };
        for wanted in [TAXI_BACKGROUND, TAXI_TEXT, HOLD_BACKGROUND, HOLD_TEXT] {
            let linear = Color::srgb(wanted[0], wanted[1], wanted[2]).to_linear();
            assert!(colors.iter().any(|color| {
                (color[0] - linear.red).abs() < 1e-6
                    && (color[1] - linear.green).abs() < 1e-6
                    && (color[2] - linear.blue).abs() < 1e-6
            }));
        }
    }

    #[test]
    fn sign_geometry_is_vertical_and_finite() {
        let (mesh, _) = sign("B", "22").expect("supported refs");
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("missing positions"),
        };
        assert!(positions.iter().flatten().all(|value| value.is_finite()));
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(values)) => values,
            _ => panic!("missing normals"),
        };
        // 地面の上向き法線ではなく、ほぼ水平な正面法線。
        let up =
            flightsim_core::LocalFrame::new(Geodetic::from_degrees(35.0, 139.0, 8.0)).up_ecef();
        assert!(normals.iter().all(|normal| {
            let normal = glam::DVec3::new(
                f64::from(normal[0]),
                f64::from(normal[1]),
                f64::from(normal[2]),
            );
            normal.dot(up).abs() < 1e-3
        }));
    }

    #[test]
    fn unsupported_or_unsafe_refs_are_omitted() {
        assert!(sign("", "22").is_none());
        assert!(sign("alpha", "22").is_none());
        assert!(sign("A", "滑走路").is_none());
        assert!(sign("ABCDEFGHI", "22").is_none());
        assert!(sign("A_1", "22").is_none());
    }

    #[test]
    fn all_supported_glyphs_fit_the_bitmap() {
        for byte in b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-/ " {
            let rows = glyph(*byte).expect("listed glyph");
            assert!(rows.iter().all(|row| *row <= 0b111));
        }
    }
}
