//! OpenStreetMap PBF から実行時空港 DB を作る。
//!
//! 生の PBF は実行時に読ませない。[`generate_airport_database`] が
//! 滑走路・誘導路中心線、apron、停止位置、明示灯火と依存 node を取り出し、
//! `flightsim-world` が検証して読む固定長形式へ焼く。

use flightsim_core::{Feet, Geodetic, Meters, Radians};
use flightsim_world::airport::io::MAX_RECORD_COUNT;
use flightsim_world::{
    AirportApron, AirportDatabase, AirportGroundLight, AirportHoldingPosition, AirportRunway,
    AirportSourceKind, AirportSurface, AirportTaxiway, GroundFeatureGeometryError, GroundLightKind,
    HoldingPositionType, RunwaySide, TaxiwayGeometryError, TaxiwayLighting, TaxiwayMetadata,
};
use osmpbf::{Element, ElementReader, IndexedReader, RelMemberType};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

const DEFAULT_RUNWAY_WIDTH: Meters = Meters(45.0);
const DEFAULT_TAXIWAY_WIDTH: Meters = Meters(15.0);
const MAX_APRON_TRIANGLE_EDGE_METERS: f64 = 75.0;
const EARTH_RADIUS_METERS: f64 = 6_378_137.0;
const MAX_SIGN_REFERENCE_BYTES: usize = 8;

/// 空港 DB 生成で採用・除外した way の件数。
///
/// 除外件数は way 単位で、最初に該当した理由へだけ加算する。そのため
/// 滑走路の採用数と滑走路用 `skipped_*` の和は `runway_ways_seen` に、誘導路の
/// 採用数と `skipped_taxiway_*` の和は `taxiway_ways_seen` に一致する。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AirportGenerationReport {
    /// PBF 内で `aeroway=runway` だった way の総数。
    pub runway_ways_seen: usize,
    /// `.fsairports` に書いた滑走路数。
    pub runways_written: usize,
    /// `width` が欠落または不正で、45 m を使った滑走路数。
    pub widths_defaulted: usize,
    /// `area=yes` なので面形状として除外した way 数。
    pub skipped_areas: usize,
    /// 先頭と末尾が同じ閉じた way なので面形状として除外した way 数。
    pub skipped_closed: usize,
    /// 先頭または末尾の参照 node が PBF に無く、除外した way 数。
    pub skipped_missing_nodes: usize,
    /// 端点の緯度経度が非有限または範囲外で、除外した way 数。
    pub skipped_bad_coordinates: usize,
    /// 端点を持たない、同一点であるなど、線分を作れず除外した way 数。
    pub skipped_degenerate: usize,
    /// PBF 内で `aeroway=taxiway` だった way の総数。
    pub taxiway_ways_seen: usize,
    /// `.fsairports` に書いた誘導路の折れ線数。
    pub taxiways_written: usize,
    /// `.fsairports` に書いた誘導路の固定長 segment レコード数。
    pub taxiway_segments_written: usize,
    /// `width` が欠落または不正で、15 m を使った誘導路数。
    pub taxiway_widths_defaulted: usize,
    /// `area=yes` なので誘導路中心線から除外した way 数。
    pub skipped_taxiway_areas: usize,
    /// 参照 node が PBF に無く、除外した誘導路 way 数。
    pub skipped_taxiway_missing_nodes: usize,
    /// node の緯度経度が非有限または範囲外で、除外した誘導路 way 数。
    pub skipped_taxiway_bad_coordinates: usize,
    /// 点不足・隣接点の縮退などで線分を作れず、除外した誘導路 way 数。
    pub skipped_taxiway_degenerate: usize,
    /// PBF 内で単純 polygon として見つけた `aeroway=apron` way 数。
    pub apron_ways_seen: usize,
    /// PBF 内で見つけた `aeroway=apron` multipolygon relation 数。
    pub apron_relations_seen: usize,
    /// `.fsairports` に書いた apron polygon 数。
    pub aprons_written: usize,
    /// apron を構成する、最大辺 75 m 以下へ細分した三角形数。
    pub apron_triangles_written: usize,
    /// 閉じていない単純 apron way 数。
    pub skipped_apron_open_ways: usize,
    /// member way が無い、または role が不正な apron relation 数。
    pub skipped_apron_bad_members: usize,
    /// member way を閉じた ring へ接続できなかった apron relation 数。
    pub skipped_apron_unclosed_rings: usize,
    /// 参照 node が無かった apron 数。
    pub skipped_apron_missing_nodes: usize,
    /// node 座標が不正だった apron 数。
    pub skipped_apron_bad_coordinates: usize,
    /// 縮退・自己交差・hole 配置・三角形分割が不正だった apron 数。
    pub skipped_apron_bad_geometry: usize,
    /// PBF 内で見つけた `aeroway=holding_position` node 数。
    pub holding_nodes_seen: usize,
    /// PBF 内で見つけた `aeroway=holding_position` way 数。
    pub holding_ways_seen: usize,
    /// `.fsairports` に書いた停止位置数。
    pub holding_positions_written: usize,
    /// 誘導路へ一意に関連付けられなかった停止位置 node 数。
    pub skipped_holding_unassociated: usize,
    /// node 不足または縮退 geometry で除外した停止位置数。
    pub skipped_holding_bad_geometry: usize,
    /// 不正座標で除外した停止位置数。
    pub skipped_holding_bad_coordinates: usize,
    /// PBF 内で見つけた明示的な空港地上灯 node 数。
    pub ground_light_nodes_seen: usize,
    /// `.fsairports` に書いた明示的な空港地上灯数。
    pub ground_lights_written: usize,
    /// 不正座標で除外した明示灯火 node 数。
    pub skipped_ground_light_bad_coordinates: usize,
    /// DB には保持したが renderer の ASCII 標識にできない `ref` 数。
    pub renderer_ineligible_non_ascii_refs: usize,
    /// DB には保持したが renderer の標識長上限を超える `ref` 数。
    pub renderer_ineligible_long_refs: usize,
}

/// 空港 DB 生成の入出力エラー。
#[derive(Debug)]
pub enum AirportGenError {
    /// 入力 PBF と出力 DB が同じファイルを指している。
    InputOutputConflict {
        /// 読み込もうとした PBF。
        input: PathBuf,
        /// 書き込もうとした DB。
        output: PathBuf,
    },
    /// 入出力が同じファイルか確認できなかった。
    ComparePaths {
        /// 読み込もうとした PBF。
        input: PathBuf,
        /// 書き込もうとした DB。
        output: PathBuf,
        /// OS が返した詳細。
        message: String,
    },
    /// OSM PBF を開く、索引する、またはデコードできなかった。
    ReadPbf {
        /// 読み込もうとしたパス。
        path: PathBuf,
        /// `osmpbf` が返した詳細。
        message: String,
    },
    /// FSAP へ書く候補レコード数が runtime safe limit を超えた。
    RecordLimitExceeded {
        /// 上限を超えると判明した時点の候補レコード数。
        attempted: usize,
        /// FSAP reader / writer と共有する上限。
        maximum: u32,
    },
    /// 変換処理が所有する collection のメモリを確保できなかった。
    AllocationFailed {
        /// 確保しようとしたデータの用途。
        context: &'static str,
        /// 確保しようとした capacity（collection の要素数または文字列の byte 数）。
        requested: usize,
    },
    /// 変換後のレコード集合が DB の不変条件を満たさなかった。
    BuildDatabase {
        /// `flightsim-world` が返した詳細。
        message: String,
    },
    /// `.fsairports` を書けなかった。
    WriteDatabase {
        /// 書き込もうとしたパス。
        path: PathBuf,
        /// ファイルまたはエンコードエラーの詳細。
        message: String,
    },
}

