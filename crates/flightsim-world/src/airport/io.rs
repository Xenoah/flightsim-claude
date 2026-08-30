//! OSM 由来の滑走路を格納する実行時空港 DB（`.fsairports`）。
//!
//! 形式とデータ境界は [ADR-0008](../../../../docs/adr/0008-osm-airport-data.md) に従う。
//! リトルエンディアン固定で、[`HEADER_LEN`] バイトのヘッダと [`RECORD_LEN`] バイトの
//! 固定長レコードからなる。実行時は OSM PBF を解析せず、この形式だけを読む。

use super::{Runway, RunwayGeometryError, validate_horizontal_coordinate};
use flightsim_core::{Geodetic, Meters};
use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::Path;

/// ファイル先頭のマジックバイト。
pub const MAGIC: [u8; 4] = *b"FSAP";

/// 現在のフォーマット版。
pub const FORMAT_VERSION: u16 = 1;

/// ヘッダのバイト数。
pub const HEADER_LEN: usize = 24;

/// v1 レコードのバイト数。
pub const RECORD_LEN: usize = 48;

const RECORD_LEN_FIELD: u32 = 48;

/// 空港 DB ファイルの拡張子。
pub const FILE_EXTENSION: &str = "fsairports";

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

/// OSM の中心線 way から得た滑走路 1 本。
///
/// DB は標高を持たないため、`runway` の楕円体高は常に 0 m。利用時は
/// [`Runway::with_elevation`] で DEM の値へ貼り直す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirportRunway {
    /// 元になった OSM way ID。
    pub source_way_id: i64,
    /// 端点から導出した滑走路幾何。楕円体高は 0 m。
    pub runway: Runway,
    // FSAP の外部境界は度。Geodetic（ラジアン）へ往復すると最下位 bit が変わり得るため、
    // 読み書きのたびに端点を動かさないよう on-disk の単位を単一の保存値にする。
    threshold_latitude_degrees: f64,
    threshold_longitude_degrees: f64,
    opposite_latitude_degrees: f64,
    opposite_longitude_degrees: f64,
}

impl AirportRunway {
    /// 既存の滑走路幾何から DB レコードを作る。
    ///
    /// 標高は 0 m へ貼り直す。反対端は [`Runway::opposite_threshold`] から作るため、
    /// OSM の元端点が手元にある変換処理では [`Self::from_endpoints`] を使う。
    ///
    /// # Errors
    ///
    /// 滑走路の座標・方位・寸法・標高が不正な場合。
    pub fn new(source_way_id: i64, runway: Runway) -> Result<Self, RunwayGeometryError> {
        validate_horizontal_coordinate(runway.threshold, "threshold")?;
        if !runway.heading.is_finite() {
            return Err(RunwayGeometryError::InvalidHeading {
                value: runway.heading.get(),
            });
        }
        if !runway.length.is_finite() || runway.length.get() <= 0.0 {
            return Err(RunwayGeometryError::InvalidLength {
                value: runway.length.get(),
            });
        }
        if !runway.width.is_finite() || runway.width.get() <= 0.0 {
            return Err(RunwayGeometryError::InvalidWidth {
                value: runway.width.get(),
            });
        }
        if !runway.elevation.is_finite() {
            return Err(RunwayGeometryError::InvalidElevation {
                value: runway.elevation.get(),
            });
        }

        let runway = runway.with_elevation(Meters::ZERO);
        Self::from_endpoints(
            source_way_id,
            runway.threshold,
            runway.opposite_threshold(),
            runway.width,
        )
    }

    /// OSM way の先頭・末尾 node から DB レコードを作る。
    ///
    /// 端点の高度は無視し、FSAP v1 の契約どおり 0 m を使う。
    ///
    /// # Errors
    ///
    /// 端点・幅が不正、または両端が縮退している場合。
    pub fn from_endpoints(
        source_way_id: i64,
        threshold: Geodetic,
        opposite_threshold: Geodetic,
        width: Meters,
    ) -> Result<Self, RunwayGeometryError> {
        Self::from_degree_endpoints(
            source_way_id,
            threshold.latitude_degrees(),
            threshold.longitude_degrees(),
            opposite_threshold.latitude_degrees(),
            opposite_threshold.longitude_degrees(),
            width,
        )
    }

    fn from_degree_endpoints(
        source_way_id: i64,
        threshold_latitude_degrees: f64,
        threshold_longitude_degrees: f64,
        opposite_latitude_degrees: f64,
        opposite_longitude_degrees: f64,
        width: Meters,
    ) -> Result<Self, RunwayGeometryError> {
        let threshold =
            Geodetic::from_degrees(threshold_latitude_degrees, threshold_longitude_degrees, 0.0);
        let opposite_threshold =
            Geodetic::from_degrees(opposite_latitude_degrees, opposite_longitude_degrees, 0.0);
        let runway = Runway::from_endpoints(threshold, opposite_threshold, width, Meters::ZERO)?;
        Ok(Self {
            source_way_id,
            runway,
            threshold_latitude_degrees,
            threshold_longitude_degrees,
            opposite_latitude_degrees,
            opposite_longitude_degrees,
        })
    }

