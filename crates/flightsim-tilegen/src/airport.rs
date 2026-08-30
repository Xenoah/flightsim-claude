//! OpenStreetMap PBF から実行時空港 DB を作る。
//!
//! 生の PBF は実行時に読ませない。[`generate_airport_database`] が
//! `aeroway=runway` と `aeroway=taxiway` の中心線 way・依存 node を取り出し、
//! `flightsim-world` が検証して読む固定長形式へ焼く。

use flightsim_core::{Feet, Geodetic, Meters};
use flightsim_world::{AirportDatabase, AirportRunway, AirportTaxiway};
use osmpbf::{Element, IndexedReader};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

const DEFAULT_RUNWAY_WIDTH: Meters = Meters(45.0);
const DEFAULT_TAXIWAY_WIDTH: Meters = Meters(15.0);

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

/// OSM PBF の滑走路・誘導路中心線を `.fsairports` へ変換する。
///
/// way は OSM ID 順に並べてから DB へ渡す。同じ入力 PBF と同じ
/// `flightsim-world` 版からは、常に同じレコード順と同じ bytes が得られる。
///
/// # Errors
///
/// 入出力が同じファイルを指す場合、PBF を読めない場合、変換後の DB を構築できない
/// 場合、または出力を書けない場合に [`AirportGenError`] を返す。個々の不正 way は
/// エラーで全体を止めず、理由別に数えて [`AirportGenerationReport`] で報告する。
pub fn generate_airport_database(
    input: &Path,
    output: &Path,
) -> Result<AirportGenerationReport, AirportGenError> {
    ensure_distinct_files(input, output)?;
    let extraction = extract_pbf(input)?;
    let (runways, report) = convert_candidates(
        extraction.runway_candidates,
        &extraction.nodes,
        extraction.report,
    );
    let (taxiways, mut report) =
        convert_taxiway_candidates(extraction.taxiway_candidates, &extraction.nodes, report);

    let database = AirportDatabase::with_taxiways(runways, taxiways).map_err(|error| {
        AirportGenError::BuildDatabase {
            message: error.to_string(),
        }
    })?;
    report.runways_written = database.runways().len();
    report.taxiways_written = database.taxiways().len();
    report.taxiway_segments_written = database
        .taxiways()
        .iter()
        .map(|taxiway| taxiway.points().len() - 1)
        .sum();
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
}

#[derive(Debug)]
struct PbfExtraction {
    runway_candidates: Vec<RunwayCandidate>,
    taxiway_candidates: Vec<TaxiwayCandidate>,
    nodes: HashMap<i64, NodeCoordinate>,
    report: AirportGenerationReport,
}

