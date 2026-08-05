//! 実行時タイル形式（`.fsdem`）の読み書き。
//!
//! 形式の決定と根拠は [ADR-0005](../../../../docs/adr/0005-runtime-tile-format.md)。
//!
//! # レイアウト
//!
//! リトルエンディアン固定。ヘッダ [`HEADER_LEN`] バイト + ペイロード。
//!
//! ```text
//! オフセット サイズ  フィールド
//!      0       4    マジック `FSDM`
//!      4       2    フォーマット版 (u16)
//!      6       1    タイルレベル (u8)
//!      7       1    フラグ (u8, 予約。0 以外はエラー)
//!      8       4    タイル x (u32)
//!     12       4    タイル y (u32)
//!     16       4    格子幅 (u32)
//!     20       4    格子高さ (u32)
//!     24       8    標高オフセット (f64, m)
//!     32       8    標高スケール (f64, m/step)
//!     40       8    幾何誤差 (f64, m)
//!     48       8    ペイロードのチェックサム (u64, FNV-1a)
//!     56     w*h*2  標高 (u16 × 格子点数、行優先・北から南)
//! ```
//!
//! 標高の復号は `elevation = offset + q * scale`。
//!
//! # 読み込みは決して panic しない
//!
//! このモジュールが読むのはディスク上のファイルであり、**壊れている可能性が常にある**。
//! [`TileId::new`] や [`HeightGrid::new`] は不正な値でパニックするので、
//! それらを呼ぶ前に必ずここで検査して [`TileReadError`] を返す。
//!
//! [`TileId::new`]: crate::tile::TileId::new
//! [`HeightGrid::new`]: crate::dem::HeightGrid::new

use crate::dem::{DemTile, HeightGrid};
use crate::tile::{MAX_LEVEL, TileId};
use flightsim_core::Meters;
use std::io::{Read, Write};
use std::path::PathBuf;

/// ファイル先頭のマジックバイト。
pub const MAGIC: [u8; 4] = *b"FSDM";

/// 現在のフォーマット版。
///
/// **形式を変えたら必ず上げること。** 古いタイルを黙って誤読するのが最悪の失敗であり、
/// 版番号はそれを構造的に防ぐためだけに存在する（ADR-0005）。
pub const FORMAT_VERSION: u16 = 1;

/// ヘッダのバイト数。
pub const HEADER_LEN: usize = 56;

/// タイルファイルの拡張子。
pub const FILE_EXTENSION: &str = "fsdem";

/// 受け付ける格子の最大辺長。
///
/// 実用上のタイルは 64〜1024 点。この上限は精度のためではなく、
/// **不正なヘッダが巨大なメモリ確保を引き起こすのを防ぐため**にある。
/// 4096 でもペイロードは 32 MiB あり、正当なタイルとしては十分すぎる。
pub const MAX_GRID_DIMENSION: u32 = 4096;

// FNV-1a 64bit。依存を増やさずに済ませるためこれを選んだ。
// 検出したいのはビット腐敗と部分書き込みであり、暗号学的強度は不要（ADR-0005）。
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// ファイルから読み出したタイル。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTile {
    /// ファイルに記録されていたタイル ID。地理的範囲はここから導出する。
    pub id: TileId,
    pub tile: DemTile,
}

/// 書き込み時のエラー。
#[derive(Debug)]
pub enum TileWriteError {
    Io(std::io::Error),
    /// 標高に NaN / Inf が含まれていた。
    ///
    /// nodata の解決はオフライン側の責務であり、実行時形式に穴の概念は無い
    /// （ADR-0005）。ここで弾かないと、量子化で静かに 0 m へ化ける。
    NonFiniteElevation {
        column: u32,
        row: u32,
    },
    /// 格子が [`MAX_GRID_DIMENSION`] を超えている。
    GridTooLarge {
        width: u32,
        height: u32,
    },
}

impl core::fmt::Display for TileWriteError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to write tile: {error}"),
            Self::NonFiniteElevation { column, row } => write!(
                formatter,
                "elevation at grid point ({column}, {row}) is not finite; \
                 nodata must be resolved before writing a runtime tile"
            ),
            Self::GridTooLarge { width, height } => write!(
                formatter,
                "grid {width}×{height} exceeds the maximum dimension of {MAX_GRID_DIMENSION}"
            ),
        }
    }
}