    /// レコードに保存する反対端。高度は 0 m。
    #[must_use]
    pub fn opposite_threshold(&self) -> Geodetic {
        Geodetic::from_degrees(
            self.opposite_latitude_degrees,
            self.opposite_longitude_degrees,
            0.0,
        )
    }

    /// 使用中の地形から得た高さを適用した実行時滑走路を返す。
    #[must_use]
    pub const fn at_elevation(&self, elevation: Meters) -> Runway {
        self.runway.with_elevation(elevation)
    }

    fn validate_for_storage(&self) -> Result<(), RunwayGeometryError> {
        let rebuilt = Self::from_degree_endpoints(
            self.source_way_id,
            self.threshold_latitude_degrees,
            self.threshold_longitude_degrees,
            self.opposite_latitude_degrees,
            self.opposite_longitude_degrees,
            self.runway.width,
        )?
        .runway;

        // フィールドは利用側から読みやすいよう公開している。書き込み前に、保存端点と
        // 派生幾何が別々の答えへ変更されていないことを確認する。
        if rebuilt != self.runway {
            return Err(RunwayGeometryError::InconsistentStoredEndpoints);
        }
        Ok(())
    }
}

/// FSAP DB の構築・読み書きに失敗した理由。
#[derive(Debug)]
pub enum AirportDatabaseError {
    Io(std::io::Error),
    /// ヘッダまたは宣言されたレコード列が途中で終わっている。
    Truncated {
        expected: usize,
        actual: usize,
    },
    NotAnAirportDatabase {
        found: [u8; 4],
    },
    UnsupportedVersion {
        found: u16,
        supported: u16,
    },
    UnsupportedFlags(u16),
    UnsupportedRecordSize {
        found: u32,
        supported: u32,
    },
    /// 宣言されたレコード列の後ろに未解釈のデータがある。
    TrailingData {
        expected: usize,
        actual: usize,
    },
    ChecksumMismatch {
        expected: u64,
        actual: u64,
    },
    /// レコード数から必要バイト数を表現できない。
    SizeOverflow {
        record_count: u32,
    },
    /// メモリ確保に失敗した。
    AllocationFailed {
        requested: usize,
    },
    TooManyRunways {
        count: usize,
    },
    InvalidRunway {
        record_index: usize,
        source_way_id: i64,
        source: RunwayGeometryError,
    },
}

impl core::fmt::Display for AirportDatabaseError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "airport database I/O failed: {error}"),
            Self::Truncated { expected, actual } => write!(
                formatter,
                "airport database is truncated: expected {expected} bytes, got {actual}"
            ),
            Self::NotAnAirportDatabase { found } => write!(
                formatter,
                "not an airport database: expected magic {:?}, found {found:?}",
                MAGIC
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "airport database version {found} is not supported (this build reads version {supported})"
            ),
            Self::UnsupportedFlags(flags) => write!(
                formatter,
                "airport database sets reserved flags {flags:#018b}; this build cannot interpret them"
            ),
            Self::UnsupportedRecordSize { found, supported } => write!(
                formatter,
                "airport record size {found} is not supported (version 1 requires {supported})"
            ),
            Self::TrailingData { expected, actual } => write!(
                formatter,
                "airport database has trailing data: expected {expected} bytes, got {actual}"
            ),
            Self::ChecksumMismatch { expected, actual } => write!(
                formatter,
                "airport payload checksum mismatch: header says {expected:#018x}, payload hashes to {actual:#018x}"
            ),
            Self::SizeOverflow { record_count } => write!(
                formatter,
                "airport record count {record_count} cannot be represented as a file size"
            ),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate {requested} bytes for the airport database"
            ),
            Self::TooManyRunways { count } => write!(
                formatter,
                "airport database has {count} runways; the format limit is {}",
                u32::MAX
            ),
            Self::InvalidRunway {
                record_index,
                source_way_id,
                source,
            } => write!(
                formatter,
                "airport record {record_index} (OSM way {source_way_id}) is invalid: {source}"
            ),
        }
    }
}