impl fmt::Display for AirportGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputOutputConflict { input, output } => write!(
                f,
                "input OSM PBF {} and output airport database {} refer to the same file",
                input.display(),
                output.display()
            ),
            Self::ComparePaths {
                input,
                output,
                message,
            } => write!(
                f,
                "failed to compare input {} with output {}: {message}",
                input.display(),
                output.display()
            ),
            Self::ReadPbf { path, message } => {
                write!(f, "failed to read OSM PBF {}: {message}", path.display())
            }
            Self::RecordLimitExceeded { attempted, maximum } => write!(
                f,
                "airport conversion encountered {attempted} candidate FSAP records; the safe runtime limit is {maximum}"
            ),
            Self::AllocationFailed { context, requested } => write!(
                f,
                "could not reserve capacity {requested} for airport conversion {context}"
            ),
            Self::BuildDatabase { message } => {
                write!(f, "failed to build airport database: {message}")
            }
            Self::WriteDatabase { path, message } => write!(
                f,
                "failed to write airport database {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for AirportGenError {}

/// OSM PBF の滑走路・誘導路・空港地上設備を `.fsairports` へ変換する。
///
/// way は OSM ID 順に並べてから DB へ渡す。同じ入力 PBF と同じ
/// `flightsim-world` 版からは、常に同じレコード順と同じ bytes が得られる。
///
/// # Errors
///
/// 入出力が同じファイルを指す場合、PBF を読めない場合、候補レコード数が安全上限を
/// 超える場合、変換用 collection を確保できない場合、変換後の DB を構築できない場合、
/// または出力を書けない場合に [`AirportGenError`] を返す。個々の不正 way はエラーで
/// 全体を止めず、理由別に数えて [`AirportGenerationReport`] で報告する。
pub fn generate_airport_database(
    input: &Path,
    output: &Path,
) -> Result<AirportGenerationReport, AirportGenError> {
    ensure_distinct_files(input, output)?;
    let PbfExtraction {
        runway_candidates,
        taxiway_candidates,
        apron_way_candidates,
        apron_relation_candidates,
        apron_member_ways,
        holding_node_candidates,
        holding_way_candidates,
        ground_light_candidates,
        nodes,
        report,
    } = extract_pbf(input)?;
    let (runways, report) = convert_candidates(runway_candidates, &nodes, report)?;
    let holding_taxiways = taxiway_candidates.clone();
    let (taxiways, report) =
        convert_taxiway_candidates(taxiway_candidates, &nodes, &ground_light_candidates, report)?;

    let mut candidate_records = 0_usize;
    add_candidate_records(&mut candidate_records, runways.len())?;
    add_candidate_records(
        &mut candidate_records,
        taxiways
            .iter()
            .map(|taxiway| taxiway.points().len().saturating_sub(1))
            .sum(),
    )?;
    add_candidate_records(&mut candidate_records, taxiways.len())?;
    add_candidate_records(
        &mut candidate_records,
        taxiways
            .iter()
            .filter(|taxiway| taxiway.reference().is_some())
            .count(),
    )?;
    let (aprons, report) = convert_apron_candidates(
        apron_way_candidates,
        apron_relation_candidates,
        &apron_member_ways,
        &nodes,
        report,
        &mut candidate_records,
    )?;
    let (holding_positions, report) = convert_holding_candidates(
        holding_node_candidates,
        holding_way_candidates,
        &holding_taxiways,
        &nodes,
        &runways,
        report,
        &mut candidate_records,
    )?;
    let (ground_lights, mut report) =
        convert_ground_lights(ground_light_candidates, report, &mut candidate_records)?;

    let database = AirportDatabase::with_ground_features(
        runways,
        taxiways,
        aprons,
        holding_positions,
        ground_lights,
    )
    .map_err(|error| AirportGenError::BuildDatabase {
        message: error.to_string(),
    })?;
    report.runways_written = database.runways().len();
    report.taxiways_written = database.taxiways().len();
    report.taxiway_segments_written = database
        .taxiways()
        .iter()
        .map(|taxiway| taxiway.points().len() - 1)
        .sum();
    report.aprons_written = database.aprons().len();
    report.apron_triangles_written = database
        .aprons()
        .iter()
        .map(|apron| apron.triangles().len())
        .sum();
    report.holding_positions_written = database.holding_positions().len();
    report.ground_lights_written = database.ground_lights().len();
    write_database_atomically(&database, output)?;
    Ok(report)
}

fn ensure_distinct_files(input: &Path, output: &Path) -> Result<(), AirportGenError> {
    match same_file::is_same_file(input, output) {
        Ok(true) => Err(AirportGenError::InputOutputConflict {
            input: input.to_path_buf(),
            output: output.to_path_buf(),
        }),
        Ok(false) => Ok(()),
        // 入力または出力がまだ無い場合は後続の読み書きが担当する。特に通常の
        // 新規出力はここへ来る。両方が存在する場合は same-file が正規化済みの
        // ファイル ID を比較するため、別表記・symlink・hard link も検出できる。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AirportGenError::ComparePaths {
            input: input.to_path_buf(),
            output: output.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

fn write_database_atomically(
    database: &AirportDatabase,
    output: &Path,
) -> Result<(), AirportGenError> {
    let bytes = database
        .to_bytes()
        .map_err(|error| AirportGenError::WriteDatabase {
            path: output.to_path_buf(),
            message: error.to_string(),
        })?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".flightsim-airportgen-")
        .tempfile_in(parent)
        .map_err(|error| AirportGenError::WriteDatabase {
            path: output.to_path_buf(),
            message: error.to_string(),
        })?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| AirportGenError::WriteDatabase {
            path: output.to_path_buf(),
            message: error.to_string(),
        })?;
    temporary
        .persist(output)
        .map_err(|error| AirportGenError::WriteDatabase {
            path: output.to_path_buf(),
            message: error.error.to_string(),
        })?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NodeCoordinate {
    latitude_degrees: f64,
    longitude_degrees: f64,
}

impl NodeCoordinate {
    fn is_valid(self) -> bool {
        self.latitude_degrees.is_finite()
            && self.longitude_degrees.is_finite()
            && (-90.0..=90.0).contains(&self.latitude_degrees)
            && (-180.0..=180.0).contains(&self.longitude_degrees)
    }

    fn to_geodetic(self) -> Geodetic {
        Geodetic::from_degrees(self.latitude_degrees, self.longitude_degrees, 0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunwayCandidate {
    source_way_id: i64,
    first_node: Option<i64>,
    last_node: Option<i64>,
    width: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaxiwayCandidate {
    source_way_id: i64,
    node_refs: Vec<i64>,
    width: Option<String>,
    reference: Option<String>,
    surface: Option<String>,
    lit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApronMemberRole {
    Outer,
    Inner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApronRelationMember {
    way_id: i64,
    role: ApronMemberRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApronRelationCandidate {
    source_relation_id: i64,
    surface: Option<String>,
    members: Vec<ApronRelationMember>,
    has_bad_members: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApronWayCandidate {
    source_way_id: i64,
    node_refs: Vec<i64>,
    surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct HoldingNodeCandidate {
    source_node_id: i64,
    coordinate: NodeCoordinate,
    holding_type: HoldingPositionType,
    reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoldingWayDiscovery {
    holding_type: HoldingPositionType,
    reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoldingWayCandidate {
    source_way_id: i64,
    node_refs: Vec<i64>,
    holding_type: HoldingPositionType,
    reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GroundLightCandidate {
    source_node_id: i64,
    coordinate: NodeCoordinate,
    kind: GroundLightKind,
}

#[derive(Debug, Default)]
struct PbfDiscovery {
    apron_relations: Vec<ApronRelationCandidate>,
    apron_member_way_ids: HashSet<i64>,
    holding_ways: HashMap<i64, HoldingWayDiscovery>,
    holding_nodes: Vec<HoldingNodeCandidate>,
    ground_lights: Vec<GroundLightCandidate>,
}

#[derive(Debug)]
struct PbfExtraction {
    runway_candidates: Vec<RunwayCandidate>,
    taxiway_candidates: Vec<TaxiwayCandidate>,
    apron_way_candidates: Vec<ApronWayCandidate>,
    apron_relation_candidates: Vec<ApronRelationCandidate>,
    apron_member_ways: HashMap<i64, Vec<i64>>,
    holding_node_candidates: Vec<HoldingNodeCandidate>,
    holding_way_candidates: Vec<HoldingWayCandidate>,
    ground_light_candidates: Vec<GroundLightCandidate>,
    nodes: HashMap<i64, NodeCoordinate>,
    report: AirportGenerationReport,
}

fn add_candidate_records(total: &mut usize, additional: usize) -> Result<(), AirportGenError> {
    let attempted = total
        .checked_add(additional)
        .ok_or(AirportGenError::RecordLimitExceeded {
            attempted: usize::MAX,
            maximum: MAX_RECORD_COUNT,
        })?;
    if attempted > MAX_RECORD_COUNT as usize {
        return Err(AirportGenError::RecordLimitExceeded {
            attempted,
            maximum: MAX_RECORD_COUNT,
        });
    }
    *total = attempted;
    Ok(())
}

fn copy_optional_tag(
    value: Option<&str>,
    context: &'static str,
) -> Result<Option<String>, AirportGenError> {
    value
        .map(|value| {
            let mut owned = String::new();
            owned.try_reserve_exact(value.len()).map_err(|_| {
                AirportGenError::AllocationFailed {
                    context,
                    requested: value.len(),
                }
            })?;
            owned.push_str(value);
            Ok(owned)
        })
        .transpose()
}

fn reserve_candidate<T>(
    candidates: &mut Vec<T>,
    context: &'static str,
) -> Result<(), AirportGenError> {
    let requested = candidates.len().saturating_add(1);
    candidates
        .try_reserve(1)
        .map_err(|_| AirportGenError::AllocationFailed { context, requested })
}

fn insert_node(
    nodes: &mut HashMap<i64, NodeCoordinate>,
    node_id: i64,
    coordinate: NodeCoordinate,
) -> Result<(), AirportGenError> {
    if !nodes.contains_key(&node_id) {
        let requested = nodes.len().saturating_add(1);
        nodes
            .try_reserve(1)
            .map_err(|_| AirportGenError::AllocationFailed {
                context: "node lookup",
                requested,
            })?;
    }
    nodes.insert(node_id, coordinate);
    Ok(())
}

fn parse_holding_type(value: Option<&str>) -> HoldingPositionType {
    match value.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("intermediate") => {
            HoldingPositionType::Intermediate
        }
        Some(value) if value.eq_ignore_ascii_case("ils") => HoldingPositionType::Ils,
        _ => HoldingPositionType::Runway,
    }
}

fn parse_ground_light_kind(value: Option<&str>) -> Option<GroundLightKind> {
    match value.map(str::trim) {
        Some("txe") => Some(GroundLightKind::TaxiwayEdge),
        Some("txc") => Some(GroundLightKind::TaxiwayCenterline),
        Some("rgl") => Some(GroundLightKind::RunwayGuard),
        _ => None,
    }
}

fn discover_node<'a>(
    node_id: i64,
    coordinate: NodeCoordinate,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
    discovery: &mut PbfDiscovery,
    report: &mut AirportGenerationReport,
) -> Result<(), AirportGenError> {
    let mut aeroway = None;
    let mut navigationaid = None;
    let mut holding_type = None;
    let mut reference = None;
    for (key, value) in tags {
        match key {
            "aeroway" if aeroway.is_none() => aeroway = Some(value),
            "navigationaid" if navigationaid.is_none() => navigationaid = Some(value),
            "holding_position" if holding_type.is_none() => holding_type = Some(value),
            "ref" if reference.is_none() => reference = Some(value),
            _ => {}
        }
    }

    if aeroway == Some("holding_position") {
        reserve_candidate(&mut discovery.holding_nodes, "holding node candidates")?;
        discovery.holding_nodes.push(HoldingNodeCandidate {
            source_node_id: node_id,
            coordinate,
            holding_type: parse_holding_type(holding_type),
            reference: copy_optional_tag(reference, "holding node reference tag")?,
        });
        report.holding_nodes_seen += 1;
    }
    if aeroway == Some("navigationaid") {
        if let Some(kind) = parse_ground_light_kind(navigationaid) {
            reserve_candidate(&mut discovery.ground_lights, "ground light candidates")?;
            discovery.ground_lights.push(GroundLightCandidate {
                source_node_id: node_id,
                coordinate,
                kind,
            });
            report.ground_light_nodes_seen += 1;
        }
    }
    Ok(())
}

fn discover_way<'a>(
    way_id: i64,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
    discovery: &mut PbfDiscovery,
    report: &mut AirportGenerationReport,
) -> Result<(), AirportGenError> {
    let mut aeroway = None;
    let mut aerodrome_marking = None;
    let mut holding_type = None;
    let mut reference = None;
    for (key, value) in tags {
        match key {
            "aeroway" if aeroway.is_none() => aeroway = Some(value),
            "aerodrome_marking" if aerodrome_marking.is_none() => {
                aerodrome_marking = Some(value);
            }
            "holding_position" if holding_type.is_none() => holding_type = Some(value),
            "ref" if reference.is_none() => reference = Some(value),
            _ => {}
        }
    }
    if aeroway != Some("holding_position")
        && !(aeroway == Some("aerodrome_marking") && aerodrome_marking == Some("holding_position"))
    {
        return Ok(());
    }
    if !discovery.holding_ways.contains_key(&way_id) {
        let requested = discovery.holding_ways.len().saturating_add(1);
        discovery
            .holding_ways
            .try_reserve(1)
            .map_err(|_| AirportGenError::AllocationFailed {
                context: "holding way discovery",
                requested,
            })?;
    }
    discovery.holding_ways.insert(
        way_id,
        HoldingWayDiscovery {
            holding_type: parse_holding_type(holding_type),
            reference: copy_optional_tag(reference, "holding way reference tag")?,
        },
    );
    report.holding_ways_seen += 1;
    Ok(())
}

fn discover_relation<'a>(
    relation: &osmpbf::Relation<'a>,
    discovery: &mut PbfDiscovery,
    report: &mut AirportGenerationReport,
) -> Result<(), AirportGenError> {
    let mut relation_type = None;
    let mut aeroway = None;
    let mut surface = None;
    for (key, value) in relation.tags() {
        match key {
            "type" if relation_type.is_none() => relation_type = Some(value),
            "aeroway" if aeroway.is_none() => aeroway = Some(value),
            "surface" if surface.is_none() => surface = Some(value),
            _ => {}
        }
    }
    if relation_type != Some("multipolygon") || aeroway != Some("apron") {
        return Ok(());
    }

    report.apron_relations_seen += 1;
    reserve_candidate(&mut discovery.apron_relations, "apron relation candidates")?;
    let member_count = relation.members().count();
    let mut members = Vec::new();
    members
        .try_reserve_exact(member_count)
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "apron relation members",
            requested: member_count,
        })?;
    let mut has_bad_members = false;
    for member in relation.members() {
        if member.member_type != RelMemberType::Way {
            has_bad_members = true;
            continue;
        }
        let role = match member.role() {
            Ok("" | "outer") => ApronMemberRole::Outer,
            Ok("inner") => ApronMemberRole::Inner,
            Ok(_) | Err(_) => {
                has_bad_members = true;
                continue;
            }
        };
        if !discovery.apron_member_way_ids.contains(&member.member_id) {
            let requested = discovery.apron_member_way_ids.len().saturating_add(1);
            discovery.apron_member_way_ids.try_reserve(1).map_err(|_| {
                AirportGenError::AllocationFailed {
                    context: "apron member way ids",
                    requested,
                }
            })?;
        }
        discovery.apron_member_way_ids.insert(member.member_id);
        members.push(ApronRelationMember {
            way_id: member.member_id,
            role,
        });
    }
    if members.is_empty() {
        has_bad_members = true;
    }
    discovery.apron_relations.push(ApronRelationCandidate {
        source_relation_id: relation.id(),
        surface: copy_optional_tag(surface, "apron relation surface tag")?,
        members,
        has_bad_members,
    });
    Ok(())
}

fn discover_pbf(
    path: &Path,
    report: &mut AirportGenerationReport,
) -> Result<PbfDiscovery, AirportGenError> {
    let reader = ElementReader::from_path(path).map_err(|error| AirportGenError::ReadPbf {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut discovery = PbfDiscovery::default();
    let mut discovery_error = None;
    reader
        .for_each(|element| {
            if discovery_error.is_some() {
                return;
            }
            let result = match element {
                Element::Node(node) => discover_node(
                    node.id(),
                    NodeCoordinate {
                        latitude_degrees: node.lat(),
                        longitude_degrees: node.lon(),
                    },
                    node.tags(),
                    &mut discovery,
                    report,
                ),
                Element::DenseNode(node) => discover_node(
                    node.id(),
                    NodeCoordinate {
                        latitude_degrees: node.lat(),
                        longitude_degrees: node.lon(),
                    },
                    node.tags(),
                    &mut discovery,
                    report,
                ),
                Element::Way(way) => discover_way(way.id(), way.tags(), &mut discovery, report),
                Element::Relation(relation) => discover_relation(&relation, &mut discovery, report),
            };
            if let Err(error) = result {
                discovery_error = Some(error);
            }
        })
        .map_err(|error| AirportGenError::ReadPbf {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if let Some(error) = discovery_error {
        return Err(error);
    }
    discovery
        .apron_relations
        .sort_unstable_by_key(|candidate| candidate.source_relation_id);
    discovery
        .holding_nodes
        .sort_unstable_by_key(|candidate| candidate.source_node_id);
    discovery
        .ground_lights
        .sort_unstable_by_key(|candidate| candidate.source_node_id);
    Ok(discovery)
}

fn extract_pbf(path: &Path) -> Result<PbfExtraction, AirportGenError> {
    let mut report = AirportGenerationReport::default();
    let discovery = discover_pbf(path, &mut report)?;
    let mut reader = IndexedReader::from_path(path).map_err(|error| AirportGenError::ReadPbf {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    let mut runway_candidates = Vec::new();
    let mut taxiway_candidates = Vec::new();
    let mut apron_way_candidates = Vec::new();
    let mut apron_member_ways = HashMap::new();
    let mut holding_way_candidates = Vec::new();
    let mut nodes = HashMap::new();
    let mut candidate_records = 0_usize;
    add_candidate_records(&mut candidate_records, discovery.holding_nodes.len())?;
    add_candidate_records(&mut candidate_records, discovery.ground_lights.len())?;
    add_candidate_records(&mut candidate_records, discovery.holding_ways.len())?;
    let halted = Cell::new(false);
    let mut way_error = None;
    let mut node_error = None;

    let read_result = reader.read_ways_and_deps(
        |way| {
            if halted.get() {
                return false;
            }

            let result = (|| {
                let mut aeroway = None;
                let mut area = None;
                let mut width = None;
                let mut reference = None;
                let mut surface = None;
                let mut lit = None;
                for (key, value) in way.tags() {
                    match key {
                        "aeroway" if aeroway.is_none() => aeroway = Some(value),
                        "area" if area.is_none() => area = Some(value),
                        "width" if width.is_none() => width = Some(value),
                        "ref" if reference.is_none() => reference = Some(value),
                        "surface" if surface.is_none() => surface = Some(value),
                        "lit" if lit.is_none() => lit = Some(value),
                        _ => {}
                    }
                }

                let node_count = way.refs().count();
                let is_apron_member = discovery.apron_member_way_ids.contains(&way.id());
                let holding_discovery = discovery.holding_ways.get(&way.id());
                let needs_all_refs = aeroway == Some("taxiway")
                    || aeroway == Some("apron")
                    || is_apron_member
                    || holding_discovery.is_some();
                let mut all_refs = Vec::new();
                if needs_all_refs {
                    all_refs.try_reserve_exact(node_count).map_err(|_| {
                        AirportGenError::AllocationFailed {
                            context: "selected way node references",
                            requested: node_count,
                        }
                    })?;
                    all_refs.extend(way.refs());
                }

                let mut selected = false;
                if is_apron_member {
                    if !apron_member_ways.contains_key(&way.id()) {
                        let requested = apron_member_ways.len().saturating_add(1);
                        apron_member_ways.try_reserve(1).map_err(|_| {
                            AirportGenError::AllocationFailed {
                                context: "apron member ways",
                                requested,
                            }
                        })?;
                    }
                    apron_member_ways.insert(way.id(), all_refs.clone());
                    selected = true;
                }
                if let Some(holding) = holding_discovery {
                    reserve_candidate(&mut holding_way_candidates, "holding way candidates")?;
                    holding_way_candidates.push(HoldingWayCandidate {
                        source_way_id: way.id(),
                        node_refs: all_refs.clone(),
                        holding_type: holding.holding_type,
                        reference: holding.reference.clone(),
                    });
                    selected = true;
                }
                if aeroway == Some("apron") {
                    report.apron_ways_seen += 1;
                }
                // relation member は relation 側を一意な source とし、単純 way として重複保存しない。
                if aeroway == Some("apron") && !is_apron_member {
                    if node_count < 4 {
                        report.skipped_apron_bad_geometry += 1;
                    } else if all_refs.first() != all_refs.last() {
                        report.skipped_apron_open_ways += 1;
                    } else {
                        reserve_candidate(&mut apron_way_candidates, "apron way candidates")?;
                        apron_way_candidates.push(ApronWayCandidate {
                            source_way_id: way.id(),
                            node_refs: all_refs.clone(),
                            surface: copy_optional_tag(surface, "apron way surface tag")?,
                        });
                        selected = true;
                    }
                }

                let first_node = (aeroway == Some("runway"))
                    .then(|| way.refs().next())
                    .flatten();
                let last_node = (aeroway == Some("runway"))
                    .then(|| way.refs().last())
                    .flatten();
                match classify_way_parts(
                    aeroway,
                    area == Some("yes"),
                    node_count,
                    first_node,
                    last_node,
                ) {
                    WayDisposition::Other => {}
                    WayDisposition::RunwayArea => {
                        report.runway_ways_seen += 1;
                        report.skipped_areas += 1;
                    }
                    WayDisposition::RunwayClosed => {
                        report.runway_ways_seen += 1;
                        report.skipped_closed += 1;
                    }
                    WayDisposition::RunwayCenterline => {
                        report.runway_ways_seen += 1;
                        add_candidate_records(&mut candidate_records, 1)?;
                        reserve_candidate(&mut runway_candidates, "runway candidates")?;
                        runway_candidates.push(RunwayCandidate {
                            source_way_id: way.id(),
                            first_node,
                            last_node,
                            width: copy_optional_tag(width, "runway width tag")?,
                        });
                        selected = true;
                    }
                    WayDisposition::TaxiwayArea => {
                        report.taxiway_ways_seen += 1;
                        report.skipped_taxiway_areas += 1;
                    }
                    WayDisposition::TaxiwayCenterline if node_count < 2 => {
                        report.taxiway_ways_seen += 1;
                        report.skipped_taxiway_degenerate += 1;
                    }
                    WayDisposition::TaxiwayCenterline => {
                        report.taxiway_ways_seen += 1;
                        add_candidate_records(&mut candidate_records, node_count - 1)?;
                        reserve_candidate(&mut taxiway_candidates, "taxiway candidates")?;
                        taxiway_candidates.push(TaxiwayCandidate {
                            source_way_id: way.id(),
                            node_refs: all_refs,
                            width: copy_optional_tag(width, "taxiway width tag")?,
                            reference: copy_optional_tag(reference, "taxiway reference tag")?,
                            surface: copy_optional_tag(surface, "taxiway surface tag")?,
                            lit: copy_optional_tag(lit, "taxiway lit tag")?,
                        });
                        selected = true;
                    }
                }
                Ok(selected)
            })();
            match result {
                Ok(selected) => selected,
                Err(error) => {
                    way_error = Some(error);
                    halted.set(true);
                    false
                }
            }
        },
        |element| {
            if halted.get() {
                return;
            }
            let result = match element {
                Element::Node(node) => insert_node(
                    &mut nodes,
                    node.id(),
                    NodeCoordinate {
                        latitude_degrees: node.lat(),
                        longitude_degrees: node.lon(),
                    },
                ),
                Element::DenseNode(node) => insert_node(
                    &mut nodes,
                    node.id(),
                    NodeCoordinate {
                        latitude_degrees: node.lat(),
                        longitude_degrees: node.lon(),
                    },
                ),
                Element::Way(_) | Element::Relation(_) => Ok(()),
            };
            if let Err(error) = result {
                node_error = Some(error);
                halted.set(true);
            }
        },
    );
    if let Some(error) = way_error.or(node_error) {
        return Err(error);
    }
    read_result.map_err(|error| AirportGenError::ReadPbf {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    Ok(PbfExtraction {
        runway_candidates,
        taxiway_candidates,
        apron_way_candidates,
        apron_relation_candidates: discovery.apron_relations,
        apron_member_ways,
        holding_node_candidates: discovery.holding_nodes,
        holding_way_candidates,
        ground_light_candidates: discovery.ground_lights,
        nodes,
        report,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WayDisposition {
    Other,
    RunwayArea,
    RunwayClosed,
    RunwayCenterline,
    TaxiwayArea,
    TaxiwayCenterline,
}

#[cfg(test)]
fn classify_way(tags: &[(&str, &str)], node_refs: &[i64]) -> WayDisposition {
    classify_way_parts(
        tag_value(tags, "aeroway"),
        tag_value(tags, "area") == Some("yes"),
        node_refs.len(),
        node_refs.first().copied(),
        node_refs.last().copied(),
    )
}

fn classify_way_parts(
    aeroway: Option<&str>,
    is_area: bool,
    node_count: usize,
    first_node: Option<i64>,
    last_node: Option<i64>,
) -> WayDisposition {
    if aeroway == Some("taxiway") {
        return if is_area {
            WayDisposition::TaxiwayArea
        } else {
            WayDisposition::TaxiwayCenterline
        };
    }
    if aeroway != Some("runway") {
        return WayDisposition::Other;
    }
    if is_area {
        return WayDisposition::RunwayArea;
    }
    // 参照 1 個だけの壊れた way は面ではなく縮退線として後段で数える。
    if node_count >= 2 && first_node == last_node {
        return WayDisposition::RunwayClosed;
    }
    WayDisposition::RunwayCenterline
}

#[cfg(test)]
fn tag_value<'a>(tags: &'a [(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    tags.iter()
        .find_map(|(candidate, value)| (*candidate == key).then_some(*value))
}

fn convert_candidates(
    mut candidates: Vec<RunwayCandidate>,
    nodes: &HashMap<i64, NodeCoordinate>,
    mut report: AirportGenerationReport,
) -> Result<(Vec<AirportRunway>, AirportGenerationReport), AirportGenError> {
    candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut runways = Vec::new();
    runways
        .try_reserve_exact(candidates.len())
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "converted runways",
            requested: candidates.len(),
        })?;

    for candidate in candidates {
        let (Some(first_node), Some(last_node)) = (candidate.first_node, candidate.last_node)
        else {
            report.skipped_degenerate += 1;
            continue;
        };
        if first_node == last_node {
            report.skipped_degenerate += 1;
            continue;
        }

        let (Some(first), Some(last)) = (nodes.get(&first_node), nodes.get(&last_node)) else {
            report.skipped_missing_nodes += 1;
            continue;
        };
        if !first.is_valid() || !last.is_valid() {
            report.skipped_bad_coordinates += 1;
            continue;
        }

        let (width, defaulted) = parse_width(candidate.width.as_deref(), DEFAULT_RUNWAY_WIDTH);
        let Ok(runway) = AirportRunway::from_endpoints(
            candidate.source_way_id,
            first.to_geodetic(),
            last.to_geodetic(),
            width,
        ) else {
            // 有限で範囲内の端点と正の幅は上で保証した。残る幾何エラーは、
            // 同一点（極で経度だけが違う場合を含む）など線分の縮退である。
            report.skipped_degenerate += 1;
            continue;
        };

        if defaulted {
            report.widths_defaulted += 1;
        }
        runways.push(runway);
    }

    report.runways_written = runways.len();
    Ok((runways, report))
}

fn convert_taxiway_candidates(
    mut candidates: Vec<TaxiwayCandidate>,
    nodes: &HashMap<i64, NodeCoordinate>,
    explicit_lights: &[GroundLightCandidate],
    mut report: AirportGenerationReport,
) -> Result<(Vec<AirportTaxiway>, AirportGenerationReport), AirportGenError> {
    candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut taxiways = Vec::new();
    taxiways.try_reserve_exact(candidates.len()).map_err(|_| {
        AirportGenError::AllocationFailed {
            context: "converted taxiways",
            requested: candidates.len(),
        }
    })?;

    for candidate in candidates {
        if candidate.node_refs.len() < 2 {
            report.skipped_taxiway_degenerate += 1;
            continue;
        }
        if candidate
            .node_refs
            .iter()
            .any(|node_id| !nodes.contains_key(node_id))
        {
            report.skipped_taxiway_missing_nodes += 1;
            continue;
        }
        if candidate.node_refs.iter().any(|node_id| {
            nodes
                .get(node_id)
                .is_none_or(|coordinate| !coordinate.is_valid())
        }) {
            report.skipped_taxiway_bad_coordinates += 1;
            continue;
        }
        let (width, defaulted) = parse_width(candidate.width.as_deref(), DEFAULT_TAXIWAY_WIDTH);
        let mut points = Vec::new();
        points
            .try_reserve_exact(candidate.node_refs.len())
            .map_err(|_| AirportGenError::AllocationFailed {
                context: "taxiway geodetic points",
                requested: candidate.node_refs.len(),
            })?;
        for node_id in &candidate.node_refs {
            if let Some(coordinate) = nodes.get(node_id) {
                points.push(coordinate.to_geodetic());
            }
        }
        let reference = candidate.reference.filter(|value| !value.is_empty());
        let lighting = taxiway_lighting_with_explicit_points(
            candidate.lit.as_deref(),
            &candidate.node_refs,
            width,
            nodes,
            explicit_lights,
        );
        let metadata = TaxiwayMetadata::new(
            reference.clone(),
            parse_surface(candidate.surface.as_deref()),
            lighting,
        );
        let taxiway = match AirportTaxiway::from_points_with_metadata(
            candidate.source_way_id,
            points,
            width,
            metadata,
        ) {
            Ok(taxiway) => taxiway,
            Err(TaxiwayGeometryError::AllocationFailed { requested }) => {
                return Err(AirportGenError::AllocationFailed {
                    context: "taxiway stored points",
                    requested,
                });
            }
            Err(_) => {
                report.skipped_taxiway_degenerate += 1;
                continue;
            }
        };
        if defaulted {
            report.taxiway_widths_defaulted += 1;
        }
        note_renderer_ineligible_reference(reference.as_deref(), &mut report);
        taxiways.push(taxiway);
    }
    report.taxiways_written = taxiways.len();
    report.taxiway_segments_written = taxiways
        .iter()
        .map(|taxiway| taxiway.points().len() - 1)
        .sum();
    Ok((taxiways, report))
}

fn parse_surface(value: Option<&str>) -> AirportSurface {
    match value.map(str::trim) {
        Some("asphalt") => AirportSurface::Asphalt,
        Some("concrete" | "concrete:lanes" | "concrete:plates") => AirportSurface::Concrete,
        Some("paved" | "paving_stones" | "sett") => AirportSurface::Paved,
        Some("grass") => AirportSurface::Grass,
        Some("gravel" | "fine_gravel") => AirportSurface::Gravel,
        Some("dirt" | "earth" | "ground") => AirportSurface::Dirt,
        Some("sand") => AirportSurface::Sand,
        _ => AirportSurface::Unknown,
    }
}

fn parse_taxiway_lighting(value: Option<&str>) -> TaxiwayLighting {
    match value.map(str::trim) {
        Some("no") => TaxiwayLighting::None,
        Some("edge") => TaxiwayLighting::Edge,
        Some("centerline" | "centreline") => TaxiwayLighting::Centerline,
        Some("edge_and_centerline" | "edge_and_centreline") => TaxiwayLighting::EdgeAndCenterline,
        Some("yes") | None | Some(_) => TaxiwayLighting::EdgeAndCenterline,
    }
}

fn distance_to_segment_meters(
    point: NodeCoordinate,
    first: NodeCoordinate,
    last: NodeCoordinate,
) -> Option<(f64, f64)> {
    let first = project_coordinate(point, first);
    let last = project_coordinate(point, last);
    let dx = last.x - first.x;
    let dy = last.y - first.y;
    let length_squared = dx * dx + dy * dy;
    if !length_squared.is_finite() || length_squared <= f64::EPSILON {
        return None;
    }
    let fraction = (-(first.x * dx + first.y * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = first.x + fraction * dx;
    let closest_y = first.y + fraction * dy;
    Some((
        (closest_x * closest_x + closest_y * closest_y).sqrt(),
        fraction,
    ))
}

fn taxiway_lighting_with_explicit_points(
    lit: Option<&str>,
    node_refs: &[i64],
    width: Meters,
    nodes: &HashMap<i64, NodeCoordinate>,
    explicit_lights: &[GroundLightCandidate],
) -> TaxiwayLighting {
    let requested = parse_taxiway_lighting(lit);
    if requested == TaxiwayLighting::None {
        return requested;
    }
    let corridor = width.get() * 0.5 + 1.0;
    let contains_kind = |kind| {
        explicit_lights.iter().any(|light| {
            light.kind == kind
                && node_refs.windows(2).any(|pair| {
                    let (Some(first), Some(last)) = (nodes.get(&pair[0]), nodes.get(&pair[1]))
                    else {
                        return false;
                    };
                    distance_to_segment_meters(light.coordinate, *first, *last)
                        .is_some_and(|(distance, _)| distance <= corridor)
                })
        })
    };
    let explicit_edge = contains_kind(GroundLightKind::TaxiwayEdge);
    let explicit_centerline = contains_kind(GroundLightKind::TaxiwayCenterline);
    match (requested, explicit_edge, explicit_centerline) {
        (TaxiwayLighting::EdgeAndCenterline, true, true) => TaxiwayLighting::None,
        (TaxiwayLighting::EdgeAndCenterline, true, false) => TaxiwayLighting::Centerline,
        (TaxiwayLighting::EdgeAndCenterline, false, true) => TaxiwayLighting::Edge,
        (TaxiwayLighting::Edge, true, _) | (TaxiwayLighting::Centerline, _, true) => {
            TaxiwayLighting::None
        }
        _ => requested,
    }
}

fn note_renderer_ineligible_reference(
    reference: Option<&str>,
    report: &mut AirportGenerationReport,
) {
    let Some(reference) = reference else {
        return;
    };
    if !reference.is_ascii() {
        report.renderer_ineligible_non_ascii_refs += 1;
    } else if reference.len() > MAX_SIGN_REFERENCE_BYTES {
        report.renderer_ineligible_long_refs += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApronBuildFailure {
    MissingMember,
    UnclosedRing,
    MissingNode,
    BadCoordinate,
    BadGeometry,
    Allocation(usize),
    RecordLimit(usize),
}

#[derive(Debug, Clone, Copy)]
struct ProjectedPoint {
    x: f64,
    y: f64,
}

fn project_coordinate(origin: NodeCoordinate, point: NodeCoordinate) -> ProjectedPoint {
    let latitude = origin.latitude_degrees.to_radians();
    let mut longitude_delta = (point.longitude_degrees - origin.longitude_degrees).to_radians();
    if longitude_delta > core::f64::consts::PI {
        longitude_delta -= core::f64::consts::TAU;
    } else if longitude_delta < -core::f64::consts::PI {
        longitude_delta += core::f64::consts::TAU;
    }
    ProjectedPoint {
        x: longitude_delta * latitude.cos() * EARTH_RADIUS_METERS,
        y: (point.latitude_degrees - origin.latitude_degrees).to_radians() * EARTH_RADIUS_METERS,
    }
}

fn ring_area(points: &[ProjectedPoint]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(left, right)| left.x * right.y - right.x * left.y)
        .sum::<f64>()
        * 0.5
}

fn orientation(a: ProjectedPoint, b: ProjectedPoint, c: ProjectedPoint) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn segments_intersect(
    a: ProjectedPoint,
    b: ProjectedPoint,
    c: ProjectedPoint,
    d: ProjectedPoint,
) -> bool {
    let first = orientation(a, b, c);
    let second = orientation(a, b, d);
    let third = orientation(c, d, a);
    let fourth = orientation(c, d, b);
    let crosses = (first > 0.0 && second < 0.0 || first < 0.0 && second > 0.0)
        && (third > 0.0 && fourth < 0.0 || third < 0.0 && fourth > 0.0);
    let on_segment =
        |value: f64, point: ProjectedPoint, left: ProjectedPoint, right: ProjectedPoint| {
            value.abs() <= 1.0e-7
                && point.x >= left.x.min(right.x) - 1.0e-7
                && point.x <= left.x.max(right.x) + 1.0e-7
                && point.y >= left.y.min(right.y) - 1.0e-7
                && point.y <= left.y.max(right.y) + 1.0e-7
        };
    crosses
        || on_segment(first, c, a, b)
        || on_segment(second, d, a, b)
        || on_segment(third, a, c, d)
        || on_segment(fourth, b, c, d)
}

fn rings_intersect(left: &[ProjectedPoint], right: &[ProjectedPoint]) -> bool {
    left.iter()
        .zip(left.iter().cycle().skip(1))
        .take(left.len())
        .any(|(left_start, left_end)| {
            right
                .iter()
                .zip(right.iter().cycle().skip(1))
                .take(right.len())
                .any(|(right_start, right_end)| {
                    segments_intersect(*left_start, *left_end, *right_start, *right_end)
                })
        })
}

fn validate_projected_ring(points: &[ProjectedPoint]) -> bool {
    if points.len() < 3 || ring_area(points).abs() <= 1.0e-6 {
        return false;
    }
    for edge in 0..points.len() {
        let next = (edge + 1) % points.len();
        if ((points[edge].x - points[next].x).powi(2) + (points[edge].y - points[next].y).powi(2))
            .sqrt()
            <= 1.0e-9
            || (0..points.len()).any(|other| {
                let other_next = (other + 1) % points.len();
                edge != other
                    && next != other
                    && edge != other_next
                    && segments_intersect(
                        points[edge],
                        points[next],
                        points[other],
                        points[other_next],
                    )
            })
        {
            return false;
        }
    }
    true
}

fn point_in_ring(point: ProjectedPoint, ring: &[ProjectedPoint]) -> bool {
    let mut inside = false;
    for (left, right) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        if (left.y > point.y) != (right.y > point.y) {
            let intersection =
                (right.x - left.x) * (point.y - left.y) / (right.y - left.y) + left.x;
            if point.x < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

fn stitch_member_rings(
    members: &[ApronRelationMember],
    role: ApronMemberRole,
    member_ways: &HashMap<i64, Vec<i64>>,
) -> Result<Vec<Vec<i64>>, ApronBuildFailure> {
    let mut fragments = Vec::new();
    for member in members.iter().filter(|member| member.role == role) {
        let refs = member_ways
            .get(&member.way_id)
            .ok_or(ApronBuildFailure::MissingMember)?;
        if refs.len() < 2 {
            return Err(ApronBuildFailure::BadGeometry);
        }
        reserve_candidate(&mut fragments, "apron ring fragments")
            .map_err(|_| ApronBuildFailure::Allocation(fragments.len().saturating_add(1)))?;
        fragments.push((member.way_id, refs.clone()));
    }
    fragments.sort_unstable_by_key(|(way_id, _)| *way_id);
    let mut rings = Vec::new();
    while !fragments.is_empty() {
        let (_, mut ring) = fragments.remove(0);
        if ring.first() != ring.last() && ring.last() < ring.first() {
            ring.reverse();
        }
        while ring.first() != ring.last() {
            let end = *ring.last().ok_or(ApronBuildFailure::BadGeometry)?;
            let next = fragments.iter().enumerate().find_map(|(index, (_, refs))| {
                if refs.first() == Some(&end) {
                    Some((index, false))
                } else if refs.last() == Some(&end) {
                    Some((index, true))
                } else {
                    None
                }
            });
            let Some((index, reverse)) = next else {
                return Err(ApronBuildFailure::UnclosedRing);
            };
            let (_, mut refs) = fragments.remove(index);
            if reverse {
                refs.reverse();
            }
            ring.try_reserve(refs.len().saturating_sub(1))
                .map_err(|_| ApronBuildFailure::Allocation(ring.len() + refs.len()))?;
            ring.extend(refs.into_iter().skip(1));
        }
        if ring.len() < 4 {
            return Err(ApronBuildFailure::BadGeometry);
        }
        ring.pop();
        reserve_candidate(&mut rings, "stitched apron rings")
            .map_err(|_| ApronBuildFailure::Allocation(rings.len().saturating_add(1)))?;
        rings.push(ring);
    }
    Ok(rings)
}

fn ring_coordinates(
    node_refs: &[i64],
    nodes: &HashMap<i64, NodeCoordinate>,
) -> Result<Vec<NodeCoordinate>, ApronBuildFailure> {
    let refs = if node_refs.first() == node_refs.last() {
        &node_refs[..node_refs.len().saturating_sub(1)]
    } else {
        node_refs
    };
    if refs.len() < 3 {
        return Err(ApronBuildFailure::BadGeometry);
    }
    let mut coordinates = Vec::new();
    coordinates
        .try_reserve_exact(refs.len())
        .map_err(|_| ApronBuildFailure::Allocation(refs.len()))?;
    for node_id in refs {
        let coordinate = nodes
            .get(node_id)
            .copied()
            .ok_or(ApronBuildFailure::MissingNode)?;
        if !coordinate.is_valid() {
            return Err(ApronBuildFailure::BadCoordinate);
        }
        coordinates.push(coordinate);
    }
    Ok(coordinates)
}

fn coordinate_distance_meters(left: NodeCoordinate, right: NodeCoordinate) -> f64 {
    let latitude_delta = (right.latitude_degrees - left.latitude_degrees).to_radians();
    let longitude_delta = (right.longitude_degrees - left.longitude_degrees).to_radians();
    let left_latitude = left.latitude_degrees.to_radians();
    let right_latitude = right.latitude_degrees.to_radians();
    let haversine = (latitude_delta * 0.5).sin().powi(2)
        + left_latitude.cos() * right_latitude.cos() * (longitude_delta * 0.5).sin().powi(2);
    2.0 * EARTH_RADIUS_METERS * haversine.sqrt().asin()
}

fn midpoint(left: NodeCoordinate, right: NodeCoordinate) -> NodeCoordinate {
    let mut longitude_delta = right.longitude_degrees - left.longitude_degrees;
    if longitude_delta > 180.0 {
        longitude_delta -= 360.0;
    } else if longitude_delta < -180.0 {
        longitude_delta += 360.0;
    }
    let longitude = left.longitude_degrees + longitude_delta * 0.5;
    NodeCoordinate {
        latitude_degrees: (left.latitude_degrees + right.latitude_degrees) * 0.5,
        longitude_degrees: if longitude > 180.0 {
            longitude - 360.0
        } else if longitude < -180.0 {
            longitude + 360.0
        } else {
            longitude
        },
    }
}

fn subdivide_triangle(
    triangle: [NodeCoordinate; 3],
    triangles: &mut Vec<[Geodetic; 3]>,
    candidate_records: &mut usize,
) -> Result<(), AirportGenError> {
    let mut pending = Vec::new();
    pending
        .try_reserve(1)
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "apron subdivision stack",
            requested: 1,
        })?;
    pending.push(triangle);
    while let Some(triangle) = pending.pop() {
        let lengths = [
            coordinate_distance_meters(triangle[0], triangle[1]),
            coordinate_distance_meters(triangle[1], triangle[2]),
            coordinate_distance_meters(triangle[2], triangle[0]),
        ];
        let longest = lengths
            .iter()
            .enumerate()
            .max_by(|(left_index, left), (right_index, right)| {
                left.total_cmp(right)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map_or(0, |(index, _)| index);
        if lengths[longest] <= MAX_APRON_TRIANGLE_EDGE_METERS {
            add_candidate_records(candidate_records, 1)?;
            reserve_candidate(triangles, "subdivided apron triangles")?;
            triangles.push(triangle.map(NodeCoordinate::to_geodetic));
            continue;
        }
        let (first, second, opposite) = match longest {
            0 => (triangle[0], triangle[1], triangle[2]),
            1 => (triangle[1], triangle[2], triangle[0]),
            _ => (triangle[2], triangle[0], triangle[1]),
        };
        let middle = midpoint(first, second);
        pending
            .try_reserve(2)
            .map_err(|_| AirportGenError::AllocationFailed {
                context: "apron subdivision stack",
                requested: pending.len().saturating_add(2),
            })?;
        pending.push([middle, second, opposite]);
        pending.push([first, middle, opposite]);
        if pending.len() > MAX_RECORD_COUNT as usize {
            return Err(AirportGenError::RecordLimitExceeded {
                attempted: pending.len(),
                maximum: MAX_RECORD_COUNT,
            });
        }
    }
    Ok(())
}

fn triangulate_rings(
    outer: &[NodeCoordinate],
    holes: &[Vec<NodeCoordinate>],
    triangles: &mut Vec<[Geodetic; 3]>,
    candidate_records: &mut usize,
) -> Result<(), ApronBuildFailure> {
    let origin = *outer.first().ok_or(ApronBuildFailure::BadGeometry)?;
    let projected_outer: Vec<_> = outer
        .iter()
        .copied()
        .map(|point| project_coordinate(origin, point))
        .collect();
    if !validate_projected_ring(&projected_outer) {
        return Err(ApronBuildFailure::BadGeometry);
    }

    let point_count = outer
        .len()
        .saturating_add(holes.iter().map(Vec::len).sum::<usize>());
    let mut coordinates = Vec::new();
    coordinates
        .try_reserve_exact(point_count)
        .map_err(|_| ApronBuildFailure::Allocation(point_count))?;
    coordinates.extend_from_slice(outer);
    let mut flat = Vec::new();
    flat.try_reserve_exact(point_count.saturating_mul(2))
        .map_err(|_| ApronBuildFailure::Allocation(point_count.saturating_mul(2)))?;
    for point in &projected_outer {
        flat.extend([point.x, point.y]);
    }
    let mut hole_indices = Vec::new();
    hole_indices
        .try_reserve_exact(holes.len())
        .map_err(|_| ApronBuildFailure::Allocation(holes.len()))?;
    let mut projected_holes = Vec::new();
    projected_holes
        .try_reserve_exact(holes.len())
        .map_err(|_| ApronBuildFailure::Allocation(holes.len()))?;
    for hole in holes {
        hole_indices.push(coordinates.len());
        let mut projected = Vec::new();
        projected
            .try_reserve_exact(hole.len())
            .map_err(|_| ApronBuildFailure::Allocation(hole.len()))?;
        projected.extend(
            hole.iter()
                .copied()
                .map(|point| project_coordinate(origin, point)),
        );
        if !validate_projected_ring(&projected)
            || !point_in_ring(projected[0], &projected_outer)
            || rings_intersect(&projected_outer, &projected)
        {
            return Err(ApronBuildFailure::BadGeometry);
        }
        coordinates.extend_from_slice(hole);
        for point in &projected {
            flat.extend([point.x, point.y]);
        }
        projected_holes.push(projected);
    }
    for (index, left) in projected_holes.iter().enumerate() {
        for right in projected_holes.iter().skip(index + 1) {
            if rings_intersect(left, right)
                || point_in_ring(left[0], right)
                || point_in_ring(right[0], left)
            {
                return Err(ApronBuildFailure::BadGeometry);
            }
        }
    }
    let indices =
        earcutr::earcut(&flat, &hole_indices, 2).map_err(|_| ApronBuildFailure::BadGeometry)?;
    if indices.is_empty() || indices.len() % 3 != 0 {
        return Err(ApronBuildFailure::BadGeometry);
    }
    for indices in indices.chunks_exact(3) {
        let triangle = [
            coordinates[indices[0]],
            coordinates[indices[1]],
            coordinates[indices[2]],
        ];
        subdivide_triangle(triangle, triangles, candidate_records).map_err(
            |error| match error {
                AirportGenError::AllocationFailed { requested, .. } => {
                    ApronBuildFailure::Allocation(requested)
                }
                AirportGenError::RecordLimitExceeded { attempted, .. } => {
                    ApronBuildFailure::RecordLimit(attempted)
                }
                _ => ApronBuildFailure::BadGeometry,
            },
        )?;
    }
    Ok(())
}

fn handle_apron_failure(
    failure: ApronBuildFailure,
    report: &mut AirportGenerationReport,
) -> Result<(), AirportGenError> {
    match failure {
        ApronBuildFailure::MissingMember => report.skipped_apron_bad_members += 1,
        ApronBuildFailure::UnclosedRing => report.skipped_apron_unclosed_rings += 1,
        ApronBuildFailure::MissingNode => report.skipped_apron_missing_nodes += 1,
        ApronBuildFailure::BadCoordinate => report.skipped_apron_bad_coordinates += 1,
        ApronBuildFailure::BadGeometry => report.skipped_apron_bad_geometry += 1,
        ApronBuildFailure::Allocation(requested) => {
            return Err(AirportGenError::AllocationFailed {
                context: "apron geometry",
                requested,
            });
        }
        ApronBuildFailure::RecordLimit(attempted) => {
            return Err(AirportGenError::RecordLimitExceeded {
                attempted,
                maximum: MAX_RECORD_COUNT,
            });
        }
    }
    Ok(())
}

fn build_apron(
    source_kind: AirportSourceKind,
    source_id: i64,
    surface: AirportSurface,
    outer_rings: &[Vec<NodeCoordinate>],
    inner_rings: Vec<Vec<NodeCoordinate>>,
    candidate_records: &mut usize,
) -> Result<AirportApron, ApronBuildFailure> {
    if outer_rings.is_empty() {
        return Err(ApronBuildFailure::BadGeometry);
    }
    let mut holes_by_outer: Vec<Vec<Vec<NodeCoordinate>>> = Vec::new();
    holes_by_outer
        .try_reserve_exact(outer_rings.len())
        .map_err(|_| ApronBuildFailure::Allocation(outer_rings.len()))?;
    holes_by_outer.resize_with(outer_rings.len(), Vec::new);
    for hole in inner_rings {
        let Some(test_point) = hole.first().copied() else {
            return Err(ApronBuildFailure::BadGeometry);
        };
        let containing: Vec<_> = outer_rings
            .iter()
            .enumerate()
            .filter_map(|(index, outer)| {
                let origin = outer.first().copied()?;
                let projected_outer: Vec<_> = outer
                    .iter()
                    .copied()
                    .map(|point| project_coordinate(origin, point))
                    .collect();
                point_in_ring(project_coordinate(origin, test_point), &projected_outer)
                    .then_some(index)
            })
            .collect();
        if containing.len() != 1 {
            return Err(ApronBuildFailure::BadGeometry);
        }
        let owner = containing[0];
        reserve_candidate(&mut holes_by_outer[owner], "apron holes")
            .map_err(|_| ApronBuildFailure::Allocation(holes_by_outer[owner].len() + 1))?;
        holes_by_outer[owner].push(hole);
    }

    let mut triangles = Vec::new();
    let mut local_record_count = *candidate_records;
    for (outer, holes) in outer_rings.iter().zip(&holes_by_outer) {
        triangulate_rings(outer, holes, &mut triangles, &mut local_record_count)?;
    }
    let apron =
        AirportApron::new(source_kind, source_id, surface, triangles).map_err(
            |error| match error {
                GroundFeatureGeometryError::AllocationFailed { requested } => {
                    ApronBuildFailure::Allocation(requested)
                }
                _ => ApronBuildFailure::BadGeometry,
            },
        )?;
    *candidate_records = local_record_count;
    Ok(apron)
}

#[allow(clippy::too_many_arguments)]
fn convert_apron_candidates(
    mut way_candidates: Vec<ApronWayCandidate>,
    relation_candidates: Vec<ApronRelationCandidate>,
    member_ways: &HashMap<i64, Vec<i64>>,
    nodes: &HashMap<i64, NodeCoordinate>,
    mut report: AirportGenerationReport,
    candidate_records: &mut usize,
) -> Result<(Vec<AirportApron>, AirportGenerationReport), AirportGenError> {
    way_candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut aprons = Vec::new();
    aprons
        .try_reserve_exact(
            way_candidates
                .len()
                .saturating_add(relation_candidates.len()),
        )
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "converted aprons",
            requested: way_candidates
                .len()
                .saturating_add(relation_candidates.len()),
        })?;

    for candidate in way_candidates {
        let result = ring_coordinates(&candidate.node_refs, nodes).and_then(|outer| {
            build_apron(
                AirportSourceKind::Way,
                candidate.source_way_id,
                parse_surface(candidate.surface.as_deref()),
                &[outer],
                Vec::new(),
                candidate_records,
            )
        });
        match result {
            Ok(apron) => aprons.push(apron),
            Err(failure) => handle_apron_failure(failure, &mut report)?,
        }
    }

    for candidate in relation_candidates {
        if candidate.has_bad_members {
            report.skipped_apron_bad_members += 1;
            continue;
        }
        let result = (|| {
            let outer_refs =
                stitch_member_rings(&candidate.members, ApronMemberRole::Outer, member_ways)?;
            let inner_refs =
                stitch_member_rings(&candidate.members, ApronMemberRole::Inner, member_ways)?;
            let mut outers = Vec::new();
            outers
                .try_reserve_exact(outer_refs.len())
                .map_err(|_| ApronBuildFailure::Allocation(outer_refs.len()))?;
            for ring in outer_refs {
                outers.push(ring_coordinates(&ring, nodes)?);
            }
            let mut inners = Vec::new();
            inners
                .try_reserve_exact(inner_refs.len())
                .map_err(|_| ApronBuildFailure::Allocation(inner_refs.len()))?;
            for ring in inner_refs {
                inners.push(ring_coordinates(&ring, nodes)?);
            }
            build_apron(
                AirportSourceKind::Relation,
                candidate.source_relation_id,
                parse_surface(candidate.surface.as_deref()),
                &outers,
                inners,
                candidate_records,
            )
        })();
        match result {
            Ok(apron) => aprons.push(apron),
            Err(failure) => handle_apron_failure(failure, &mut report)?,
        }
    }
    report.aprons_written = aprons.len();
    report.apron_triangles_written = aprons.iter().map(|apron| apron.triangles().len()).sum();
    Ok((aprons, report))
}

#[derive(Debug, Clone, Copy)]
struct TaxiwayAssociation {
    source_way_id: i64,
    segment_index: usize,
    distance_meters: f64,
    heading: Radians,
    width: Meters,
}

fn heading_between(first: NodeCoordinate, last: NodeCoordinate) -> Option<Radians> {
    let last = project_coordinate(first, last);
    if last.x.abs() <= f64::EPSILON && last.y.abs() <= f64::EPSILON {
        return None;
    }
    Some(Radians(last.x.atan2(last.y)))
}

fn find_taxiway_association(
    point: NodeCoordinate,
    source_node_id: Option<i64>,
    taxiways: &[TaxiwayCandidate],
    nodes: &HashMap<i64, NodeCoordinate>,
) -> Option<TaxiwayAssociation> {
    let mut matches = Vec::new();
    for taxiway in taxiways {
        let (width, _) = parse_width(taxiway.width.as_deref(), DEFAULT_TAXIWAY_WIDTH);
        if let Some(node_id) = source_node_id {
            for (node_index, candidate_id) in taxiway.node_refs.iter().enumerate() {
                if *candidate_id != node_id {
                    continue;
                }
                let endpoints = match node_index {
                    0 if taxiway.node_refs.len() >= 2 => {
                        Some((taxiway.node_refs[0], taxiway.node_refs[1]))
                    }
                    index if index + 1 == taxiway.node_refs.len() => {
                        Some((taxiway.node_refs[index - 1], taxiway.node_refs[index]))
                    }
                    index => Some((taxiway.node_refs[index - 1], taxiway.node_refs[index + 1])),
                };
                let Some((first_id, last_id)) = endpoints else {
                    continue;
                };
                let (Some(first), Some(last)) = (nodes.get(&first_id), nodes.get(&last_id)) else {
                    continue;
                };
                let Some(heading) = heading_between(*first, *last) else {
                    continue;
                };
                matches.push(TaxiwayAssociation {
                    source_way_id: taxiway.source_way_id,
                    segment_index: node_index.saturating_sub(1),
                    distance_meters: 0.0,
                    heading,
                    width,
                });
            }
        }
    }
    if matches.is_empty() {
        for taxiway in taxiways {
            let (width, _) = parse_width(taxiway.width.as_deref(), DEFAULT_TAXIWAY_WIDTH);
            for (segment_index, pair) in taxiway.node_refs.windows(2).enumerate() {
                let (Some(first), Some(last)) = (nodes.get(&pair[0]), nodes.get(&pair[1])) else {
                    continue;
                };
                let Some((distance, _)) = distance_to_segment_meters(point, *first, *last) else {
                    continue;
                };
                if distance > width.get() * 0.5 + 1.0 {
                    continue;
                }
                let Some(heading) = heading_between(*first, *last) else {
                    continue;
                };
                matches.push(TaxiwayAssociation {
                    source_way_id: taxiway.source_way_id,
                    segment_index,
                    distance_meters: distance,
                    heading,
                    width,
                });
            }
        }
    }
    matches.into_iter().min_by(|left, right| {
        left.distance_meters
            .total_cmp(&right.distance_meters)
            .then_with(|| left.source_way_id.cmp(&right.source_way_id))
            .then_with(|| left.segment_index.cmp(&right.segment_index))
    })
}

fn runway_side_for_heading(
    position: NodeCoordinate,
    heading: Radians,
    runways: &[AirportRunway],
) -> RunwaySide {
    let mut runway_vectors: Vec<_> = runways
        .iter()
        .map(|runway| {
            let centre = runway.runway.center();
            let vector = project_coordinate(
                position,
                NodeCoordinate {
                    latitude_degrees: centre.latitude_degrees(),
                    longitude_degrees: centre.longitude_degrees(),
                },
            );
            (
                vector.x * vector.x + vector.y * vector.y,
                runway.source_way_id,
                vector,
            )
        })
        .filter(|(distance, _, _)| distance.is_finite())
        .collect();
    runway_vectors.sort_unstable_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    let Some((nearest_distance, _, vector)) = runway_vectors.first().copied() else {
        return RunwaySide::Unknown;
    };
    if runway_vectors
        .get(1)
        .is_some_and(|(distance, _, _)| (distance.sqrt() - nearest_distance.sqrt()).abs() <= 1.0)
    {
        return RunwaySide::Unknown;
    }
    let forward_east = heading.get().sin();
    let forward_north = heading.get().cos();
    let dot = vector.x * forward_east + vector.y * forward_north;
    if dot > 1.0 {
        RunwaySide::Forward
    } else if dot < -1.0 {
        RunwaySide::Backward
    } else {
        RunwaySide::Unknown
    }
}

fn associated_reference(
    own_reference: Option<String>,
    association: Option<TaxiwayAssociation>,
    taxiways: &[TaxiwayCandidate],
) -> Option<String> {
    own_reference
        .filter(|reference| !reference.is_empty())
        .or_else(|| {
            association.and_then(|association| {
                taxiways
                    .iter()
                    .find(|taxiway| taxiway.source_way_id == association.source_way_id)
                    .and_then(|taxiway| taxiway.reference.clone())
                    .filter(|reference| !reference.is_empty())
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn convert_holding_candidates(
    mut node_candidates: Vec<HoldingNodeCandidate>,
    mut way_candidates: Vec<HoldingWayCandidate>,
    taxiway_candidates: &[TaxiwayCandidate],
    nodes: &HashMap<i64, NodeCoordinate>,
    runways: &[AirportRunway],
    mut report: AirportGenerationReport,
    candidate_records: &mut usize,
) -> Result<(Vec<AirportHoldingPosition>, AirportGenerationReport), AirportGenError> {
    node_candidates.sort_unstable_by_key(|candidate| candidate.source_node_id);
    way_candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut holdings = Vec::new();
    holdings
        .try_reserve_exact(node_candidates.len().saturating_add(way_candidates.len()))
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "converted holding positions",
            requested: node_candidates.len().saturating_add(way_candidates.len()),
        })?;

    for candidate in node_candidates {
        if !candidate.coordinate.is_valid() {
            report.skipped_holding_bad_coordinates += 1;
            continue;
        }
        let association = find_taxiway_association(
            candidate.coordinate,
            Some(candidate.source_node_id),
            taxiway_candidates,
            nodes,
        );
        let Some(association) = association else {
            report.skipped_holding_unassociated += 1;
            continue;
        };
        let reference =
            associated_reference(candidate.reference, Some(association), taxiway_candidates);
        let holding = AirportHoldingPosition::new(
            AirportSourceKind::Node,
            candidate.source_node_id,
            candidate.coordinate.to_geodetic(),
            candidate.holding_type,
            association.heading,
            association.width,
            reference.clone(),
            Some(association.source_way_id),
            runway_side_for_heading(candidate.coordinate, association.heading, runways),
        );
        match holding {
            Ok(holding) => {
                add_candidate_records(candidate_records, 1)?;
                if reference.is_some() {
                    add_candidate_records(candidate_records, 1)?;
                }
                note_renderer_ineligible_reference(reference.as_deref(), &mut report);
                holdings.push(holding);
            }
            Err(GroundFeatureGeometryError::AllocationFailed { requested }) => {
                return Err(AirportGenError::AllocationFailed {
                    context: "holding position",
                    requested,
                });
            }
            Err(_) => report.skipped_holding_bad_geometry += 1,
        }
    }

    for candidate in way_candidates {
        if candidate.node_refs.len() < 2 {
            report.skipped_holding_bad_geometry += 1;
            continue;
        }
        let (Some(first), Some(last)) = (
            candidate
                .node_refs
                .first()
                .and_then(|node_id| nodes.get(node_id))
                .copied(),
            candidate
                .node_refs
                .last()
                .and_then(|node_id| nodes.get(node_id))
                .copied(),
        ) else {
            report.skipped_holding_bad_geometry += 1;
            continue;
        };
        if !first.is_valid() || !last.is_valid() {
            report.skipped_holding_bad_coordinates += 1;
            continue;
        }
        let position = midpoint(first, last);
        let Some(marking_heading) = heading_between(first, last) else {
            report.skipped_holding_bad_geometry += 1;
            continue;
        };
        let taxiway_heading = Radians(marking_heading.get() + core::f64::consts::FRAC_PI_2);
        let width = Meters(coordinate_distance_meters(first, last));
        let association = find_taxiway_association(position, None, taxiway_candidates, nodes);
        let reference = associated_reference(candidate.reference, association, taxiway_candidates);
        let holding = AirportHoldingPosition::new(
            AirportSourceKind::Way,
            candidate.source_way_id,
            position.to_geodetic(),
            candidate.holding_type,
            taxiway_heading,
            width,
            reference.clone(),
            association.map(|association| association.source_way_id),
            runway_side_for_heading(position, taxiway_heading, runways),
        );
        match holding {
            Ok(holding) => {
                add_candidate_records(candidate_records, 1)?;
                if reference.is_some() {
                    add_candidate_records(candidate_records, 1)?;
                }
                note_renderer_ineligible_reference(reference.as_deref(), &mut report);
                holdings.push(holding);
            }
            Err(GroundFeatureGeometryError::AllocationFailed { requested }) => {
                return Err(AirportGenError::AllocationFailed {
                    context: "holding position",
                    requested,
                });
            }
            Err(_) => report.skipped_holding_bad_geometry += 1,
        }
    }
    report.holding_positions_written = holdings.len();
    Ok((holdings, report))
}

fn convert_ground_lights(
    mut candidates: Vec<GroundLightCandidate>,
    mut report: AirportGenerationReport,
    candidate_records: &mut usize,
) -> Result<(Vec<AirportGroundLight>, AirportGenerationReport), AirportGenError> {
    candidates.sort_unstable_by_key(|candidate| candidate.source_node_id);
    let mut lights = Vec::new();
    lights
        .try_reserve_exact(candidates.len())
        .map_err(|_| AirportGenError::AllocationFailed {
            context: "converted ground lights",
            requested: candidates.len(),
        })?;
    for candidate in candidates {
        if !candidate.coordinate.is_valid() {
            report.skipped_ground_light_bad_coordinates += 1;
            continue;
        }
        match AirportGroundLight::new(
            AirportSourceKind::Node,
            candidate.source_node_id,
            candidate.coordinate.to_geodetic(),
            candidate.kind,
        ) {
            Ok(light) => {
                add_candidate_records(candidate_records, 1)?;
                lights.push(light);
            }
            Err(GroundFeatureGeometryError::AllocationFailed { requested }) => {
                return Err(AirportGenError::AllocationFailed {
                    context: "ground light",
                    requested,
                });
            }
            Err(_) => report.skipped_ground_light_bad_coordinates += 1,
        }
    }
    report.ground_lights_written = lights.len();
    Ok((lights, report))
}

fn parse_width(text: Option<&str>, fallback: Meters) -> (Meters, bool) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return (fallback, true);
    };

    let (number, feet) = if let Some(number) = text.strip_suffix("ft") {
        (number.trim(), true)
    } else if let Some(number) = text.strip_suffix('m') {
        (number.trim(), false)
    } else {
        (text, false)
    };

    let Ok(value) = number.parse::<f64>() else {
        return (fallback, true);
    };
    let width = if feet {
        Feet(value).to_meters()
    } else {
        Meters(value)
    };
    if width.is_finite() && width.get() > 0.0 {
        (width, false)
    } else {
        (fallback, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn candidate(
        source_way_id: i64,
        first_node: Option<i64>,
        last_node: Option<i64>,
        width: Option<&str>,
    ) -> RunwayCandidate {
        RunwayCandidate {
            source_way_id,
            first_node,
            last_node,
            width: width.map(str::to_owned),
        }
    }

    fn coordinates(entries: &[(i64, f64, f64)]) -> HashMap<i64, NodeCoordinate> {
        entries
            .iter()
            .map(|&(id, latitude_degrees, longitude_degrees)| {
                (
                    id,
                    NodeCoordinate {
                        latitude_degrees,
                        longitude_degrees,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn only_open_runway_centerlines_are_selected() {
        let runway = [("aeroway", "runway")];
        assert_eq!(
            classify_way(&runway, &[1, 2]),
            WayDisposition::RunwayCenterline
        );
        assert_eq!(
            classify_way(&[("aeroway", "apron")], &[1, 2]),
            WayDisposition::Other
        );
        assert_eq!(
            classify_way(&[("aeroway", "runway"), ("area", "yes")], &[1, 2]),
            WayDisposition::RunwayArea
        );
        assert_eq!(
            classify_way(&runway, &[1, 2, 3, 1]),
            WayDisposition::RunwayClosed
        );
        assert_eq!(
            classify_way(&runway, &[1]),
            WayDisposition::RunwayCenterline,
            "a one-node way is a degenerate line, not an area"
        );
    }

    #[test]
    fn taxiway_centerlines_include_closed_ways_but_not_areas() {
        let taxiway = [("aeroway", "taxiway")];
        assert_eq!(
            classify_way(&taxiway, &[1, 2, 3, 1]),
            WayDisposition::TaxiwayCenterline
        );
        assert_eq!(
            classify_way(&[("aeroway", "taxiway"), ("area", "yes")], &[1, 2, 1]),
            WayDisposition::TaxiwayArea
        );

        let nodes = coordinates(&[(1, 35.0, 139.0), (2, 35.001, 139.002), (3, 35.002, 139.0)]);
        let (taxiways, report) = convert_taxiway_candidates(
            vec![TaxiwayCandidate {
                source_way_id: 77,
                node_refs: vec![1, 2, 3, 1],
                width: Some("12 m".to_owned()),
                reference: None,
                surface: None,
                lit: Some("no".to_owned()),
            }],
            &nodes,
            &[],
            AirportGenerationReport::default(),
        )
        .expect("closed centerline conversion should allocate");
        assert_eq!(report.taxiway_segments_written, 3);

        let database = AirportDatabase::with_taxiways(Vec::new(), taxiways)
            .expect("closed centerline should be valid FSAP geometry");
        let bytes = database
            .to_bytes()
            .expect("FSAP v2 encoding should succeed");
        assert_eq!(&bytes[4..6], &2_u16.to_le_bytes());
        let restored = AirportDatabase::from_bytes(&bytes).expect("FSAP v2 should decode");
        let restored_points = restored.taxiways()[0].points();
        assert_eq!(restored_points.len(), 4);
        assert_eq!(restored_points.first(), restored_points.last());
    }

    #[test]
    fn candidate_record_budget_counts_segments_and_rejects_excess_or_overflow() {
        let mut records = 0;
        add_candidate_records(&mut records, 1).expect("one runway record fits");
        add_candidate_records(&mut records, 4 - 1).expect("three taxiway segments fit");
        assert_eq!(records, 4);

        records = MAX_RECORD_COUNT as usize;
        assert!(matches!(
            add_candidate_records(&mut records, 1),
            Err(AirportGenError::RecordLimitExceeded {
                attempted,
                maximum: MAX_RECORD_COUNT,
            }) if attempted == MAX_RECORD_COUNT as usize + 1
        ));
        assert_eq!(records, MAX_RECORD_COUNT as usize);

        records = usize::MAX;
        assert!(matches!(
            add_candidate_records(&mut records, 1),
            Err(AirportGenError::RecordLimitExceeded {
                attempted: usize::MAX,
                maximum: MAX_RECORD_COUNT,
            })
        ));
    }

    #[test]
    fn widths_accept_bare_metres_and_explicit_units() {
        for text in ["30", "30m", " 30 m "] {
            let (width, defaulted) = parse_width(Some(text), DEFAULT_RUNWAY_WIDTH);
            assert!(!defaulted, "{text}");
            assert!((width.get() - 30.0).abs() < 1e-12, "{text}");
        }

        let (width, defaulted) = parse_width(Some("150 ft"), DEFAULT_RUNWAY_WIDTH);
        assert!(!defaulted);
        // 国際フィートの定義 0.3048 m/ft から独立に検算した値。
        assert!((width.get() - 45.72).abs() < 1e-12);
    }

    #[test]
    fn missing_non_finite_and_non_positive_widths_use_the_default() {
        for text in [
            None,
            Some(""),
            Some("wide"),
            Some("NaN"),
            Some("inf"),
            Some("0"),
            Some("-5"),
            Some("12 yd"),
        ] {
            let (width, defaulted) = parse_width(text, DEFAULT_RUNWAY_WIDTH);
            assert!(defaulted, "{text:?}");
            assert_eq!(width, DEFAULT_RUNWAY_WIDTH, "{text:?}");
        }
    }

    #[test]
    fn conversion_resolves_endpoints_sorts_ids_and_counts_width_fallbacks() {
        let nodes = coordinates(&[
            (1, 35.0, 139.0),
            (2, 35.01, 139.01),
            (3, 36.0, 140.0),
            (4, 36.02, 140.02),
        ]);
        let candidates = vec![
            candidate(20, Some(3), Some(4), None),
            candidate(10, Some(1), Some(2), Some("60")),
        ];

        let (runways, report) = convert_candidates(
            candidates,
            &nodes,
            AirportGenerationReport {
                runway_ways_seen: 2,
                ..AirportGenerationReport::default()
            },
        )
        .expect("runway conversion should allocate");

        assert_eq!(
            runways
                .iter()
                .map(|runway| runway.source_way_id)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert!((runways[0].runway.threshold.latitude_degrees() - 35.0).abs() < 1e-12);
        assert!((runways[0].opposite_threshold().latitude_degrees() - 35.01).abs() < 1e-12);
        assert_eq!(runways[0].runway.width, Meters(60.0));
        assert_eq!(runways[1].runway.width, DEFAULT_RUNWAY_WIDTH);
        assert_eq!(report.runways_written, 2);
        assert_eq!(report.widths_defaulted, 1);
    }

    #[test]
    fn missing_bad_and_degenerate_endpoints_are_reported_separately() {
        let nodes = coordinates(&[
            (1, 35.0, 139.0),
            (2, f64::NAN, 139.1),
            (3, 91.0, 139.2),
            (4, 36.0, 140.0),
            (5, 36.0, 140.0),
        ]);
        let candidates = vec![
            candidate(1, Some(1), Some(99), Some("45")),
            candidate(2, Some(1), Some(2), Some("45")),
            candidate(3, Some(1), Some(3), Some("45")),
            candidate(4, Some(4), Some(5), Some("45")),
            candidate(5, None, None, Some("45")),
            candidate(6, Some(1), Some(1), Some("45")),
        ];

        let (runways, report) =
            convert_candidates(candidates, &nodes, AirportGenerationReport::default())
                .expect("runway conversion should allocate");

        assert!(runways.is_empty());
        assert_eq!(report.skipped_missing_nodes, 1);
        assert_eq!(report.skipped_bad_coordinates, 2);
        assert_eq!(report.skipped_degenerate, 3);
        assert_eq!(report.widths_defaulted, 0);
    }

    #[test]
    fn different_longitudes_at_the_pole_are_still_degenerate() {
        let nodes = coordinates(&[(1, 90.0, 0.0), (2, 90.0, 90.0)]);
        let (runways, report) = convert_candidates(
            vec![candidate(1, Some(1), Some(2), Some("45"))],
            &nodes,
            AirportGenerationReport::default(),
        )
        .expect("runway conversion should allocate");
        assert!(runways.is_empty());
        assert_eq!(report.skipped_degenerate, 1);
    }

    #[test]
    fn taxiway_conversion_is_sorted_preserves_points_and_uses_15m_fallback() {
        let nodes = coordinates(&[
            (1, 35.0, 139.0),
            (2, 35.001, 139.002),
            (3, 35.002, 139.004),
            (4, 36.0, 140.0),
            (5, 36.001, 140.002),
        ]);
        let candidates = vec![
            TaxiwayCandidate {
                source_way_id: 20,
                node_refs: vec![4, 5],
                width: Some("8 m".to_owned()),
                reference: None,
                surface: None,
                lit: None,
            },
            TaxiwayCandidate {
                source_way_id: 10,
                node_refs: vec![1, 2, 3],
                width: None,
                reference: None,
                surface: None,
                lit: None,
            },
        ];

        let (taxiways, report) =
            convert_taxiway_candidates(candidates, &nodes, &[], AirportGenerationReport::default())
                .expect("taxiway conversion should allocate");

        assert_eq!(
            taxiways
                .iter()
                .map(|taxiway| taxiway.source_way_id)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert_eq!(taxiways[0].points().len(), 3);
        assert_eq!(taxiways[0].width, DEFAULT_TAXIWAY_WIDTH);
        assert_eq!(taxiways[1].width, Meters(8.0));
        assert_eq!(report.taxiways_written, 2);
        assert_eq!(report.taxiway_segments_written, 3);
        assert_eq!(report.taxiway_widths_defaulted, 1);
    }

    #[test]
    fn taxiway_conversion_reports_missing_bad_and_collapsed_ways() {
        let nodes = coordinates(&[(1, 35.0, 139.0), (2, f64::NAN, 139.1), (3, 35.0, 139.0)]);
        let candidates = vec![
            TaxiwayCandidate {
                source_way_id: 1,
                node_refs: vec![1, 99],
                width: None,
                reference: None,
                surface: None,
                lit: None,
            },
            TaxiwayCandidate {
                source_way_id: 2,
                node_refs: vec![1, 2],
                width: None,
                reference: None,
                surface: None,
                lit: None,
            },
            TaxiwayCandidate {
                source_way_id: 3,
                node_refs: vec![1, 3],
                width: None,
                reference: None,
                surface: None,
                lit: None,
            },
        ];
        let (taxiways, report) =
            convert_taxiway_candidates(candidates, &nodes, &[], AirportGenerationReport::default())
                .expect("taxiway conversion should allocate");
        assert!(taxiways.is_empty());
        assert_eq!(report.skipped_taxiway_missing_nodes, 1);
        assert_eq!(report.skipped_taxiway_bad_coordinates, 1);
        assert_eq!(report.skipped_taxiway_degenerate, 1);
        assert_eq!(report.taxiway_widths_defaulted, 0);
    }

    #[test]
    fn taxiway_metadata_preserves_utf8_and_materializes_lighting() {
        let nodes = coordinates(&[(1, 35.0, 139.0), (2, 35.001, 139.001)]);
        let candidate = TaxiwayCandidate {
            source_way_id: 42,
            node_refs: vec![1, 2],
            width: None,
            reference: Some("誘導路A".to_owned()),
            surface: Some("concrete".to_owned()),
            lit: None,
        };
        let (taxiways, report) = convert_taxiway_candidates(
            vec![candidate],
            &nodes,
            &[],
            AirportGenerationReport::default(),
        )
        .expect("metadata conversion should succeed");
        assert_eq!(taxiways[0].reference(), Some("誘導路A"));
        assert_eq!(taxiways[0].surface(), AirportSurface::Concrete);
        assert_eq!(taxiways[0].lighting(), TaxiwayLighting::EdgeAndCenterline);
        assert_eq!(report.renderer_ineligible_non_ascii_refs, 1);
    }

    #[test]
    fn explicit_taxiway_lights_suppress_only_the_matching_fallback_channel() {
        let nodes = coordinates(&[(1, 35.0, 139.0), (2, 35.001, 139.0)]);
        let lights = [GroundLightCandidate {
            source_node_id: 9,
            coordinate: NodeCoordinate {
                latitude_degrees: 35.0005,
                longitude_degrees: 139.0,
            },
            kind: GroundLightKind::TaxiwayEdge,
        }];
        assert_eq!(
            taxiway_lighting_with_explicit_points(None, &[1, 2], Meters(15.0), &nodes, &lights,),
            TaxiwayLighting::Centerline
        );
        assert_eq!(
            taxiway_lighting_with_explicit_points(
                Some("no"),
                &[1, 2],
                Meters(15.0),
                &nodes,
                &lights,
            ),
            TaxiwayLighting::None
        );
    }

    #[test]
    fn holding_marking_ways_are_discovered_in_the_sequential_pass() {
        let mut discovery = PbfDiscovery::default();
        let mut report = AirportGenerationReport::default();
        discover_way(
            1_104_043_730,
            [
                ("aeroway", "aerodrome_marking"),
                ("aerodrome_marking", "holding_position"),
            ]
            .into_iter(),
            &mut discovery,
            &mut report,
        )
        .expect("holding marking discovery should allocate");
        assert!(discovery.holding_ways.contains_key(&1_104_043_730));
        assert_eq!(report.holding_ways_seen, 1);
    }

    #[test]
    fn multipolygon_members_stitch_in_way_id_order_with_reversal() {
        let members = vec![
            ApronRelationMember {
                way_id: 20,
                role: ApronMemberRole::Outer,
            },
            ApronRelationMember {
                way_id: 10,
                role: ApronMemberRole::Outer,
            },
            ApronRelationMember {
                way_id: 30,
                role: ApronMemberRole::Outer,
            },
        ];
        let member_ways = HashMap::from([(10, vec![1, 2]), (20, vec![3, 2]), (30, vec![3, 1])]);
        assert_eq!(
            stitch_member_rings(&members, ApronMemberRole::Outer, &member_ways)
                .expect("fragments form one ring"),
            vec![vec![1, 2, 3]]
        );
    }

    #[test]
    fn apron_hole_is_triangulated_and_subdivided_to_the_dem_sampling_limit() {
        let outer = vec![
            NodeCoordinate {
                latitude_degrees: 35.0,
                longitude_degrees: 139.0,
            },
            NodeCoordinate {
                latitude_degrees: 35.0,
                longitude_degrees: 139.003,
            },
            NodeCoordinate {
                latitude_degrees: 35.003,
                longitude_degrees: 139.003,
            },
            NodeCoordinate {
                latitude_degrees: 35.003,
                longitude_degrees: 139.0,
            },
        ];
        let hole = vec![
            NodeCoordinate {
                latitude_degrees: 35.001,
                longitude_degrees: 139.001,
            },
            NodeCoordinate {
                latitude_degrees: 35.001,
                longitude_degrees: 139.002,
            },
            NodeCoordinate {
                latitude_degrees: 35.002,
                longitude_degrees: 139.002,
            },
            NodeCoordinate {
                latitude_degrees: 35.002,
                longitude_degrees: 139.001,
            },
        ];
        let mut records = 0;
        let apron = build_apron(
            AirportSourceKind::Relation,
            5,
            AirportSurface::Asphalt,
            &[outer],
            vec![hole],
            &mut records,
        )
        .expect("valid apron with a hole should triangulate");
        assert_eq!(records, apron.triangles().len());
        assert!(apron.triangles().iter().all(|triangle| {
            triangle
                .iter()
                .zip(triangle.iter().cycle().skip(1))
                .take(3)
                .all(|(left, right)| {
                    coordinate_distance_meters(
                        NodeCoordinate {
                            latitude_degrees: left.latitude_degrees(),
                            longitude_degrees: left.longitude_degrees(),
                        },
                        NodeCoordinate {
                            latitude_degrees: right.latitude_degrees(),
                            longitude_degrees: right.longitude_degrees(),
                        },
                    ) <= MAX_APRON_TRIANGLE_EDGE_METERS + 1.0e-6
                })
        }));
    }

    #[test]
    fn identical_input_and_output_are_rejected_without_truncation() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let input = directory.path().join("source.osm.pbf");
        let original = b"not a PBF, but it must remain untouched";
        fs::write(&input, original).expect("fixture should be written");

        let error = generate_airport_database(&input, &input)
            .expect_err("the same input and output must be rejected before decoding");

        assert!(matches!(error, AirportGenError::InputOutputConflict { .. }));
        assert_eq!(
            fs::read(&input).expect("input should remain readable"),
            original
        );
    }

    #[test]
    fn hard_link_output_is_rejected_without_truncation() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let input = directory.path().join("source.osm.pbf");
        let output = directory.path().join("output.fsairports");
        let original = b"hard-linked input must remain untouched";
        fs::write(&input, original).expect("fixture should be written");
        fs::hard_link(&input, &output).expect("hard link should be created");

        let error = generate_airport_database(&input, &output)
            .expect_err("hard-linked input and output must be rejected before decoding");

        assert!(matches!(error, AirportGenError::InputOutputConflict { .. }));
        assert_eq!(
            fs::read(&input).expect("input should remain readable"),
            original
        );
        assert_eq!(
            fs::read(&output).expect("hard link should remain readable"),
            original
        );
    }

    #[test]
    fn atomic_writer_replaces_an_existing_database() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let output = directory.path().join("airport.fsairports");
        fs::write(&output, b"old partial data").expect("old output should be written");
        let database = AirportDatabase::new(Vec::new()).expect("empty database is valid");

        write_database_atomically(&database, &output).expect("replacement should succeed");

        assert_eq!(
            fs::read(&output).expect("replacement should be readable"),
            database.to_bytes().expect("database should encode")
        );
        assert_eq!(
            fs::read_dir(directory.path())
                .expect("directory should be readable")
                .count(),
            1,
            "the temporary file must not remain after persistence"
        );
    }
}