impl std::error::Error for TileWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TileWriteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// 読み込み時のエラー。
///
/// **どの分岐もパニックしない。** 壊れたファイルは日常的に起こり得る。
#[derive(Debug)]
pub enum TileReadError {
    Io(std::io::Error),
    /// ヘッダが [`HEADER_LEN`] に満たない、またはペイロードが宣言より短い。
    Truncated {
        expected: usize,
        actual: usize,
    },
    NotATileFile {
        found: [u8; 4],
    },
    /// 未知のフォーマット版。**古い版を推測で読まない。**
    UnsupportedVersion {
        found: u16,
        supported: u16,
    },
    /// 予約フラグに 0 以外が入っていた。将来の圧縮フラグ等を先取りしないため。
    UnsupportedFlags(u8),
    InvalidTileId {
        level: u8,
        x: u32,
        y: u32,
    },
    InvalidGridSize {
        width: u32,
        height: u32,
    },
    /// ヘッダの浮動小数点値が NaN / Inf、またはスケール・幾何誤差が負。
    InvalidHeaderValue {
        field: &'static str,
        value: f64,
    },
    /// 長さは正しいが中身が壊れている。
    ChecksumMismatch {
        expected: u64,
        actual: u64,
    },
}

impl core::fmt::Display for TileReadError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to read tile: {error}"),
            Self::Truncated { expected, actual } => write!(
                formatter,
                "tile file is truncated: expected {expected} bytes, got {actual}"
            ),
            Self::NotATileFile { found } => write!(
                formatter,
                "not a tile file: expected magic {:?}, found {found:?}",
                MAGIC
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "tile format version {found} is not supported (this build reads version {supported})"
            ),
            Self::UnsupportedFlags(flags) => write!(
                formatter,
                "tile header sets reserved flags {flags:#010b}; this build cannot interpret them"
            ),
            Self::InvalidTileId { level, x, y } => write!(
                formatter,
                "tile id level={level} x={x} y={y} is outside the tiling scheme"
            ),
            Self::InvalidGridSize { width, height } => write!(
                formatter,
                "grid size {width}×{height} is invalid (must be 2..={MAX_GRID_DIMENSION} on each axis)"
            ),
            Self::InvalidHeaderValue { field, value } => {
                write!(
                    formatter,
                    "header field `{field}` has invalid value {value}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => write!(
                formatter,
                "payload checksum mismatch: header says {expected:#018x}, payload hashes to {actual:#018x}"
            ),
        }
    }
}

impl std::error::Error for TileReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TileReadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// 標高の量子化パラメータ。
///
/// スケールはタイル毎に標高レンジから決める。全球固定スケールより常に精度が良く、
/// 将来 海底地形を扱う際にも形式を変えずに済む（ADR-0005）。
#[derive(Debug, Clone, Copy)]
struct Quantisation {
    offset: f64,
    scale: f64,
}

impl Quantisation {
    fn for_grid(grid: &HeightGrid) -> Self {
        let (min, max) = grid.elevation_range();
        let offset = min.get();
        let span = max.get() - offset;
        Self {
            offset,
            // 平坦なタイルではスケール 0。復号は offset をそのまま返す。
            scale: if span > 0.0 {
                span / f64::from(u16::MAX)
            } else {
                0.0
            },
        }
    }

    fn encode(self, elevation: f64) -> u16 {
        if self.scale <= 0.0 {
            return 0;
        }
        let steps = ((elevation - self.offset) / self.scale).round();
        // 丸めで 65535 を僅かに超え得るためクランプする。
        // 呼び出し前に有限性を検査済みなので NaN は入らない。
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamp により 0..=65535 の有限値であることが保証されている"
        )]
        let quantised = steps.clamp(0.0, f64::from(u16::MAX)) as u16;
        quantised
    }

    fn decode(self, quantised: u16) -> f64 {
        self.offset + f64::from(quantised) * self.scale
    }
}

/// タイルの標準的な相対パス `{level}/{x}/{y}.fsdem`。
#[must_use]
pub fn tile_relative_path(id: TileId) -> PathBuf {
    PathBuf::from(id.level.to_string())
        .join(id.x.to_string())
        .join(format!("{}.{FILE_EXTENSION}", id.y))
}