impl std::error::Error for AirportDatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidRunway { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AirportDatabaseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// 検証済みの実行時空港 DB。
#[derive(Debug, Clone, PartialEq)]
pub struct AirportDatabase {
    runways: Vec<AirportRunway>,
}

impl AirportDatabase {
    /// 滑走路を検証し、OSM way ID と幾何の順で決定論的に並べる。
    ///
    /// # Errors
    ///
    /// レコード数が FSAP v1 の上限を超える場合、または滑走路が不正な場合。
    pub fn new(mut runways: Vec<AirportRunway>) -> Result<Self, AirportDatabaseError> {
        if u32::try_from(runways.len()).is_err() {
            return Err(AirportDatabaseError::TooManyRunways {
                count: runways.len(),
            });
        }

        for (record_index, runway) in runways.iter().enumerate() {
            runway.validate_for_storage().map_err(|source| {
                AirportDatabaseError::InvalidRunway {
                    record_index,
                    source_way_id: runway.source_way_id,
                    source,
                }
            })?;
        }

        runways.sort_by(compare_runways);
        Ok(Self { runways })
    }

    /// 格納された滑走路。順序は決定論的。
    #[must_use]
    pub fn runways(&self) -> &[AirportRunway] {
        &self.runways
    }

    /// DB が空か。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runways.is_empty()
    }

