//! OpenStreetMap PBF から実行時空港 DB を作る。
//!
//! 生の PBF は実行時に読ませない。[`generate_airport_database`] が
//! `aeroway=runway` の中心線 way と依存 node を取り出し、
//! `flightsim-world` が検証して読む固定長形式へ焼く。

use flightsim_core::{Feet, Geodetic, Meters};
use flightsim_world::{AirportDatabase, AirportRunway};
use osmpbf::{Element, IndexedReader};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

const DEFAULT_RUNWAY_WIDTH: Meters = Meters(45.0);

/// 空港 DB 生成で採用・除外した way の件数。
///
/// 除外件数は way 単位で、最初に該当した理由へだけ加算する。そのため
/// `runways_written` とすべての `skipped_*` の和は `runway_ways_seen` に一致する。
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
}

/// 空港 DB 生成の入出力エラー。
#[derive(Debug)]
pub enum AirportGenError {
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

/// OSM PBF の滑走路中心線を `.fsairports` へ変換する。
///
/// way は OSM ID 順に並べてから DB へ渡す。同じ入力 PBF と同じ
/// `flightsim-world` 版からは、常に同じレコード順と同じ bytes が得られる。
///
/// # Errors
///
/// PBF を読めない場合、変換後の DB を構築できない場合、または出力を書けない場合に
/// [`AirportGenError`] を返す。個々の不正 way はエラーで全体を止めず、理由別に数えて
/// [`AirportGenerationReport`] で報告する。
pub fn generate_airport_database(
    input: &Path,
    output: &Path,
) -> Result<AirportGenerationReport, AirportGenError> {
    let extraction = extract_pbf(input)?;
    let (runways, mut report) =
        convert_candidates(extraction.candidates, &extraction.nodes, extraction.report);

    let database =
        AirportDatabase::new(runways).map_err(|error| AirportGenError::BuildDatabase {
            message: error.to_string(),
        })?;
    report.runways_written = database.runways().len();
    database
        .write_to_path(output)
        .map_err(|error| AirportGenError::WriteDatabase {
            path: output.to_path_buf(),
            message: error.to_string(),
        })?;
    Ok(report)
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

#[derive(Debug)]
struct PbfExtraction {
    candidates: Vec<RunwayCandidate>,
    nodes: HashMap<i64, NodeCoordinate>,
    report: AirportGenerationReport,
}

fn extract_pbf(path: &Path) -> Result<PbfExtraction, AirportGenError> {
    let mut reader = IndexedReader::from_path(path).map_err(|error| AirportGenError::ReadPbf {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    let mut candidates = Vec::new();
    let mut nodes = HashMap::new();
    let mut report = AirportGenerationReport::default();

    reader
        .read_ways_and_deps(
            |way| {
                let tags: Vec<_> = way.tags().collect();
                let node_refs: Vec<_> = way.refs().collect();
                match classify_way(&tags, &node_refs) {
                    WayDisposition::NotRunway => false,
                    WayDisposition::Area => {
                        report.runway_ways_seen += 1;
                        report.skipped_areas += 1;
                        false
                    }
                    WayDisposition::Closed => {
                        report.runway_ways_seen += 1;
                        report.skipped_closed += 1;
                        false
                    }
                    WayDisposition::Centerline => {
                        report.runway_ways_seen += 1;
                        candidates.push(RunwayCandidate {
                            source_way_id: way.id(),
                            first_node: node_refs.first().copied(),
                            last_node: node_refs.last().copied(),
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
        candidates,
        nodes,
        report,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WayDisposition {
    NotRunway,
    Area,
    Closed,
    Centerline,
}

fn classify_way(tags: &[(&str, &str)], node_refs: &[i64]) -> WayDisposition {
    if tag_value(tags, "aeroway") != Some("runway") {
        return WayDisposition::NotRunway;
    }
    if tag_value(tags, "area") == Some("yes") {
        return WayDisposition::Area;
    }
    // 参照 1 個だけの壊れた way は面ではなく縮退線として後段で数える。
    if node_refs.len() >= 2 && node_refs.first() == node_refs.last() {
        return WayDisposition::Closed;
    }
    WayDisposition::Centerline
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

        let (width, defaulted) = parse_width(candidate.width.as_deref());
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

fn parse_width(text: Option<&str>) -> (Meters, bool) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return (DEFAULT_RUNWAY_WIDTH, true);
    };

    let (number, feet) = if let Some(number) = text.strip_suffix("ft") {
        (number.trim(), true)
    } else if let Some(number) = text.strip_suffix('m') {
        (number.trim(), false)
    } else {
        (text, false)
    };

    let Ok(value) = number.parse::<f64>() else {
        return (DEFAULT_RUNWAY_WIDTH, true);
    };
    let width = if feet {
        Feet(value).to_meters()
    } else {
        Meters(value)
    };
    if width.is_finite() && width.get() > 0.0 {
        (width, false)
    } else {
        (DEFAULT_RUNWAY_WIDTH, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(classify_way(&runway, &[1, 2]), WayDisposition::Centerline);
        assert_eq!(
            classify_way(&[("aeroway", "taxiway")], &[1, 2]),
            WayDisposition::NotRunway
        );
        assert_eq!(
            classify_way(&[("aeroway", "runway"), ("area", "yes")], &[1, 2]),
            WayDisposition::Area
        );
        assert_eq!(classify_way(&runway, &[1, 2, 3, 1]), WayDisposition::Closed);
        assert_eq!(
            classify_way(&runway, &[1]),
            WayDisposition::Centerline,
            "a one-node way is a degenerate line, not an area"
        );
    }

    #[test]
    fn widths_accept_bare_metres_and_explicit_units() {
        for text in ["30", "30m", " 30 m "] {
            let (width, defaulted) = parse_width(Some(text));
            assert!(!defaulted, "{text}");
            assert!((width.get() - 30.0).abs() < 1e-12, "{text}");
        }

        let (width, defaulted) = parse_width(Some("150 ft"));
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
            let (width, defaulted) = parse_width(text);
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
}