/// 標高格子を実行時タイル形式で書き出す。
///
/// タイルの地理的範囲は `id` から導出できるため書き込まない。二重に持つと
/// 不整合が起き得るので、単一の出所に統一している（ADR-0005）。
///
/// 埋め込む幾何誤差は**量子化後の格子**から算出する。読み戻した格子と
/// 一致させるため。量子化前の値を書くと、LOD 選択が実際に描画される地形と
/// わずかに食い違う。
///
/// # Errors
///
/// 標高に NaN / Inf が含まれる場合、格子が大きすぎる場合、
/// および書き込みに失敗した場合。
pub fn write_tile<W: Write>(
    writer: &mut W,
    id: TileId,
    grid: &HeightGrid,
) -> Result<(), TileWriteError> {
    let (width, height) = (grid.width(), grid.height());
    if width > MAX_GRID_DIMENSION || height > MAX_GRID_DIMENSION {
        return Err(TileWriteError::GridTooLarge { width, height });
    }

    for row in 0..height {
        for column in 0..width {
            if !grid.sample_at(column, row).get().is_finite() {
                return Err(TileWriteError::NonFiniteElevation { column, row });
            }
        }
    }

    let quantisation = Quantisation::for_grid(grid);

    let mut payload = Vec::with_capacity((width as usize) * (height as usize) * 2);
    let mut decoded = Vec::with_capacity((width as usize) * (height as usize));
    for row in 0..height {
        for column in 0..width {
            let quantised = quantisation.encode(grid.sample_at(column, row).get());
            payload.extend_from_slice(&quantised.to_le_bytes());
            #[allow(
                clippy::cast_possible_truncation,
                reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で DEM 格子には十分"
            )]
            decoded.push(quantisation.decode(quantised) as f32);
        }
    }

    // 読み戻した格子と同じものから幾何誤差を求める。
    let geometric_error = HeightGrid::new(width, height, decoded).geometric_error();

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(&MAGIC);
    header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    header.push(id.level);
    header.push(0); // 予約フラグ
    header.extend_from_slice(&id.x.to_le_bytes());
    header.extend_from_slice(&id.y.to_le_bytes());
    header.extend_from_slice(&width.to_le_bytes());
    header.extend_from_slice(&height.to_le_bytes());
    header.extend_from_slice(&quantisation.offset.to_le_bytes());
    header.extend_from_slice(&quantisation.scale.to_le_bytes());
    header.extend_from_slice(&geometric_error.get().to_le_bytes());
    header.extend_from_slice(&fnv1a(&payload).to_le_bytes());
    debug_assert_eq!(header.len(), HEADER_LEN);

    writer.write_all(&header)?;
    writer.write_all(&payload)?;
    Ok(())
}

/// 実行時タイル形式を読み込む。
///
/// # Errors
///
/// マジック・版・フラグの不一致、タイル ID や格子サイズの範囲外、
/// ヘッダの数値が不正、切り詰め、チェックサム不一致、および読み込み失敗。
///
/// **どの経路でもパニックしない。** ディスク上のファイルは壊れ得る。
pub fn read_tile<R: Read>(reader: &mut R) -> Result<StoredTile, TileReadError> {
    let mut header = [0_u8; HEADER_LEN];
    read_exact_or_truncated(reader, &mut header, HEADER_LEN)?;

    let magic: [u8; 4] = header[0..4].try_into().unwrap_or_default();
    if magic != MAGIC {
        return Err(TileReadError::NotATileFile { found: magic });
    }

    let version = read_u16(&header, 4);
    if version != FORMAT_VERSION {
        return Err(TileReadError::UnsupportedVersion {
            found: version,
            supported: FORMAT_VERSION,
        });
    }

    let flags = header[7];
    if flags != 0 {
        return Err(TileReadError::UnsupportedFlags(flags));
    }

    // TileId::new は範囲外でパニックするため、その前に検査する。
    let level = header[6];
    let x = read_u32(&header, 8);
    let y = read_u32(&header, 12);
    if level > MAX_LEVEL || x >= TileId::columns(level) || y >= TileId::rows(level) {
        return Err(TileReadError::InvalidTileId { level, x, y });
    }
    let id = TileId::new(level, x, y);

    // HeightGrid::new は 2×2 未満でパニックする。上限は巨大な確保を防ぐため。
    let width = read_u32(&header, 16);
    let height = read_u32(&header, 20);
    if !(2..=MAX_GRID_DIMENSION).contains(&width) || !(2..=MAX_GRID_DIMENSION).contains(&height) {
        return Err(TileReadError::InvalidGridSize { width, height });
    }

    let offset = read_f64(&header, 24);
    let scale = read_f64(&header, 32);
    let geometric_error = read_f64(&header, 40);
    let checksum = read_u64(&header, 48);

    if !offset.is_finite() {
        return Err(TileReadError::InvalidHeaderValue {
            field: "elevation_offset",
            value: offset,
        });
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(TileReadError::InvalidHeaderValue {
            field: "elevation_scale",
            value: scale,
        });
    }
    if !geometric_error.is_finite() || geometric_error < 0.0 {
        return Err(TileReadError::InvalidHeaderValue {
            field: "geometric_error",
            value: geometric_error,
        });
    }

    let sample_count = (width as usize) * (height as usize);
    let mut payload = vec![0_u8; sample_count * 2];
    read_exact_or_truncated(reader, &mut payload, HEADER_LEN + sample_count * 2)?;

    let actual = fnv1a(&payload);
    if actual != checksum {
        return Err(TileReadError::ChecksumMismatch {
            expected: checksum,
            actual,
        });
    }

    let quantisation = Quantisation { offset, scale };
    let mut samples = Vec::with_capacity(sample_count);
    for chunk in payload.chunks_exact(2) {
        let quantised = u16::from_le_bytes([chunk[0], chunk[1]]);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で DEM 格子には十分"
        )]
        samples.push(quantisation.decode(quantised) as f32);
    }

    Ok(StoredTile {
        id,
        tile: DemTile::from_parts(
            id.bounds(),
            HeightGrid::new(width, height, samples),
            Meters(geometric_error),
        ),
    })
}