    /// 滑走路数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.runways.len()
    }

    /// 検索地点から滑走路中心までの ECEF 直線距離が最小の 1 本を返す。
    ///
    /// 同距離なら OSM way ID が小さい方を選ぶ。入力順には依存しない。DB が空、または
    /// 検索地点が非有限・測地範囲外なら `None`。
    #[must_use]
    pub fn nearest(&self, query: Geodetic) -> Option<&AirportRunway> {
        if validate_horizontal_coordinate(query, "query").is_err() || !query.altitude.is_finite() {
            return None;
        }
        let query_ecef = query.to_ecef();
        if !query_ecef.is_finite() {
            return None;
        }

        self.runways.iter().min_by(|left, right| {
            let left_distance = left
                .runway
                .center()
                .to_ecef()
                .0
                .distance_squared(query_ecef.0);
            let right_distance = right
                .runway
                .center()
                .to_ecef()
                .0
                .distance_squared(query_ecef.0);
            left_distance
                .total_cmp(&right_distance)
                .then_with(|| left.source_way_id.cmp(&right.source_way_id))
                .then_with(|| compare_runways(left, right))
        })
    }

    /// FSAP v1 の bytes を検証して読む。
    ///
    /// # Errors
    ///
    /// ヘッダ、全長、checksum、またはレコード幾何が不正な場合。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AirportDatabaseError> {
        if bytes.len() < HEADER_LEN {
            return Err(AirportDatabaseError::Truncated {
                expected: HEADER_LEN,
                actual: bytes.len(),
            });
        }

        let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if magic != MAGIC {
            return Err(AirportDatabaseError::NotAnAirportDatabase { found: magic });
        }

        let version = read_u16(bytes, 4);
        if version != FORMAT_VERSION {
            return Err(AirportDatabaseError::UnsupportedVersion {
                found: version,
                supported: FORMAT_VERSION,
            });
        }

        let flags = read_u16(bytes, 6);
        if flags != 0 {
            return Err(AirportDatabaseError::UnsupportedFlags(flags));
        }

        let record_count = read_u32(bytes, 8);
        let record_size = read_u32(bytes, 12);
        if record_size != RECORD_LEN_FIELD {
            return Err(AirportDatabaseError::UnsupportedRecordSize {
                found: record_size,
                supported: RECORD_LEN_FIELD,
            });
        }

        let count = usize::try_from(record_count)
            .map_err(|_| AirportDatabaseError::SizeOverflow { record_count })?;
        let payload_len = count
            .checked_mul(RECORD_LEN)
            .ok_or(AirportDatabaseError::SizeOverflow { record_count })?;
        let expected_len = HEADER_LEN
            .checked_add(payload_len)
            .ok_or(AirportDatabaseError::SizeOverflow { record_count })?;
        match bytes.len().cmp(&expected_len) {
            Ordering::Less => {
                return Err(AirportDatabaseError::Truncated {
                    expected: expected_len,
                    actual: bytes.len(),
                });
            }
            Ordering::Greater => {
                return Err(AirportDatabaseError::TrailingData {
                    expected: expected_len,
                    actual: bytes.len(),
                });
            }
            Ordering::Equal => {}
        }

        let payload = &bytes[HEADER_LEN..];
        let expected_checksum = read_u64(bytes, 16);
        let actual_checksum = fnv1a(payload);
        if actual_checksum != expected_checksum {
            return Err(AirportDatabaseError::ChecksumMismatch {
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        let mut runways = Vec::new();
        runways
            .try_reserve_exact(count)
            .map_err(|_| AirportDatabaseError::AllocationFailed {
                requested: payload_len,
            })?;
        for (record_index, record) in payload.chunks_exact(RECORD_LEN).enumerate() {
            let source_way_id = read_i64(record, 0);
            let threshold_latitude = read_f64(record, 8);
            let threshold_longitude = read_f64(record, 16);
            let opposite_latitude = read_f64(record, 24);
            let opposite_longitude = read_f64(record, 32);
            let width = Meters(read_f64(record, 40));

            let runway = AirportRunway::from_degree_endpoints(
                source_way_id,
                threshold_latitude,
                threshold_longitude,
                opposite_latitude,
                opposite_longitude,
                width,
            )
            .map_err(|source| AirportDatabaseError::InvalidRunway {
                record_index,
                source_way_id,
                source,
            })?;
            runways.push(runway);
        }

        Self::new(runways)
    }

    /// FSAP v1 bytes を作る。
    ///
    /// # Errors
    ///
    /// 公開フィールドが保存端点と不整合になっている場合、件数が上限を超える場合、
    /// または必要なメモリを確保できない場合。
    pub fn to_bytes(&self) -> Result<Vec<u8>, AirportDatabaseError> {
        let record_count = u32::try_from(self.runways.len()).map_err(|_| {
            AirportDatabaseError::TooManyRunways {
                count: self.runways.len(),
            }
        })?;
        let payload_len = self.runways.len().checked_mul(RECORD_LEN).ok_or(
            AirportDatabaseError::TooManyRunways {
                count: self.runways.len(),
            },
        )?;

        let mut payload = Vec::new();
        payload.try_reserve_exact(payload_len).map_err(|_| {
            AirportDatabaseError::AllocationFailed {
                requested: payload_len,
            }
        })?;
        for (record_index, airport_runway) in self.runways.iter().enumerate() {
            airport_runway.validate_for_storage().map_err(|source| {
                AirportDatabaseError::InvalidRunway {
                    record_index,
                    source_way_id: airport_runway.source_way_id,
                    source,
                }
            })?;
            payload.extend_from_slice(&airport_runway.source_way_id.to_le_bytes());
            payload.extend_from_slice(&airport_runway.threshold_latitude_degrees.to_le_bytes());
            payload.extend_from_slice(&airport_runway.threshold_longitude_degrees.to_le_bytes());
            payload.extend_from_slice(&airport_runway.opposite_latitude_degrees.to_le_bytes());
            payload.extend_from_slice(&airport_runway.opposite_longitude_degrees.to_le_bytes());
            payload.extend_from_slice(&airport_runway.runway.width.get().to_le_bytes());
        }
        debug_assert_eq!(payload.len(), payload_len);

        let total_len = HEADER_LEN + payload_len;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total_len)
            .map_err(|_| AirportDatabaseError::AllocationFailed {
                requested: total_len,
            })?;
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&record_count.to_le_bytes());
        bytes.extend_from_slice(&RECORD_LEN_FIELD.to_le_bytes());
        bytes.extend_from_slice(&fnv1a(&payload).to_le_bytes());
        debug_assert_eq!(bytes.len(), HEADER_LEN);
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    /// reader から FSAP v1 DB を読む。
    ///
    /// # Errors
    ///
    /// I/O または [`Self::from_bytes`] の検証に失敗した場合。
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, AirportDatabaseError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }

    /// path の FSAP v1 DB を読む。
    ///
    /// # Errors
    ///
    /// ファイル I/O または形式の検証に失敗した場合。
    pub fn read_from_path(path: impl AsRef<Path>) -> Result<Self, AirportDatabaseError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    /// writer へ FSAP v1 DB を書く。
    ///
    /// # Errors
    ///
    /// DB の検証、メモリ確保、または I/O に失敗した場合。
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), AirportDatabaseError> {
        writer.write_all(&self.to_bytes()?)?;
        Ok(())
    }

    /// path へ FSAP v1 DB を書く。
    ///
    /// # Errors
    ///
    /// DB の検証、メモリ確保、またはファイル I/O に失敗した場合。
    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<(), AirportDatabaseError> {
        let mut file = std::fs::File::create(path)?;
        self.write_to(&mut file)
    }
}

fn compare_runways(left: &AirportRunway, right: &AirportRunway) -> Ordering {
    left.source_way_id
        .cmp(&right.source_way_id)
        .then_with(|| {
            left.threshold_latitude_degrees
                .total_cmp(&right.threshold_latitude_degrees)
        })
        .then_with(|| {
            left.threshold_longitude_degrees
                .total_cmp(&right.threshold_longitude_degrees)
        })
        .then_with(|| {
            left.opposite_latitude_degrees
                .total_cmp(&right.opposite_latitude_degrees)
        })
        .then_with(|| {
            left.opposite_longitude_degrees
                .total_cmp(&right.opposite_longitude_degrees)
        })
        .then_with(|| left.runway.width.get().total_cmp(&right.runway.width.get()))
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

fn read_i64(bytes: &[u8], offset: usize) -> i64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    i64::from_le_bytes(value)
}