fn extract_pbf(path: &Path) -> Result<PbfExtraction, AirportGenError> {
    let mut reader = IndexedReader::from_path(path).map_err(|error| AirportGenError::ReadPbf {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    let mut runway_candidates = Vec::new();
    let mut taxiway_candidates = Vec::new();
    let mut nodes = HashMap::new();
    let mut report = AirportGenerationReport::default();

    reader
        .read_ways_and_deps(
            |way| {
                let tags: Vec<_> = way.tags().collect();
                let node_refs: Vec<_> = way.refs().collect();
                match classify_way(&tags, &node_refs) {
                    WayDisposition::Other => false,
                    WayDisposition::RunwayArea => {
                        report.runway_ways_seen += 1;
                        report.skipped_areas += 1;
                        false
                    }
                    WayDisposition::RunwayClosed => {
                        report.runway_ways_seen += 1;
                        report.skipped_closed += 1;
                        false
                    }
                    WayDisposition::RunwayCenterline => {
                        report.runway_ways_seen += 1;
                        runway_candidates.push(RunwayCandidate {
                            source_way_id: way.id(),
                            first_node: node_refs.first().copied(),
                            last_node: node_refs.last().copied(),
                            width: tag_value(&tags, "width").map(str::to_owned),
                        });
                        true
                    }
                    WayDisposition::TaxiwayArea => {
                        report.taxiway_ways_seen += 1;
                        report.skipped_taxiway_areas += 1;
                        false
                    }
                    WayDisposition::TaxiwayCenterline => {
                        report.taxiway_ways_seen += 1;
                        taxiway_candidates.push(TaxiwayCandidate {
                            source_way_id: way.id(),
                            node_refs,
                            width: tag_value(&tags, "width").map(str::to_owned),
                        });
                        true
                    }
                }
            },
            |element| match element {
                Element::Node(node) => {
                    nodes.insert(
                        node.id(),
                        NodeCoordinate {
                            latitude_degrees: node.lat(),
                            longitude_degrees: node.lon(),
                        },
                    );
                }
                Element::DenseNode(node) => {
                    nodes.insert(
                        node.id(),
                        NodeCoordinate {
                            latitude_degrees: node.lat(),
                            longitude_degrees: node.lon(),
                        },
                    );
                }
                Element::Way(_) | Element::Relation(_) => {}
            },
        )
        .map_err(|error| AirportGenError::ReadPbf {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    Ok(PbfExtraction {
        runway_candidates,
        taxiway_candidates,
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

fn classify_way(tags: &[(&str, &str)], node_refs: &[i64]) -> WayDisposition {
    let aeroway = tag_value(tags, "aeroway");
    let is_area = tag_value(tags, "area") == Some("yes");
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
    if node_refs.len() >= 2 && node_refs.first() == node_refs.last() {
        return WayDisposition::RunwayClosed;
    }
    WayDisposition::RunwayCenterline
}

fn tag_value<'a>(tags: &'a [(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    tags.iter()
        .find_map(|(candidate, value)| (*candidate == key).then_some(*value))
}

fn convert_candidates(
    mut candidates: Vec<RunwayCandidate>,
    nodes: &HashMap<i64, NodeCoordinate>,
    mut report: AirportGenerationReport,
) -> (Vec<AirportRunway>, AirportGenerationReport) {
    candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut runways = Vec::with_capacity(candidates.len());

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
    (runways, report)
}

fn convert_taxiway_candidates(
    mut candidates: Vec<TaxiwayCandidate>,
    nodes: &HashMap<i64, NodeCoordinate>,
    mut report: AirportGenerationReport,
) -> (Vec<AirportTaxiway>, AirportGenerationReport) {
    candidates.sort_unstable_by_key(|candidate| candidate.source_way_id);
    let mut taxiways = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        if candidate.node_refs.len() < 2 {
            report.skipped_taxiway_degenerate += 1;
            continue;
        }
        let Some(coordinates) = candidate
            .node_refs
            .iter()
            .map(|node_id| nodes.get(node_id).copied())
            .collect::<Option<Vec<_>>>()
        else {
            report.skipped_taxiway_missing_nodes += 1;
            continue;
        };
        if coordinates.iter().any(|coordinate| !coordinate.is_valid()) {
            report.skipped_taxiway_bad_coordinates += 1;
            continue;
        }
        let (width, defaulted) = parse_width(candidate.width.as_deref(), DEFAULT_TAXIWAY_WIDTH);
        let points = coordinates
            .into_iter()
            .map(NodeCoordinate::to_geodetic)
            .collect();
        let Ok(taxiway) = AirportTaxiway::from_points(candidate.source_way_id, points, width)
        else {
            report.skipped_taxiway_degenerate += 1;
            continue;
        };
        if defaulted {
            report.taxiway_widths_defaulted += 1;
        }
        taxiways.push(taxiway);
    }
    report.taxiways_written = taxiways.len();
    report.taxiway_segments_written = taxiways
        .iter()
        .map(|taxiway| taxiway.points().len() - 1)
        .sum();
    (taxiways, report)
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
        );

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
            convert_candidates(candidates, &nodes, AirportGenerationReport::default());

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
        );
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
            },
            TaxiwayCandidate {
                source_way_id: 10,
                node_refs: vec![1, 2, 3],
                width: None,
            },
        ];

        let (taxiways, report) =
            convert_taxiway_candidates(candidates, &nodes, AirportGenerationReport::default());

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
            },
            TaxiwayCandidate {
                source_way_id: 2,
                node_refs: vec![1, 2],
                width: None,
            },
            TaxiwayCandidate {
                source_way_id: 3,
                node_refs: vec![1, 3],
                width: None,
            },
        ];
        let (taxiways, report) =
            convert_taxiway_candidates(candidates, &nodes, AirportGenerationReport::default());
        assert!(taxiways.is_empty());
        assert_eq!(report.skipped_taxiway_missing_nodes, 1);
        assert_eq!(report.skipped_taxiway_bad_coordinates, 1);
        assert_eq!(report.skipped_taxiway_degenerate, 1);
        assert_eq!(report.taxiway_widths_defaulted, 0);
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