/// `read_exact` の `UnexpectedEof` を切り詰めエラーへ翻訳する。
///
/// `expected` はファイル全体としての期待バイト数（エラーメッセージ用）。
fn read_exact_or_truncated<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    expected: usize,
) -> Result<(), TileReadError> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => {
                return Err(TileReadError::Truncated {
                    expected,
                    actual: expected - (buffer.len() - filled),
                });
            }
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(TileReadError::Io(error)),
        }
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

fn read_f64(bytes: &[u8], offset: usize) -> f64 {
    f64::from_bits(read_u64(bytes, offset))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の標高データ生成。f32 の精度で十分"
    )]

    use super::*;

    /// 起伏のある地形。量子化誤差の検証に使う。
    fn hilly(width: u32, height: u32) -> HeightGrid {
        let mut samples = Vec::new();
        for row in 0..height {
            for column in 0..width {
                let x = f64::from(column) / f64::from(width - 1);
                let y = f64::from(row) / f64::from(height - 1);
                samples.push((1_500.0 * (x * 6.0).sin() * (y * 4.0).cos() + 2_000.0 * x) as f32);
            }
        }
        HeightGrid::new(width, height, samples)
    }

    fn encoded(id: TileId, grid: &HeightGrid) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_tile(&mut bytes, id, grid).expect("writing a valid grid should succeed");
        bytes
    }

    // --- 往復 ---

    #[test]
    fn round_trip_preserves_the_tile_id() {
        for id in [
            TileId::new(0, 0, 0),
            TileId::new(1, 3, 1),
            TileId::new(12, 4_095, 2_047),
            TileId::new(MAX_LEVEL, 0, 0),
        ] {
            let grid = hilly(8, 8);
            let stored = read_tile(&mut encoded(id, &grid).as_slice()).expect("round trip");
            assert_eq!(stored.id, id);
            assert_eq!(stored.tile.bounds(), id.bounds());
        }
    }

    #[test]
    fn round_trip_keeps_elevations_within_half_a_quantisation_step() {
        // ADR-0005 の精度保証そのもの。丸めが最近傍なら誤差はスケールの半分以下。
        let grid = hilly(33, 33);
        let (min, max) = grid.elevation_range();
        let scale = (max.get() - min.get()) / f64::from(u16::MAX);

        let stored =
            read_tile(&mut encoded(TileId::new(6, 10, 20), &grid).as_slice()).expect("round trip");

        for row in 0..grid.height() {
            for column in 0..grid.width() {
                let original = grid.sample_at(column, row).get();
                let restored = stored.tile.grid().sample_at(column, row).get();
                assert!(
                    (original - restored).abs() <= scale / 2.0 + 1e-3,
                    "at ({column}, {row}): {original} m became {restored} m, \
                     which exceeds half a quantisation step ({} m)",
                    scale / 2.0
                );
            }
        }
    }

    #[test]
    fn quantisation_is_exact_at_the_elevation_extremes() {
        // オフセットと最大ステップは端点にちょうど乗る。ここがずれていると
        // 隣接タイルの境界で段差が出る。
        let grid = hilly(17, 17);
        let (min, max) = grid.elevation_range();

        let stored =
            read_tile(&mut encoded(TileId::new(5, 1, 1), &grid).as_slice()).expect("round trip");
        let (restored_min, restored_max) = stored.tile.grid().elevation_range();

        assert!((restored_min.get() - min.get()).abs() < 1e-3);
        assert!((restored_max.get() - max.get()).abs() < 1e-3);
    }

    #[test]
    fn a_second_round_trip_changes_nothing() {
        // 量子化済みの格子を再度書いても値が動かないこと。
        // ここが安定しないと、タイルを焼き直すたびに地形がわずかに動く。
        let grid = hilly(17, 17);
        let id = TileId::new(7, 40, 30);

        let first = read_tile(&mut encoded(id, &grid).as_slice()).expect("first round trip");
        let second =
            read_tile(&mut encoded(id, first.tile.grid()).as_slice()).expect("second round trip");

        assert_eq!(first.tile.grid(), second.tile.grid());
    }

    #[test]
    fn flat_tiles_round_trip_exactly() {
        // 標高レンジ 0 はスケールが 0 になる縮退ケース。ゼロ除算になり得る。
        let grid = HeightGrid::flat(8, 8, Meters(123.5));
        let stored =
            read_tile(&mut encoded(TileId::new(3, 2, 1), &grid).as_slice()).expect("round trip");

        for row in 0..8 {
            for column in 0..8 {
                let value = stored.tile.grid().sample_at(column, row).get();
                assert!(
                    (value - 123.5).abs() < 1e-6,
                    "flat tile sampled as {value} m"
                );
            }
        }
        assert!(stored.tile.geometric_error().get() < 1e-6);
    }

    #[test]
    fn sea_level_tiles_round_trip_exactly() {
        let grid = HeightGrid::flat(4, 4, Meters::ZERO);
        let stored =
            read_tile(&mut encoded(TileId::new(2, 0, 0), &grid).as_slice()).expect("round trip");
        assert!(stored.tile.grid().sample_at(0, 0).get().abs() < 1e-9);
    }

    #[test]
    fn below_sea_level_elevations_survive() {
        // 死海は -430 m。オフセットが負になる経路を通す。
        let mut samples = vec![-430.0_f32; 16];
        samples[0] = -400.0;
        let grid = HeightGrid::new(4, 4, samples);

        let stored =
            read_tile(&mut encoded(TileId::new(4, 9, 4), &grid).as_slice()).expect("round trip");
        assert!((stored.tile.grid().sample_at(1, 1).get() + 430.0).abs() < 0.01);
        assert!((stored.tile.grid().sample_at(0, 0).get() + 400.0).abs() < 0.01);
    }

    // --- 幾何誤差 ---

    #[test]
    fn the_geometric_error_is_embedded_and_matches_the_stored_grid() {
        // LOD 選択はタイルを読む前にこの値を必要とする。定数にしてはいけない。
        let grid = hilly(33, 33);
        let stored =
            read_tile(&mut encoded(TileId::new(8, 100, 50), &grid).as_slice()).expect("round trip");

        let recomputed = stored.tile.grid().geometric_error();
        assert!(
            (stored.tile.geometric_error().get() - recomputed.get()).abs() < 1e-6,
            "embedded geometric error {} disagrees with the grid's own {}",
            stored.tile.geometric_error(),
            recomputed
        );
        assert!(
            stored.tile.geometric_error().get() > 1.0,
            "hilly terrain should have a real error"
        );
    }

    #[test]
    fn rough_tiles_embed_a_larger_geometric_error_than_flat_tiles() {
        let flat = read_tile(
            &mut encoded(
                TileId::new(6, 1, 1),
                &HeightGrid::flat(33, 33, Meters(100.0)),
            )
            .as_slice(),
        )
        .expect("round trip");
        let rough = read_tile(&mut encoded(TileId::new(6, 1, 1), &hilly(33, 33)).as_slice())
            .expect("round trip");

        assert!(rough.tile.geometric_error().get() > flat.tile.geometric_error().get());
    }

    // --- 壊れた入力 ---

    #[test]
    fn a_wrong_magic_is_rejected() {
        let mut bytes = encoded(TileId::new(3, 1, 1), &hilly(8, 8));
        bytes[0..4].copy_from_slice(b"PNG\0");
        assert!(matches!(
            read_tile(&mut bytes.as_slice()),
            Err(TileReadError::NotATileFile { .. })
        ));
    }

    #[test]
    fn an_unknown_format_version_is_rejected_rather_than_guessed() {
        // 古いタイルを黙って誤読するのが最悪の失敗（ADR-0005）。
        let mut bytes = encoded(TileId::new(3, 1, 1), &hilly(8, 8));
        bytes[4..6].copy_from_slice(&999_u16.to_le_bytes());
        assert!(matches!(
            read_tile(&mut bytes.as_slice()),
            Err(TileReadError::UnsupportedVersion { found: 999, .. })
        ));
    }

    #[test]
    fn reserved_flags_are_rejected() {
        let mut bytes = encoded(TileId::new(3, 1, 1), &hilly(8, 8));
        bytes[7] = 0b0000_0001;
        assert!(matches!(
            read_tile(&mut bytes.as_slice()),
            Err(TileReadError::UnsupportedFlags(1))
        ));
    }

    #[test]
    fn an_out_of_range_tile_id_is_rejected_without_panicking() {
        // TileId::new はパニックする。読み込み経路で呼ぶ前に検査していることの確認。
        let grid = hilly(8, 8);

        let mut too_deep = encoded(TileId::new(3, 1, 1), &grid);
        too_deep[6] = MAX_LEVEL + 1;
        assert!(matches!(
            read_tile(&mut too_deep.as_slice()),
            Err(TileReadError::InvalidTileId { .. })
        ));

        let mut x_out_of_range = encoded(TileId::new(3, 1, 1), &grid);
        x_out_of_range[8..12].copy_from_slice(&9_999_u32.to_le_bytes());
        assert!(matches!(
            read_tile(&mut x_out_of_range.as_slice()),
            Err(TileReadError::InvalidTileId { .. })
        ));

        let mut y_out_of_range = encoded(TileId::new(3, 1, 1), &grid);
        y_out_of_range[12..16].copy_from_slice(&9_999_u32.to_le_bytes());
        assert!(matches!(
            read_tile(&mut y_out_of_range.as_slice()),
            Err(TileReadError::InvalidTileId { .. })
        ));
    }

    #[test]
    fn a_degenerate_or_enormous_grid_size_is_rejected_without_allocating() {
        // HeightGrid::new は 2×2 未満でパニックする。巨大な値は確保で落ちる。
        let grid = hilly(8, 8);

        for (width, height) in [(0_u32, 8_u32), (1, 8), (8, 1), (MAX_GRID_DIMENSION + 1, 8)] {
            let mut bytes = encoded(TileId::new(3, 1, 1), &grid);
            bytes[16..20].copy_from_slice(&width.to_le_bytes());
            bytes[20..24].copy_from_slice(&height.to_le_bytes());
            assert!(
                matches!(
                    read_tile(&mut bytes.as_slice()),
                    Err(TileReadError::InvalidGridSize { .. })
                ),
                "grid size {width}×{height} should have been rejected"
            );
        }
    }

    #[test]
    fn non_finite_header_values_are_rejected() {
        let grid = hilly(8, 8);

        for (offset, field) in [
            (24_usize, "elevation_offset"),
            (32, "elevation_scale"),
            (40, "geometric_error"),
        ] {
            let mut bytes = encoded(TileId::new(3, 1, 1), &grid);
            bytes[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
            match read_tile(&mut bytes.as_slice()) {
                Err(TileReadError::InvalidHeaderValue { field: found, .. }) => {
                    assert_eq!(found, field);
                }
                other => panic!("NaN in `{field}` should be rejected, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_negative_scale_or_geometric_error_is_rejected() {
        let grid = hilly(8, 8);

        let mut negative_scale = encoded(TileId::new(3, 1, 1), &grid);
        negative_scale[32..40].copy_from_slice(&(-1.0_f64).to_le_bytes());
        assert!(matches!(
            read_tile(&mut negative_scale.as_slice()),
            Err(TileReadError::InvalidHeaderValue {
                field: "elevation_scale",
                ..
            })
        ));

        let mut negative_error = encoded(TileId::new(3, 1, 1), &grid);
        negative_error[40..48].copy_from_slice(&(-1.0_f64).to_le_bytes());
        assert!(matches!(
            read_tile(&mut negative_error.as_slice()),
            Err(TileReadError::InvalidHeaderValue {
                field: "geometric_error",
                ..
            })
        ));
    }

    #[test]
    fn a_corrupted_payload_is_caught_by_the_checksum() {
        // 長さは正しいまま中身だけ壊れたファイル。サイズ検査では捕まらない。
        let mut bytes = encoded(TileId::new(3, 1, 1), &hilly(8, 8));
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;

        assert!(matches!(
            read_tile(&mut bytes.as_slice()),
            Err(TileReadError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn a_single_flipped_bit_anywhere_in_the_payload_is_caught() {
        let grid = hilly(8, 8);
        let reference = encoded(TileId::new(3, 1, 1), &grid);

        for index in HEADER_LEN..reference.len() {
            let mut bytes = reference.clone();
            bytes[index] ^= 0b0000_0001;
            assert!(
                matches!(
                    read_tile(&mut bytes.as_slice()),
                    Err(TileReadError::ChecksumMismatch { .. })
                ),
                "a flipped bit at byte {index} slipped through the checksum"
            );
        }
    }

    #[test]
    fn truncation_at_any_length_is_reported_rather_than_panicking() {
        // 部分書き込みされたファイル。全ての切り詰め位置で試す。
        let reference = encoded(TileId::new(3, 1, 1), &hilly(8, 8));

        for length in 0..reference.len() {
            let result = read_tile(&mut &reference[..length]);
            assert!(
                result.is_err(),
                "a file truncated to {length} bytes was accepted"
            );
        }

        // 完全な長さなら通る。
        assert!(read_tile(&mut reference.as_slice()).is_ok());
    }

    #[test]
    fn arbitrary_garbage_never_panics() {
        // 決定論的な擬似乱数。壊れたファイルは日常的に起こり得るので、
        // 読み込み経路にパニックが残っていないことを確認する。
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            let length = (state % 200) as usize;
            let bytes: Vec<u8> = (0..length)
                .map(|i| ((state >> (i % 56)) ^ (i as u64)) as u8)
                .collect();

            // 戻り値は問わない。パニックしないことだけを検査する。
            let _ = read_tile(&mut bytes.as_slice());
        }
    }

    // --- 書き込み側の検査 ---

    #[test]
    fn non_finite_elevations_are_rejected_on_write() {
        // nodata の解決はオフライン側の責務。ここで弾かないと静かに 0 m へ化ける。
        let mut samples = vec![100.0_f32; 16];
        samples[5] = f32::NAN;
        let grid = HeightGrid::new(4, 4, samples);

        match write_tile(&mut Vec::new(), TileId::new(3, 1, 1), &grid) {
            Err(TileWriteError::NonFiniteElevation { column, row }) => {
                assert_eq!((column, row), (1, 1));
            }
            other => panic!("NaN elevation should be rejected, got {other:?}"),
        }

        samples = vec![100.0_f32; 16];
        samples[0] = f32::INFINITY;
        assert!(matches!(
            write_tile(
                &mut Vec::new(),
                TileId::new(3, 1, 1),
                &HeightGrid::new(4, 4, samples)
            ),
            Err(TileWriteError::NonFiniteElevation { .. })
        ));
    }

    #[test]
    fn the_header_length_is_what_the_adr_documents() {
        // 形式の互換性そのもの。ここが変わったら FORMAT_VERSION を上げる必要がある。
        let bytes = encoded(TileId::new(4, 3, 2), &hilly(8, 8));
        assert_eq!(bytes.len(), HEADER_LEN + 8 * 8 * 2);
        assert_eq!(&bytes[0..4], b"FSDM");
        assert_eq!(read_u16(&bytes, 4), FORMAT_VERSION);
    }

    // --- パス ---

    #[test]
    fn tile_paths_follow_the_documented_layout() {
        let path = tile_relative_path(TileId::new(12, 3_456, 789));
        assert_eq!(
            path,
            PathBuf::from("12").join("3456").join("789.fsdem"),
            "tile path layout changed; the tilegen CLI and the runtime loader must agree"
        );
    }
}