fn read_f64(bytes: &[u8], offset: usize) -> f64 {
    f64::from_bits(read_u64(bytes, offset))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        reason = "deterministic garbage generator deliberately keeps the low byte"
    )]

    use super::*;
    use flightsim_core::{Degrees, LocalFrame, Radians};

    macro_rules! assert_close {
        ($actual:expr, $expected:expr, $tolerance:expr) => {{
            let (actual, expected, tolerance) = ($actual, $expected, $tolerance);
            assert!(
                (actual - expected).abs() <= tolerance,
                "expected {actual} to be within {tolerance} of {expected}"
            );
        }};
    }

    fn airport_runway(
        source_way_id: i64,
        threshold_latitude: f64,
        threshold_longitude: f64,
        opposite_latitude: f64,
        opposite_longitude: f64,
        width: f64,
    ) -> AirportRunway {
        AirportRunway::from_endpoints(
            source_way_id,
            Geodetic::from_degrees(threshold_latitude, threshold_longitude, 9_999.0),
            Geodetic::from_degrees(opposite_latitude, opposite_longitude, -500.0),
            Meters(width),
        )
        .expect("test runway geometry should be valid")
    }

    fn one_record_file(
        source_way_id: i64,
        threshold_latitude: f64,
        threshold_longitude: f64,
        opposite_latitude: f64,
        opposite_longitude: f64,
        width: f64,
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(RECORD_LEN);
        payload.extend_from_slice(&source_way_id.to_le_bytes());
        payload.extend_from_slice(&threshold_latitude.to_le_bytes());
        payload.extend_from_slice(&threshold_longitude.to_le_bytes());
        payload.extend_from_slice(&opposite_latitude.to_le_bytes());
        payload.extend_from_slice(&opposite_longitude.to_le_bytes());
        payload.extend_from_slice(&width.to_le_bytes());

        let mut bytes = Vec::with_capacity(HEADER_LEN + RECORD_LEN);
        bytes.extend_from_slice(b"FSAP");
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&48_u32.to_le_bytes());
        bytes.extend_from_slice(&fnv1a(&payload).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn sample_database() -> AirportDatabase {
        AirportDatabase::new(vec![
            airport_runway(900, 35.55, 139.77, 35.57, 139.79, 45.0),
            airport_runway(-12, -33.95, 151.17, -33.93, 151.19, 60.0),
        ])
        .expect("sample database should be valid")
    }

    // --- FSAP v1 の互換性 ---

    #[test]
    fn round_trip_preserves_records_and_is_byte_stable() {
        let database = sample_database();
        let bytes = database.to_bytes().expect("encoding should succeed");
        let restored = AirportDatabase::from_bytes(&bytes).expect("decoding should succeed");

        assert_eq!(restored, database);
        assert_eq!(
            restored.to_bytes().expect("re-encoding should succeed"),
            bytes,
            "a read/write cycle moved an endpoint or changed record order"
        );

        let mut via_reader = bytes.as_slice();
        assert_eq!(
            AirportDatabase::read_from(&mut via_reader).expect("reader API should succeed"),
            database
        );
        let mut via_writer = Vec::new();
        database
            .write_to(&mut via_writer)
            .expect("writer API should succeed");
        assert_eq!(via_writer, bytes);
    }

    #[test]
    fn independently_hand_built_v1_record_has_the_documented_layout() {
        // この checksum は下の 48 bytes に対して実装とは独立に計算した FNV-1a 値。
        // write helper や fnv1a() を使って組み立てると、同じ offset の誤りを見逃す。
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x46, 0x53, 0x41, 0x50]); // FSAP
        bytes.extend_from_slice(&[0x01, 0x00]); // version 1
        bytes.extend_from_slice(&[0x00, 0x00]); // flags 0
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // one record
        bytes.extend_from_slice(&[0x30, 0x00, 0x00, 0x00]); // 48-byte record
        bytes.extend_from_slice(&[0x3d, 0x49, 0xbe, 0xb5, 0xe7, 0x39, 0x7a, 0xef]);
        bytes.extend_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // -2 i64
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f]); // 1.0
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40]); // 2.0
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x40]); // 3.0
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x40]); // 4.0
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x40]); // 5.0

        assert_eq!(bytes.len(), 72);
        let database = AirportDatabase::from_bytes(&bytes).expect("hand-built v1 file is valid");
        let record = &database.runways()[0];
        assert_eq!(record.source_way_id, -2);
        assert_close!(record.runway.threshold.latitude_degrees(), 1.0, 1.0e-14);
        assert_close!(record.runway.threshold.longitude_degrees(), 2.0, 1.0e-14);
        assert_close!(record.opposite_threshold().latitude_degrees(), 3.0, 1.0e-14);
        assert_close!(
            record.opposite_threshold().longitude_degrees(),
            4.0,
            1.0e-14
        );
        assert_close!(record.runway.width.get(), 5.0, 0.0);

        assert_eq!(
            database
                .to_bytes()
                .expect("hand-built file should re-encode"),
            bytes
        );
    }

    #[test]
    fn writer_sets_every_header_field_at_the_documented_offset() {
        let bytes = sample_database()
            .to_bytes()
            .expect("encoding should succeed");
        assert_eq!(&bytes[0..4], b"FSAP");
        assert_eq!(&bytes[4..6], &1_u16.to_le_bytes());
        assert_eq!(&bytes[6..8], &0_u16.to_le_bytes());
        assert_eq!(&bytes[8..12], &2_u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &48_u32.to_le_bytes());
        assert_eq!(read_u64(&bytes, 16), fnv1a(&bytes[24..]));
        assert_eq!(bytes.len(), 24 + 2 * 48);
        // Database::new sorts by signed OSM way ID, not caller order.
        assert_eq!(read_i64(&bytes, 24), -12);
        assert_eq!(read_i64(&bytes, 24 + 48), 900);
    }

    #[test]
    fn an_empty_database_has_a_valid_empty_payload_checksum() {
        let database = AirportDatabase::new(Vec::new()).expect("empty databases are valid");
        let bytes = database.to_bytes().expect("encoding should succeed");
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(read_u32(&bytes, 8), 0);
        assert_eq!(read_u64(&bytes, 16), FNV_OFFSET_BASIS);
        assert!(
            AirportDatabase::from_bytes(&bytes)
                .expect("decoding should succeed")
                .is_empty()
        );
    }

    // --- ヘッダ・全長・checksum ---

    #[test]
    fn unknown_version_flags_and_record_size_are_rejected() {
        let reference = sample_database()
            .to_bytes()
            .expect("encoding should succeed");

        let mut version = reference.clone();
        version[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            AirportDatabase::from_bytes(&version),
            Err(AirportDatabaseError::UnsupportedVersion {
                found: 2,
                supported: 1
            })
        ));

        let mut flags = reference.clone();
        flags[6..8].copy_from_slice(&0x8001_u16.to_le_bytes());
        assert!(matches!(
            AirportDatabase::from_bytes(&flags),
            Err(AirportDatabaseError::UnsupportedFlags(0x8001))
        ));

        let mut record_size = reference;
        record_size[12..16].copy_from_slice(&56_u32.to_le_bytes());
        assert!(matches!(
            AirportDatabase::from_bytes(&record_size),
            Err(AirportDatabaseError::UnsupportedRecordSize {
                found: 56,
                supported: 48
            })
        ));
    }

    #[test]
    fn wrong_magic_is_not_accepted_as_an_airport_database() {
        let mut bytes = sample_database()
            .to_bytes()
            .expect("encoding should succeed");
        bytes[0..4].copy_from_slice(b"FSDM");
        assert!(matches!(
            AirportDatabase::from_bytes(&bytes),
            Err(AirportDatabaseError::NotAnAirportDatabase { found }) if &found == b"FSDM"
        ));
    }

    #[test]
    fn truncation_at_every_byte_and_trailing_data_are_reported() {
        let reference = sample_database()
            .to_bytes()
            .expect("encoding should succeed");
        for length in 0..reference.len() {
            assert!(
                matches!(
                    AirportDatabase::from_bytes(&reference[..length]),
                    Err(AirportDatabaseError::Truncated { .. })
                ),
                "a file truncated to {length} bytes was not reported as truncated"
            );
        }

        let mut trailing = reference.clone();
        trailing.extend_from_slice(&[0xaa, 0x55]);
        assert!(matches!(
            AirportDatabase::from_bytes(&trailing),
            Err(AirportDatabaseError::TrailingData {
                expected,
                actual
            }) if expected == reference.len() && actual == reference.len() + 2
        ));
    }

    #[test]
    fn record_count_must_match_the_exact_file_length() {
        let reference = one_record_file(1, 10.0, 20.0, 10.01, 20.01, 45.0);

        let mut declares_two = reference.clone();
        declares_two[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert!(matches!(
            AirportDatabase::from_bytes(&declares_two),
            Err(AirportDatabaseError::Truncated {
                expected: 120,
                actual: 72
            })
        ));

        let mut declares_zero = reference;
        declares_zero[8..12].copy_from_slice(&0_u32.to_le_bytes());
        assert!(matches!(
            AirportDatabase::from_bytes(&declares_zero),
            Err(AirportDatabaseError::TrailingData {
                expected: 24,
                actual: 72
            })
        ));
    }

    #[test]
    fn a_payload_bit_flip_is_caught_before_geometry_is_used() {
        let mut bytes = sample_database()
            .to_bytes()
            .expect("encoding should succeed");
        bytes[HEADER_LEN + 17] ^= 0x40;
        assert!(matches!(
            AirportDatabase::from_bytes(&bytes),
            Err(AirportDatabaseError::ChecksumMismatch { .. })
        ));
    }

    // --- 外部データ境界の幾何検証 ---

    #[test]
    fn invalid_coordinates_width_and_collapsed_endpoints_are_errors() {
        let invalid_files = [
            one_record_file(1, f64::NAN, 20.0, 10.01, 20.01, 45.0),
            one_record_file(2, 91.0, 20.0, 10.01, 20.01, 45.0),
            one_record_file(3, 10.0, -181.0, 10.01, 20.01, 45.0),
            one_record_file(4, 10.0, 20.0, f64::INFINITY, 20.01, 45.0),
            one_record_file(5, 10.0, 20.0, 10.01, 20.01, 0.0),
            one_record_file(6, 10.0, 20.0, 10.01, 20.01, f64::NAN),
            one_record_file(7, 10.0, 20.0, 10.0, 20.0, 45.0),
        ];

        for (index, bytes) in invalid_files.iter().enumerate() {
            assert!(
                matches!(
                    AirportDatabase::from_bytes(bytes),
                    Err(AirportDatabaseError::InvalidRunway { .. })
                ),
                "invalid geometry case {index} was accepted"
            );
        }
    }

    #[test]
    fn runway_constructors_reject_non_finite_and_degenerate_geometry() {
        let valid = Geodetic::from_degrees(35.0, 139.0, 0.0);
        assert!(matches!(
            Runway::from_endpoints(valid, valid, Meters(45.0), Meters::ZERO),
            Err(RunwayGeometryError::CollapsedEndpoints)
        ));
        assert!(matches!(
            Runway::from_endpoints(
                valid,
                Geodetic::from_degrees(35.01, 139.01, 0.0),
                Meters(-1.0),
                Meters::ZERO
            ),
            Err(RunwayGeometryError::InvalidWidth { value: -1.0 })
        ));

        let invalid = Runway::new(
            valid,
            Radians(f64::NAN),
            Meters(1_000.0),
            Meters(45.0),
            Meters::ZERO,
        );
        assert!(matches!(
            AirportRunway::new(99, invalid),
            Err(RunwayGeometryError::InvalidHeading { .. })
        ));
    }

    #[test]
    fn arbitrary_short_garbage_never_panics() {
        let mut state = 0x7a8f_91ce_b47d_2305_u64;
        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let length = (state % 180) as usize;
            let bytes: Vec<u8> = (0..length)
                .map(|index| ((state >> (index % 56)) ^ (index as u64)) as u8)
                .collect();
            let _ = AirportDatabase::from_bytes(&bytes);
        }
    }

    // --- 端点変換・標高・最近傍 ---

    #[test]
    fn endpoints_crossing_the_dateline_produce_a_short_eastbound_runway() {
        let threshold = Geodetic::from_degrees(10.0, 179.999, 200.0);
        let opposite = Geodetic::from_degrees(10.0, -179.999, -80.0);
        let runway = Runway::from_endpoints(threshold, opposite, Meters(45.0), Meters(12.0))
            .expect("dateline endpoints should be valid");

        assert!(runway.is_finite());
        assert_close!(runway.heading.to_degrees().get(), 89.999_826_35, 0.001);
        assert!((200.0..=225.0).contains(&runway.length.get()));
        assert_close!(runway.threshold.longitude_degrees(), 179.999, 1.0e-12);
        assert_close!(runway.threshold.altitude.get(), 12.0, 0.0);
    }

    #[test]
    fn high_latitude_endpoint_construction_matches_core_ned_conversion() {
        let threshold = Geodetic::from_degrees(89.9, 40.0, 0.0);
        let opposite = Geodetic::from_degrees(89.9, 40.1, 0.0);
        let runway = Runway::from_endpoints(threshold, opposite, Meters(30.0), Meters::ZERO)
            .expect("high-latitude endpoints should be valid");

        let expected_ned = LocalFrame::new(threshold).ecef_to_ned_position(opposite.to_ecef());
        assert_close!(
            runway.length.get(),
            expected_ned.horizontal_magnitude(),
            1.0e-12
        );
        assert_close!(runway.heading.get(), expected_ned.bearing().get(), 1.0e-15);
        assert!(runway.length.get() > 10.0);
        assert!(runway.heading.to_degrees().get() > 89.0);
    }

    #[test]
    fn elevation_rebase_changes_only_the_single_elevation_source() {
        let runway = Runway::from_endpoints(
            Geodetic::from_degrees(35.0, 139.0, -999.0),
            Geodetic::from_degrees(35.02, 139.02, 8_000.0),
            Meters(45.0),
            Meters::ZERO,
        )
        .expect("geometry should be valid");
        let raised = runway.with_elevation(Meters(1_234.5));

        assert_eq!(raised.heading, runway.heading);
        assert_eq!(raised.length, runway.length);
        assert_eq!(raised.width, runway.width);
        assert_eq!(raised.threshold.latitude, runway.threshold.latitude);
        assert_eq!(raised.threshold.longitude, runway.threshold.longitude);
        assert_close!(raised.threshold.altitude.get(), 1_234.5, 0.0);
        assert_close!(raised.elevation.get(), 1_234.5, 0.0);
    }

    #[test]
    fn nearest_uses_runway_centres_and_not_input_order() {
        let far = airport_runway(40, 0.0, 40.0, 0.01, 40.0, 45.0);
        let near = airport_runway(80, 0.0, 1.0, 0.01, 1.0, 45.0);
        let database = AirportDatabase::new(vec![far, near]).expect("database should be valid");

        let selected = database
            .nearest(Geodetic::from_degrees(0.005, 1.001, 10_000.0))
            .expect("database is not empty");
        assert_eq!(selected.source_way_id, 80);
    }

    #[test]
    fn exact_distance_ties_use_the_smaller_osm_way_id() {
        let same_geometry_high_id = airport_runway(900, 20.0, 30.0, 20.01, 30.0, 45.0);
        let same_geometry_low_id = airport_runway(-7, 20.0, 30.0, 20.01, 30.0, 45.0);
        let database = AirportDatabase::new(vec![same_geometry_high_id, same_geometry_low_id])
            .expect("database should be valid");

        let selected = database
            .nearest(Geodetic::from_degrees(20.5, 30.0, 0.0))
            .expect("database is not empty");
        assert_eq!(selected.source_way_id, -7);
    }

    #[test]
    fn nearest_selection_handles_dateline_and_high_latitude_queries() {
        let dateline = airport_runway(1, 10.0, 179.98, 10.0, -179.98, 45.0);
        let greenwich = airport_runway(2, 10.0, 0.0, 10.02, 0.0, 45.0);
        let polar_near = airport_runway(3, 85.0, 10.0, 85.01, 10.0, 45.0);
        let polar_far = airport_runway(4, 85.0, 100.0, 85.01, 100.0, 45.0);
        let database = AirportDatabase::new(vec![greenwich, polar_far, dateline, polar_near])
            .expect("database should be valid");

        assert_eq!(
            database
                .nearest(Geodetic::from_degrees(10.0, -179.99, 0.0))
                .expect("database is not empty")
                .source_way_id,
            1
        );
        assert_eq!(
            database
                .nearest(Geodetic::from_degrees(85.0, 11.0, 0.0))
                .expect("database is not empty")
                .source_way_id,
            3
        );
    }

    #[test]
    fn empty_or_invalid_queries_return_none() {
        let empty = AirportDatabase::new(Vec::new()).expect("empty databases are valid");
        assert!(
            empty
                .nearest(Geodetic::from_degrees(0.0, 0.0, 0.0))
                .is_none()
        );

        let database = sample_database();
        for invalid in [
            Geodetic::from_degrees(f64::NAN, 0.0, 0.0),
            Geodetic::from_degrees(91.0, 0.0, 0.0),
            Geodetic::from_degrees(0.0, 181.0, 0.0),
            Geodetic::from_degrees(0.0, 0.0, f64::INFINITY),
        ] {
            assert!(database.nearest(invalid).is_none());
        }
    }

    #[test]
    fn endpoint_heading_uses_compass_convention() {
        let north = Runway::from_endpoints(
            Geodetic::from_degrees(0.0, 0.0, 0.0),
            Geodetic::from_degrees(0.01, 0.0, 0.0),
            Meters(45.0),
            Meters::ZERO,
        )
        .expect("north runway should be valid");
        let east = Runway::from_endpoints(
            Geodetic::from_degrees(0.0, 0.0, 0.0),
            Geodetic::from_degrees(0.0, 0.01, 0.0),
            Meters(45.0),
            Meters::ZERO,
        )
        .expect("east runway should be valid");
        assert_close!(north.heading, Radians::ZERO, Radians(1.0e-12));
        assert_close!(east.heading, Degrees(90.0).to_radians(), Radians(1.0e-12));
    }
}
